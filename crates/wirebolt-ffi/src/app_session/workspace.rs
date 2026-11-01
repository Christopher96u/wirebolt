//! Typed workspace application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{
    AppSecretPlatform, AppSessionLayout, AppSessionSecrets, AppSessionTabs, AppWorkspaceDocuments,
    AppWorkspacePlatform, Arc, DomainAuthentication, DomainCollection, DomainEnvironment,
    DomainGroup, DomainLocation, DomainProxy, DomainRequestDraft, DomainTransport, DomainWorkspace,
    Path, SessionCommandBatchResult, SessionDeleteTarget, SessionDocument, SessionDocumentEffect,
    SessionEditorGroup, SessionRestoreResult, SessionTabDocument, SessionTabSnapshot,
    SessionWorkspaceCommand, SessionWorkspaceDelta, SessionWorkspaceEdit, SessionWorkspaceNodeKind,
    SessionWorkspacePlan, WorkspaceAction, WorkspaceBridge, WorkspaceClose, WorkspaceDiscard,
    WorkspaceExport, WorkspaceFailure, WorkspaceGitAction, WorkspaceGitOperation,
    WorkspaceGitStatus, WorkspaceImport, WorkspaceImportFile, WorkspaceImportSummary,
    WorkspaceLoad, WorkspaceMutation, WorkspacePresentation, WorkspaceRepositoryProblem,
    WorkspaceRestore, WorkspaceSave, WorkspaceSelectionProblem, WorkspaceUndoToken, lock,
};
#[uniffi::remote(Enum)]
pub enum SessionWorkspaceEdit {
    RestoreEnvironment {
        environment: DomainEnvironment,
        index: u64,
    },
    RemoveEnvironment {
        id: String,
    },
    RestoreRequest {
        location: DomainLocation,
    },
    RemoveRequest {
        collection_id: String,
        id: String,
    },
    DuplicateRequest {
        collection_id: String,
        id: String,
        new_id: String,
        name: String,
    },
    RestoreGroups {
        collection_id: String,
        groups: Vec<DomainGroup>,
        requests: Vec<DomainLocation>,
    },
    RemoveGroup {
        collection_id: String,
        id: String,
    },
    RestoreCollection {
        collection: DomainCollection,
    },
    RemoveCollection {
        id: String,
    },
    RenameRequest {
        collection_id: String,
        id: String,
        name: String,
    },
    RenameGroup {
        collection_id: String,
        id: String,
        name: String,
    },
    RenameCollection {
        id: String,
        name: String,
    },
    MoveRequest {
        from_collection_id: String,
        id: String,
        to_collection_id: String,
        group_id: Option<String>,
        order: i64,
    },
    MoveGroup {
        collection_id: String,
        id: String,
        parent_id: Option<String>,
        order: i64,
    },
    Reorder {
        collection_id: String,
        parent_id: Option<String>,
        items: Vec<String>,
    },
}

#[uniffi::remote(Enum)]
pub enum SessionWorkspaceCommand {
    SaveWorkspaceProxy {
        proxy: Option<DomainProxy>,
    },
    SaveWorkspaceSettings {
        transport: DomainTransport,
    },
    RenameWorkspace {
        name: String,
    },
    SaveEnvironment {
        environment: DomainEnvironment,
    },
    DeleteEnvironment {
        id: String,
    },
    SaveRequest {
        location: DomainLocation,
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
    CreateGroup {
        collection_id: String,
        group: DomainGroup,
    },
    DeleteGroup {
        collection_id: String,
        id: String,
    },
    CreateCollection {
        collection: DomainCollection,
    },
    DeleteCollection {
        id: String,
    },
    RenameGroup {
        collection_id: String,
        id: String,
        name: String,
    },
    RenameCollection {
        id: String,
        name: String,
    },
    MoveRequest {
        from_collection_id: String,
        request_id: String,
        to_collection_id: String,
        group_id: Option<String>,
        order: i64,
    },
    MoveGroup {
        collection_id: String,
        id: String,
        parent_id: Option<String>,
        order: i64,
    },
    ReorderChildren {
        collection_id: String,
        parent_id: Option<String>,
        items: Vec<String>,
    },
}

#[uniffi::remote(Record)]
pub struct SessionWorkspacePlan {
    commands: Vec<SessionWorkspaceCommand>,
    inverse: SessionWorkspaceEdit,
    collections: Vec<DomainCollection>,
    removed_collection_ids: Vec<String>,
    environments: Option<Vec<DomainEnvironment>>,
    selected_environment_id: Option<String>,
    changes_environment_selection: bool,
    effects: Vec<SessionDocumentEffect>,
    observed_secrets: Vec<String>,
}

#[derive(Debug, uniffi::Object)]
pub struct AppWorkspaceSession {
    pub(super) inner: Arc<wirebolt_app::session::workspace::WorkspaceSession>,
}

#[uniffi::export]
impl AppWorkspaceSession {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(wirebolt_app::session::workspace::WorkspaceSession::default()),
        })
    }
    pub fn replace(&self, workspace: DomainWorkspace) {
        self.inner.replace(workspace);
    }
    pub fn plan(
        &self,
        selected: Option<String>,
        step: SessionWorkspaceEdit,
    ) -> Option<SessionWorkspacePlan> {
        self.inner.plan(selected, step)
    }
    pub fn commit(&self, plan: SessionWorkspacePlan) {
        self.inner.commit(plan);
    }
}

#[uniffi::remote(Record)]
pub struct SessionWorkspaceDelta {
    pub version: u64,
    pub kind: super::SessionWorkspaceNodeKind,
    pub affected_ids: Vec<String>,
}

#[uniffi::export]
impl WorkspaceBridge {
    /// # Errors
    /// Returns the core storage error.
    pub fn apply_session_command(
        &self,
        command: SessionWorkspaceCommand,
    ) -> Result<SessionWorkspaceDelta, crate::WorkspaceBridgeError> {
        self.inner.apply_session_command(command)
    }
    /// # Errors
    /// Returns the first core storage error.
    pub fn apply_session_commands(
        &self,
        commands: Vec<SessionWorkspaceCommand>,
    ) -> Result<(), crate::WorkspaceBridgeError> {
        self.inner.apply_session_commands(commands)
    }
}

#[uniffi::export]
impl WorkspaceBridge {
    /// # Errors
    /// Returns a workspace storage or projection error.
    pub fn load_session(
        &self,
        application: Arc<AppWorkspaceSession>,
    ) -> Result<DomainWorkspace, crate::WorkspaceBridgeError> {
        let workspace = self.inner.domain_snapshot()?;
        application.inner.replace(workspace.clone());
        Ok(workspace)
    }
}

#[uniffi::remote(Enum)]
pub enum SessionDeleteTarget {
    Collection { id: String },
    Group { collection_id: String, id: String },
    Request { collection_id: String, id: String },
}

#[uniffi::export]
impl AppWorkspaceSession {
    pub fn delete_consequences(
        &self,
        target: SessionDeleteTarget,
        documents: Vec<SessionTabDocument>,
    ) -> String {
        self.inner.delete_consequences(target, &documents)
    }
}

#[uniffi::remote(Record)]
pub struct SessionCommandBatchResult {
    pub completed: u64,
    pub failure_reason: Option<String>,
}

#[uniffi::export]
impl AppWorkspaceSession {
    pub fn partial(
        &self,
        plan: SessionWorkspacePlan,
        completed: u64,
    ) -> Option<SessionWorkspacePlan> {
        self.inner.partial(plan, completed)
    }
}

#[uniffi::export]
impl WorkspaceBridge {
    #[must_use]
    pub fn execute_session_commands(
        &self,
        commands: Vec<SessionWorkspaceCommand>,
    ) -> SessionCommandBatchResult {
        self.inner.execute_session_commands(commands)
    }
}

#[uniffi::export]
pub fn session_existing_workspace_paths(paths: Vec<String>) -> Vec<String> {
    wirebolt_app::session::tabs::existing_workspace_paths(paths)
}

#[uniffi::remote(Enum)]
pub enum WorkspaceAction {
    SaveEnvironment {
        environment: DomainEnvironment,
    },
    DeleteEnvironment {
        id: String,
    },
    CreateRequest {
        web_socket: bool,
        collection_id: Option<String>,
        group_id: Option<String>,
    },
    CreateCollection {
        name: String,
    },
    CreateGroup {
        collection_id: String,
        parent_id: Option<String>,
        name: String,
    },
    RenameRequest {
        collection_id: String,
        id: String,
        name: String,
    },
    RenameGroup {
        collection_id: String,
        id: String,
        name: String,
    },
    RenameCollection {
        id: String,
        name: String,
    },
    DeleteRequest {
        collection_id: String,
        id: String,
    },
    DeleteGroup {
        collection_id: String,
        id: String,
    },
    DeleteCollection {
        id: String,
    },
    DuplicateRequest {
        collection_id: String,
        id: String,
    },
    MoveRequest {
        from_collection_id: String,
        id: String,
        to_collection_id: String,
        group_id: Option<String>,
        order: i64,
    },
    MoveGroup {
        collection_id: String,
        id: String,
        parent_id: Option<String>,
        order: i64,
    },
    Reorder {
        identifier: String,
        target: String,
        after: bool,
        collection_id: String,
        parent_id: Option<String>,
    },
    MoveBy {
        identifier: String,
        delta: i64,
    },
    MoveTo {
        identifier: String,
        collection_id: String,
        parent_id: Option<String>,
    },
}

#[uniffi::remote(Record)]
pub struct WorkspacePresentation {
    pub loading: bool,
    pub loaded: bool,
    pub git_busy: bool,
    pub importing: bool,
    pub git_status: Option<WorkspaceGitStatus>,
    pub git_operation: Option<WorkspaceGitOperation>,
    pub git_failure: Option<WorkspaceFailure>,
    pub import_failure: Option<String>,
    pub export_failure: Option<String>,
    pub import_summary: Option<WorkspaceImportSummary>,
    pub operation_failure: Option<wirebolt_app::response::ResponseFailureKind>,
}

#[uniffi::remote(Record)]
pub struct WorkspaceUndoToken {
    pub token: u64,
    pub name: String,
}

#[uniffi::remote(Record)]
pub struct WorkspaceMutation {
    pub succeeded: bool,
    pub plan: Option<SessionWorkspacePlan>,
    pub undo: Option<WorkspaceUndoToken>,
    pub created_id: Option<String>,
    pub root: Option<DomainCollection>,
}

#[uniffi::remote(Record)]
pub struct WorkspaceLoad {
    pub workspace: DomainWorkspace,
    pub selected_environment_id: Option<String>,
    pub select: Option<DomainLocation>,
}

#[uniffi::remote(Record)]
pub struct WorkspaceSave {
    pub original_authentication: DomainAuthentication,
    pub document_id: String,
    pub draft: DomainRequestDraft,
    pub location: Option<DomainLocation>,
    pub succeeded: bool,
}

#[derive(Debug, uniffi::Object)]
pub struct AppWorkspaceOperations {
    pub(super) inner: wirebolt_app::session::workspace::operations::WorkspaceOperations,
}

#[uniffi::export]
impl AppWorkspaceOperations {
    #[uniffi::constructor]
    pub fn new(workspace: Arc<AppWorkspaceSession>) -> Arc<Self> {
        Arc::new(Self {
            inner: wirebolt_app::session::workspace::operations::WorkspaceOperations::new(
                workspace.inner.clone(),
            ),
        })
    }
    pub fn presentation(&self) -> WorkspacePresentation {
        self.inner.presentation()
    }
    pub fn clear_undo(&self) {
        self.inner.clear_undo();
    }
    pub fn begin_load(&self, configured: bool) -> bool {
        self.inner.begin_load(configured)
    }
    pub fn reverse_token(&self, token: u64) -> Option<WorkspaceUndoToken> {
        self.inner.reverse_token(token)
    }
    pub fn begin_git(&self, configured: bool, action: WorkspaceGitAction, dirty: bool) -> bool {
        self.inner.begin_git(configured, action, dirty)
    }
    pub fn begin_import(&self, configured: bool, pasted: bool) -> bool {
        self.inner.begin_import(configured, pasted)
    }
    pub fn finish_import_failure(&self, message: String, pasted: bool) -> String {
        self.inner.finish_import_failure(message, pasted)
    }
    pub fn can_reorder(
        &self,
        identifier: String,
        target: String,
        collection_id: String,
        parent: Option<String>,
    ) -> bool {
        self.inner
            .can_reorder(identifier, target, collection_id, parent)
    }
    pub fn can_move_to(
        &self,
        identifier: String,
        collection_id: String,
        parent: Option<String>,
    ) -> bool {
        self.inner.can_move_to(identifier, collection_id, parent)
    }
    pub fn next_order(&self, collection_id: String, parent: Option<String>) -> i64 {
        self.inner.next_order(collection_id, parent)
    }
    pub fn new_request(
        &self,
        web_socket: bool,
        collection_id: String,
        group_id: Option<String>,
    ) -> DomainLocation {
        self.inner.new_request(web_socket, collection_id, group_id)
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AppWorkspaceOperations {
    pub async fn load(
        &self,
        platform: Arc<AppWorkspacePlatform>,
        switching: bool,
    ) -> Option<WorkspaceLoad> {
        self.inner.load(platform.as_ref(), switching).await
    }
    pub async fn rename(
        &self,
        proposed: String,
        platform: Arc<AppWorkspacePlatform>,
    ) -> Option<String> {
        self.inner.rename(proposed, platform.as_ref()).await
    }
    pub async fn mutate(
        &self,
        command: WorkspaceAction,
        selected: Option<String>,
        platform: Arc<AppWorkspacePlatform>,
        secrets: Arc<AppSessionSecrets>,
        secret_platform: Arc<AppSecretPlatform>,
    ) -> WorkspaceMutation {
        self.inner
            .mutate(
                command,
                selected,
                platform.as_ref(),
                &secrets.inner,
                secret_platform.as_ref(),
            )
            .await
    }
    pub async fn undo(
        &self,
        token: u64,
        reverse: u64,
        selected: Option<String>,
        platform: Arc<AppWorkspacePlatform>,
        secrets: Arc<AppSessionSecrets>,
        secret_platform: Arc<AppSecretPlatform>,
    ) -> WorkspaceMutation {
        self.inner
            .undo(
                token,
                reverse,
                selected,
                platform.as_ref(),
                &secrets.inner,
                secret_platform.as_ref(),
            )
            .await
    }
    pub async fn save(
        &self,
        document: SessionDocument,
        fallback: Option<String>,
        platform: Arc<AppWorkspacePlatform>,
        secrets: Arc<AppSessionSecrets>,
        secret_platform: Arc<AppSecretPlatform>,
    ) -> WorkspaceSave {
        self.inner
            .save(
                document,
                fallback,
                platform.as_ref(),
                &secrets.inner,
                secret_platform.as_ref(),
            )
            .await
    }
    pub async fn git(
        &self,
        action: WorkspaceGitAction,
        platform: Arc<AppWorkspacePlatform>,
        persistence: Option<Arc<AppWorkspacePlatform>>,
    ) -> Option<WorkspaceLoad> {
        self.inner
            .git(
                action,
                platform.as_ref(),
                persistence
                    .as_deref()
                    .map(|p| p as &dyn wirebolt_app::session::workspace::io::WorkspacePlatform),
            )
            .await
    }
    pub async fn import(
        &self,
        request: WorkspaceImport,
        platform: Arc<AppWorkspacePlatform>,
    ) -> Option<WorkspaceLoad> {
        self.inner.import(request, platform.as_ref()).await
    }
    pub async fn export(
        &self,
        target: WorkspaceExport,
        platform: Arc<AppWorkspacePlatform>,
    ) -> Option<String> {
        self.inner.export(target, platform.as_ref()).await
    }
}

#[uniffi::export]
pub fn workspace_new_environment() -> DomainEnvironment {
    wirebolt_app::session::workspace::actions::new_environment()
}

#[uniffi::export]
pub fn workspace_import_failure(reason: Option<String>, name: Option<String>) -> String {
    wirebolt_app::session::workspace::operations::import_failure_message(
        reason.as_deref(),
        name.as_deref(),
    )
}

#[uniffi::remote(Record)]
pub struct WorkspaceDiscard {
    pub id: String,
    pub saved: Option<DomainRequestDraft>,
}

#[uniffi::remote(Record)]
pub struct WorkspaceRestore {
    pub result: SessionRestoreResult,
    pub restore_history: bool,
}

#[uniffi::export]
pub fn workspace_dirty_ids(
    groups: Vec<SessionEditorGroup>,
    documents: Vec<SessionTabDocument>,
) -> Vec<String> {
    wirebolt_app::session::workspace::lifecycle::dirty_ids(&groups, &documents)
}

#[uniffi::export]
pub fn workspace_discard(documents: Vec<SessionDocument>) -> Vec<WorkspaceDiscard> {
    wirebolt_app::session::workspace::lifecycle::discard(documents)
}

#[uniffi::export]
pub fn workspace_move_row_id(identifier: String) -> Option<String> {
    wirebolt_app::session::workspace::actions::move_row_id(&identifier)
}

#[uniffi::export]
impl AppWorkspaceOperations {
    pub fn restore_tabs(
        &self,
        tabs: Arc<AppSessionTabs>,
        layout: Option<AppSessionLayout>,
    ) -> WorkspaceRestore {
        let mut state = lock(&tabs.tabs);
        let (tabs, documents) = &mut *state;
        self.inner.restore_tabs(tabs, documents, layout)
    }
}

#[uniffi::remote(Enum)]
pub enum WorkspaceImportFile {
    Bruno,
    Document {
        source: String,
        format: crate::app_domain::DomainImportFormat,
    },
    Failure {
        message: String,
    },
}

#[uniffi::export]
pub fn workspace_read_import(
    path: String,
    preferred: Option<crate::app_domain::DomainImportFormat>,
    formats: String,
) -> WorkspaceImportFile {
    wirebolt_app::session::workspace::files::read_import(Path::new(&path), preferred, &formats)
}

#[uniffi::export]
pub fn workspace_import_display_name(format: crate::app_domain::DomainImportFormat) -> String {
    wirebolt_app::session::workspace::files::display_name(format)
}

#[uniffi::export]
pub fn workspace_import_unreadable(name: String) -> String {
    wirebolt_app::session::workspace::files::unreadable(&name)
}

#[uniffi::export]
pub fn workspace_import_unrecognized(name: String, contents: String, formats: String) -> String {
    wirebolt_app::session::workspace::files::unrecognized(&name, &contents, &formats)
}

#[uniffi::export]
pub fn workspace_recent_paths(path: String, paths: Vec<String>) -> Vec<String> {
    wirebolt_app::session::workspace::files::recent(&path, paths)
}

#[uniffi::export]
pub fn workspace_default_name(path: String, built_in: String) -> String {
    wirebolt_app::session::workspace::files::default_name(Path::new(&path), Path::new(&built_in))
}

#[uniffi::remote(Record)]
pub struct WorkspaceImport {
    pub source: String,
    pub format: crate::app_domain::DomainImportFormat,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub selected: Option<String>,
    pub pasted: bool,
}

#[uniffi::export]
impl AppWorkspaceOperations {
    pub fn take_failure(&self) -> Option<wirebolt_app::response::ResponseFailureKind> {
        self.inner.take_failure()
    }
}

#[uniffi::remote(Record)]
pub struct WorkspaceClose {
    pub blocked: Vec<String>,
    pub removed: Vec<WorkspaceDiscard>,
    pub snapshot: SessionTabSnapshot,
}

#[uniffi::remote(Enum)]
pub enum WorkspaceRepositoryProblem {
    NotRepository,
    NotRepositoryRoot,
    GitUnavailable,
}

#[uniffi::export]
pub fn workspace_repository_problem(
    kind: wirebolt_app::session::workspace::io::WorkspaceFailureKind,
) -> Option<WorkspaceRepositoryProblem> {
    wirebolt_app::session::workspace::operations::repository_problem(&kind)
}

#[uniffi::remote(Enum)]
pub enum WorkspaceSelectionProblem {
    NotFound,
    AlreadyExists,
}

#[uniffi::export]
pub fn workspace_read_document(path: String) -> Option<String> {
    wirebolt_app::session::workspace::files::read_document(Path::new(&path))
}

#[uniffi::export]
pub fn workspace_selection_problem(
    path: String,
    mode: String,
) -> Option<WorkspaceSelectionProblem> {
    wirebolt_app::session::workspace::files::selection_problem(Path::new(&path), &mode)
}

#[uniffi::export]
impl AppWorkspaceOperations {
    pub fn environment_unchanged(&self, environment: DomainEnvironment) -> bool {
        self.inner.environment_unchanged(&environment)
    }
}

#[uniffi::export]
impl AppWorkspaceOperations {
    pub fn request_matches(
        &self,
        request: wirebolt_app::domain::sidebar::DomainSidebarRequest,
        normalized: String,
    ) -> bool {
        self.inner.request_matches(request, &normalized)
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AppWorkspaceOperations {
    pub async fn save_all_live(
        &self,
        ids: Vec<String>,
        platform: Arc<AppWorkspacePlatform>,
        documents: Arc<AppWorkspaceDocuments>,
        secrets: Arc<AppSessionSecrets>,
        secret_platform: Arc<AppSecretPlatform>,
    ) -> bool {
        self.inner
            .save_all_live(
                ids,
                platform.as_ref(),
                documents.as_ref(),
                &secrets.inner,
                secret_platform.as_ref(),
            )
            .await
    }
}

#[uniffi::export]
pub fn workspace_saved_authentication(
    current: DomainAuthentication,
    original: DomainAuthentication,
    saved: DomainAuthentication,
) -> DomainAuthentication {
    wirebolt_app::session::workspace::documents::saved_authentication(current, &original, &saved)
}

#[uniffi::remote(Enum)]
pub enum SessionWorkspaceNodeKind {
    Workspace,
    Collection,
    Group,
    Request,
    Environment,
}
