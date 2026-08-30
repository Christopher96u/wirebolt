mod model;

use std::{
    collections::HashMap,
    error::Error,
    fmt, fs,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

use serde::{Serialize, de::DeserializeOwned};

pub use model::{
    ApiKeyPlacement, CURRENT_SCHEMA_VERSION, Collection, CollectionSnapshot, DocumentId,
    Environment, IdentifierError, Request, RequestAuthentication, RequestBody, RequestHeader,
    RequestValueField, SecretName, ValueSource, Workspace, WorkspaceDocument, WorkspaceSnapshot,
};

const WORKSPACE_FILE: &str = "wirebolt.toml";
const COLLECTIONS_DIRECTORY: &str = "collections";
const COLLECTION_FILE: &str = "collection.toml";
const REQUESTS_DIRECTORY: &str = "requests";
const ENVIRONMENTS_DIRECTORY: &str = "environments";
const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveOutcome {
    Created,
    Updated,
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationReport {
    pub migrated_documents: usize,
}

/// One document that could not be loaded. The rest of the workspace is still
/// returned so a single conflicted or newer file never hides everything else.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentProblem {
    /// Path relative to the workspace root.
    pub path: PathBuf,
    pub kind: DocumentProblemKind,
    /// Never echoes document contents.
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentProblemKind {
    InvalidToml,
    InvalidDocument,
    UnsupportedSchema,
    DocumentTooLarge,
    Io,
}

impl DocumentProblem {
    fn new(root: &Path, error: &StorageError) -> Self {
        let (path, kind) = match error {
            StorageError::InvalidToml { path, .. } => (path, DocumentProblemKind::InvalidToml),
            StorageError::UnsupportedSchema { path, .. } => {
                (path, DocumentProblemKind::UnsupportedSchema)
            }
            StorageError::DocumentTooLarge { path, .. } => {
                (path, DocumentProblemKind::DocumentTooLarge)
            }
            StorageError::Io { path, .. }
            | StorageError::Missing { path }
            | StorageError::AlreadyExists { path }
            | StorageError::Serialization { path } => (path, DocumentProblemKind::Io),
            StorageError::InvalidDocument { path, .. } => {
                (path, DocumentProblemKind::InvalidDocument)
            }
            StorageError::MissingCollection { .. } => {
                (&PathBuf::new(), DocumentProblemKind::InvalidDocument)
            }
        };
        Self {
            path: path.strip_prefix(root).unwrap_or(path).to_owned(),
            kind,
            reason: error.to_string(),
        }
    }
}

/// How hard a write pushes bytes to stable storage. `Full` is macOS
/// `F_FULLFSYNC` on the file and its directory (several milliseconds each);
/// `Fast` orders the data before the rename without forcing a full flush.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Durability {
    Full,
    Fast,
}

#[derive(Clone, Debug)]
struct CachedDocument {
    modified: SystemTime,
    len: u64,
    migrated: bool,
    document: CachedKind,
}

#[derive(Clone, Debug)]
enum CachedKind {
    Workspace(Workspace),
    Collection(Collection),
    Request(Box<Request>),
    Environment(Environment),
}

/// Documents that can be parsed, cached, and stamped with the current schema.
trait Document: DeserializeOwned + Clone {
    fn set_schema_version(&mut self, version: u32);
    fn into_cached(self) -> CachedKind;
    fn from_cached(cached: &CachedKind) -> Option<&Self>;
}

macro_rules! document {
    ($type:ident, $wrap:expr) => {
        impl Document for $type {
            fn set_schema_version(&mut self, version: u32) {
                self.schema_version = version;
            }

            fn into_cached(self) -> CachedKind {
                CachedKind::$type($wrap(self))
            }

            fn from_cached(cached: &CachedKind) -> Option<&Self> {
                match cached {
                    CachedKind::$type(document) => Some(document),
                    _ => None,
                }
            }
        }
    };
}

document!(Workspace, std::convert::identity);
document!(Collection, std::convert::identity);
document!(Request, Box::new);
document!(Environment, std::convert::identity);

/// A workspace directory plus a parse cache keyed by file identity, so a
/// reload after one save re-parses one file rather than the whole tree.
#[derive(Clone, Debug)]
pub struct WorkspaceStore {
    root: PathBuf,
    cache: Arc<Mutex<HashMap<PathBuf, CachedDocument>>>,
}

impl PartialEq for WorkspaceStore {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}

impl Eq for WorkspaceStore {}

impl WorkspaceStore {
    fn at(root: PathBuf) -> Self {
        Self {
            root,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Creates a workspace without overwriting an existing manifest.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when the root cannot be created, already
    /// contains a workspace, or the manifest cannot be written atomically.
    pub fn create(root: impl Into<PathBuf>, workspace: &Workspace) -> Result<Self, StorageError> {
        let store = Self::at(root.into());
        fs::create_dir_all(&store.root).map_err(|source| {
            StorageError::io("create workspace directory", &store.root, source)
        })?;

        let manifest_path = store.workspace_path();
        if manifest_path.exists() {
            return Err(StorageError::AlreadyExists {
                path: manifest_path,
            });
        }

        fs::create_dir_all(store.collections_path()).map_err(|source| {
            StorageError::io(
                "create collections directory",
                store.collections_path(),
                source,
            )
        })?;
        fs::create_dir_all(store.environments_path()).map_err(|source| {
            StorageError::io(
                "create environments directory",
                store.environments_path(),
                source,
            )
        })?;
        store.write(&manifest_path, workspace, Durability::Full)?;

        Ok(store)
    }

    /// Opens a workspace after validating its manifest.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when the manifest is absent, corrupt, too
    /// large, or uses an unsupported schema version.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let store = Self::at(root.into());
        store.read::<Workspace>(&store.workspace_path())?;
        Ok(store)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Loads a deterministic snapshot of every readable workspace document.
    ///
    /// Documents that fail to parse, use a newer schema, or disagree with
    /// their path are reported in [`WorkspaceSnapshot::problems`] and skipped.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] only when the manifest itself cannot be read
    /// or a workspace directory cannot be listed.
    pub fn load(&self) -> Result<WorkspaceSnapshot, StorageError> {
        self.load_with_migrations().map(|(snapshot, _)| snapshot)
    }

    /// Atomically saves one workspace document.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when the document has a stale schema, its
    /// collection does not exist, or its destination cannot be replaced.
    pub fn save(&self, document: &WorkspaceDocument) -> Result<SaveOutcome, StorageError> {
        match document {
            WorkspaceDocument::Workspace(workspace) => {
                let path = self.workspace_path();
                require_current_schema(workspace.schema_version(), &path)?;
                self.write(&path, workspace, Durability::Full)
            }
            WorkspaceDocument::Collection(collection) => {
                let directory = self.collection_path(&collection.id);
                fs::create_dir_all(directory.join(REQUESTS_DIRECTORY)).map_err(|source| {
                    StorageError::io("create collection directory", &directory, source)
                })?;
                let path = directory.join(COLLECTION_FILE);
                require_current_schema(collection.schema_version(), &path)?;
                self.write(&path, collection, Durability::Fast)
            }
            WorkspaceDocument::Request {
                collection_id,
                request,
            } => {
                let collection_path = self.collection_path(collection_id).join(COLLECTION_FILE);
                if !collection_path.is_file() {
                    return Err(StorageError::MissingCollection {
                        id: collection_id.clone(),
                    });
                }

                let path = self.request_path(collection_id, &request.id);
                require_current_schema(request.schema_version(), &path)?;
                self.write(&path, request, Durability::Fast)
            }
            WorkspaceDocument::Environment(environment) => {
                let path = self.environment_path(&environment.id);
                require_current_schema(environment.schema_version(), &path)?;
                self.write(&path, environment, Durability::Fast)
            }
        }
    }

    /// Rewrites only documents loaded from an older supported schema.
    ///
    /// The migration is restartable: each document is replaced atomically and
    /// a later call skips documents already on the current schema. Documents
    /// that could not be loaded are left untouched.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when discovery, decoding, or an atomic
    /// replacement fails.
    pub fn migrate(&self) -> Result<MigrationReport, StorageError> {
        let (_, migrations) = self.load_with_migrations()?;
        for document in &migrations {
            self.save(document)?;
        }

        Ok(MigrationReport {
            migrated_documents: migrations.len(),
        })
    }

    fn load_with_migrations(
        &self,
    ) -> Result<(WorkspaceSnapshot, Vec<WorkspaceDocument>), StorageError> {
        let (workspace, workspace_migrated): (Workspace, bool) =
            self.read(&self.workspace_path())?;
        let mut migrations = Vec::new();
        let mut problems = Vec::new();
        if workspace_migrated {
            migrations.push(WorkspaceDocument::Workspace(workspace.clone()));
        }

        let mut collections = Vec::new();
        for directory in child_directories(&self.collections_path())? {
            match self.load_collection(&directory, &mut migrations, &mut problems) {
                Ok(snapshot) => collections.push(snapshot),
                Err(error) => problems.push(DocumentProblem::new(&self.root, &error)),
            }
        }
        collections.sort_by(|left, right| left.collection.id.cmp(&right.collection.id));

        let mut environments = Vec::new();
        for environment_file in toml_files(&self.environments_path())? {
            match self.load_environment(&environment_file) {
                Ok((environment, migrated)) => {
                    if migrated {
                        migrations.push(WorkspaceDocument::Environment(environment.clone()));
                    }
                    environments.push(environment);
                }
                Err(error) => problems.push(DocumentProblem::new(&self.root, &error)),
            }
        }
        environments.sort_by(|left, right| left.id.cmp(&right.id));

        Ok((
            WorkspaceSnapshot {
                workspace,
                collections,
                environments,
                problems,
            },
            migrations,
        ))
    }

    fn load_collection(
        &self,
        directory: &Path,
        migrations: &mut Vec<WorkspaceDocument>,
        problems: &mut Vec<DocumentProblem>,
    ) -> Result<CollectionSnapshot, StorageError> {
        let directory_name = file_name(directory)?;
        let expected_id =
            DocumentId::new(directory_name).map_err(|_| StorageError::InvalidDocument {
                path: directory.to_owned(),
                reason: "collection directory is not a valid document ID",
            })?;
        let collection_file = directory.join(COLLECTION_FILE);
        let (collection, collection_migrated): (Collection, bool) = self.read(&collection_file)?;
        require_matching_id(&collection.id, &expected_id, &collection_file)?;
        if collection_migrated {
            migrations.push(WorkspaceDocument::Collection(collection.clone()));
        }

        let mut requests = Vec::new();
        for request_file in toml_files(&directory.join(REQUESTS_DIRECTORY))? {
            match self.load_request(&request_file) {
                Ok((request, migrated)) => {
                    if migrated {
                        migrations.push(WorkspaceDocument::Request {
                            collection_id: collection.id.clone(),
                            request: request.clone(),
                        });
                    }
                    requests.push(request);
                }
                Err(error) => problems.push(DocumentProblem::new(&self.root, &error)),
            }
        }
        requests.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(CollectionSnapshot {
            collection,
            requests,
        })
    }

    fn load_request(&self, path: &Path) -> Result<(Request, bool), StorageError> {
        let expected_id = document_id_from_file(path)?;
        let (request, migrated): (Request, bool) = self.read(path)?;
        require_matching_id(&request.id, &expected_id, path)?;
        Ok((request, migrated))
    }

    fn load_environment(&self, path: &Path) -> Result<(Environment, bool), StorageError> {
        let expected_id = document_id_from_file(path)?;
        let (environment, migrated): (Environment, bool) = self.read(path)?;
        require_matching_id(&environment.id, &expected_id, path)?;
        Ok((environment, migrated))
    }

    /// Reads one document, serving it from the cache when the file's
    /// modification time and size are unchanged.
    fn read<T: Document>(&self, path: &Path) -> Result<(T, bool), StorageError> {
        let metadata = fs::symlink_metadata(path).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                StorageError::Missing {
                    path: path.to_owned(),
                }
            } else {
                StorageError::io("read document metadata", path, source)
            }
        })?;
        if !metadata.file_type().is_file() {
            return Err(StorageError::InvalidDocument {
                path: path.to_owned(),
                reason: "document must be a regular file",
            });
        }
        if metadata.len() > MAX_DOCUMENT_BYTES {
            return Err(StorageError::DocumentTooLarge {
                path: path.to_owned(),
                bytes: metadata.len(),
            });
        }
        let modified = metadata
            .modified()
            .map_err(|source| StorageError::io("read document metadata", path, source))?;
        if let Some(entry) = self.lock_cache().get(path)
            && entry.modified == modified
            && entry.len == metadata.len()
            && let Some(document) = T::from_cached(&entry.document)
        {
            return Ok((document.clone(), entry.migrated));
        }

        let source = fs::read_to_string(path)
            .map_err(|source| StorageError::io("read document", path, source))?;
        let (document, migrated) = parse_document::<T>(path, &source)?;
        self.lock_cache().insert(
            path.to_owned(),
            CachedDocument {
                modified,
                len: metadata.len(),
                migrated,
                document: document.clone().into_cached(),
            },
        );
        Ok((document, migrated))
    }

    fn write<T: Document + Serialize>(
        &self,
        path: &Path,
        document: &T,
        durability: Durability,
    ) -> Result<SaveOutcome, StorageError> {
        let outcome = write_document(path, document, durability)?;
        if outcome != SaveOutcome::Unchanged
            && let Ok(metadata) = fs::symlink_metadata(path)
            && let Ok(modified) = metadata.modified()
        {
            self.lock_cache().insert(
                path.to_owned(),
                CachedDocument {
                    modified,
                    len: metadata.len(),
                    migrated: false,
                    document: document.clone().into_cached(),
                },
            );
        }
        Ok(outcome)
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, CachedDocument>> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn workspace_path(&self) -> PathBuf {
        self.root.join(WORKSPACE_FILE)
    }

    fn collections_path(&self) -> PathBuf {
        self.root.join(COLLECTIONS_DIRECTORY)
    }

    fn collection_path(&self, id: &DocumentId) -> PathBuf {
        self.collections_path().join(id.as_str())
    }

    fn request_path(&self, collection_id: &DocumentId, request_id: &DocumentId) -> PathBuf {
        self.collection_path(collection_id)
            .join(REQUESTS_DIRECTORY)
            .join(format!("{}.toml", request_id.as_str()))
    }

    fn environments_path(&self) -> PathBuf {
        self.root.join(ENVIRONMENTS_DIRECTORY)
    }

    fn environment_path(&self, id: &DocumentId) -> PathBuf {
        self.environments_path()
            .join(format!("{}.toml", id.as_str()))
    }
}

#[derive(Debug)]
pub enum StorageError {
    AlreadyExists {
        path: PathBuf,
    },
    Missing {
        path: PathBuf,
    },
    MissingCollection {
        id: DocumentId,
    },
    DocumentTooLarge {
        path: PathBuf,
        bytes: u64,
    },
    InvalidDocument {
        path: PathBuf,
        reason: &'static str,
    },
    InvalidToml {
        path: PathBuf,
        line: Option<usize>,
        column: Option<usize>,
    },
    UnsupportedSchema {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    Serialization {
        path: PathBuf,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl StorageError {
    fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists { path } => {
                write!(formatter, "workspace already exists at {}", path.display())
            }
            Self::Missing { path } => write!(formatter, "missing document at {}", path.display()),
            Self::MissingCollection { id } => {
                write!(
                    formatter,
                    "collection {id} must exist before saving its request"
                )
            }
            Self::DocumentTooLarge { path, bytes } => write!(
                formatter,
                "document at {} is too large ({bytes} bytes)",
                path.display()
            ),
            Self::InvalidDocument { path, reason } => {
                write!(
                    formatter,
                    "invalid document at {}: {reason}",
                    path.display()
                )
            }
            Self::InvalidToml { path, line, column } => {
                write!(formatter, "invalid TOML at {}", path.display())?;
                if let (Some(line), Some(column)) = (line, column) {
                    write!(formatter, ":{line}:{column}")?;
                }
                Ok(())
            }
            Self::UnsupportedSchema {
                path,
                found,
                supported,
            } => write!(
                formatter,
                "unsupported schema {found} at {}; current schema is {supported}",
                path.display()
            ),
            Self::Serialization { path } => {
                write!(
                    formatter,
                    "could not serialize document for {}",
                    path.display()
                )
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "could not {operation} at {}: {source}",
                path.display()
            ),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn require_current_schema(version: u32, path: &Path) -> Result<(), StorageError> {
    if version == CURRENT_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(StorageError::UnsupportedSchema {
            path: path.to_owned(),
            found: version,
            supported: CURRENT_SCHEMA_VERSION,
        })
    }
}

fn require_matching_id(
    actual: &DocumentId,
    expected: &DocumentId,
    path: &Path,
) -> Result<(), StorageError> {
    if actual == expected {
        Ok(())
    } else {
        Err(StorageError::InvalidDocument {
            path: path.to_owned(),
            reason: "document ID does not match its path",
        })
    }
}

/// Parses a document straight into its type. The schema version is read
/// from the leading top-level keys first, so newer documents are refused
/// before their unknown fields turn into a misleading TOML error.
fn parse_document<T: Document>(path: &Path, source: &str) -> Result<(T, bool), StorageError> {
    let version = peek_schema_version(source).map_err(|reason| StorageError::InvalidDocument {
        path: path.to_owned(),
        reason,
    })?;
    let migrated = match version {
        None | Some(0..=2) => true,
        Some(CURRENT_SCHEMA_VERSION) => false,
        Some(found) => {
            return Err(StorageError::UnsupportedSchema {
                path: path.to_owned(),
                found,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
    };

    let mut document: T = toml::from_str(source).map_err(|error| {
        let (line, column) = error
            .span()
            .map(|span| line_and_column(source, span.start))
            .map_or((None, None), |(line, column)| (Some(line), Some(column)));
        StorageError::InvalidToml {
            path: path.to_owned(),
            line,
            column,
        }
    })?;
    if migrated {
        document.set_schema_version(CURRENT_SCHEMA_VERSION);
    }
    Ok((document, migrated))
}

/// Finds `schema_version` among the root keys, which TOML requires to appear
/// before any `[table]` header. Returns `Ok(None)` when absent.
fn peek_schema_version(source: &str) -> Result<Option<u32>, &'static str> {
    for line in source.lines() {
        let line = line.trim_start();
        if line.starts_with('[') {
            break;
        }
        let Some(rest) = line.strip_prefix("schema_version") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(value) = rest.strip_prefix('=') else {
            continue;
        };
        let value = value.split('#').next().unwrap_or_default().trim();
        return match value.parse::<i64>() {
            Ok(version) => u32::try_from(version)
                .map(Some)
                .map_err(|_| "schema_version must be a non-negative 32-bit integer"),
            Err(_) => Err("schema_version must be an integer"),
        };
    }
    Ok(None)
}

fn write_document<T: Serialize>(
    path: &Path,
    document: &T,
    durability: Durability,
) -> Result<SaveOutcome, StorageError> {
    let mut contents =
        toml::to_string_pretty(document).map_err(|_| StorageError::Serialization {
            path: path.to_owned(),
        })?;
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    atomic_write_if_changed(path, contents.as_bytes(), durability)
}

fn atomic_write_if_changed(
    path: &Path,
    contents: &[u8],
    durability: Durability,
) -> Result<SaveOutcome, StorageError> {
    let existed = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(StorageError::InvalidDocument {
                    path: path.to_owned(),
                    reason: "destination must be a regular file",
                });
            }
            let same_length = u64::try_from(contents.len()).is_ok_and(|len| len == metadata.len());
            if same_length
                && fs::read(path)
                    .map_err(|source| StorageError::io("compare document", path, source))?
                    == contents
            {
                return Ok(SaveOutcome::Unchanged);
            }
            true
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => false,
        Err(source) => return Err(StorageError::io("inspect destination", path, source)),
    };

    let parent = path.parent().ok_or_else(|| StorageError::InvalidDocument {
        path: path.to_owned(),
        reason: "document path must have a parent directory",
    })?;
    fs::create_dir_all(parent)
        .map_err(|source| StorageError::io("create document directory", parent, source))?;
    let (temporary_path, mut temporary_file) = create_temporary_file(parent, path)?;

    let write_result = (|| -> io::Result<()> {
        temporary_file.write_all(contents)?;
        match durability {
            Durability::Full => temporary_file.sync_all()?,
            Durability::Fast => temporary_file.sync_data()?,
        }
        drop(temporary_file);
        fs::rename(&temporary_path, path)?;
        if durability == Durability::Full {
            sync_directory(parent)?;
        }
        Ok(())
    })();

    if let Err(source) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(StorageError::io(
            "replace document atomically",
            path,
            source,
        ));
    }

    Ok(if existed {
        SaveOutcome::Updated
    } else {
        SaveOutcome::Created
    })
}

fn create_temporary_file(
    parent: &Path,
    destination: &Path,
) -> Result<(PathBuf, File), StorageError> {
    let destination_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document");

    for _ in 0..32 {
        let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".{destination_name}.wirebolt-{}-{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(StorageError::io("create temporary document", path, source)),
        }
    }

    Err(StorageError::io(
        "create temporary document",
        parent,
        io::Error::new(io::ErrorKind::AlreadyExists, "temporary name collision"),
    ))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn child_directories(path: &Path) -> Result<Vec<PathBuf>, StorageError> {
    entries_matching(path, |entry_path, file_type| {
        file_type.is_dir() && !is_hidden(entry_path)
    })
}

fn toml_files(path: &Path) -> Result<Vec<PathBuf>, StorageError> {
    entries_matching(path, |entry_path, file_type| {
        file_type.is_file()
            && entry_path
                .extension()
                .is_some_and(|extension| extension == "toml")
            && !is_hidden(entry_path)
    })
}

fn entries_matching(
    path: &Path,
    include: impl Fn(&Path, fs::FileType) -> bool,
) -> Result<Vec<PathBuf>, StorageError> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(StorageError::io("read workspace directory", path, source)),
    };

    let mut paths = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|source| StorageError::io("read directory entry", path, source))?;
        let entry_path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| StorageError::io("read entry type", &entry_path, source))?;
        if include(&entry_path, file_type) {
            paths.push(entry_path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

fn file_name(path: &Path) -> Result<String, StorageError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| StorageError::InvalidDocument {
            path: path.to_owned(),
            reason: "document path must be valid UTF-8",
        })
}

fn document_id_from_file(path: &Path) -> Result<DocumentId, StorageError> {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| StorageError::InvalidDocument {
            path: path.to_owned(),
            reason: "document filename must be valid UTF-8",
        })?;
    DocumentId::new(stem).map_err(|_| StorageError::InvalidDocument {
        path: path.to_owned(),
        reason: "document filename is not a valid document ID",
    })
}

fn line_and_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix.chars().count() + 1, |(_, tail)| {
            tail.chars().count() + 1
        });
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peeks_the_root_schema_version_only() {
        assert_eq!(peek_schema_version("name = \"x\"\n"), Ok(None));
        assert_eq!(
            peek_schema_version("# comment\nschema_version = 3 # current\nname = \"x\"\n"),
            Ok(Some(3))
        );
        assert_eq!(
            peek_schema_version("name = \"x\"\n[body]\nschema_version = 9\n"),
            Ok(None)
        );
        assert!(peek_schema_version("schema_version = \"3\"\n").is_err());
        assert!(peek_schema_version("schema_version = -1\n").is_err());
    }
}
