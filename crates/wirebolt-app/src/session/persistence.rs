//! Workspace storage, import/export and Git orchestration. Platform credentials
//! enter through a trait; all workspace operations call wirebolt-core directly.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use wirebolt_core::{
    Collection, DocumentId, DocumentProblem, DocumentProblemKind, Environment, EnvironmentVariable,
    GitChange, GitDelta, GitError, GitErrorKind, GitOperation, GitOperationOutcome, GitStatus,
    GitWorkspace, Group, ImportEngine, ImportFormat, ProxyMode, Request, RequestAuthentication,
    RequestBody, RequestHeader, RequestValueField, SecretName, TransportSettings, ValueSource,
    Workspace, WorkspaceDocument, WorkspaceSnapshot, WorkspaceStore,
};
pub trait WorkspaceSecrets: fmt::Debug + Send + Sync {
    /// # Errors
    /// Returns a storage error if credentials cannot be read.
    fn read(&self, name: &str) -> Result<Option<String>, WorkspaceError>;
    /// # Errors
    /// Returns a storage error if credentials cannot be saved.
    fn save(&self, name: &str, value: &str) -> Result<(), WorkspaceError>;
    /// # Errors
    /// Returns a storage error if credentials cannot be deleted.
    fn remove(&self, name: &str) -> Result<(), WorkspaceError>;
}
const fn empty_body() -> RequestBody {
    RequestBody::Empty
}
const fn default_inherits_workspace_transport() -> bool {
    true
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceError {
    OperationFailed { reason: String },
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationFailed { reason } => formatter.write_str(reason),
        }
    }
}

impl Error for WorkspaceError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceGitError {
    OperationFailed { kind: String, reason: String },
}

impl fmt::Display for WorkspaceGitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationFailed { reason, .. } => formatter.write_str(reason),
        }
    }
}

impl Error for WorkspaceGitError {}

#[derive(Debug)]
pub struct SessionPersistence {
    pub store: WorkspaceStore,
    secrets: Arc<dyn WorkspaceSecrets>,
    version: AtomicU64,
}

#[derive(Debug, Serialize)]
struct WorkspaceSnapshotDocument<'a> {
    name: &'a str,
    proxy: &'a Option<ProxyMode>,
    transport: &'a TransportSettings,
    collections: Vec<CollectionSnapshotDocument<'a>>,
    environments: Vec<EnvironmentSnapshotDocument<'a>>,
    problems: Vec<DocumentProblemDocument<'a>>,
}

#[derive(Debug, Serialize)]
struct CollectionSnapshotDocument<'a> {
    id: &'a str,
    name: &'a str,
    order: i64,
    groups: &'a [Group],
    requests: Vec<RequestSnapshotDocument<'a>>,
}

#[derive(Debug, Serialize)]
struct RequestSnapshotDocument<'a> {
    id: &'a str,
    name: &'a str,
    group_id: Option<&'a str>,
    order: i64,
    method: &'a str,
    url: &'a str,
    web_socket: bool,
    note: &'a str,
    query: &'a [RequestValueField],
    headers: &'a [RequestHeader],
    authentication: &'a RequestAuthentication,
    body: &'a RequestBody,
    proxy: &'a Option<ProxyMode>,
    transport: &'a TransportSettings,
    inherits_workspace_transport: bool,
}

#[derive(Debug, Serialize)]
struct EnvironmentSnapshotDocument<'a> {
    id: &'a str,
    name: &'a str,
    variables: &'a [EnvironmentVariable],
}

#[derive(Debug, Serialize)]
struct DocumentProblemDocument<'a> {
    path: String,
    kind: &'static str,
    reason: &'a str,
}

#[derive(Debug, Serialize)]
struct GitStatusDocument<'a> {
    branch: &'a Option<String>,
    upstream: &'a Option<String>,
    ahead: u64,
    behind: u64,
    revision: &'a Option<String>,
    changes: Vec<GitChangeDocument<'a>>,
    merging: bool,
}

#[derive(Debug, Serialize)]
struct GitChangeDocument<'a> {
    path: &'a str,
    previous_path: &'a Option<String>,
    staged: &'static str,
    unstaged: &'static str,
    conflicted: bool,
}

#[derive(Debug, Serialize)]
struct GitOperationDocument<'a> {
    outcome: &'static str,
    revision: &'a Option<String>,
    status: GitStatusDocument<'a>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SavedRequestDocument {
    id: String,
    name: String,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    order: i64,
    method: String,
    url: String,
    #[serde(default)]
    web_socket: bool,
    #[serde(default)]
    note: String,
    #[serde(default)]
    query: Vec<RequestValueField>,
    #[serde(default)]
    headers: Vec<RequestHeader>,
    #[serde(default)]
    authentication: RequestAuthentication,
    #[serde(default = "empty_body")]
    body: RequestBody,
    #[serde(default)]
    proxy: Option<ProxyMode>,
    #[serde(default)]
    transport: TransportSettings,
    #[serde(default = "default_inherits_workspace_transport")]
    inherits_workspace_transport: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SavedEnvironmentDocument {
    id: String,
    name: String,
    #[serde(default)]
    variables: Vec<EnvironmentVariable>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
enum WorkspaceCommandDocument {
    SaveWorkspaceProxy {
        proxy: Option<ProxyMode>,
    },
    SaveWorkspaceSettings {
        transport: TransportSettings,
    },
    RenameWorkspace {
        name: String,
    },
    CreateCollection {
        id: String,
        name: String,
        order: i64,
    },
    RenameCollection {
        id: String,
        name: String,
    },
    DeleteCollection {
        id: String,
    },
    CreateGroup {
        collection_id: String,
        group: SavedGroupDocument,
    },
    RenameGroup {
        collection_id: String,
        id: String,
        name: String,
    },
    DeleteGroup {
        collection_id: String,
        id: String,
    },
    ReorderChildren {
        collection_id: String,
        parent_id: Option<String>,
        items: Vec<String>,
    },
    MoveGroup {
        collection_id: String,
        id: String,
        parent_id: Option<String>,
        order: i64,
    },
    SaveRequest {
        collection_id: String,
        request: Box<SavedRequestDocument>,
    },
    DeleteRequest {
        collection_id: String,
        id: String,
    },
    DuplicateRequest {
        collection_id: String,
        id: String,
        new_id: String,
        name: String,
    },
    MoveRequest {
        from_collection_id: String,
        request_id: String,
        to_collection_id: String,
        group_id: Option<String>,
        order: i64,
    },
    SaveEnvironment {
        environment: SavedEnvironmentDocument,
    },
    DeleteEnvironment {
        id: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedGroupDocument {
    id: String,
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    order: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionWorkspaceNodeKind {
    Workspace,
    Collection,
    Group,
    Request,
    Environment,
}
#[derive(Debug, Serialize)]
struct WorkspaceDeltaDocument {
    version: u64,
    kind: SessionWorkspaceNodeKind,
    affected_ids: Vec<String>,
}

/// What an import creates, shown before and after committing it.
#[derive(Debug, Serialize)]
struct ImportSummaryDocument {
    /// The first collection's name, kept for the preview's single-collection shape.
    collection_name: String,
    collection_names: Vec<String>,
    request_count: usize,
    group_count: usize,
    environments: Vec<ImportedEnvironmentDocument>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ImportedEnvironmentDocument {
    /// Empty in a preview, where nothing has been created yet.
    id: String,
    name: String,
}

impl ImportSummaryDocument {
    fn new(imported: &wirebolt_core::ParsedImport) -> Self {
        let collections = &imported.workspace.collections;
        let collection_names: Vec<String> = collections
            .iter()
            .map(|collection| collection.name.clone())
            .collect();
        Self {
            collection_name: collection_names.first().cloned().unwrap_or_default(),
            collection_names,
            request_count: collections
                .iter()
                .map(|collection| collection.requests.len())
                .sum(),
            group_count: collections
                .iter()
                .map(|collection| collection.groups.len())
                .sum(),
            environments: imported
                .workspace
                .environments
                .iter()
                .map(|environment| ImportedEnvironmentDocument {
                    id: String::new(),
                    name: environment.name.clone(),
                })
                .collect(),
            warnings: imported.warnings().map(str::to_owned).collect(),
        }
    }
}

/// A committed import: the workspace delta plus its summary for the app.
#[derive(Debug, Serialize)]
struct ImportCommitDocument {
    #[serde(flatten)]
    delta: WorkspaceDeltaDocument,
    summary: ImportSummaryDocument,
}

impl SessionPersistence {
    /// Opens an existing workspace or creates an empty one.
    ///
    /// Documents that cannot be migrated are skipped and reported through
    /// the snapshot's `problems`; they never prevent the workspace from
    /// opening.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] when storage cannot be initialized.
    pub fn open_or_create(
        path: String,
        name: String,
        secrets: Arc<dyn WorkspaceSecrets>,
    ) -> Result<Arc<Self>, WorkspaceError> {
        let root = PathBuf::from(path);
        let store = if root.join("wirebolt.toml").is_file() {
            WorkspaceStore::open(root)
        } else {
            WorkspaceStore::create(root, &Workspace::new(name))
        }
        .map_err(|_| WorkspaceError::operation("workspace could not be opened"))?;
        store
            .migrate()
            .map_err(|_| WorkspaceError::operation("workspace could not be migrated"))?;
        Ok(Arc::new(Self {
            store,
            secrets,
            version: AtomicU64::new(0),
        }))
    }

    /// Returns one stable JSON snapshot without exposing filesystem layout.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] when the workspace cannot be loaded.
    pub fn snapshot_json(&self) -> Result<String, WorkspaceError> {
        let snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?;
        serde_json::to_string(&WorkspaceSnapshotDocument::from(&snapshot))
            .map_err(|_| WorkspaceError::operation("workspace snapshot could not be encoded"))
    }

    /// Exports the complete saved workspace with secret references intact.
    ///
    /// # Errors
    /// Returns [`WorkspaceError`] if loading or encoding fails.
    pub fn export_workspace_json(&self) -> Result<String, WorkspaceError> {
        let snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?;
        wirebolt_core::export_legacy_v1_workspace(&snapshot)
            .map_err(|error| WorkspaceError::operation(&error.to_string()))
    }

    /// Exports one collection and all of its requests as a portable JSON document.
    /// Secret material is absent because saved requests contain references only.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] if the collection is absent or cannot be encoded.
    pub fn export_collection_json(&self, id: &str) -> Result<String, WorkspaceError> {
        let id = document_id(id.to_owned())?;
        let snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?
            .collections
            .into_iter()
            .find(|collection| collection.collection.id == id)
            .ok_or_else(|| WorkspaceError::operation("collection does not exist"))?;
        wirebolt_core::export_legacy_v1_collection(&snapshot.collection, &snapshot.requests)
            .map_err(|error| WorkspaceError::operation(&error.to_string()))
    }

    /// Exports one saved request as portable JSON with secret references intact.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] if the request is absent or cannot be encoded.
    pub fn export_request_json(
        &self,
        collection_id: &str,
        id: &str,
    ) -> Result<String, WorkspaceError> {
        let request = self.request(collection_id, id)?;
        wirebolt_core::export_legacy_v1_request(&request)
            .map_err(|error| WorkspaceError::operation(&error.to_string()))
    }

    /// Creates or updates a collection.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] for invalid IDs or storage failures.
    pub fn save_collection(&self, id: String, name: String) -> Result<(), WorkspaceError> {
        let id = document_id(id)?;
        self.store
            .save(&WorkspaceDocument::Collection(Collection::new(id, name)))
            .map(|_| ())
            .map_err(|_| WorkspaceError::operation("collection could not be saved"))
    }

    /// Creates or updates one request from a bridge JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] for invalid input or storage failures.
    pub fn save_request(
        &self,
        collection_id: String,
        request_json: &str,
    ) -> Result<(), WorkspaceError> {
        let collection_id = document_id(collection_id)?;
        let document: SavedRequestDocument = serde_json::from_str(request_json)
            .map_err(|_| WorkspaceError::operation("request document is invalid"))?;
        let request = request_from_document(document)?;
        self.store
            .save(&WorkspaceDocument::Request {
                collection_id,
                request,
            })
            .map(|_| ())
            .map_err(|_| WorkspaceError::operation("request could not be saved"))
    }

    /// Creates or updates one environment from a bridge JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] for invalid input or storage failures.
    pub fn save_environment(&self, environment_json: &str) -> Result<(), WorkspaceError> {
        let document: SavedEnvironmentDocument = serde_json::from_str(environment_json)
            .map_err(|_| WorkspaceError::operation("environment document is invalid"))?;
        let environment =
            Environment::from_rows(document_id(document.id)?, document.name, document.variables);
        self.store
            .save(&WorkspaceDocument::Environment(environment))
            .map(|_| ())
            .map_err(|_| WorkspaceError::operation("environment could not be saved"))
    }

    /// Applies one versioned workspace mutation and returns a compact delta.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] when the command is malformed, references
    /// a missing document, creates a group cycle, or cannot be persisted.
    pub fn apply_workspace_command(&self, command_json: &str) -> Result<String, WorkspaceError> {
        let command: WorkspaceCommandDocument = serde_json::from_str(command_json)
            .map_err(|_| WorkspaceError::operation("workspace command is invalid"))?;
        let (kind, affected_ids) = self.apply_workspace_command_document(command)?;
        let delta = WorkspaceDeltaDocument {
            version: self.version.fetch_add(1, Ordering::Relaxed) + 1,
            kind,
            affected_ids,
        };
        serde_json::to_string(&delta)
            .map_err(|_| WorkspaceError::operation("workspace delta could not be encoded"))
    }

    /// Parses an import fully and returns its metadata without mutating storage.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] for unsupported formats or invalid source.
    pub fn preview_import(&self, format: &str, source: &str) -> Result<String, WorkspaceError> {
        let imported = parse_import(format, source, None)?;
        serde_json::to_string(&ImportSummaryDocument::new(&imported))
            .map_err(|_| WorkspaceError::operation("import preview could not be encoded"))
    }

    /// Commits a fully parsed import: its collections, environments and
    /// Keychain credentials, or none of them.
    ///
    /// Returns the workspace delta with a `summary` of what was created and
    /// every import warning.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] with the import's user-facing reason
    /// for parse failures, or a storage failure.
    pub fn commit_import(&self, format: &str, source: &str) -> Result<String, WorkspaceError> {
        self.commit_imported(parse_import(format, source, None)?)
    }

    /// Imports a file under its filename, preserving every exported folder. A
    /// Wirebolt workspace export creates each of its collections and environments.
    ///
    /// # Errors
    /// Returns [`WorkspaceError`] with the import's user-facing reason
    /// for invalid source, or a storage failure.
    pub fn commit_import_file(
        &self,
        format: &str,
        source: &str,
        name: &str,
    ) -> Result<String, WorkspaceError> {
        self.commit_imported(parse_import(format, source, Some(name))?)
    }

    /// Reads a credential only for the authentication editor, never diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] for invalid names or Keychain failures.
    pub fn read_secret(&self, name: &str) -> Result<Option<String>, WorkspaceError> {
        self.secrets.read(name)
    }

    /// Stores secret material in Keychain, never in the workspace.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] for invalid names or Keychain failures.
    pub fn save_secret(&self, name: &str, value: &str) -> Result<(), WorkspaceError> {
        self.secrets.save(name, value)
    }

    /// Removes a credential Wirebolt created once nothing references it. A missing item
    /// counts as removed.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] for invalid names or Keychain failures.
    pub fn delete_secret(&self, name: &str) -> Result<(), WorkspaceError> {
        self.secrets.remove(name)
    }

    /// Returns a read-only Git status snapshot without fetching.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceGitError`] when this workspace is not a repository or
    /// Git cannot inspect it.
    pub fn git_status_json(&self) -> Result<String, WorkspaceGitError> {
        let status = self.git_workspace()?.status()?;
        encode_git_document(&GitStatusDocument::from(&status))
    }

    /// Pulls the configured upstream explicitly and leaves conflicts intact.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceGitError`] when Git rejects or cannot run the pull.
    pub fn git_pull_json(&self) -> Result<String, WorkspaceGitError> {
        let operation = self.git_workspace()?.pull()?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Commits only Wirebolt-managed workspace documents.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceGitError`] for invalid messages, missing Git identity,
    /// or another failed Git operation.
    pub fn git_commit_json(&self, message: &str) -> Result<String, WorkspaceGitError> {
        let operation = self.git_workspace()?.commit(message)?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Pushes the current branch using the user's Git credentials.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceGitError`] when the repository has no usable branch or
    /// remote, authentication fails, or the update is rejected.
    pub fn git_push_json(&self) -> Result<String, WorkspaceGitError> {
        let operation = self.git_workspace()?.push()?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Aborts the merge left behind by a conflicted pull.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceGitError`] when no merge is in progress or Git cannot abort it.
    pub fn git_abort_merge_json(&self) -> Result<String, WorkspaceGitError> {
        let operation = self.git_workspace()?.abort_merge()?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Makes the workspace folder the root of a new Git repository and
    /// returns its status.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceGitError`] when the workspace is inside another
    /// repository or Git cannot initialize it.
    pub fn git_initialize_json(&self) -> Result<String, WorkspaceGitError> {
        let status = GitWorkspace::initialize(self.store.root())?.status()?;
        encode_git_document(&GitStatusDocument::from(&status))
    }
}

impl SessionPersistence {
    /// Creates every imported collection, environment and credential, or none of them.
    fn commit_imported(
        &self,
        imported: wirebolt_core::ParsedImport,
    ) -> Result<String, WorkspaceError> {
        let sequence = self.version.load(Ordering::Relaxed) + 1;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| WorkspaceError::operation("import identifier could not be created"))?
            .as_nanos();
        let prefix = format!("import-{timestamp:x}-{:x}-{sequence:x}", std::process::id());
        let mut summary = ImportSummaryDocument::new(&imported);
        let mut created = CreatedImport::default();
        if let Err(error) = self.save_imported(&prefix, imported, &mut created) {
            for id in &created.collections {
                let _ = self.store.delete_collection(id);
            }
            for id in &created.environments {
                let _ = self.store.delete_environment(id);
            }
            for name in created.secrets {
                let _ = self.secrets.remove(name.as_str());
            }
            return Err(error);
        }
        for (environment, id) in summary.environments.iter_mut().zip(&created.environments) {
            environment.id = id.to_string();
        }

        let delta = WorkspaceDeltaDocument {
            version: self.version.fetch_add(1, Ordering::Relaxed) + 1,
            kind: SessionWorkspaceNodeKind::Collection,
            affected_ids: created
                .collections
                .iter()
                .map(ToString::to_string)
                .collect(),
        };
        serde_json::to_string(&ImportCommitDocument { delta, summary })
            .map_err(|_| WorkspaceError::operation("import delta could not be encoded"))
    }

    fn save_imported(
        &self,
        prefix: &str,
        imported: wirebolt_core::ParsedImport,
        created: &mut CreatedImport,
    ) -> Result<(), WorkspaceError> {
        let wirebolt_core::ParsedImport { workspace, secrets } = imported;
        let wirebolt_core::ImportedWorkspace {
            collections,
            environments,
            mut request_settings,
        } = workspace;
        // Placeholder references become unique Keychain names for this import.
        let mut keychain_names = BTreeMap::new();
        for secret in secrets {
            let name = SecretName::new(format!("{prefix}-{}", secret.reference))
                .map_err(|_| WorkspaceError::operation("import credential name is invalid"))?;
            self.secrets
                .save(name.as_str(), &secret.material)
                .map_err(|_| WorkspaceError::operation("import credentials could not be saved"))?;
            created.secrets.push(name.clone());
            keychain_names.insert(secret.reference, name);
        }
        for (index, imported) in collections.into_iter().enumerate() {
            let collection_id = document_id(format!("{prefix}-{index}"))?;
            self.save_imported_collection(
                &collection_id,
                imported,
                &mut request_settings,
                &keychain_names,
                created,
            )?;
        }
        let has_global = environments.iter().any(|environment| environment.global)
            && self
                .store
                .load()
                .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?
                .environments
                .iter()
                .any(|environment| environment.id.as_str() == wirebolt_core::GLOBAL_ENVIRONMENT_ID);
        for (index, environment) in environments.into_iter().enumerate() {
            // Never replace this workspace's own global environment.
            let id = if environment.global && !has_global {
                wirebolt_core::GLOBAL_ENVIRONMENT_ID.to_owned()
            } else {
                format!("{prefix}-env-{index}")
            };
            let mut variables = environment.variables;
            for variable in &mut variables {
                if let ValueSource::Secret { secret } = &mut variable.value
                    && let Some(name) = keychain_names.get(secret)
                {
                    *secret = name.clone();
                }
            }
            let environment = Environment::from_rows(document_id(id)?, environment.name, variables);
            created.environments.push(environment.id.clone());
            self.store
                .save(&WorkspaceDocument::Environment(environment))
                .map_err(|_| WorkspaceError::operation("import environment could not be saved"))?;
        }
        Ok(())
    }

    fn save_imported_collection(
        &self,
        collection_id: &DocumentId,
        imported: wirebolt_core::ImportedCollection,
        request_settings: &mut std::collections::BTreeMap<
            String,
            wirebolt_core::ImportedRequestSettings,
        >,
        keychain_names: &BTreeMap<SecretName, SecretName>,
        created: &mut CreatedImport,
    ) -> Result<(), WorkspaceError> {
        let mut group_ids = HashMap::new();
        for (index, group) in imported.groups.iter().enumerate() {
            group_ids.insert(
                group.source_id.clone(),
                document_id(format!("group-{index}"))?,
            );
        }
        let mut collection = Collection::new(collection_id.clone(), imported.name);
        collection.groups = imported
            .groups
            .into_iter()
            .map(|group| {
                let parent_id = match group.parent_source_id {
                    Some(parent) => Some(group_ids.get(&parent).cloned().ok_or_else(|| {
                        WorkspaceError::operation("import group hierarchy is invalid")
                    })?),
                    None => None,
                };
                Ok(Group::new(
                    group_ids[&group.source_id].clone(),
                    group.name,
                    parent_id,
                    group.order,
                ))
            })
            .collect::<Result<Vec<_>, WorkspaceError>>()?;
        self.store
            .save(&WorkspaceDocument::Collection(collection))
            .map_err(|_| WorkspaceError::operation("import collection could not be created"))?;
        created.collections.push(collection_id.clone());

        for (index, imported_request) in imported.requests.into_iter().enumerate() {
            let request_id = document_id(format!("request-{index}"))?;
            let group_id = imported_request
                .group_source_id
                .as_ref()
                .and_then(|source| group_ids.get(source))
                .cloned();
            let settings = request_settings.remove(&imported_request.source_id);
            let mut request = imported_request.into_request(request_id, group_id);
            if let Some(settings) = settings {
                settings.apply(&mut request);
            }
            secure_import_authentication(
                self.secrets.as_ref(),
                &mut request.authentication,
                &format!("{collection_id}-{}", request.id),
                keychain_names,
                &mut created.secrets,
            )?;
            self.store
                .save(&WorkspaceDocument::Request {
                    collection_id: collection_id.clone(),
                    request,
                })
                .map_err(|_| WorkspaceError::operation("import request could not be saved"))?;
        }
        Ok(())
    }
}

/// Documents and credentials an import has written so far, for rollback.
#[derive(Default)]
struct CreatedImport {
    collections: Vec<DocumentId>,
    environments: Vec<DocumentId>,
    secrets: Vec<SecretName>,
}

impl SessionPersistence {
    #[expect(
        clippy::too_many_lines,
        reason = "the command router keeps each transactional mutation and its affected IDs in one exhaustive match"
    )]
    fn apply_workspace_command_document(
        &self,
        command: WorkspaceCommandDocument,
    ) -> Result<(SessionWorkspaceNodeKind, Vec<String>), WorkspaceError> {
        match command {
            WorkspaceCommandDocument::SaveWorkspaceProxy { proxy } => {
                let mut workspace = self
                    .store
                    .load()
                    .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?
                    .workspace;
                workspace.proxy = proxy;
                self.store
                    .save(&WorkspaceDocument::Workspace(workspace))
                    .map_err(|_| WorkspaceError::operation("workspace proxy could not be saved"))?;
                Ok((
                    SessionWorkspaceNodeKind::Workspace,
                    vec!["proxy".to_owned()],
                ))
            }
            WorkspaceCommandDocument::SaveWorkspaceSettings { transport } => {
                let mut workspace = self
                    .store
                    .load()
                    .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?
                    .workspace;
                workspace.transport = transport;
                self.store
                    .save(&WorkspaceDocument::Workspace(workspace))
                    .map_err(|_| {
                        WorkspaceError::operation("workspace settings could not be saved")
                    })?;
                Ok((
                    SessionWorkspaceNodeKind::Workspace,
                    vec!["transport".to_owned()],
                ))
            }
            WorkspaceCommandDocument::RenameWorkspace { name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(WorkspaceError::operation(
                        "workspace name must not be empty",
                    ));
                }
                let mut workspace = self
                    .store
                    .load()
                    .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?
                    .workspace;
                name.clone_into(&mut workspace.name);
                self.store
                    .save(&WorkspaceDocument::Workspace(workspace))
                    .map_err(|_| WorkspaceError::operation("workspace could not be renamed"))?;
                Ok((SessionWorkspaceNodeKind::Workspace, vec!["name".to_owned()]))
            }
            WorkspaceCommandDocument::CreateCollection { id, name, order } => {
                let mut collection = Collection::new(document_id(id.clone())?, name);
                collection.order = order;
                self.store
                    .save(&WorkspaceDocument::Collection(collection))
                    .map_err(|_| WorkspaceError::operation("collection could not be created"))?;
                Ok((SessionWorkspaceNodeKind::Collection, vec![id]))
            }
            WorkspaceCommandDocument::RenameCollection { id, name } => {
                let mut collection = self.collection(&id)?;
                collection.name = name;
                self.store
                    .save(&WorkspaceDocument::Collection(collection))
                    .map_err(|_| WorkspaceError::operation("collection could not be renamed"))?;
                Ok((SessionWorkspaceNodeKind::Collection, vec![id]))
            }
            WorkspaceCommandDocument::DeleteCollection { id } => {
                self.store
                    .delete_collection(&document_id(id.clone())?)
                    .map_err(|_| WorkspaceError::operation("collection could not be deleted"))?;
                Ok((SessionWorkspaceNodeKind::Collection, vec![id]))
            }
            WorkspaceCommandDocument::CreateGroup {
                collection_id,
                group,
            } => {
                let mut collection = self.collection(&collection_id)?;
                let group_id = document_id(group.id.clone())?;
                let parent_id = group.parent_id.map(document_id).transpose()?;
                if parent_id.as_ref().is_some_and(|parent| {
                    collection
                        .groups
                        .iter()
                        .all(|existing| &existing.id != parent)
                }) {
                    return Err(WorkspaceError::operation("parent group does not exist"));
                }
                if collection
                    .groups
                    .iter()
                    .any(|existing| existing.id == group_id)
                {
                    return Err(WorkspaceError::operation("group already exists"));
                }
                collection
                    .groups
                    .push(Group::new(group_id, group.name, parent_id, group.order));
                self.save_collection_document(collection)?;
                Ok((
                    SessionWorkspaceNodeKind::Group,
                    vec![collection_id, group.id],
                ))
            }
            WorkspaceCommandDocument::RenameGroup {
                collection_id,
                id,
                name,
            } => {
                let mut collection = self.collection(&collection_id)?;
                let group_id = document_id(id.clone())?;
                let group = collection
                    .groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .ok_or_else(|| WorkspaceError::operation("group does not exist"))?;
                group.name = name;
                self.save_collection_document(collection)?;
                Ok((SessionWorkspaceNodeKind::Group, vec![collection_id, id]))
            }
            WorkspaceCommandDocument::DeleteGroup { collection_id, id } => {
                let mut collection = self.collection(&collection_id)?;
                let group_id = document_id(id.clone())?;
                let descendants = descendant_group_ids(&collection.groups, &group_id);
                if descendants.is_empty() {
                    return Err(WorkspaceError::operation("group does not exist"));
                }
                let snapshot = self
                    .store
                    .load()
                    .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?;
                if let Some(found) = snapshot
                    .collections
                    .iter()
                    .find(|found| found.collection.id == collection.id)
                {
                    for request in &found.requests {
                        if request
                            .group_id
                            .as_ref()
                            .is_some_and(|group| descendants.contains(group))
                        {
                            self.store
                                .delete_request(&collection.id, &request.id)
                                .map_err(|_| {
                                    WorkspaceError::operation("group requests could not be deleted")
                                })?;
                        }
                    }
                }
                collection
                    .groups
                    .retain(|group| !descendants.contains(&group.id));
                self.save_collection_document(collection)?;
                Ok((SessionWorkspaceNodeKind::Group, vec![collection_id, id]))
            }
            WorkspaceCommandDocument::ReorderChildren {
                collection_id,
                parent_id,
                items,
            } => {
                self.reorder_children(&collection_id, parent_id, items)?;
                Ok((SessionWorkspaceNodeKind::Collection, vec![collection_id]))
            }
            WorkspaceCommandDocument::MoveGroup {
                collection_id,
                id,
                parent_id,
                order,
            } => {
                let mut collection = self.collection(&collection_id)?;
                let group_id = document_id(id.clone())?;
                let parent_id = parent_id.map(document_id).transpose()?;
                let descendants = descendant_group_ids(&collection.groups, &group_id);
                if parent_id
                    .as_ref()
                    .is_some_and(|parent| descendants.contains(parent))
                {
                    return Err(WorkspaceError::operation("a group cannot contain itself"));
                }
                if parent_id.as_ref().is_some_and(|parent| {
                    collection
                        .groups
                        .iter()
                        .all(|existing| &existing.id != parent)
                }) {
                    return Err(WorkspaceError::operation("parent group does not exist"));
                }
                let group = collection
                    .groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .ok_or_else(|| WorkspaceError::operation("group does not exist"))?;
                group.parent_id = parent_id;
                group.order = order;
                self.save_collection_document(collection)?;
                Ok((SessionWorkspaceNodeKind::Group, vec![collection_id, id]))
            }
            WorkspaceCommandDocument::SaveRequest {
                collection_id,
                request,
            } => {
                let id = request.id.clone();
                let request = request_from_document(*request)?;
                self.store
                    .save(&WorkspaceDocument::Request {
                        collection_id: document_id(collection_id.clone())?,
                        request,
                    })
                    .map_err(|_| WorkspaceError::operation("request could not be saved"))?;
                Ok((SessionWorkspaceNodeKind::Request, vec![collection_id, id]))
            }
            WorkspaceCommandDocument::DeleteRequest { collection_id, id } => {
                self.store
                    .delete_request(
                        &document_id(collection_id.clone())?,
                        &document_id(id.clone())?,
                    )
                    .map_err(|_| WorkspaceError::operation("request could not be deleted"))?;
                Ok((SessionWorkspaceNodeKind::Request, vec![collection_id, id]))
            }
            WorkspaceCommandDocument::DuplicateRequest {
                collection_id,
                id,
                new_id,
                name,
            } => {
                let snapshot = self.request(&collection_id, &id)?;
                let mut duplicate = snapshot;
                duplicate.id = document_id(new_id.clone())?;
                duplicate.name = name;
                duplicate.order += 1;
                self.store
                    .save(&WorkspaceDocument::Request {
                        collection_id: document_id(collection_id.clone())?,
                        request: duplicate,
                    })
                    .map_err(|_| WorkspaceError::operation("request could not be duplicated"))?;
                Ok((
                    SessionWorkspaceNodeKind::Request,
                    vec![collection_id, id, new_id],
                ))
            }
            WorkspaceCommandDocument::MoveRequest {
                from_collection_id,
                request_id,
                to_collection_id,
                group_id,
                order,
            } => {
                let mut request = self.request(&from_collection_id, &request_id)?;
                request.group_id = group_id.map(document_id).transpose()?;
                request.order = order;
                let destination = document_id(to_collection_id.clone())?;
                self.store
                    .save(&WorkspaceDocument::Request {
                        collection_id: destination,
                        request,
                    })
                    .map_err(|_| WorkspaceError::operation("request could not be moved"))?;
                if from_collection_id != to_collection_id {
                    self.store
                        .delete_request(
                            &document_id(from_collection_id.clone())?,
                            &document_id(request_id.clone())?,
                        )
                        .map_err(|_| {
                            WorkspaceError::operation("source request could not be removed")
                        })?;
                }
                Ok((
                    SessionWorkspaceNodeKind::Request,
                    vec![from_collection_id, to_collection_id, request_id],
                ))
            }
            WorkspaceCommandDocument::SaveEnvironment { environment } => {
                let id = environment.id.clone();
                let environment = Environment::from_rows(
                    document_id(environment.id)?,
                    environment.name,
                    environment.variables,
                );
                self.store
                    .save(&WorkspaceDocument::Environment(environment))
                    .map_err(|_| WorkspaceError::operation("environment could not be saved"))?;
                Ok((SessionWorkspaceNodeKind::Environment, vec![id]))
            }
            WorkspaceCommandDocument::DeleteEnvironment { id } => {
                self.store
                    .delete_environment(&document_id(id.clone())?)
                    .map_err(|_| WorkspaceError::operation("environment could not be deleted"))?;
                Ok((SessionWorkspaceNodeKind::Environment, vec![id]))
            }
        }
    }

    fn reorder_children(
        &self,
        collection_id: &str,
        parent_id: Option<String>,
        items: Vec<String>,
    ) -> Result<(), WorkspaceError> {
        let id = document_id(collection_id.to_owned())?;
        let parent = parent_id.map(document_id).transpose()?;
        let mut snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?
            .collections
            .into_iter()
            .find(|entry| entry.collection.id == id)
            .ok_or_else(|| WorkspaceError::operation("collection does not exist"))?;
        let expected: std::collections::HashSet<String> = snapshot
            .collection
            .groups
            .iter()
            .filter(|group| group.parent_id == parent)
            .map(|group| format!("group:{}", group.id))
            .chain(
                snapshot
                    .requests
                    .iter()
                    .filter(|request| request.group_id == parent)
                    .map(|request| format!("request:{}", request.id)),
            )
            .collect();
        let supplied: std::collections::HashSet<String> = items.iter().cloned().collect();
        if supplied != expected || supplied.len() != items.len() {
            return Err(WorkspaceError::operation(
                "reorder must contain each sibling exactly once",
            ));
        }
        let orders: std::collections::HashMap<String, i64> = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| (item, i64::try_from(index).expect("sibling count fits i64")))
            .collect();
        let mut changes = Vec::new();
        let original_collection = snapshot.collection.clone();
        for group in &mut snapshot.collection.groups {
            if let Some(order) = orders.get(&format!("group:{}", group.id)) {
                group.order = *order;
            }
        }
        if snapshot.collection != original_collection {
            changes.push((
                WorkspaceDocument::Collection(original_collection),
                WorkspaceDocument::Collection(snapshot.collection),
            ));
        }
        for mut request in snapshot.requests {
            if let Some(order) = orders.get(&format!("request:{}", request.id))
                && request.order != *order
            {
                let original = request.clone();
                request.order = *order;
                changes.push((
                    WorkspaceDocument::Request {
                        collection_id: id.clone(),
                        request: original,
                    },
                    WorkspaceDocument::Request {
                        collection_id: id.clone(),
                        request,
                    },
                ));
            }
        }
        for (index, (_, updated)) in changes.iter().enumerate() {
            if self.store.save(updated).is_err() {
                for (original, _) in changes[..index].iter().rev() {
                    self.store
                        .save(original)
                        .map_err(|_| WorkspaceError::operation("reorder rollback failed"))?;
                }
                return Err(WorkspaceError::operation("items could not be reordered"));
            }
        }
        Ok(())
    }

    /// # Errors
    /// Returns a missing collection or storage error.
    pub fn collection(&self, id: &str) -> Result<Collection, WorkspaceError> {
        let id = document_id(id.to_owned())?;
        self.store
            .load()
            .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?
            .collections
            .into_iter()
            .find(|snapshot| snapshot.collection.id == id)
            .map(|snapshot| snapshot.collection)
            .ok_or_else(|| WorkspaceError::operation("collection does not exist"))
    }

    /// # Errors
    /// Returns a missing request or storage error.
    pub fn request(
        &self,
        collection_id: &str,
        request_id: &str,
    ) -> Result<Request, WorkspaceError> {
        let collection_id = document_id(collection_id.to_owned())?;
        let request_id = document_id(request_id.to_owned())?;
        self.store
            .load()
            .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?
            .collections
            .into_iter()
            .find(|snapshot| snapshot.collection.id == collection_id)
            .and_then(|snapshot| {
                snapshot
                    .requests
                    .into_iter()
                    .find(|request| request.id == request_id)
            })
            .ok_or_else(|| WorkspaceError::operation("request does not exist"))
    }

    fn save_collection_document(&self, collection: Collection) -> Result<(), WorkspaceError> {
        self.store
            .save(&WorkspaceDocument::Collection(collection))
            .map(|_| ())
            .map_err(|_| WorkspaceError::operation("collection could not be saved"))
    }

    fn git_workspace(&self) -> Result<GitWorkspace, WorkspaceGitError> {
        GitWorkspace::open(self.store.root()).map_err(Into::into)
    }
}

impl WorkspaceError {
    #[must_use]
    pub fn operation(reason: &str) -> Self {
        Self::OperationFailed {
            reason: reason.to_owned(),
        }
    }
}

impl From<GitError> for WorkspaceGitError {
    fn from(error: GitError) -> Self {
        Self::OperationFailed {
            kind: git_error_kind(error.kind).to_owned(),
            reason: error.to_string(),
        }
    }
}

impl<'a> From<&'a GitStatus> for GitStatusDocument<'a> {
    fn from(status: &'a GitStatus) -> Self {
        Self {
            branch: &status.branch,
            upstream: &status.upstream,
            ahead: status.ahead,
            behind: status.behind,
            revision: &status.revision,
            changes: status.changes.iter().map(Into::into).collect(),
            merging: status.merging,
        }
    }
}

impl<'a> From<&'a GitChange> for GitChangeDocument<'a> {
    fn from(change: &'a GitChange) -> Self {
        Self {
            path: &change.path,
            previous_path: &change.previous_path,
            staged: git_delta(change.staged),
            unstaged: git_delta(change.unstaged),
            conflicted: change.conflicted,
        }
    }
}

impl<'a> From<&'a GitOperation> for GitOperationDocument<'a> {
    fn from(operation: &'a GitOperation) -> Self {
        Self {
            outcome: git_operation_outcome(operation.outcome),
            revision: &operation.revision,
            status: GitStatusDocument::from(&operation.status),
        }
    }
}

fn encode_git_document(document: &impl Serialize) -> Result<String, WorkspaceGitError> {
    serde_json::to_string(document).map_err(|_| WorkspaceGitError::OperationFailed {
        kind: "encoding".to_owned(),
        reason: "Git result could not be encoded".to_owned(),
    })
}

const fn git_delta(delta: GitDelta) -> &'static str {
    match delta {
        GitDelta::None => "none",
        GitDelta::Added => "added",
        GitDelta::Modified => "modified",
        GitDelta::Deleted => "deleted",
        GitDelta::Renamed => "renamed",
        GitDelta::Copied => "copied",
        GitDelta::TypeChanged => "type_changed",
        GitDelta::Untracked => "untracked",
        GitDelta::Unmerged => "unmerged",
    }
}

const fn git_operation_outcome(outcome: GitOperationOutcome) -> &'static str {
    match outcome {
        GitOperationOutcome::NothingToCommit => "nothing_to_commit",
        GitOperationOutcome::Committed => "committed",
        GitOperationOutcome::Updated => "updated",
        GitOperationOutcome::UpToDate => "up_to_date",
        GitOperationOutcome::Pushed => "pushed",
        GitOperationOutcome::Conflicted => "conflicted",
        GitOperationOutcome::MergeAborted => "merge_aborted",
    }
}

const fn git_error_kind(kind: GitErrorKind) -> &'static str {
    match kind {
        GitErrorKind::GitUnavailable => "git_unavailable",
        GitErrorKind::NotRepository => "not_repository",
        GitErrorKind::WorkspaceNotRepositoryRoot => "workspace_not_repository_root",
        GitErrorKind::CommandFailed => "command_failed",
        GitErrorKind::InvalidOutput => "invalid_output",
        GitErrorKind::InvalidCommitMessage => "invalid_commit_message",
        GitErrorKind::IdentityMissing => "identity_missing",
        GitErrorKind::AuthenticationRequired => "authentication_required",
        GitErrorKind::DirtyWorkspace => "dirty_workspace",
        GitErrorKind::MissingUpstream => "missing_upstream",
        GitErrorKind::MissingRemote => "missing_remote",
        GitErrorKind::DetachedHead => "detached_head",
        GitErrorKind::TimedOut => "timed_out",
        GitErrorKind::NoMergeInProgress => "no_merge_in_progress",
    }
}

const fn document_problem_kind(kind: DocumentProblemKind) -> &'static str {
    match kind {
        DocumentProblemKind::InvalidToml => "invalid_toml",
        DocumentProblemKind::InvalidDocument => "invalid_document",
        DocumentProblemKind::UnsupportedSchema => "unsupported_schema",
        DocumentProblemKind::DocumentTooLarge => "document_too_large",
        DocumentProblemKind::Io => "io",
    }
}

fn document_id(value: String) -> Result<DocumentId, WorkspaceError> {
    DocumentId::new(value).map_err(|_| WorkspaceError::operation("document ID is invalid"))
}

fn parse_import(
    format: &str,
    source: &str,
    file_name: Option<&str>,
) -> Result<wirebolt_core::ParsedImport, WorkspaceError> {
    let format =
        ImportFormat::parse(format).map_err(|error| WorkspaceError::operation(error.reason()))?;
    // Import reasons are user-facing and never quote the source.
    ImportEngine::parse_import(format, source, file_name)
        .map_err(|error| WorkspaceError::operation(error.reason()))
}

fn request_from_document(document: SavedRequestDocument) -> Result<Request, WorkspaceError> {
    let mut request = Request::new(
        document_id(document.id)?,
        document.name,
        document.method,
        document.url,
    );
    request.web_socket = document.web_socket;
    request.note = document.note;
    request.group_id = document.group_id.map(document_id).transpose()?;
    request.order = document.order;
    request.query = document.query;
    request.headers = document.headers;
    request.authentication = document.authentication;
    request.body = document.body;
    request.proxy_override = document.proxy;
    request.transport = document.transport;
    request.inherits_workspace_transport = document.inherits_workspace_transport;
    Ok(request)
}

fn descendant_group_ids(groups: &[Group], root: &DocumentId) -> Vec<DocumentId> {
    if groups.iter().all(|group| &group.id != root) {
        return Vec::new();
    }
    let mut result = vec![root.clone()];
    let mut offset = 0;
    while offset < result.len() {
        let parent = result[offset].clone();
        for group in groups {
            if group.parent_id.as_ref() == Some(&parent) && !result.contains(&group.id) {
                result.push(group.id.clone());
            }
        }
        offset += 1;
    }
    result
}

impl<'a> From<&'a WorkspaceSnapshot> for WorkspaceSnapshotDocument<'a> {
    fn from(snapshot: &'a WorkspaceSnapshot) -> Self {
        Self {
            name: &snapshot.workspace.name,
            proxy: &snapshot.workspace.proxy,
            transport: &snapshot.workspace.transport,
            collections: snapshot
                .collections
                .iter()
                .map(|snapshot| CollectionSnapshotDocument {
                    id: snapshot.collection.id.as_str(),
                    name: &snapshot.collection.name,
                    order: snapshot.collection.order,
                    groups: &snapshot.collection.groups,
                    requests: snapshot
                        .requests
                        .iter()
                        .map(RequestSnapshotDocument::from)
                        .collect(),
                })
                .collect(),
            environments: snapshot
                .environments
                .iter()
                .map(|environment| EnvironmentSnapshotDocument {
                    id: environment.id.as_str(),
                    name: &environment.name,
                    variables: &environment.variables,
                })
                .collect(),
            problems: snapshot
                .problems
                .iter()
                .map(DocumentProblemDocument::from)
                .collect(),
        }
    }
}

impl<'a> From<&'a DocumentProblem> for DocumentProblemDocument<'a> {
    fn from(problem: &'a DocumentProblem) -> Self {
        Self {
            path: problem.path.to_string_lossy().into_owned(),
            kind: document_problem_kind(problem.kind),
            reason: &problem.reason,
        }
    }
}

impl<'a> From<&'a Request> for RequestSnapshotDocument<'a> {
    fn from(request: &'a Request) -> Self {
        Self {
            id: request.id.as_str(),
            name: &request.name,
            group_id: request.group_id.as_ref().map(DocumentId::as_str),
            order: request.order,
            method: &request.method,
            url: &request.url,
            web_socket: request.web_socket,
            note: &request.note,
            query: &request.query,
            headers: &request.headers,
            authentication: &request.authentication,
            body: &request.body,
            proxy: &request.proxy_override,
            transport: &request.transport,
            inherits_workspace_transport: request.inherits_workspace_transport,
        }
    }
}

fn secure_import_authentication(
    secrets: &dyn WorkspaceSecrets,
    authentication: &mut RequestAuthentication,
    prefix: &str,
    keychain_names: &BTreeMap<SecretName, SecretName>,
    created: &mut Vec<SecretName>,
) -> Result<(), WorkspaceError> {
    let secret_name = |label: &str| {
        SecretName::new(format!("{prefix}-{label}"))
            .map_err(|_| WorkspaceError::operation("import credential name is invalid"))
    };
    let mut store = |source: &mut ValueSource, label: &str| -> Result<(), WorkspaceError> {
        let ValueSource::Literal(material) = source else {
            return Ok(());
        };
        if material.is_empty() || material.contains("{{") {
            return Ok(());
        }
        let name = secret_name(label)?;
        secrets
            .save(name.as_str(), material)
            .map_err(|_| WorkspaceError::operation("import credentials could not be saved"))?;
        created.push(name.clone());
        *source = ValueSource::secret(name);
        Ok(())
    };
    match authentication {
        RequestAuthentication::Basic { username, password } => {
            store(username, "username")?;
            store(password, "password")?;
        }
        RequestAuthentication::Bearer { token } => store(token, "token")?,
        RequestAuthentication::ApiKey { value, .. } => store(value, "api-key")?,
        RequestAuthentication::Oauth2 { configuration } => {
            configuration.client_secret_reference =
                match keychain_names.get(&configuration.client_secret_reference) {
                    Some(name) => name.clone(),
                    None => secret_name("oauth-client-secret")?,
                };
            configuration.access_token_reference = secret_name("oauth-access-token")?;
        }
        RequestAuthentication::None => {}
    }
    Ok(())
}

use super::conversions::{
    auth_to_core, body_to_core, environment_from_core, field_to_core, group_from_core,
    header_to_core, proxy_from_core, proxy_to_core, request_from_core, transport_from_core,
    transport_to_core, variable_to_core,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionWorkspaceDelta {
    pub version: u64,
    pub kind: SessionWorkspaceNodeKind,
    pub affected_ids: Vec<String>,
}
impl SessionPersistence {
    /// Applies a typed command directly to the core store.
    /// # Errors
    /// Returns the existing workspace operation error.
    pub fn apply_session_command(
        &self,
        command: super::workspace::SessionWorkspaceCommand,
    ) -> Result<SessionWorkspaceDelta, WorkspaceError> {
        use super::workspace::SessionWorkspaceCommand as C;
        use WorkspaceCommandDocument as D;
        let command = match command {
            C::SaveWorkspaceProxy { proxy } => D::SaveWorkspaceProxy {
                proxy: proxy.map(proxy_to_core).transpose()?,
            },
            C::SaveWorkspaceSettings { transport } => D::SaveWorkspaceSettings {
                transport: transport_to_core(transport)?,
            },
            C::RenameWorkspace { name } => D::RenameWorkspace { name },
            C::SaveEnvironment { environment } => D::SaveEnvironment {
                environment: saved_environment(environment)?,
            },
            C::DeleteEnvironment { id } => D::DeleteEnvironment { id },
            C::SaveRequest { location } => D::SaveRequest {
                collection_id: location.collection_id.clone(),
                request: Box::new(saved_request(location)?),
            },
            C::DeleteRequest { collection_id, id } => D::DeleteRequest { collection_id, id },
            C::DuplicateRequest {
                collection_id,
                id,
                new_id,
                name,
            } => D::DuplicateRequest {
                collection_id,
                id,
                new_id,
                name,
            },
            C::CreateGroup {
                collection_id,
                group,
            } => D::CreateGroup {
                collection_id,
                group: saved_group(group),
            },
            C::DeleteGroup { collection_id, id } => D::DeleteGroup { collection_id, id },
            C::CreateCollection { collection } => D::CreateCollection {
                id: collection.id,
                name: collection.name,
                order: collection.order,
            },
            C::DeleteCollection { id } => D::DeleteCollection { id },
            C::RenameGroup {
                collection_id,
                id,
                name,
            } => D::RenameGroup {
                collection_id,
                id,
                name,
            },
            C::RenameCollection { id, name } => D::RenameCollection { id, name },
            C::MoveRequest {
                from_collection_id,
                request_id,
                to_collection_id,
                group_id,
                order,
            } => D::MoveRequest {
                from_collection_id,
                request_id,
                to_collection_id,
                group_id,
                order,
            },
            C::MoveGroup {
                collection_id,
                id,
                parent_id,
                order,
            } => D::MoveGroup {
                collection_id,
                id,
                parent_id,
                order,
            },
            C::ReorderChildren {
                collection_id,
                parent_id,
                items,
            } => D::ReorderChildren {
                collection_id,
                parent_id,
                items,
            },
        };
        let (kind, affected_ids) = self.apply_workspace_command_document(command)?;
        Ok(SessionWorkspaceDelta {
            version: self.version.fetch_add(1, Ordering::Relaxed) + 1,
            kind,
            affected_ids,
        })
    }
    /// Executes a batch without round-tripping each command through the interface.
    /// # Errors
    /// Stops at the first failed command, preserving preceding writes.
    pub fn apply_session_commands(
        &self,
        commands: Vec<super::workspace::SessionWorkspaceCommand>,
    ) -> Result<(), WorkspaceError> {
        for command in commands {
            self.apply_session_command(command)?;
        }
        Ok(())
    }
}

impl SessionPersistence {
    /// Loads typed application records.
    /// # Errors
    /// Returns a storage or projection error.
    pub fn domain_snapshot(&self) -> Result<crate::domain::DomainWorkspace, WorkspaceError> {
        use crate::domain::{DomainCollection, DomainLocation, DomainWorkspace};

        let snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceError::operation("workspace could not be loaded"))?;
        let mut collections = vec![];
        for c in snapshot.collections {
            let mut requests = vec![];
            for r in c.requests {
                let group_id = r.group_id.as_ref().map(ToString::to_string);
                let order = r.order;
                let draft = request_from_core(r);
                requests.push(DomainLocation {
                    collection_id: c.collection.id.to_string(),
                    group_id,
                    order,
                    request: draft,
                });
            }
            collections.push(DomainCollection {
                id: c.collection.id.to_string(),
                name: c.collection.name,
                order: c.collection.order,
                groups: c
                    .collection
                    .groups
                    .into_iter()
                    .map(group_from_core)
                    .collect(),
                requests,
            });
        }
        Ok(DomainWorkspace {
            name: snapshot.workspace.name,
            proxy: snapshot.workspace.proxy.map(proxy_from_core),
            transport: transport_from_core(snapshot.workspace.transport),
            collections,
            environments: snapshot
                .environments
                .into_iter()
                .map(environment_from_core)
                .collect(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionCommandBatchResult {
    pub completed: u64,
    pub failure_reason: Option<String>,
}
impl SessionPersistence {
    pub fn execute_session_commands(
        &self,
        commands: Vec<super::workspace::SessionWorkspaceCommand>,
    ) -> SessionCommandBatchResult {
        let mut completed = 0;
        for command in commands {
            match self.apply_session_command(command) {
                Ok(_) => completed += 1,
                Err(WorkspaceError::OperationFailed { reason }) => {
                    return SessionCommandBatchResult {
                        completed,
                        failure_reason: Some(reason),
                    };
                }
            }
        }
        SessionCommandBatchResult {
            completed,
            failure_reason: None,
        }
    }
}

fn saved_request(
    location: crate::domain::DomainLocation,
) -> Result<SavedRequestDocument, WorkspaceError> {
    let r = location.request;
    let mut request = SavedRequestDocument {
        id: r.id,
        name: r.name,
        group_id: None,
        order: 0,
        method: r.method,
        url: r.url,
        web_socket: r.web_socket,
        note: r.note,
        query: r
            .query
            .into_iter()
            .map(field_to_core)
            .collect::<Result<_, _>>()?,
        headers: r
            .headers
            .into_iter()
            .map(header_to_core)
            .collect::<Result<_, _>>()?,
        authentication: auth_to_core(r.authentication)?,
        body: body_to_core(r.body)?,
        proxy: r.proxy.map(proxy_to_core).transpose()?,
        transport: transport_to_core(r.transport)?,
        inherits_workspace_transport: r.inherits_workspace_transport,
    };
    request.group_id = location.group_id;
    request.order = location.order;
    Ok(request)
}

fn saved_environment(
    e: crate::domain::DomainEnvironment,
) -> Result<SavedEnvironmentDocument, WorkspaceError> {
    Ok(SavedEnvironmentDocument {
        id: e.id,
        name: e.name,
        variables: e
            .variables
            .into_iter()
            .map(variable_to_core)
            .collect::<Result<_, _>>()?,
    })
}
fn saved_group(g: crate::domain::DomainGroup) -> SavedGroupDocument {
    SavedGroupDocument {
        id: g.id,
        name: g.name,
        parent_id: g.parent_id,
        order: g.order,
    }
}
