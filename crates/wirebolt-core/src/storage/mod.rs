mod model;

use std::{
    error::Error,
    fmt, fs,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Serialize, de::DeserializeOwned};
use toml::Value;

pub use model::{
    CURRENT_SCHEMA_VERSION, Collection, CollectionSnapshot, DocumentId, Environment,
    IdentifierError, Request, RequestBody, RequestHeader, SecretName, ValueSource, Workspace,
    WorkspaceDocument, WorkspaceSnapshot,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceStore {
    root: PathBuf,
}

impl WorkspaceStore {
    /// Creates a workspace without overwriting an existing manifest.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when the root cannot be created, already
    /// contains a workspace, or the manifest cannot be written atomically.
    pub fn create(root: impl Into<PathBuf>, workspace: &Workspace) -> Result<Self, StorageError> {
        let store = Self { root: root.into() };
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
        write_document(&manifest_path, workspace)?;

        Ok(store)
    }

    /// Opens a workspace after validating its manifest.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when the manifest is absent, corrupt, too
    /// large, or uses an unsupported schema version.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let store = Self { root: root.into() };
        read_document::<Workspace>(&store.workspace_path())?;
        Ok(store)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Loads a deterministic snapshot of every known workspace document.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for I/O failures, corrupt documents,
    /// unsupported schemas, or IDs that disagree with their paths.
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
                require_current_schema(workspace.schema_version(), &self.workspace_path())?;
                write_document(&self.workspace_path(), workspace)
            }
            WorkspaceDocument::Collection(collection) => {
                let directory = self.collection_path(&collection.id);
                fs::create_dir_all(directory.join(REQUESTS_DIRECTORY)).map_err(|source| {
                    StorageError::io("create collection directory", &directory, source)
                })?;
                let path = directory.join(COLLECTION_FILE);
                require_current_schema(collection.schema_version(), &path)?;
                write_document(&path, collection)
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
                write_document(&path, request)
            }
            WorkspaceDocument::Environment(environment) => {
                let path = self.environment_path(&environment.id);
                require_current_schema(environment.schema_version(), &path)?;
                write_document(&path, environment)
            }
        }
    }

    /// Rewrites only documents loaded from an older supported schema.
    ///
    /// The migration is restartable: each document is replaced atomically and
    /// a later call skips documents already on the current schema.
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
            read_document(&self.workspace_path())?;
        let mut migrations = Vec::new();
        if workspace_migrated {
            migrations.push(WorkspaceDocument::Workspace(workspace.clone()));
        }

        let mut collections = Vec::new();
        for directory in child_directories(&self.collections_path())? {
            let directory_name = file_name(&directory)?;
            let expected_id =
                DocumentId::new(directory_name).map_err(|_| StorageError::InvalidDocument {
                    path: directory.clone(),
                    reason: "collection directory is not a valid document ID",
                })?;
            let collection_file = directory.join(COLLECTION_FILE);
            let (collection, collection_migrated): (Collection, bool) =
                read_document(&collection_file)?;
            require_matching_id(&collection.id, &expected_id, &collection_file)?;
            if collection_migrated {
                migrations.push(WorkspaceDocument::Collection(collection.clone()));
            }

            let mut requests = Vec::new();
            for request_file in toml_files(&directory.join(REQUESTS_DIRECTORY))? {
                let expected_request_id = document_id_from_file(&request_file)?;
                let (request, request_migrated): (Request, bool) = read_document(&request_file)?;
                require_matching_id(&request.id, &expected_request_id, &request_file)?;
                if request_migrated {
                    migrations.push(WorkspaceDocument::Request {
                        collection_id: collection.id.clone(),
                        request: request.clone(),
                    });
                }
                requests.push(request);
            }
            requests.sort_by(|left, right| left.id.cmp(&right.id));
            collections.push(CollectionSnapshot {
                collection,
                requests,
            });
        }
        collections.sort_by(|left, right| left.collection.id.cmp(&right.collection.id));

        let mut environments = Vec::new();
        for environment_file in toml_files(&self.environments_path())? {
            let expected_id = document_id_from_file(&environment_file)?;
            let (environment, environment_migrated): (Environment, bool) =
                read_document(&environment_file)?;
            require_matching_id(&environment.id, &expected_id, &environment_file)?;
            if environment_migrated {
                migrations.push(WorkspaceDocument::Environment(environment.clone()));
            }
            environments.push(environment);
        }
        environments.sort_by(|left, right| left.id.cmp(&right.id));

        Ok((
            WorkspaceSnapshot {
                workspace,
                collections,
                environments,
            },
            migrations,
        ))
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

fn read_document<T: DeserializeOwned>(path: &Path) -> Result<(T, bool), StorageError> {
    let source = read_document_text(path)?;
    let mut value: Value = toml::from_str(&source).map_err(|error| {
        let (line, column) = error
            .span()
            .map(|span| line_and_column(&source, span.start))
            .map_or((None, None), |(line, column)| (Some(line), Some(column)));
        StorageError::InvalidToml {
            path: path.to_owned(),
            line,
            column,
        }
    })?;

    let table = value
        .as_table_mut()
        .ok_or_else(|| StorageError::InvalidDocument {
            path: path.to_owned(),
            reason: "document root must be a TOML table",
        })?;
    let (version, migrated) = match table.get("schema_version") {
        None | Some(Value::Integer(0)) => {
            table.insert(
                "schema_version".to_owned(),
                Value::Integer(i64::from(CURRENT_SCHEMA_VERSION)),
            );
            (CURRENT_SCHEMA_VERSION, true)
        }
        Some(Value::Integer(version)) => {
            let version = u32::try_from(*version).map_err(|_| StorageError::InvalidDocument {
                path: path.to_owned(),
                reason: "schema_version must be a non-negative 32-bit integer",
            })?;
            (version, false)
        }
        Some(_) => {
            return Err(StorageError::InvalidDocument {
                path: path.to_owned(),
                reason: "schema_version must be an integer",
            });
        }
    };

    require_current_schema(version, path)?;
    let document = value
        .try_into::<T>()
        .map_err(|_| StorageError::InvalidToml {
            path: path.to_owned(),
            line: None,
            column: None,
        })?;
    Ok((document, migrated))
}

fn read_document_text(path: &Path) -> Result<String, StorageError> {
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

    fs::read_to_string(path).map_err(|source| StorageError::io("read document", path, source))
}

fn write_document<T: Serialize>(path: &Path, document: &T) -> Result<SaveOutcome, StorageError> {
    let mut contents =
        toml::to_string_pretty(document).map_err(|_| StorageError::Serialization {
            path: path.to_owned(),
        })?;
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    atomic_write_if_changed(path, contents.as_bytes())
}

fn atomic_write_if_changed(path: &Path, contents: &[u8]) -> Result<SaveOutcome, StorageError> {
    let existed = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(StorageError::InvalidDocument {
                    path: path.to_owned(),
                    reason: "destination must be a regular file",
                });
            }
            if fs::read(path)
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
        temporary_file.sync_all()?;
        drop(temporary_file);
        fs::rename(&temporary_path, path)?;
        sync_directory(parent)?;
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
        let file_type = entry
            .file_type()
            .map_err(|source| StorageError::io("read entry type", entry.path(), source))?;
        if include(&entry.path(), file_type) {
            paths.push(entry.path());
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
