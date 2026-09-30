uniffi::setup_scaffolding!();
mod websocket;

use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    ffi::c_void,
    fmt,
    path::PathBuf,
    ptr,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
#[cfg(not(target_vendor = "apple"))]
use wirebolt_core::NoSecrets;
use wirebolt_core::{
    Collection, DocumentId, DocumentProblem, DocumentProblemKind, Environment, EnvironmentVariable,
    GitChange, GitDelta, GitError, GitErrorKind, GitOperation, GitOperationOutcome, GitStatus,
    GitWorkspace, Group, HttpEngine, HttpEngineConfig, HttpVersion, ImportEngine, ImportFormat,
    ProxyMode, ProxyPolicy, Request, RequestAuthentication, RequestBody, RequestHeader,
    RequestIssue, RequestIssueKind, RequestPipeline, RequestValueField, ResolvedProxy,
    RunCancellation, RunError, RunErrorKind, RunHead, RunOptions, SecretName, SecretResolver,
    StreamControl, TransportSettings, ValueSource, Workspace, WorkspaceDocument, WorkspaceSnapshot,
    WorkspaceStore,
};

#[derive(Debug, Eq, PartialEq, uniffi::Record)]
pub struct CoreHandshake {
    pub product: String,
    pub core_version: String,
    pub stream_abi_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct HeaderField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct BridgeRequestDraft {
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderField>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct PreparedRequestSummary {
    pub method: String,
    pub url: String,
    pub header_count: u64,
    pub body_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Error)]
pub enum RequestPreparationError {
    InvalidRequest { reason: String },
}

impl fmt::Display for RequestPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { reason } => formatter.write_str(reason),
        }
    }
}

impl Error for RequestPreparationError {}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Error)]
pub enum WorkspaceBridgeError {
    OperationFailed { reason: String },
}

impl fmt::Display for WorkspaceBridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationFailed { reason } => formatter.write_str(reason),
        }
    }
}

impl Error for WorkspaceBridgeError {}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Error)]
pub enum GitBridgeError {
    OperationFailed { kind: String, reason: String },
}

impl fmt::Display for GitBridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationFailed { reason, .. } => formatter.write_str(reason),
        }
    }
}

impl Error for GitBridgeError {}

#[derive(Debug, uniffi::Object)]
pub struct WorkspaceBridge {
    store: WorkspaceStore,
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

#[derive(Debug, Serialize)]
struct WorkspaceDeltaDocument {
    version: u64,
    kind: &'static str,
    affected_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ImportPreviewDocument<'a> {
    collection_name: &'a str,
    request_count: usize,
    group_count: usize,
    warnings: &'a [String],
}

#[uniffi::export]
impl WorkspaceBridge {
    #[uniffi::constructor]
    /// Opens an existing workspace or creates an empty one.
    ///
    /// Documents that cannot be migrated are skipped and reported through
    /// the snapshot's `problems`; they never prevent the workspace from
    /// opening.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] when storage cannot be initialized.
    pub fn open_or_create(path: String, name: String) -> Result<Arc<Self>, WorkspaceBridgeError> {
        let root = PathBuf::from(path);
        let store = if root.join("wirebolt.toml").is_file() {
            WorkspaceStore::open(root)
        } else {
            WorkspaceStore::create(root, &Workspace::new(name))
        }
        .map_err(|_| WorkspaceBridgeError::operation("workspace could not be opened"))?;
        store
            .migrate()
            .map_err(|_| WorkspaceBridgeError::operation("workspace could not be migrated"))?;
        Ok(Arc::new(Self {
            store,
            version: AtomicU64::new(0),
        }))
    }

    /// Returns one stable JSON snapshot without exposing filesystem layout.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] when the workspace cannot be loaded.
    pub fn snapshot_json(&self) -> Result<String, WorkspaceBridgeError> {
        let snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?;
        serde_json::to_string(&WorkspaceSnapshotDocument::from(&snapshot))
            .map_err(|_| WorkspaceBridgeError::operation("workspace snapshot could not be encoded"))
    }

    /// Exports the complete saved workspace with secret references intact.
    ///
    /// # Errors
    /// Returns [`WorkspaceBridgeError`] if loading or encoding fails.
    pub fn export_workspace_json(&self) -> Result<String, WorkspaceBridgeError> {
        let snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?;
        wirebolt_core::export_legacy_v1_workspace(&snapshot)
            .map_err(|error| WorkspaceBridgeError::operation(&error.to_string()))
    }

    /// Exports one collection and all of its requests as a portable JSON document.
    /// Secret material is absent because saved requests contain references only.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] if the collection is absent or cannot be encoded.
    pub fn export_collection_json(&self, id: &str) -> Result<String, WorkspaceBridgeError> {
        let id = document_id(id.to_owned())?;
        let snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?
            .collections
            .into_iter()
            .find(|collection| collection.collection.id == id)
            .ok_or_else(|| WorkspaceBridgeError::operation("collection does not exist"))?;
        wirebolt_core::export_legacy_v1_collection(&snapshot.collection, &snapshot.requests)
            .map_err(|error| WorkspaceBridgeError::operation(&error.to_string()))
    }

    /// Exports one saved request as portable JSON with secret references intact.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] if the request is absent or cannot be encoded.
    pub fn export_request_json(
        &self,
        collection_id: &str,
        id: &str,
    ) -> Result<String, WorkspaceBridgeError> {
        let request = self.request(collection_id, id)?;
        wirebolt_core::export_legacy_v1_request(&request)
            .map_err(|error| WorkspaceBridgeError::operation(&error.to_string()))
    }

    /// Creates or updates a collection.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for invalid IDs or storage failures.
    pub fn save_collection(&self, id: String, name: String) -> Result<(), WorkspaceBridgeError> {
        let id = document_id(id)?;
        self.store
            .save(&WorkspaceDocument::Collection(Collection::new(id, name)))
            .map(|_| ())
            .map_err(|_| WorkspaceBridgeError::operation("collection could not be saved"))
    }

    /// Creates or updates one request from a bridge JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for invalid input or storage failures.
    pub fn save_request(
        &self,
        collection_id: String,
        request_json: &str,
    ) -> Result<(), WorkspaceBridgeError> {
        let collection_id = document_id(collection_id)?;
        let document: SavedRequestDocument = serde_json::from_str(request_json)
            .map_err(|_| WorkspaceBridgeError::operation("request document is invalid"))?;
        let request = request_from_document(document)?;
        self.store
            .save(&WorkspaceDocument::Request {
                collection_id,
                request,
            })
            .map(|_| ())
            .map_err(|_| WorkspaceBridgeError::operation("request could not be saved"))
    }

    /// Creates or updates one environment from a bridge JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for invalid input or storage failures.
    pub fn save_environment(&self, environment_json: &str) -> Result<(), WorkspaceBridgeError> {
        let document: SavedEnvironmentDocument = serde_json::from_str(environment_json)
            .map_err(|_| WorkspaceBridgeError::operation("environment document is invalid"))?;
        let environment =
            Environment::from_rows(document_id(document.id)?, document.name, document.variables);
        self.store
            .save(&WorkspaceDocument::Environment(environment))
            .map(|_| ())
            .map_err(|_| WorkspaceBridgeError::operation("environment could not be saved"))
    }

    /// Applies one versioned workspace mutation and returns a compact delta.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] when the command is malformed, references
    /// a missing document, creates a group cycle, or cannot be persisted.
    pub fn apply_workspace_command(
        &self,
        command_json: &str,
    ) -> Result<String, WorkspaceBridgeError> {
        let command: WorkspaceCommandDocument = serde_json::from_str(command_json)
            .map_err(|_| WorkspaceBridgeError::operation("workspace command is invalid"))?;
        let (kind, affected_ids) = self.apply_workspace_command_document(command)?;
        let delta = WorkspaceDeltaDocument {
            version: self.version.fetch_add(1, Ordering::Relaxed) + 1,
            kind,
            affected_ids,
        };
        serde_json::to_string(&delta)
            .map_err(|_| WorkspaceBridgeError::operation("workspace delta could not be encoded"))
    }

    /// Parses an import fully and returns its metadata without mutating storage.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for unsupported formats or invalid source.
    pub fn preview_import(
        &self,
        format: &str,
        source: &str,
    ) -> Result<String, WorkspaceBridgeError> {
        let imported = parse_import(format, source)?;
        serde_json::to_string(&ImportPreviewDocument {
            collection_name: &imported.name,
            request_count: imported.requests.len(),
            group_count: imported.groups.len(),
            warnings: &imported.warnings,
        })
        .map_err(|_| WorkspaceBridgeError::operation("import preview could not be encoded"))
    }

    /// Commits a fully parsed import as one new collection.
    ///
    /// If any request write fails, the newly created collection is removed so
    /// an invalid import never leaves a partial workspace behind.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for parse or storage failures.
    pub fn commit_import(
        &self,
        format: &str,
        source: &str,
    ) -> Result<String, WorkspaceBridgeError> {
        self.commit_imported(parse_import(format, source)?.into())
    }

    /// Imports a file under its filename, preserving every exported folder. A
    /// Wirebolt workspace export creates each of its collections and environments.
    ///
    /// # Errors
    /// Returns [`WorkspaceBridgeError`] for invalid source or storage failures.
    pub fn commit_import_file(
        &self,
        format: &str,
        source: &str,
        name: &str,
    ) -> Result<String, WorkspaceBridgeError> {
        let format = ImportFormat::parse(format)
            .map_err(|_| WorkspaceBridgeError::operation("unsupported import format"))?;
        let imported = ImportEngine::parse_workspace_file(format, source, name)
            .map_err(|_| WorkspaceBridgeError::operation("import could not be parsed"))?;
        self.commit_imported(imported)
    }

    /// Reads a credential only for the authentication editor, never diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for invalid names or Keychain failures.
    pub fn read_secret(&self, name: String) -> Result<Option<String>, WorkspaceBridgeError> {
        let name = SecretName::new(name)
            .map_err(|_| WorkspaceBridgeError::operation("secret name is invalid"))?;
        #[cfg(target_vendor = "apple")]
        {
            wirebolt_core::KeychainSecretStore::default()
                .read(&name)
                .map_err(|_| WorkspaceBridgeError::operation("secret could not be read"))
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = name;
            Err(WorkspaceBridgeError::operation(
                "secret storage is unavailable",
            ))
        }
    }

    /// Stores secret material in Keychain, never in the workspace.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for invalid names or Keychain failures.
    pub fn save_secret(&self, name: String, value: &str) -> Result<(), WorkspaceBridgeError> {
        let name = SecretName::new(name)
            .map_err(|_| WorkspaceBridgeError::operation("secret name is invalid"))?;
        #[cfg(target_vendor = "apple")]
        {
            wirebolt_core::KeychainSecretStore::default()
                .save(&name, value)
                .map_err(|_| WorkspaceBridgeError::operation("secret could not be saved"))?;
            clear_manual_http_engines();
            Ok(())
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (name, value);
            Err(WorkspaceBridgeError::operation(
                "secret storage is unavailable",
            ))
        }
    }

    /// Returns a read-only Git status snapshot without fetching.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] when this workspace is not a repository or
    /// Git cannot inspect it.
    pub fn git_status_json(&self) -> Result<String, GitBridgeError> {
        let status = self.git_workspace()?.status()?;
        encode_git_document(&GitStatusDocument::from(&status))
    }

    /// Pulls the configured upstream explicitly and leaves conflicts intact.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] when Git rejects or cannot run the pull.
    pub fn git_pull_json(&self) -> Result<String, GitBridgeError> {
        let operation = self.git_workspace()?.pull()?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Commits only Wirebolt-managed workspace documents.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] for invalid messages, missing Git identity,
    /// or another failed Git operation.
    pub fn git_commit_json(&self, message: &str) -> Result<String, GitBridgeError> {
        let operation = self.git_workspace()?.commit(message)?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Pushes the current branch using the user's Git credentials.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] when the repository has no usable branch or
    /// remote, authentication fails, or the update is rejected.
    pub fn git_push_json(&self) -> Result<String, GitBridgeError> {
        let operation = self.git_workspace()?.push()?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Aborts the merge left behind by a conflicted pull.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] when no merge is in progress or Git cannot abort it.
    pub fn git_abort_merge_json(&self) -> Result<String, GitBridgeError> {
        let operation = self.git_workspace()?.abort_merge()?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Makes the workspace folder the root of a new Git repository and
    /// returns its status.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] when the workspace is inside another
    /// repository or Git cannot initialize it.
    pub fn git_initialize_json(&self) -> Result<String, GitBridgeError> {
        let status = GitWorkspace::initialize(self.store.root())?.status()?;
        encode_git_document(&GitStatusDocument::from(&status))
    }
}

impl WorkspaceBridge {
    /// Creates every imported collection and environment, or none of them.
    fn commit_imported(
        &self,
        imported: wirebolt_core::ImportedWorkspace,
    ) -> Result<String, WorkspaceBridgeError> {
        let sequence = self.version.load(Ordering::Relaxed) + 1;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| WorkspaceBridgeError::operation("import identifier could not be created"))?
            .as_nanos();
        let prefix = format!("import-{timestamp:x}-{:x}-{sequence:x}", std::process::id());
        let mut created = CreatedImport::default();
        if let Err(error) = self.save_imported(&prefix, imported, &mut created) {
            for id in &created.collections {
                let _ = self.store.delete_collection(id);
            }
            for id in &created.environments {
                let _ = self.store.delete_environment(id);
            }
            #[cfg(target_vendor = "apple")]
            for name in created.secrets {
                let _ = wirebolt_core::KeychainSecretStore::default().remove(&name);
            }
            return Err(error);
        }

        let delta = WorkspaceDeltaDocument {
            version: self.version.fetch_add(1, Ordering::Relaxed) + 1,
            kind: "collection",
            affected_ids: created
                .collections
                .iter()
                .map(ToString::to_string)
                .collect(),
        };
        serde_json::to_string(&delta)
            .map_err(|_| WorkspaceBridgeError::operation("import delta could not be encoded"))
    }

    fn save_imported(
        &self,
        prefix: &str,
        imported: wirebolt_core::ImportedWorkspace,
        created: &mut CreatedImport,
    ) -> Result<(), WorkspaceBridgeError> {
        let wirebolt_core::ImportedWorkspace {
            collections,
            environments,
            mut request_settings,
        } = imported;
        for (index, imported) in collections.into_iter().enumerate() {
            let collection_id = document_id(format!("{prefix}-{index}"))?;
            self.save_imported_collection(
                &collection_id,
                imported,
                &mut request_settings,
                created,
            )?;
        }
        let has_global = environments.iter().any(|environment| environment.global)
            && self
                .store
                .load()
                .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?
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
            let environment =
                Environment::from_rows(document_id(id)?, environment.name, environment.variables);
            created.environments.push(environment.id.clone());
            self.store
                .save(&WorkspaceDocument::Environment(environment))
                .map_err(|_| {
                    WorkspaceBridgeError::operation("import environment could not be saved")
                })?;
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
        created: &mut CreatedImport,
    ) -> Result<(), WorkspaceBridgeError> {
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
                        WorkspaceBridgeError::operation("import group hierarchy is invalid")
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
            .collect::<Result<Vec<_>, WorkspaceBridgeError>>()?;
        self.store
            .save(&WorkspaceDocument::Collection(collection))
            .map_err(|_| {
                WorkspaceBridgeError::operation("import collection could not be created")
            })?;
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
                &mut request.authentication,
                &format!("{collection_id}-{}", request.id),
                &mut created.secrets,
            )?;
            self.store
                .save(&WorkspaceDocument::Request {
                    collection_id: collection_id.clone(),
                    request,
                })
                .map_err(|_| {
                    WorkspaceBridgeError::operation("import request could not be saved")
                })?;
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

impl WorkspaceBridge {
    #[expect(
        clippy::too_many_lines,
        reason = "the command router keeps each transactional mutation and its affected IDs in one exhaustive match"
    )]
    fn apply_workspace_command_document(
        &self,
        command: WorkspaceCommandDocument,
    ) -> Result<(&'static str, Vec<String>), WorkspaceBridgeError> {
        match command {
            WorkspaceCommandDocument::SaveWorkspaceProxy { proxy } => {
                let mut workspace = self
                    .store
                    .load()
                    .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?
                    .workspace;
                workspace.proxy = proxy;
                self.store
                    .save(&WorkspaceDocument::Workspace(workspace))
                    .map_err(|_| {
                        WorkspaceBridgeError::operation("workspace proxy could not be saved")
                    })?;
                Ok(("workspace", vec!["proxy".to_owned()]))
            }
            WorkspaceCommandDocument::SaveWorkspaceSettings { transport } => {
                let mut workspace = self
                    .store
                    .load()
                    .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?
                    .workspace;
                workspace.transport = transport;
                self.store
                    .save(&WorkspaceDocument::Workspace(workspace))
                    .map_err(|_| {
                        WorkspaceBridgeError::operation("workspace settings could not be saved")
                    })?;
                Ok(("workspace", vec!["transport".to_owned()]))
            }
            WorkspaceCommandDocument::RenameWorkspace { name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(WorkspaceBridgeError::operation(
                        "workspace name must not be empty",
                    ));
                }
                let mut workspace = self
                    .store
                    .load()
                    .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?
                    .workspace;
                name.clone_into(&mut workspace.name);
                self.store
                    .save(&WorkspaceDocument::Workspace(workspace))
                    .map_err(|_| {
                        WorkspaceBridgeError::operation("workspace could not be renamed")
                    })?;
                Ok(("workspace", vec!["name".to_owned()]))
            }
            WorkspaceCommandDocument::CreateCollection { id, name, order } => {
                let mut collection = Collection::new(document_id(id.clone())?, name);
                collection.order = order;
                self.store
                    .save(&WorkspaceDocument::Collection(collection))
                    .map_err(|_| {
                        WorkspaceBridgeError::operation("collection could not be created")
                    })?;
                Ok(("collection", vec![id]))
            }
            WorkspaceCommandDocument::RenameCollection { id, name } => {
                let mut collection = self.collection(&id)?;
                collection.name = name;
                self.store
                    .save(&WorkspaceDocument::Collection(collection))
                    .map_err(|_| {
                        WorkspaceBridgeError::operation("collection could not be renamed")
                    })?;
                Ok(("collection", vec![id]))
            }
            WorkspaceCommandDocument::DeleteCollection { id } => {
                self.store
                    .delete_collection(&document_id(id.clone())?)
                    .map_err(|_| {
                        WorkspaceBridgeError::operation("collection could not be deleted")
                    })?;
                Ok(("collection", vec![id]))
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
                    return Err(WorkspaceBridgeError::operation(
                        "parent group does not exist",
                    ));
                }
                if collection
                    .groups
                    .iter()
                    .any(|existing| existing.id == group_id)
                {
                    return Err(WorkspaceBridgeError::operation("group already exists"));
                }
                collection
                    .groups
                    .push(Group::new(group_id, group.name, parent_id, group.order));
                self.save_collection_document(collection)?;
                Ok(("group", vec![collection_id, group.id]))
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
                    .ok_or_else(|| WorkspaceBridgeError::operation("group does not exist"))?;
                group.name = name;
                self.save_collection_document(collection)?;
                Ok(("group", vec![collection_id, id]))
            }
            WorkspaceCommandDocument::DeleteGroup { collection_id, id } => {
                let mut collection = self.collection(&collection_id)?;
                let group_id = document_id(id.clone())?;
                let descendants = descendant_group_ids(&collection.groups, &group_id);
                if descendants.is_empty() {
                    return Err(WorkspaceBridgeError::operation("group does not exist"));
                }
                let snapshot = self.store.load().map_err(|_| {
                    WorkspaceBridgeError::operation("workspace could not be loaded")
                })?;
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
                                    WorkspaceBridgeError::operation(
                                        "group requests could not be deleted",
                                    )
                                })?;
                        }
                    }
                }
                collection
                    .groups
                    .retain(|group| !descendants.contains(&group.id));
                self.save_collection_document(collection)?;
                Ok(("group", vec![collection_id, id]))
            }
            WorkspaceCommandDocument::ReorderChildren {
                collection_id,
                parent_id,
                items,
            } => {
                self.reorder_children(&collection_id, parent_id, items)?;
                Ok(("collection", vec![collection_id]))
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
                    return Err(WorkspaceBridgeError::operation(
                        "a group cannot contain itself",
                    ));
                }
                if parent_id.as_ref().is_some_and(|parent| {
                    collection
                        .groups
                        .iter()
                        .all(|existing| &existing.id != parent)
                }) {
                    return Err(WorkspaceBridgeError::operation(
                        "parent group does not exist",
                    ));
                }
                let group = collection
                    .groups
                    .iter_mut()
                    .find(|group| group.id == group_id)
                    .ok_or_else(|| WorkspaceBridgeError::operation("group does not exist"))?;
                group.parent_id = parent_id;
                group.order = order;
                self.save_collection_document(collection)?;
                Ok(("group", vec![collection_id, id]))
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
                    .map_err(|_| WorkspaceBridgeError::operation("request could not be saved"))?;
                Ok(("request", vec![collection_id, id]))
            }
            WorkspaceCommandDocument::DeleteRequest { collection_id, id } => {
                self.store
                    .delete_request(
                        &document_id(collection_id.clone())?,
                        &document_id(id.clone())?,
                    )
                    .map_err(|_| WorkspaceBridgeError::operation("request could not be deleted"))?;
                Ok(("request", vec![collection_id, id]))
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
                    .map_err(|_| {
                        WorkspaceBridgeError::operation("request could not be duplicated")
                    })?;
                Ok(("request", vec![collection_id, id, new_id]))
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
                    .map_err(|_| WorkspaceBridgeError::operation("request could not be moved"))?;
                if from_collection_id != to_collection_id {
                    self.store
                        .delete_request(
                            &document_id(from_collection_id.clone())?,
                            &document_id(request_id.clone())?,
                        )
                        .map_err(|_| {
                            WorkspaceBridgeError::operation("source request could not be removed")
                        })?;
                }
                Ok((
                    "request",
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
                    .map_err(|_| {
                        WorkspaceBridgeError::operation("environment could not be saved")
                    })?;
                Ok(("environment", vec![id]))
            }
            WorkspaceCommandDocument::DeleteEnvironment { id } => {
                self.store
                    .delete_environment(&document_id(id.clone())?)
                    .map_err(|_| {
                        WorkspaceBridgeError::operation("environment could not be deleted")
                    })?;
                Ok(("environment", vec![id]))
            }
        }
    }

    fn reorder_children(
        &self,
        collection_id: &str,
        parent_id: Option<String>,
        items: Vec<String>,
    ) -> Result<(), WorkspaceBridgeError> {
        let id = document_id(collection_id.to_owned())?;
        let parent = parent_id.map(document_id).transpose()?;
        let mut snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?
            .collections
            .into_iter()
            .find(|entry| entry.collection.id == id)
            .ok_or_else(|| WorkspaceBridgeError::operation("collection does not exist"))?;
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
            return Err(WorkspaceBridgeError::operation(
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
                        .map_err(|_| WorkspaceBridgeError::operation("reorder rollback failed"))?;
                }
                return Err(WorkspaceBridgeError::operation(
                    "items could not be reordered",
                ));
            }
        }
        Ok(())
    }

    fn collection(&self, id: &str) -> Result<Collection, WorkspaceBridgeError> {
        let id = document_id(id.to_owned())?;
        self.store
            .load()
            .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?
            .collections
            .into_iter()
            .find(|snapshot| snapshot.collection.id == id)
            .map(|snapshot| snapshot.collection)
            .ok_or_else(|| WorkspaceBridgeError::operation("collection does not exist"))
    }

    fn request(
        &self,
        collection_id: &str,
        request_id: &str,
    ) -> Result<Request, WorkspaceBridgeError> {
        let collection_id = document_id(collection_id.to_owned())?;
        let request_id = document_id(request_id.to_owned())?;
        self.store
            .load()
            .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?
            .collections
            .into_iter()
            .find(|snapshot| snapshot.collection.id == collection_id)
            .and_then(|snapshot| {
                snapshot
                    .requests
                    .into_iter()
                    .find(|request| request.id == request_id)
            })
            .ok_or_else(|| WorkspaceBridgeError::operation("request does not exist"))
    }

    fn save_collection_document(&self, collection: Collection) -> Result<(), WorkspaceBridgeError> {
        self.store
            .save(&WorkspaceDocument::Collection(collection))
            .map(|_| ())
            .map_err(|_| WorkspaceBridgeError::operation("collection could not be saved"))
    }

    fn git_workspace(&self) -> Result<GitWorkspace, GitBridgeError> {
        GitWorkspace::open(self.store.root()).map_err(Into::into)
    }
}

impl WorkspaceBridgeError {
    fn operation(reason: &str) -> Self {
        Self::OperationFailed {
            reason: reason.to_owned(),
        }
    }
}

impl From<GitError> for GitBridgeError {
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

fn encode_git_document(document: &impl Serialize) -> Result<String, GitBridgeError> {
    serde_json::to_string(document).map_err(|_| GitBridgeError::OperationFailed {
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

fn document_id(value: String) -> Result<DocumentId, WorkspaceBridgeError> {
    DocumentId::new(value).map_err(|_| WorkspaceBridgeError::operation("document ID is invalid"))
}

fn parse_import(
    format: &str,
    source: &str,
) -> Result<wirebolt_core::ImportedCollection, WorkspaceBridgeError> {
    let format = ImportFormat::parse(format)
        .map_err(|_| WorkspaceBridgeError::operation("import format is unsupported"))?;
    ImportEngine::parse(format, source)
        .map_err(|_| WorkspaceBridgeError::operation("import source is invalid"))
}

fn request_from_document(document: SavedRequestDocument) -> Result<Request, WorkspaceBridgeError> {
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

#[uniffi::export]
#[must_use]
pub fn core_handshake() -> CoreHandshake {
    wirebolt_core::handshake().into()
}

#[uniffi::export]
/// Prepares one coarse-grained request across the Swift–Rust boundary.
///
/// # Errors
///
/// Returns [`RequestPreparationError`] when the Rust core rejects any request
/// component.
pub fn prepare_request(
    draft: BridgeRequestDraft,
) -> Result<PreparedRequestSummary, RequestPreparationError> {
    let prepared = wirebolt_core::prepare_request(draft.into()).map_err(|error| {
        RequestPreparationError::InvalidRequest {
            reason: error.to_string(),
        }
    })?;

    Ok(PreparedRequestSummary {
        method: prepared.method().to_string(),
        url: prepared.url().to_string(),
        header_count: prepared.headers().len().try_into().unwrap_or(u64::MAX),
        body_bytes: prepared.body_byte_count(),
    })
}

#[uniffi::export]
/// Resolves values with the transport's template rules, without sending a request.
/// This is only for runtime routing and explicit exports, never persisted snapshots.
///
/// # Errors
/// Returns a redacted preparation failure for invalid input, templates or missing secrets.
#[allow(clippy::needless_pass_by_value)] // UniFFI exports require owned strings.
pub fn resolve_request_values(
    values_json: String,
    variables_json: String,
) -> Result<Vec<String>, RequestPreparationError> {
    let invalid = || RequestPreparationError::InvalidRequest {
        reason: "invalid resolution input".to_owned(),
    };
    let values: Vec<ValueSource> = serde_json::from_str(&values_json).map_err(|_| invalid())?;
    let variables: BTreeMap<String, ValueSource> =
        serde_json::from_str(&variables_json).map_err(|_| invalid())?;
    let environment = Environment::new(
        DocumentId::new("active").map_err(|_| invalid())?,
        "Active".to_owned(),
        variables,
    );
    #[cfg(target_vendor = "apple")]
    let secrets = wirebolt_core::KeychainSecretResolver::default();
    #[cfg(not(target_vendor = "apple"))]
    let secrets = NoSecrets;
    RequestPipeline::new(Some(&environment), &secrets)
        .resolve_values(&values)
        .map_err(|error| {
            let failure = RunFailureDocument {
                kind: "invalid_request",
                issues: error.issues.into_iter().map(Into::into).collect(),
            };
            RequestPreparationError::InvalidRequest {
                reason: serde_json::to_string(&failure)
                    .unwrap_or_else(|_| "invalid resolution input".to_owned()),
            }
        })
}

#[unsafe(no_mangle)]
pub extern "C" fn wirebolt_stream_abi_version() -> u32 {
    wirebolt_core::STREAM_ABI_VERSION
}

/// Starts the shared runtime and warms the direct and system HTTP engines in
/// the background, so the first request does not pay for TLS and proxy
/// discovery. Returns 1 when the runtime is available.
#[unsafe(no_mangle)]
pub extern "C" fn wirebolt_runtime_warmup() -> u8 {
    let Some(runtime) = SHARED_RUNTIME.as_ref() else {
        return 0;
    };
    runtime.spawn_blocking(|| {
        for mode in [ProxyMode::Direct, ProxyMode::System] {
            let proxy = ProxyPolicy::with_workspace(mode).resolve(None);
            let _ = shared_http_engine(
                &proxy,
                &HttpEngineConfig::default(),
                &wirebolt_core::NoSecrets,
            );
        }
    });
    1
}

/// Drops every pooled HTTP engine so the next run rebuilds its client. Call
/// this when the network configuration changes: the system proxy settings
/// are captured once per engine, and pooled connections may be dead.
#[uniffi::export]
pub fn reset_http_engines() {
    SHARED_HTTP_ENGINES.reset();
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WireboltRunCallbacks {
    pub on_prepared: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
    pub on_head: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
    pub on_chunk: Option<extern "C" fn(*mut c_void, *const u8, usize) -> u8>,
    pub on_complete: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
    pub on_error: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
    pub on_cookies: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
}

#[derive(Debug)]
pub struct WireboltRunSession {
    cancellation: RunCancellation,
    completion: Mutex<Option<mpsc::Receiver<()>>>,
}

static SHARED_HTTP_ENGINES: LazyLock<HttpEngineCache> = LazyLock::new(HttpEngineCache::default);
static SHARED_RUNTIME: LazyLock<Option<tokio::runtime::Runtime>> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("wirebolt-http")
        .enable_all()
        .build()
        .ok()
});

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunInput {
    method: String,
    url: String,
    #[serde(default)]
    query: Vec<RequestValueField>,
    #[serde(default)]
    headers: Vec<RequestHeader>,
    #[serde(default)]
    authentication: RequestAuthentication,
    #[serde(default = "empty_body")]
    body: RequestBody,
    #[serde(default)]
    variables: BTreeMap<String, ValueSource>,
    #[serde(default)]
    workspace_proxy: Option<ProxyMode>,
    #[serde(default)]
    app_proxy: Option<ProxyMode>,
    #[serde(default)]
    request_proxy: Option<ProxyMode>,
    #[serde(default = "default_total_timeout_ms")]
    total_timeout_ms: u64,
    #[serde(default = "default_read_timeout_ms")]
    read_timeout_ms: u64,
    #[serde(default)]
    max_response_bytes: Option<u64>,
    #[serde(default = "default_decode_content")]
    decode_content: bool,
    #[serde(default = "default_validate_tls")]
    validate_tls: bool,
    #[serde(default)]
    follow_redirects: bool,
    #[serde(default = "default_maximum_redirects")]
    maximum_redirects: u8,
    #[serde(default)]
    client_certificate_reference: Option<SecretName>,
    #[serde(default)]
    custom_ca_path: Option<String>,
}

const fn empty_body() -> RequestBody {
    RequestBody::Empty
}

const fn default_total_timeout_ms() -> u64 {
    30_000
}

const fn default_read_timeout_ms() -> u64 {
    10_000
}

const fn default_decode_content() -> bool {
    true
}

const fn default_validate_tls() -> bool {
    true
}
const fn default_maximum_redirects() -> u8 {
    10
}
const fn default_inherits_workspace_transport() -> bool {
    true
}

/// Zero disables the deadline; the engine then arms no timer at all.
fn timeout_from_millis(milliseconds: u64) -> Option<Duration> {
    (milliseconds != 0).then(|| Duration::from_millis(milliseconds))
}

#[derive(Debug, Serialize)]
struct ResponseHeadDocument<'a> {
    status: u16,
    version: &'static str,
    headers: Vec<ResponseHeaderDocument<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_encoding: Option<&'static str>,
    warnings: &'a [RequestIssueDocument],
    time_to_headers_ns: u64,
}

#[derive(Debug, Serialize)]
struct PreparedRunSnapshotDocument {
    method: String,
    url: String,
    headers: Vec<PreparedHeaderDocument>,
    body: PreparedBodyDocument,
    transport: TransportSettings,
    proxy: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct PreparedHeaderDocument {
    name: String,
    value: String,
    redacted: bool,
}

#[derive(Debug, Serialize)]
struct PreparedBodyDocument {
    byte_count: u64,
    content_type: Option<String>,
    text_preview: Option<String>,
    redacted: bool,
}

#[derive(Debug, Serialize)]
struct ResponseHeaderDocument<'a> {
    name: &'a str,
    value: std::borrow::Cow<'a, str>,
}

#[derive(Debug, Serialize)]
struct RunCompleteDocument {
    bytes_received: u64,
    bytes_decoded: u64,
    total_time_ns: u64,
}

#[derive(Debug, Serialize)]
struct RunFailureDocument {
    kind: &'static str,
    issues: Vec<RequestIssueDocument>,
}

#[derive(Debug, Serialize)]
struct RequestIssueDocument {
    path: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
}

impl From<RequestIssue> for RequestIssueDocument {
    fn from(issue: RequestIssue) -> Self {
        Self {
            path: issue.path,
            kind: request_issue_kind(issue.kind),
            reference: issue.reference,
        }
    }
}

/// Starts one asynchronous HTTP run.
///
/// The input bytes are copied before this function returns. The callback
/// context and callback functions must remain valid until a terminal callback
/// has returned. Callback byte buffers are borrowed only during the callback.
/// Do not call [`wirebolt_run_free`] from inside a callback.
///
/// # Safety
///
/// `input_json` must point to `input_length` readable bytes, unless the length
/// is zero. `context` must satisfy the lifetime contract above. The returned
/// pointer must be released exactly once with [`wirebolt_run_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_run_start(
    input_json: *const u8,
    input_length: usize,
    callbacks: WireboltRunCallbacks,
    context: *mut c_void,
) -> *mut WireboltRunSession {
    if callbacks.on_head.is_none()
        || callbacks.on_chunk.is_none()
        || callbacks.on_complete.is_none()
        || callbacks.on_error.is_none()
        || (input_json.is_null() && input_length != 0)
    {
        return ptr::null_mut();
    }

    let input = if input_length == 0 {
        Vec::new()
    } else {
        // SAFETY: The caller promises a readable buffer for `input_length` bytes.
        unsafe { std::slice::from_raw_parts(input_json, input_length) }.to_vec()
    };
    let Some(runtime) = SHARED_RUNTIME.as_ref() else {
        return ptr::null_mut();
    };
    let cancellation = RunCancellation::new();
    let worker_cancellation = cancellation.clone();
    let context_address = context as usize;
    let (completion_sender, completion) = mpsc::sync_channel(1);
    runtime.spawn(async move {
        // The run executes in its own task so that a panic anywhere inside
        // it still ends with a terminal callback instead of a silent hang.
        let worker = tokio::spawn(execute_run(
            input,
            worker_cancellation,
            callbacks,
            context_address,
        ));
        if worker.await.is_err() {
            emit_failure(
                &callbacks,
                context_address,
                &RunFailureDocument::new("internal"),
            );
        }
        let _ = completion_sender.send(());
    });

    Box::into_raw(Box::new(WireboltRunSession {
        cancellation,
        completion: Mutex::new(Some(completion)),
    }))
}

/// Requests cancellation of an active run.
///
/// # Safety
///
/// `session` must be null or a live pointer returned by
/// [`wirebolt_run_start`] that has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_run_cancel(session: *mut WireboltRunSession) {
    // SAFETY: The caller owns a live session for the duration of this call.
    if let Some(session) = unsafe { session.as_ref() } {
        session.cancellation.cancel();
    }
}

/// Cancels, joins, and releases one run session.
///
/// # Safety
///
/// `session` must be null or a live pointer returned by
/// [`wirebolt_run_start`]. It must be passed to this function at most once and
/// this function must not be called from a run callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_run_free(session: *mut WireboltRunSession) {
    if session.is_null() {
        return;
    }
    // SAFETY: Ownership is transferred back exactly once by the caller.
    let session = unsafe { Box::from_raw(session) };
    session.cancellation.cancel();
    let completion = session
        .completion
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(completion) = completion {
        let _ = completion.recv();
    }
}

async fn execute_run(
    input: Vec<u8>,
    cancellation: RunCancellation,
    callbacks: WireboltRunCallbacks,
    context: usize,
) {
    let parsed = serde_json::from_slice::<RunInput>(&input);
    let Ok(input) = parsed else {
        emit_failure(
            &callbacks,
            context,
            &RunFailureDocument::new("invalid_input"),
        );
        return;
    };

    #[cfg(target_vendor = "apple")]
    execute_run_with_secrets(
        input,
        cancellation,
        callbacks,
        context,
        wirebolt_core::KeychainSecretResolver::default(),
    )
    .await;
    #[cfg(not(target_vendor = "apple"))]
    execute_run_with_secrets(input, cancellation, callbacks, context, NoSecrets).await;
}

async fn execute_run_with_secrets<R>(
    input: RunInput,
    cancellation: RunCancellation,
    callbacks: WireboltRunCallbacks,
    context: usize,
    secrets: R,
) where
    R: SecretResolver + Clone + Send + 'static,
{
    let options = RunOptions {
        total_timeout: timeout_from_millis(input.total_timeout_ms),
        read_timeout: timeout_from_millis(input.read_timeout_ms),
        max_response_bytes: input.max_response_bytes,
        decode_content: input.decode_content,
    };
    // Secret lookups can block on Keychain (even prompting the user) and
    // building a client loads TLS and proxy state, so neither runs on the
    // two async workers that stream every other response.
    let prepared = tokio::task::spawn_blocking(move || prepare_run(input, &secrets)).await;
    let (prepared, engine, snapshot) = match prepared {
        Ok(Ok(prepared)) => prepared,
        Ok(Err(failure)) => {
            emit_failure(&callbacks, context, &failure);
            return;
        }
        Err(_) => {
            emit_failure(&callbacks, context, &RunFailureDocument::new("internal"));
            return;
        }
    };
    emit_json(callbacks.on_prepared, context, &snapshot);
    let warnings: Vec<RequestIssueDocument> = prepared
        .warnings()
        .iter()
        .cloned()
        .map(Into::into)
        .collect();
    let result = engine
        .run_observed_with_cookies(
            prepared,
            options,
            &cancellation,
            |head| emit_head(&callbacks, context, head, &warnings),
            |chunk| {
                let should_continue = callbacks.on_chunk.map_or(0, |callback| {
                    callback(context as *mut c_void, chunk.as_ptr(), chunk.len())
                });
                if should_continue == 0 {
                    StreamControl::Stop
                } else {
                    StreamControl::Continue
                }
            },
            |url, headers| {
                let values: Vec<_> = headers
                    .iter()
                    .map(|(name, value)| ResponseHeaderDocument {
                        name: name.as_str(),
                        value: String::from_utf8_lossy(value.as_bytes()),
                    })
                    .collect();
                emit_json(
                    callbacks.on_cookies,
                    context,
                    &serde_json::json!({"url": url.as_str(), "headers": values}),
                );
            },
        )
        .await;
    match result {
        Ok(run) => emit_json(
            callbacks.on_complete,
            context,
            &RunCompleteDocument {
                bytes_received: run.bytes_received,
                bytes_decoded: run.bytes_decoded,
                total_time_ns: duration_ns(run.total_time),
            },
        ),
        Err(error) => emit_failure(
            &callbacks,
            context,
            &RunFailureDocument::from_run_error(&error),
        ),
    }
}

fn secure_import_authentication(
    authentication: &mut RequestAuthentication,
    prefix: &str,
    created: &mut Vec<SecretName>,
) -> Result<(), WorkspaceBridgeError> {
    let mut store = |source: &mut ValueSource, label: &str| -> Result<(), WorkspaceBridgeError> {
        let ValueSource::Literal(material) = source else {
            return Ok(());
        };
        let name = SecretName::new(format!("{prefix}-{label}"))
            .map_err(|_| WorkspaceBridgeError::operation("import credential name is invalid"))?;
        save_import_credential(&name, material)?;
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
        RequestAuthentication::None | RequestAuthentication::Oauth2 { .. } => {}
    }
    Ok(())
}

fn save_import_credential(name: &SecretName, material: &str) -> Result<(), WorkspaceBridgeError> {
    #[cfg(target_vendor = "apple")]
    {
        wirebolt_core::KeychainSecretStore::default()
            .save(name, material)
            .map_err(|_| WorkspaceBridgeError::operation("import credentials could not be saved"))
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = (name, material);
        Err(WorkspaceBridgeError::operation(
            "credential import requires secure storage",
        ))
    }
}

type PreparedRun = (
    wirebolt_core::PreparedRequest,
    Arc<HttpEngine>,
    PreparedRunSnapshotDocument,
);

fn prepare_run<R: SecretResolver + ?Sized>(
    input: RunInput,
    secrets: &R,
) -> Result<PreparedRun, RunFailureDocument> {
    prepare_protocol_run(input, secrets, false)
}

fn prepare_protocol_run<R: SecretResolver + ?Sized>(
    input: RunInput,
    secrets: &R,
    websocket: bool,
) -> Result<PreparedRun, RunFailureDocument> {
    let mut request = Request::new(
        DocumentId::new("draft").expect("static document ID"),
        "Draft",
        input.method,
        input.url,
    );
    request.query = input.query;
    request.headers = input.headers;
    request.authentication = input.authentication;
    request.body = input.body;
    request.proxy_override = input.request_proxy;
    request.transport = TransportSettings {
        validate_tls: input.validate_tls,
        follow_redirects: input.follow_redirects,
        maximum_redirects: input.maximum_redirects.min(10),
        total_timeout_ms: input.total_timeout_ms,
        read_timeout_ms: input.read_timeout_ms,
        client_certificate_reference: input.client_certificate_reference,
        custom_ca_path: input.custom_ca_path,
    };
    let environment = Environment::new(
        DocumentId::new("active").expect("static document ID"),
        "Active".to_owned(),
        input.variables,
    );
    let pipeline = RequestPipeline::new(Some(&environment), secrets);
    let prepared = if websocket {
        pipeline.prepare_websocket(&request)
    } else {
        pipeline.prepare(&request)
    }
    .map_err(|error| RunFailureDocument {
        kind: "invalid_request",
        issues: error.issues.into_iter().map(Into::into).collect(),
    })?;
    let proxy = ProxyPolicy::new(input.workspace_proxy)
        .with_app_default(input.app_proxy)
        .resolve(request.proxy_override.as_ref());
    let config = HttpEngineConfig {
        validate_tls: request.transport.validate_tls,
        maximum_redirects: request
            .transport
            .follow_redirects
            .then_some(request.transport.maximum_redirects),
        ..HttpEngineConfig::default()
    }
    .resolve_tls(
        request.transport.client_certificate_reference.as_ref(),
        request.transport.custom_ca_path.as_deref(),
        secrets,
    )
    .map_err(|_| RunFailureDocument::new("tls_configuration"))?;
    let engine = shared_http_engine(&proxy, &config, secrets)
        .map_err(|error| RunFailureDocument::from_run_error(&error))?;
    let snapshot = prepared_run_snapshot(&prepared, request.transport, &proxy);
    Ok((prepared, engine, snapshot))
}

fn shared_http_engine<R: SecretResolver + ?Sized>(
    proxy: &ResolvedProxy,
    config: &HttpEngineConfig,
    secrets: &R,
) -> Result<Arc<HttpEngine>, RunError> {
    SHARED_HTTP_ENGINES.engine_with_config(proxy.mode(), config, || {
        HttpEngine::with_proxy(config, proxy, secrets)
    })
}

fn prepared_run_snapshot(
    prepared: &wirebolt_core::PreparedRequest,
    transport: TransportSettings,
    proxy: &ResolvedProxy,
) -> PreparedRunSnapshotDocument {
    let headers = if prepared.header_names_are_sensitive() {
        vec![PreparedHeaderDocument {
            name: "[REDACTED]".to_owned(),
            value: "••••••••".to_owned(),
            redacted: true,
        }]
    } else {
        prepared
            .headers()
            .iter()
            .map(|(name, value)| PreparedHeaderDocument {
                name: name.as_str().to_owned(),
                value: if value.is_sensitive() {
                    "••••••••".to_owned()
                } else {
                    String::from_utf8_lossy(value.as_bytes()).into_owned()
                },
                redacted: value.is_sensitive(),
            })
            .collect()
    };
    let redacted = prepared.body_is_sensitive();
    let text_preview = if redacted || prepared.body_is_file_backed() {
        None
    } else {
        std::str::from_utf8(prepared.body())
            .ok()
            .map(|text| text.chars().take(32 * 1024).collect())
    };
    PreparedRunSnapshotDocument {
        method: prepared.method().as_str().to_owned(),
        url: if prepared.url_is_sensitive() {
            "[REDACTED]".to_owned()
        } else {
            prepared.url().as_str().to_owned()
        },
        headers,
        body: PreparedBodyDocument {
            byte_count: prepared.body_byte_count(),
            content_type: prepared
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            text_preview,
            redacted,
        },
        transport,
        proxy: serde_json::json!({
            "configuration": match proxy.mode() {
                ProxyMode::Direct => serde_json::json!({"mode": "direct"}),
                ProxyMode::System => serde_json::json!({"mode": "system"}),
                ProxyMode::Manual(_) => serde_json::json!({"mode": "manual", "routes": proxy.diagnostic().routes().iter().map(|route| serde_json::json!({
                    "destination": route.destination(), "endpoint": route.endpoint()
                })).collect::<Vec<_>>() }),
            },
            "source": match proxy.source() {
                wirebolt_core::ProxySource::Request => "request",
                wirebolt_core::ProxySource::Workspace => "workspace",
                wirebolt_core::ProxySource::AppDefault => "app_default",
                wirebolt_core::ProxySource::SystemDefault => "system_default",
            }
        }),
    }
}

fn clear_manual_http_engines() {
    SHARED_HTTP_ENGINES.clear_manual();
}

/// Pooled HTTP engines keyed by proxy mode. One lock guards every slot, so a
/// reset is observed as a whole; clients are built outside the lock, and a
/// build that a reset overtook serves its own run but is never pooled.
#[derive(Debug, Default)]
struct HttpEngineCache {
    state: Mutex<HttpEngineCacheState>,
}

#[derive(Debug, Default)]
struct HttpEngineCacheState {
    /// Bumped by every reset; a build that started under an older generation
    /// may have captured proxy or secret state the reset meant to discard.
    generation: u64,
    system: Option<(HttpEngineConfig, Arc<HttpEngine>)>,
    direct: Option<(HttpEngineConfig, Arc<HttpEngine>)>,
    /// Manual engines are keyed by proxy mode alone: the same configuration
    /// reached through a workspace policy or a request override shares one
    /// pool. Least recently created is evicted first.
    manual: Vec<(ProxyMode, HttpEngineConfig, Arc<HttpEngine>)>,
}

const MAX_CACHED_MANUAL_ENGINES: usize = 8;

impl HttpEngineCache {
    #[cfg(test)]
    fn engine(
        &self,
        mode: &ProxyMode,
        build: impl FnOnce() -> Result<HttpEngine, RunError>,
    ) -> Result<Arc<HttpEngine>, RunError> {
        self.engine_with_config(mode, &HttpEngineConfig::default(), build)
    }

    fn engine_with_config(
        &self,
        mode: &ProxyMode,
        config: &HttpEngineConfig,
        build: impl FnOnce() -> Result<HttpEngine, RunError>,
    ) -> Result<Arc<HttpEngine>, RunError> {
        let generation = {
            let state = self.lock();
            if let Some(engine) = state.lookup(mode, config) {
                return Ok(engine);
            }
            state.generation
        };
        let engine = Arc::new(build()?);
        let mut state = self.lock();
        if state.generation != generation {
            return Ok(engine);
        }
        // Another run may have built the same engine meanwhile; keep the
        // pooled one so both share its connections.
        Ok(state.insert(mode, config.clone(), engine))
    }

    fn reset(&self) {
        let mut state = self.lock();
        state.generation += 1;
        state.system = None;
        state.direct = None;
        state.manual.clear();
    }

    /// Manual engines embed proxy credentials, so a changed secret retires
    /// them; direct and system pools carry nothing secret and stay.
    fn clear_manual(&self) {
        let mut state = self.lock();
        state.generation += 1;
        state.manual.clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HttpEngineCacheState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl HttpEngineCacheState {
    fn lookup(&self, mode: &ProxyMode, config: &HttpEngineConfig) -> Option<Arc<HttpEngine>> {
        match mode {
            ProxyMode::System => self
                .system
                .as_ref()
                .filter(|(stored, _)| stored == config)
                .map(|(_, engine)| Arc::clone(engine)),
            ProxyMode::Direct => self
                .direct
                .as_ref()
                .filter(|(stored, _)| stored == config)
                .map(|(_, engine)| Arc::clone(engine)),
            ProxyMode::Manual(_) => self
                .manual
                .iter()
                .find(|(candidate, stored, _)| candidate == mode && stored == config)
                .map(|(_, _, engine)| Arc::clone(engine)),
        }
    }

    fn insert(
        &mut self,
        mode: &ProxyMode,
        config: HttpEngineConfig,
        engine: Arc<HttpEngine>,
    ) -> Arc<HttpEngine> {
        if let Some(existing) = self.lookup(mode, &config) {
            return existing;
        }
        match mode {
            ProxyMode::System => self.system = Some((config, Arc::clone(&engine))),
            ProxyMode::Direct => self.direct = Some((config, Arc::clone(&engine))),
            ProxyMode::Manual(_) => {
                if self.manual.len() == MAX_CACHED_MANUAL_ENGINES {
                    self.manual.remove(0);
                }
                self.manual
                    .push((mode.clone(), config, Arc::clone(&engine)));
            }
        }
        engine
    }
}

fn emit_head(
    callbacks: &WireboltRunCallbacks,
    context: usize,
    head: &RunHead,
    warnings: &[RequestIssueDocument],
) {
    emit_json(
        callbacks.on_head,
        context,
        &ResponseHeadDocument {
            status: head.status,
            version: http_version(head.version),
            headers: head
                .headers
                .iter()
                .map(|(name, value)| ResponseHeaderDocument {
                    name: name.as_str(),
                    value: String::from_utf8_lossy(value.as_bytes()),
                })
                .collect(),
            remote_addr: head.remote_addr.map(|address| address.to_string()),
            content_encoding: head
                .content_encoding
                .map(wirebolt_core::ContentEncoding::token),
            warnings,
            time_to_headers_ns: duration_ns(head.time_to_headers),
        },
    );
}

fn emit_failure(callbacks: &WireboltRunCallbacks, context: usize, failure: &RunFailureDocument) {
    emit_json(callbacks.on_error, context, failure);
}

fn emit_json<T: Serialize>(
    callback: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
    context: usize,
    value: &T,
) {
    let Some(callback) = callback else { return };
    let Ok(json) = serde_json::to_vec(value) else {
        return;
    };
    callback(context as *mut c_void, json.as_ptr(), json.len());
}

impl RunFailureDocument {
    const fn new(kind: &'static str) -> Self {
        Self {
            kind,
            issues: Vec::new(),
        }
    }

    fn from_run_error(error: &RunError) -> Self {
        Self::new(run_error_kind(error.kind()))
    }
}

const fn run_error_kind(kind: RunErrorKind) -> &'static str {
    match kind {
        RunErrorKind::Cancelled => "cancelled",
        RunErrorKind::TotalTimeout => "total_timeout",
        RunErrorKind::ReadTimeout => "read_timeout",
        RunErrorKind::ConnectTimeout => "connect_timeout",
        RunErrorKind::StreamStopped => "stream_stopped",
        RunErrorKind::ResponseTooLarge => "response_too_large",
        RunErrorKind::Connection => "connection",
        RunErrorKind::Request => "request",
        RunErrorKind::ResponseBody => "response_body",
        RunErrorKind::Proxy => "proxy",
        RunErrorKind::Transport => "transport",
    }
}

const fn request_issue_kind(kind: RequestIssueKind) -> &'static str {
    match kind {
        RequestIssueKind::InvalidMethod => "invalid_method",
        RequestIssueKind::InvalidUrl => "invalid_url",
        RequestIssueKind::InvalidHeaderName => "invalid_header_name",
        RequestIssueKind::InvalidHeaderValue => "invalid_header_value",
        RequestIssueKind::ConflictingHeader => "conflicting_header",
        RequestIssueKind::InvalidJson => "invalid_json",
        RequestIssueKind::InvalidBody => "invalid_body",
        RequestIssueKind::FileUnavailable => "file_unavailable",
        RequestIssueKind::InvalidTemplate => "invalid_template",
        RequestIssueKind::TemplateTooDeep => "template_too_deep",
        RequestIssueKind::ResolvedValueTooLarge => "resolved_value_too_large",
        RequestIssueKind::MissingVariable => "missing_variable",
        RequestIssueKind::CyclicVariable => "cyclic_variable",
        RequestIssueKind::MissingSecret => "missing_secret",
    }
}

const fn http_version(version: HttpVersion) -> &'static str {
    match version {
        HttpVersion::Http09 => "HTTP/0.9",
        HttpVersion::Http10 => "HTTP/1.0",
        HttpVersion::Http11 => "HTTP/1.1",
        HttpVersion::Http2 => "HTTP/2",
        HttpVersion::Http3 => "HTTP/3",
        HttpVersion::Other => "HTTP/?",
    }
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

impl From<wirebolt_core::CoreHandshake> for CoreHandshake {
    fn from(value: wirebolt_core::CoreHandshake) -> Self {
        Self {
            product: value.product.to_owned(),
            core_version: value.core_version.to_owned(),
            stream_abi_version: value.stream_abi_version,
        }
    }
}

impl From<BridgeRequestDraft> for wirebolt_core::RequestDraft {
    fn from(value: BridgeRequestDraft) -> Self {
        Self {
            method: value.method,
            url: value.url,
            headers: value.headers.into_iter().map(Into::into).collect(),
            body: value.body,
        }
    }
}

impl From<HeaderField> for wirebolt_core::HeaderField {
    fn from(value: HeaderField) -> Self {
        Self {
            name: value.name,
            value: value.value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::Command,
        sync::{Condvar, Mutex},
        thread,
        time::Duration,
    };

    #[test]
    fn reorder_mixed_siblings_persists_and_rejects_incomplete_or_foreign_items() {
        let directory = tempfile::tempdir().unwrap();
        WorkspaceStore::create(directory.path(), &Workspace::new("Reorder")).unwrap();
        let path = directory.path().to_string_lossy().into_owned();
        let bridge = WorkspaceBridge::open_or_create(path.clone(), "Reorder".into()).unwrap();
        for command in [
            serde_json::json!({"kind":"create_collection","id":"api","name":"API","order":0}),
            serde_json::json!({"kind":"create_group","collection_id":"api","group":{"id":"folder","name":"Folder","order":0}}),
            serde_json::json!({"kind":"save_request","collection_id":"api","request":{"id":"a","name":"A","order":0,"method":"GET","url":"https://example.com/a","body":{"kind":"empty"}}}),
            serde_json::json!({"kind":"save_request","collection_id":"api","request":{"id":"b","name":"B","order":99,"method":"GET","url":"https://example.com/b","body":{"kind":"empty"}}}),
            serde_json::json!({"kind":"save_request","collection_id":"api","request":{"id":"nested","name":"Nested","group_id":"folder","order":42,"method":"GET","url":"https://example.com/nested","body":{"kind":"empty"}}}),
        ] {
            bridge
                .apply_workspace_command(&command.to_string())
                .unwrap();
        }
        bridge.apply_workspace_command(&serde_json::json!({"kind":"reorder_children","collection_id":"api","items":["request:b","group:folder","request:a"]}).to_string()).unwrap();
        let reopened = WorkspaceBridge::open_or_create(path, "Reorder".into()).unwrap();
        assert_eq!(reopened.request("api", "b").unwrap().order, 0);
        assert_eq!(reopened.collection("api").unwrap().groups[0].order, 1);
        assert_eq!(reopened.request("api", "a").unwrap().order, 2);
        assert_eq!(reopened.request("api", "nested").unwrap().order, 42);
        assert_eq!(
            reopened.request("api", "b").unwrap().url,
            "https://example.com/b"
        );
        let before = reopened.snapshot_json().unwrap();
        for invalid in [
            vec!["request:a"],
            vec!["request:a", "request:a", "group:folder"],
            vec!["request:a", "request:nested", "group:folder"],
            vec!["request:a", "request:missing", "group:folder"],
        ] {
            assert!(reopened.apply_workspace_command(&serde_json::json!({"kind":"reorder_children","collection_id":"api","items":invalid}).to_string()).is_err());
            assert_eq!(reopened.snapshot_json().unwrap(), before);
        }
    }

    fn test_secret(name: &str) -> ValueSource {
        ValueSource::secret(SecretName::new(name).unwrap())
    }

    /// Exports a workspace with two collections, a global and a staging environment.
    fn exported_source_workspace() -> String {
        let id = |value: &str| DocumentId::new(value).unwrap();
        let secret = test_secret;
        let source_directory = tempfile::tempdir().unwrap();
        let source =
            WorkspaceStore::create(source_directory.path(), &Workspace::new("Source")).unwrap();
        let mut request = Request::new(
            id("search"),
            "Search",
            "GET",
            "https://api.example.test/items",
        );
        request.headers = vec![RequestHeader {
            sensitive: true,
            ..RequestHeader::enabled("X-Api-Token", secret("inventory.token"))
        }];
        request.proxy_override = Some(ProxyMode::Direct);
        request.transport.validate_tls = false;
        request.inherits_workspace_transport = false;
        let documents = [
            WorkspaceDocument::Collection(Collection::new(id("inventory"), "Inventory".into())),
            WorkspaceDocument::Request {
                collection_id: id("inventory"),
                request,
            },
            WorkspaceDocument::Collection(Collection::new(id("billing"), "Billing".into())),
            WorkspaceDocument::Request {
                collection_id: id("billing"),
                request: Request::new(
                    id("invoices"),
                    "Invoices",
                    "GET",
                    "https://billing.example.test",
                ),
            },
            WorkspaceDocument::Environment(Environment::new(
                id("global"),
                "Source globals".into(),
                [(
                    "baseUrl".to_owned(),
                    ValueSource::literal("https://api.example.test"),
                )]
                .into(),
            )),
            WorkspaceDocument::Environment(Environment::new(
                id("staging"),
                "Staging".into(),
                [("token".to_owned(), secret("staging.token"))].into(),
            )),
        ];
        for document in &documents {
            source.save(document).unwrap();
        }
        WorkspaceBridge::open_or_create(
            source_directory.path().to_string_lossy().into_owned(),
            "Source".into(),
        )
        .unwrap()
        .export_workspace_json()
        .unwrap()
    }

    #[test]
    fn workspace_export_file_imports_as_separate_collections_and_environments() {
        let exported = exported_source_workspace();
        let secret = test_secret;
        let target_directory = tempfile::tempdir().unwrap();
        let target =
            WorkspaceStore::create(target_directory.path(), &Workspace::new("Target")).unwrap();
        target
            .save(&WorkspaceDocument::Environment(Environment::new(
                DocumentId::new("global").unwrap(),
                "Target globals".into(),
                BTreeMap::new(),
            )))
            .unwrap();
        let bridge = WorkspaceBridge::open_or_create(
            target_directory.path().to_string_lossy().into_owned(),
            "Target".into(),
        )
        .unwrap();
        let delta: serde_json::Value = serde_json::from_str(
            &bridge
                .commit_import_file("legacy_workspace_v1", &exported, "source-export")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(delta["affected_ids"].as_array().unwrap().len(), 2);

        let snapshot = target.load().unwrap();
        let mut collections: Vec<_> = snapshot
            .collections
            .iter()
            .map(|entry| entry.collection.name.as_str())
            .collect();
        collections.sort_unstable();
        assert_eq!(collections, ["Billing", "Inventory"]);
        let inventory = snapshot
            .collections
            .iter()
            .find(|entry| entry.collection.name == "Inventory")
            .unwrap();
        assert!(inventory.collection.groups.is_empty());
        let imported = &inventory.requests[0];
        assert_eq!(imported.headers[0].value, secret("inventory.token"));
        assert!(imported.headers[0].sensitive);
        assert_eq!(imported.proxy_override, Some(ProxyMode::Direct));
        assert!(!imported.transport.validate_tls);
        assert!(!imported.inherits_workspace_transport);

        let environment = |name: &str| {
            snapshot
                .environments
                .iter()
                .find(|environment| environment.name == name)
                .unwrap()
        };
        assert_eq!(snapshot.environments.len(), 3);
        assert_eq!(environment("Target globals").id.as_str(), "global");
        assert_ne!(environment("Source globals").id.as_str(), "global");
        assert_eq!(
            environment("Staging").variables[0].value,
            secret("staging.token")
        );
    }

    #[test]
    fn workspace_proxy_commands_round_trip_and_reset_without_changing_transport() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_string_lossy().into_owned();
        let mut workspace = Workspace::new("Proxy fixture");
        workspace.transport.total_timeout_ms = 123;
        WorkspaceStore::create(directory.path(), &workspace).unwrap();
        let bridge = WorkspaceBridge::open_or_create(path.clone(), "Proxy fixture".into()).unwrap();
        let proxy = serde_json::json!({"mode":"manual","routes":[{"destination":"all","endpoint":"http://localhost:8080"}]});
        bridge
            .apply_workspace_command(
                &serde_json::json!({"kind":"save_workspace_proxy","proxy":proxy}).to_string(),
            )
            .unwrap();
        let reopened = WorkspaceBridge::open_or_create(path, "Proxy fixture".into()).unwrap();
        let snapshot: serde_json::Value =
            serde_json::from_str(&reopened.snapshot_json().unwrap()).unwrap();
        assert_eq!(snapshot["proxy"]["mode"], "manual");
        assert_eq!(snapshot["transport"]["total_timeout_ms"], 123);
        reopened
            .apply_workspace_command(r#"{"kind":"save_workspace_proxy","proxy":null}"#)
            .unwrap();
        let reset: serde_json::Value =
            serde_json::from_str(&reopened.snapshot_json().unwrap()).unwrap();
        assert!(reset["proxy"].is_null());
        assert_eq!(reset["transport"]["total_timeout_ms"], 123);
    }

    #[test]
    fn a_failed_app_proxy_never_retries_the_destination_directly() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let unused = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_address = unused.local_addr().unwrap();
        drop(unused);
        let input = serde_json::json!({"method":"GET", "url":format!("http://{}/must-not-arrive", destination.local_addr().unwrap()),
            "app_proxy":{"mode":"manual", "routes":[{"destination":"all", "endpoint":format!("http://{proxy_address}")}]}});
        let (events, pointer) = run_to_completion(&input.to_string());
        // SAFETY: The worker has joined and this is the sole free of its context.
        let state = unsafe { Box::from_raw(pointer) };
        assert_eq!(events, ["prepared", "error"]);
        assert_eq!(
            state.failure.lock().unwrap().as_ref().unwrap()["kind"],
            "connection"
        );
        assert!(
            matches!(destination.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }

    #[test]
    fn app_proxy_routes_over_the_stream_bridge_and_request_direct_bypasses_it() {
        for direct in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let server = thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    if let Ok((stream, _)) = listener.accept() {
                        break stream;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "proxy was never reached"
                    );
                    thread::sleep(Duration::from_millis(5));
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = [0_u8; 4096];
                let count = stream.read(&mut bytes).unwrap();
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npong",
                    )
                    .unwrap();
                String::from_utf8_lossy(&bytes[..count]).into_owned()
            });
            let endpoint = if direct {
                "http://127.0.0.1:9".to_owned()
            } else {
                format!("http://{address}")
            };
            let url = if direct {
                format!("http://{address}/proxy-test")
            } else {
                "http://127.0.0.1:9/proxy-test".to_owned()
            };
            let mut input = serde_json::json!({"method":"GET","url":url,"app_proxy":{"mode":"manual","routes":[{"destination":"all","endpoint":endpoint}]}});
            if direct {
                input["request_proxy"] = serde_json::json!({"mode":"direct"});
            }
            let (events, pointer) = run_to_completion(&input.to_string());
            // SAFETY: run_to_completion joined the worker and this is the sole free.
            let state = unsafe { Box::from_raw(pointer) };
            assert_eq!(events, ["prepared", "head", "chunk:pong", "complete"]);
            let snapshot = state.prepared.lock().unwrap().clone().unwrap();
            assert_eq!(
                snapshot["proxy"]["source"],
                if direct { "request" } else { "app_default" }
            );
            assert_eq!(
                snapshot["proxy"]["configuration"]["mode"],
                if direct { "direct" } else { "manual" }
            );
            let request = server.join().unwrap();
            assert!(request.starts_with(if direct {
                "GET /proxy-test "
            } else {
                "GET http://127.0.0.1:9/proxy-test "
            }));
        }
    }

    #[test]
    fn imports_after_reopening_keep_previously_imported_collections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_string_lossy().into_owned();
        let first = WorkspaceBridge::open_or_create(path.clone(), "Fixture".into()).unwrap();
        let first_delta: serde_json::Value = serde_json::from_str(
            &first
                .commit_import("curl", "curl 'https://example.com/first'")
                .unwrap(),
        )
        .unwrap();
        drop(first);
        let reopened = WorkspaceBridge::open_or_create(path, "Fixture".into()).unwrap();
        let second_delta: serde_json::Value = serde_json::from_str(
            &reopened
                .commit_import("curl", "curl 'https://example.com/second'")
                .unwrap(),
        )
        .unwrap();
        assert_ne!(first_delta["affected_ids"], second_delta["affected_ids"]);
        let snapshot: serde_json::Value =
            serde_json::from_str(&reopened.snapshot_json().unwrap()).unwrap();
        assert_eq!(snapshot["collections"].as_array().unwrap().len(), 2);
        let serialized = snapshot.to_string();
        assert!(serialized.contains("https://example.com/first"));
        assert!(serialized.contains("https://example.com/second"));
        assert!(
            reopened
                .commit_import("legacy_workspace_v1", r#"{"version":2,"nodes":[]}"#)
                .is_err()
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&reopened.snapshot_json().unwrap()).unwrap(),
            snapshot
        );
    }

    #[test]
    fn both_bridge_surfaces_share_the_same_version() {
        let handshake = core_handshake();

        assert_eq!(handshake.product, "Wirebolt");
        assert_eq!(handshake.stream_abi_version, wirebolt_stream_abi_version());
    }

    #[test]
    fn prepares_a_request_through_the_coarse_bridge() {
        let summary = prepare_request(BridgeRequestDraft {
            method: "POST".to_owned(),
            url: "https://api.example.com/v1/items".to_owned(),
            headers: vec![HeaderField {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
            }],
            body: br#"{"fast":true}"#.to_vec(),
        })
        .expect("valid request");

        assert_eq!(summary.method, "POST");
        assert_eq!(summary.url, "https://api.example.com/v1/items");
        assert_eq!(summary.header_count, 1);
        assert_eq!(summary.body_bytes, 13);
    }

    #[test]
    fn stream_abi_delivers_head_chunks_and_completion_in_order() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept HTTP client");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read HTTP request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nX-Wirebolt: ffi\r\nConnection: close\r\n\r\npong",
                )
                .expect("write HTTP response");
        });
        let input = serde_json::json!({
            "method": "GET",
            "url": format!("http://{address}/ffi"),
            "workspace_proxy": { "mode": "direct" }
        })
        .to_string();
        let (events, state_pointer) = run_to_completion(&input);
        server.join().expect("HTTP server");
        assert_eq!(events, ["prepared", "head", "chunk:pong", "complete"]);
        // SAFETY: The callback context is no longer used after session join.
        drop(unsafe { Box::from_raw(state_pointer) });
    }

    #[test]
    fn failure_documents_always_carry_an_issues_array() {
        let input = serde_json::json!({
            "method": "GET",
            "url": "http://127.0.0.1:1/refused",
            "workspace_proxy": { "mode": "direct" }
        })
        .to_string();
        let (events, state_pointer) = run_to_completion(&input);
        // SAFETY: The callback context is no longer used after session join.
        let state = unsafe { Box::from_raw(state_pointer) };

        assert_eq!(events, ["prepared", "error"]);
        let failure = state
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("failure document");
        assert!(failure["issues"].is_array(), "{failure}");
        assert_eq!(failure["kind"], "connection");
    }

    #[test]
    fn workspace_bridge_round_trips_hierarchy_without_secret_values() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        let mut workspace = Workspace::new("Demo");
        workspace.proxy = Some(ProxyMode::Direct);
        WorkspaceStore::create(temporary.path(), &workspace).expect("create workspace");
        let bridge = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Demo".to_owned(),
        )
        .expect("workspace bridge");
        bridge
            .save_collection("api".to_owned(), "API".to_owned())
            .expect("save collection");
        bridge
            .save_request(
                "api".to_owned(),
                serde_json::json!({
                    "id": "health",
                    "name": "Health",
                    "method": "GET",
                    "url": "{{endpoint}}",
                    "web_socket": true,
                    "note": "café 東京 🚀",
                    "authentication": {
                        "kind": "bearer",
                        "token": { "secret": "api.token" }
                    },
                    "body": { "kind": "empty" }
                })
                .to_string()
                .as_str(),
            )
            .expect("save request");
        std::fs::write(
            temporary.path().join("environments/broken.toml"),
            "<<<<<<< HEAD\nid = \"broken\"\n=======\n",
        )
        .expect("write conflicted environment");

        let snapshot = bridge.snapshot_json().expect("workspace snapshot");
        let document: serde_json::Value = serde_json::from_str(&snapshot).expect("snapshot JSON");

        assert_eq!(
            document["collections"][0]["requests"][0]["web_socket"],
            true
        );
        assert_eq!(
            document["collections"][0]["requests"][0]["note"],
            "café 東京 🚀"
        );
        let reopened = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Demo".to_owned(),
        )
        .expect("reopen");
        let restored: serde_json::Value =
            serde_json::from_str(&reopened.snapshot_json().unwrap()).unwrap();
        assert_eq!(
            restored["collections"][0]["requests"][0]["web_socket"],
            true
        );
        assert_eq!(
            restored["collections"][0]["requests"][0]["note"],
            "café 東京 🚀"
        );
        assert_eq!(document["name"], "Demo");
        assert_eq!(document["proxy"]["mode"], "direct");
        assert_eq!(document["collections"][0]["id"], "api");
        assert_eq!(document["collections"][0]["requests"][0]["id"], "health");
        assert_eq!(
            document["collections"][0]["requests"][0]["authentication"]["token"]["secret"],
            "api.token"
        );
        assert_eq!(document["problems"][0]["path"], "environments/broken.toml");
        assert_eq!(document["problems"][0]["kind"], "invalid_toml");
        assert!(!snapshot.contains("secret-value"));
        let request_export = bridge
            .export_request_json("api", "health")
            .expect("export request");
        let collection_export = bridge
            .export_collection_json("api")
            .expect("export collection");
        let workspace_export = bridge.export_workspace_json().expect("export workspace");
        for exported in [request_export, collection_export, workspace_export] {
            assert!(exported.contains("api.token"));
            assert!(!exported.contains("secret-value"));
        }
    }

    #[test]
    fn workspace_commands_persist_groups_and_return_monotonic_deltas() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        WorkspaceStore::create(temporary.path(), &Workspace::new("Commands"))
            .expect("create workspace");
        let bridge = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Commands".to_owned(),
        )
        .expect("workspace bridge");

        let collection_delta = bridge
            .apply_workspace_command(
                &serde_json::json!({
                    "kind": "create_collection",
                    "id": "api",
                    "name": "API",
                    "order": 0
                })
                .to_string(),
            )
            .expect("create collection");
        let group_delta = bridge
            .apply_workspace_command(
                &serde_json::json!({
                    "kind": "create_group",
                    "collection_id": "api",
                    "group": {
                        "id": "auth",
                        "name": "Authentication",
                        "parent_id": null,
                        "order": 0
                    }
                })
                .to_string(),
            )
            .expect("create group");
        bridge
            .apply_workspace_command(
                &serde_json::json!({
                    "kind": "save_request",
                    "collection_id": "api",
                    "request": {
                        "id": "login",
                        "name": "Login",
                        "group_id": "auth",
                        "order": 0,
                        "method": "POST",
                        "url": "https://example.com/login",
                        "body": { "kind": "empty" }
                    }
                })
                .to_string(),
            )
            .expect("save grouped request");

        let first: serde_json::Value =
            serde_json::from_str(&collection_delta).expect("collection delta");
        let second: serde_json::Value = serde_json::from_str(&group_delta).expect("group delta");
        assert_eq!(first["version"], 1);
        assert_eq!(second["version"], 2);

        let snapshot: serde_json::Value =
            serde_json::from_str(&bridge.snapshot_json().expect("workspace snapshot"))
                .expect("snapshot JSON");
        assert_eq!(snapshot["collections"][0]["groups"][0]["id"], "auth");
        assert_eq!(
            snapshot["collections"][0]["requests"][0]["group_id"],
            "auth"
        );
    }

    #[test]
    fn workspace_bridge_exposes_git_operations_as_stable_json() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        WorkspaceStore::create(temporary.path(), &Workspace::new("Demo"))
            .expect("create workspace");
        let bridge = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Demo".to_owned(),
        )
        .expect("workspace bridge");
        for arguments in [
            &["init", "-b", "main"][..],
            &["config", "user.name", "Wirebolt Tests"][..],
            &["config", "user.email", "wirebolt@example.invalid"][..],
            &["config", "commit.gpgsign", "false"][..],
        ] {
            let output = Command::new("git")
                .arg("-C")
                .arg(temporary.path())
                .args(arguments)
                .output()
                .expect("run Git");
            assert!(output.status.success());
        }

        let before: serde_json::Value =
            serde_json::from_str(&bridge.git_status_json().expect("Git status JSON"))
                .expect("decode Git status");
        assert_eq!(before["branch"], "main");
        assert_eq!(before["revision"], serde_json::Value::Null);
        assert_eq!(before["changes"][0]["path"], "wirebolt.toml");
        assert_eq!(before["changes"][0]["unstaged"], "untracked");

        let operation: serde_json::Value = serde_json::from_str(
            &bridge
                .git_commit_json("initial workspace")
                .expect("Git commit JSON"),
        )
        .expect("decode Git operation");
        assert_eq!(operation["outcome"], "committed");
        assert!(operation["revision"].is_string());
        assert_eq!(operation["status"]["changes"], serde_json::json!([]));
        assert_eq!(operation["status"]["merging"], false);

        let Err(GitBridgeError::OperationFailed { kind, .. }) = bridge.git_abort_merge_json()
        else {
            panic!("aborting without a merge must fail");
        };
        assert_eq!(kind, "no_merge_in_progress");
    }

    #[test]
    fn workspace_bridge_initializes_a_repository_at_the_workspace_root() {
        let temporary = tempfile::tempdir().expect("temporary parent");
        let root = temporary.path().join("Payments API");
        let bridge = WorkspaceBridge::open_or_create(
            root.to_string_lossy().into_owned(),
            "Payments API".to_owned(),
        )
        .expect("workspace bridge");
        let Err(GitBridgeError::OperationFailed { kind, .. }) = bridge.git_status_json() else {
            panic!("a new workspace folder is not a repository");
        };
        assert_eq!(kind, "not_repository");

        let status: serde_json::Value =
            serde_json::from_str(&bridge.git_initialize_json().expect("initialize Git"))
                .expect("decode Git status");

        assert_eq!(status["revision"], serde_json::Value::Null);
        assert_eq!(status["merging"], false);
        assert!(root.join(".git").exists());
        assert!(bridge.git_status_json().is_ok());
    }

    #[test]
    fn rename_workspace_persists_a_trimmed_name_and_rejects_blank_names() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        let path = temporary.path().to_string_lossy().into_owned();
        let bridge = WorkspaceBridge::open_or_create(path.clone(), "Checkout".into())
            .expect("workspace bridge");

        let delta: serde_json::Value = serde_json::from_str(
            &bridge
                .apply_workspace_command(
                    &serde_json::json!({"kind":"rename_workspace","name":"  Checkout Team  "})
                        .to_string(),
                )
                .expect("rename workspace"),
        )
        .expect("delta JSON");
        assert_eq!(delta["kind"], "workspace");
        assert!(
            bridge
                .apply_workspace_command(
                    &serde_json::json!({"kind":"rename_workspace","name":"   "}).to_string()
                )
                .is_err()
        );

        let reopened = WorkspaceBridge::open_or_create(path, "Ignored".into()).expect("reopen");
        let snapshot: serde_json::Value =
            serde_json::from_str(&reopened.snapshot_json().expect("snapshot")).expect("JSON");
        assert_eq!(snapshot["name"], "Checkout Team");
    }

    /// Each test owns its cache, so resets in one test cannot race lookups
    /// in another the way a shared static would.
    fn engine_for(cache: &HttpEngineCache, proxy: &ResolvedProxy) -> Arc<HttpEngine> {
        cache
            .engine(proxy.mode(), || {
                HttpEngine::with_proxy(
                    &HttpEngineConfig::default(),
                    proxy,
                    &wirebolt_core::NoSecrets,
                )
            })
            .expect("HTTP engine")
    }

    fn manual_proxy_mode() -> ProxyMode {
        let route = wirebolt_core::ProxyRoute::new(
            wirebolt_core::ProxyDestination::All,
            wirebolt_core::ProxyEndpoint::new("http://proxy.internal:8080")
                .expect("proxy endpoint"),
        );
        ProxyMode::Manual(wirebolt_core::ManualProxy::new(vec![route]).expect("manual proxy"))
    }

    #[test]
    fn shared_direct_engine_reuses_the_same_client_pool() {
        let cache = HttpEngineCache::default();
        let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);

        let first = engine_for(&cache, &proxy);
        let second = engine_for(&cache, &proxy);

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn shared_manual_engine_is_keyed_by_mode_and_reset_on_demand() {
        let cache = HttpEngineCache::default();
        let manual = manual_proxy_mode();
        let from_workspace = ProxyPolicy::with_workspace(manual.clone()).resolve(None);
        let from_request = ProxyPolicy::default().resolve(Some(&manual));

        let first = engine_for(&cache, &from_workspace);
        let second = engine_for(&cache, &from_request);
        assert!(Arc::ptr_eq(&first, &second));

        cache.reset();
        let replacement = engine_for(&cache, &from_workspace);
        assert!(!Arc::ptr_eq(&first, &replacement));
    }

    #[test]
    fn reset_retires_every_slot_at_once() {
        let cache = HttpEngineCache::default();
        let direct = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);
        let system = ProxyPolicy::with_workspace(ProxyMode::System).resolve(None);
        let manual = ProxyPolicy::with_workspace(manual_proxy_mode()).resolve(None);
        let before = [
            engine_for(&cache, &direct),
            engine_for(&cache, &system),
            engine_for(&cache, &manual),
        ];

        cache.reset();

        let after = [
            engine_for(&cache, &direct),
            engine_for(&cache, &system),
            engine_for(&cache, &manual),
        ];
        for (old, new) in before.iter().zip(&after) {
            assert!(!Arc::ptr_eq(old, new));
        }
        for (old, new) in after.iter().zip([
            engine_for(&cache, &direct),
            engine_for(&cache, &system),
            engine_for(&cache, &manual),
        ]) {
            assert!(Arc::ptr_eq(old, &new));
        }
    }

    #[test]
    fn an_engine_built_while_a_reset_ran_is_used_once_but_never_pooled() {
        let cache = HttpEngineCache::default();
        let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);

        let raced = cache
            .engine(proxy.mode(), || {
                // Simulates a network change landing while the client is
                // still being built with the pre-change proxy settings.
                cache.reset();
                HttpEngine::with_proxy(
                    &HttpEngineConfig::default(),
                    &proxy,
                    &wirebolt_core::NoSecrets,
                )
            })
            .expect("engine for this run");

        let pooled = engine_for(&cache, &proxy);
        assert!(!Arc::ptr_eq(&raced, &pooled));
        assert!(Arc::ptr_eq(&pooled, &engine_for(&cache, &proxy)));
    }

    #[test]
    fn concurrent_lookups_and_resets_never_hand_out_a_retired_engine() {
        let cache = Arc::new(HttpEngineCache::default());
        let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let proxy = proxy.clone();
                thread::spawn(move || {
                    for _ in 0..50 {
                        drop(engine_for(&cache, &proxy));
                    }
                })
            })
            .collect();
        for _ in 0..20 {
            cache.reset();
            thread::sleep(Duration::from_millis(1));
        }
        for worker in workers {
            worker.join().expect("worker thread");
        }

        let retired = engine_for(&cache, &proxy);
        cache.reset();
        let fresh = engine_for(&cache, &proxy);
        assert!(!Arc::ptr_eq(&retired, &fresh));
        assert!(Arc::ptr_eq(&fresh, &engine_for(&cache, &proxy)));
    }

    #[test]
    fn zero_timeouts_disable_the_deadline() {
        assert_eq!(timeout_from_millis(0), None);
        assert_eq!(timeout_from_millis(250), Some(Duration::from_millis(250)));
    }

    fn run_to_completion(input: &str) -> (Vec<String>, *mut CallbackState) {
        let state_pointer = Box::into_raw(Box::new(CallbackState::default()));
        let callbacks = WireboltRunCallbacks {
            on_cookies: None,
            on_prepared: Some(record_prepared),
            on_head: Some(record_head),
            on_chunk: Some(record_chunk),
            on_complete: Some(record_complete),
            on_error: Some(record_error),
        };

        // SAFETY: The input and callback context outlive the run session.
        let session = unsafe {
            wirebolt_run_start(input.as_ptr(), input.len(), callbacks, state_pointer.cast())
        };
        assert!(!session.is_null());
        // SAFETY: `state_pointer` remains owned by the caller until after join.
        let state = unsafe { &*state_pointer };
        let events = state.wait_for_terminal();

        // SAFETY: The worker reached a terminal callback and this is the sole free.
        unsafe { wirebolt_run_free(session) };
        (events, state_pointer)
    }

    #[derive(Debug, Default)]
    struct CallbackState {
        events: Mutex<Vec<String>>,
        failure: Mutex<Option<serde_json::Value>>,
        prepared: Mutex<Option<serde_json::Value>>,
        terminal: Condvar,
    }

    impl CallbackState {
        fn wait_for_terminal(&self) -> Vec<String> {
            let events = self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (events, timeout) = self
                .terminal
                .wait_timeout_while(events, Duration::from_secs(5), |events| {
                    !events
                        .last()
                        .is_some_and(|event| matches!(event.as_str(), "complete" | "error"))
                })
                .expect("wait for terminal callback");
            assert!(!timeout.timed_out(), "run callback timed out");
            events.clone()
        }
    }

    extern "C" fn record_prepared(context: *mut c_void, json: *const u8, length: usize) {
        let state = callback_state(context);
        let document: serde_json::Value =
            serde_json::from_slice(callback_bytes(json, length)).expect("prepared snapshot JSON");
        assert_eq!(document["method"], "GET");
        assert!(document["headers"].is_array());
        *state.prepared.lock().unwrap() = Some(document);
        state
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push("prepared".to_owned());
    }

    extern "C" fn record_head(context: *mut c_void, json: *const u8, length: usize) {
        let state = callback_state(context);
        let document = callback_bytes(json, length);
        let head: serde_json::Value = serde_json::from_slice(document).expect("head JSON");
        assert_eq!(head["status"], 200);
        assert!(head["warnings"].is_array());
        state
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push("head".to_owned());
    }

    extern "C" fn record_chunk(context: *mut c_void, bytes: *const u8, length: usize) -> u8 {
        let state = callback_state(context);
        let body = String::from_utf8_lossy(callback_bytes(bytes, length));
        state
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(format!("chunk:{body}"));
        1
    }

    extern "C" fn record_complete(context: *mut c_void, json: *const u8, length: usize) {
        let state = callback_state(context);
        let document: serde_json::Value =
            serde_json::from_slice(callback_bytes(json, length)).expect("completion JSON");
        assert_eq!(document["bytes_received"], 4);
        assert_eq!(document["bytes_decoded"], 4);
        state
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push("complete".to_owned());
        state.terminal.notify_all();
    }

    extern "C" fn record_error(context: *mut c_void, json: *const u8, length: usize) {
        let state = callback_state(context);
        let document: serde_json::Value =
            serde_json::from_slice(callback_bytes(json, length)).expect("failure JSON");
        *state
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(document);
        state
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push("error".to_owned());
        state.terminal.notify_all();
    }

    fn callback_state(context: *mut c_void) -> &'static CallbackState {
        // SAFETY: Tests pass a live `CallbackState` for every callback.
        unsafe { &*context.cast::<CallbackState>() }
    }

    fn callback_bytes(pointer: *const u8, length: usize) -> &'static [u8] {
        // SAFETY: The ABI guarantees readable bytes for the callback duration.
        unsafe { std::slice::from_raw_parts(pointer, length) }
    }
}
