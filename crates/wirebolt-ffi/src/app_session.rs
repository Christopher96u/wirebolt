//! Typed session bridge. State transitions and body ownership live in wirebolt-app.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use crate::WorkspaceBridge;
use crate::app_response::AppResponseBody;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};
use wirebolt_app::{
    domain::{DomainBody, DomainLocation, DomainProxy, DomainRequestDraft, DomainTransport},
    response::{ResponseHeaderValue, StoredCookie},
    session::{
        run::{
            DocumentRun, SessionCompletion, SessionFailure, SessionHead, SessionIssue,
            SessionPrepared, SessionPreparedBody, SessionPreparedHeader, SessionProxy,
            SessionRunEvent, SessionRunSnapshot,
        },
        tabs::{
            AppSessionLayout, AppSessionLayoutGroup, SessionCloseScope, SessionDocument,
            SessionEditorGroup, SessionKind, SessionLayoutTab, SessionRestoreResult,
            SessionTabDocument, SessionTabSnapshot, SessionTabs,
        },
        websocket::{
            SessionSocket, SessionSocketMessage, SessionSocketNotice, SessionSocketSnapshot,
            SessionSocketStatus, decode_binary, message_body,
        },
    },
};
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
#[uniffi::remote(Record)]
pub struct SessionHead {
    status: u16,
    version: String,
    headers: Vec<ResponseHeaderValue>,
    time_to_headers_ns: u64,
}
#[uniffi::remote(Record)]
pub struct SessionCompletion {
    bytes_received: u64,
    total_time_ns: u64,
}
#[uniffi::remote(Record)]
pub struct SessionIssue {
    path: String,
    kind: String,
    reference: Option<String>,
}
#[uniffi::remote(Record)]
pub struct SessionFailure {
    kind: String,
    issues: Vec<SessionIssue>,
}
#[uniffi::remote(Record)]
pub struct SessionPreparedHeader {
    name: String,
    value: String,
    redacted: bool,
}
#[uniffi::remote(Record)]
pub struct SessionPreparedBody {
    byte_count: u64,
    content_type: Option<String>,
    text_preview: Option<String>,
    redacted: bool,
}
#[uniffi::remote(Record)]
pub struct SessionProxy {
    configuration: DomainProxy,
    source: String,
}
#[uniffi::remote(Record)]
pub struct SessionPrepared {
    proxy: Option<SessionProxy>,
    method: String,
    url: String,
    headers: Vec<SessionPreparedHeader>,
    body: SessionPreparedBody,
    transport: DomainTransport,
}
#[uniffi::remote(Enum)]
pub enum SessionRunEvent {
    Prepared { snapshot: SessionPrepared },
    Head { head: SessionHead },
    Complete { completion: SessionCompletion },
}
#[uniffi::remote(Record)]
pub struct SessionRunSnapshot {
    head: Option<SessionHead>,
    prepared: Option<SessionPrepared>,
    text: String,
    data: Vec<u8>,
    bytes: u64,
    truncated: bool,
    completion: Option<SessionCompletion>,
    failure: Option<SessionFailure>,
    active_run_id: Option<String>,
    started_at: Option<f64>,
    awaiting_head: bool,
    cookies: Vec<StoredCookie>,
    body_path: Option<String>,
}
#[uniffi::remote(Enum)]
pub enum SessionKind {
    Http,
    WebSocket,
}
#[uniffi::remote(Record)]
pub struct SessionDocument {
    id: String,
    kind: SessionKind,
    collection_id: Option<String>,
    request_id: String,
    draft: DomainRequestDraft,
    saved_draft: Option<DomainRequestDraft>,
    touched: bool,
}
#[uniffi::remote(Record)]
pub struct SessionTabDocument {
    id: String,
    kind: SessionKind,
    collection_id: Option<String>,
    request_id: String,
    dirty: bool,
    touched: bool,
}
#[uniffi::remote(Record)]
pub struct SessionEditorGroup {
    id: String,
    tab_ids: Vec<String>,
    selected_tab_id: Option<String>,
    backward_tab_ids: Vec<String>,
    forward_tab_ids: Vec<String>,
    preview_tab_id: Option<String>,
}
#[uniffi::remote(Record)]
pub struct SessionLayoutTab {
    collection_id: String,
    request_id: String,
}
#[uniffi::remote(Record)]
pub struct AppSessionLayoutGroup {
    tabs: Vec<SessionLayoutTab>,
    selected_index: Option<i64>,
    preview_index: Option<i64>,
}
#[uniffi::remote(Record)]
pub struct AppSessionLayout {
    groups: Vec<AppSessionLayoutGroup>,
    active_group_index: i64,
}
#[uniffi::remote(Enum)]
pub enum SessionCloseScope {
    One { id: String },
    Others { id: String },
    RightOf { id: String },
    All,
}
#[uniffi::remote(Record)]
pub struct SessionTabSnapshot {
    groups: Vec<SessionEditorGroup>,
    active_group_id: String,
    session_ids: Vec<String>,
}
#[uniffi::remote(Enum)]
pub enum SessionSocketStatus {
    Disconnected,
    Connecting,
    Connected,
}
#[uniffi::remote(Enum)]
pub enum SessionSocketNotice {
    Connected,
    Disconnected,
    Failed,
}
#[uniffi::remote(Record)]
pub struct SessionSocketMessage {
    id: String,
    timestamp: f64,
    data: Vec<u8>,
    binary: bool,
    outgoing: bool,
    notice: Option<SessionSocketNotice>,
    control: Option<String>,
    text: String,
}
#[uniffi::remote(Record)]
pub struct SessionSocketSnapshot {
    first_sequence: u64,
    next_sequence: u64,
    generation: String,
    status: SessionSocketStatus,
    messages: Vec<SessionSocketMessage>,
    error: Option<String>,
    headers: Vec<ResponseHeaderValue>,
}
#[derive(Debug, uniffi::Object)]
pub struct AppDocumentSession {
    document: Arc<Mutex<SessionDocument>>,
    run: Arc<Mutex<DocumentRun>>,
}
#[uniffi::export]
impl AppDocumentSession {
    #[uniffi::constructor]
    pub fn new(
        id: String,
        kind: SessionKind,
        collection_id: Option<String>,
        request_id: Option<String>,
        draft: DomainRequestDraft,
        saved_draft: Option<DomainRequestDraft>,
        note: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            document: Arc::new(Mutex::new(SessionDocument::new(
                id,
                kind,
                collection_id,
                request_id,
                draft,
                saved_draft,
                &note,
            ))),
            run: Arc::new(Mutex::new(DocumentRun::default())),
        })
    }
    pub fn document(&self) -> SessionDocument {
        lock(&self.document).clone()
    }
    pub fn edit(&self, draft: DomainRequestDraft) {
        lock(&self.document).edit(draft);
    }
    pub fn relocate(&self, collection_id: String) {
        lock(&self.document).collection_id = Some(collection_id);
    }
    pub fn rename(&self, name: String) {
        lock(&self.document).rename(&name);
    }
    pub fn save(&self, draft: DomainRequestDraft, replace: bool) {
        let mut document = lock(&self.document);
        if replace {
            document.mark_saved(draft);
        } else {
            document.saved_draft = Some(draft);
        }
    }
    pub fn snapshot(&self) -> SessionRunSnapshot {
        lock(&self.run).snapshot()
    }
    pub fn body(&self) -> Option<Arc<AppResponseBody>> {
        lock(&self.run)
            .body()
            .map(|inner| Arc::new(AppResponseBody { inner }))
    }
    pub fn begin(&self, run_id: String, directory: String) {
        lock(&self.document).touched = true;
        lock(&self.run).begin(&run_id, Path::new(&directory));
    }
    pub fn consume(&self, run_id: String, event: SessionRunEvent) {
        lock(&self.run).consume(&run_id, event);
    }
    pub fn append_batch(&self, run_id: String, chunks: Vec<Vec<u8>>) -> bool {
        lock(&self.run)
            .append_batch(&run_id, &chunks, Instant::now())
            .is_some()
    }
    pub fn publish(&self, run_id: String) {
        let mut run = lock(&self.run);
        if run.accepts(&run_id) {
            run.publish();
        }
    }
    pub fn finish(&self, run_id: String, failure: Option<SessionFailure>) {
        lock(&self.run).finish(&run_id, failure);
    }
    pub fn cancel(&self, run_id: String) {
        lock(&self.run).cancel(&run_id);
    }
    pub fn reset(&self) {
        lock(&self.run).reset();
    }
    pub fn copy_completed(&self, source: Arc<Self>) {
        if std::ptr::eq(self, source.as_ref()) {
            return;
        }
        let saved = lock(&source.document).saved_draft.clone();
        let run = lock(&source.run).clone();
        lock(&self.document).saved_draft = saved;
        lock(&self.run).copy_completed(&run);
    }
    pub fn cookies(&self, cookies: Vec<StoredCookie>, append: bool) {
        if append {
            lock(&self.run).append_cookies(cookies);
        } else {
            lock(&self.run).set_cookies(cookies);
        }
    }
    pub fn restore(&self, snapshot: SessionRunSnapshot, path: String) {
        lock(&self.run).restore(snapshot, PathBuf::from(path));
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppSessionTabs {
    tabs: Mutex<(SessionTabs, HashMap<String, SessionTabDocument>)>,
}
#[uniffi::export]
impl AppSessionTabs {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tabs: Mutex::new((SessionTabs::default(), HashMap::new())),
        })
    }
    pub fn snapshot(&self) -> SessionTabSnapshot {
        let state = lock(&self.tabs);
        state.0.snapshot(&state.1)
    }
    pub fn synchronize(&self, documents: Vec<SessionTabDocument>) {
        lock(&self.tabs).1 = documents.into_iter().map(|d| (d.id.clone(), d)).collect();
    }
    pub fn activate_group(&self, id: String) {
        lock(&self.tabs).0.active_group_id = id;
    }
    pub fn open(
        &self,
        document: SessionTabDocument,
        group: Option<String>,
        force: bool,
        preview: bool,
    ) -> String {
        let mut state = lock(&self.tabs);
        let (tabs, documents) = &mut *state;
        tabs.open(document, documents, group.as_deref(), force, preview)
            .0
    }
    pub fn reopen(&self, document: SessionTabDocument) {
        let mut state = lock(&self.tabs);
        let (tabs, documents) = &mut *state;
        tabs.reopen(document, documents);
    }
    pub fn select(&self, id: String, group: Option<String>) {
        lock(&self.tabs).0.select(&id, group.as_deref());
    }
    pub fn navigate(&self, forward: bool) {
        lock(&self.tabs).0.navigate(forward);
    }
    pub fn pin(&self, id: String) {
        lock(&self.tabs).0.pin(&id);
    }
    pub fn split(&self) -> String {
        let mut state = lock(&self.tabs);
        let group = SessionEditorGroup::default();
        let id = group.id.clone();
        state.0.active_group_id.clone_from(&id);
        state.0.groups.push(group);
        id
    }
    pub fn close(
        &self,
        scope: SessionCloseScope,
        group: Option<String>,
        allow_dirty: bool,
    ) -> Vec<String> {
        let mut state = lock(&self.tabs);
        let (tabs, documents) = &mut *state;
        tabs.close(&scope, group.as_deref(), allow_dirty, documents)
    }
    pub fn remove_all(&self) {
        *lock(&self.tabs) = (SessionTabs::default(), HashMap::new());
    }
    pub fn layout(&self) -> AppSessionLayout {
        let state = lock(&self.tabs);
        state.0.layout(&state.1)
    }
    pub fn restore(
        &self,
        layout: AppSessionLayout,
        resolved: Vec<Option<DomainLocation>>,
    ) -> SessionRestoreResult {
        let mut state = lock(&self.tabs);
        let (tabs, documents) = &mut *state;
        let restored = tabs.restore(&layout, resolved, documents);
        SessionRestoreResult {
            documents: restored,
            snapshot: tabs.snapshot(documents),
        }
    }
    pub fn restore_groups(&self, groups: Vec<SessionEditorGroup>, active_group_id: String) {
        let mut state = lock(&self.tabs);
        state.0.groups = groups;
        state.0.active_group_id = active_group_id;
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppSessionSocket {
    inner: Mutex<SessionSocket>,
}
#[uniffi::export]
impl AppSessionSocket {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(SessionSocket::default()),
        })
    }
    pub fn snapshot(&self, since: u64) -> SessionSocketSnapshot {
        lock(&self.inner).snapshot_since(since)
    }
    pub fn begin(&self) -> Option<String> {
        lock(&self.inner).begin()
    }
    pub fn connected(&self, generation: String, headers: Vec<ResponseHeaderValue>, url: String) {
        lock(&self.inner).connected(&generation, headers, &url);
    }
    pub fn append(
        &self,
        generation: String,
        data: Vec<u8>,
        binary: bool,
        outgoing: bool,
        control: Option<String>,
    ) {
        lock(&self.inner).append(&generation, data, binary, outgoing, control);
    }
    pub fn closed(&self, generation: String) {
        lock(&self.inner).closed(&generation);
    }
    pub fn finish(&self, generation: String, error: Option<String>) {
        lock(&self.inner).finish(&generation, error);
    }
    pub fn disconnect(&self) {
        lock(&self.inner).disconnect();
    }
    pub fn clear(&self) {
        lock(&self.inner).clear();
    }
    pub fn set_error(&self, error: Option<String>) {
        lock(&self.inner).set_error(error);
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SessionMessageBody {
    pub data: Vec<u8>,
    pub binary: bool,
}
#[uniffi::export]
pub fn session_decode_binary(
    text: String,
    hex: bool,
) -> Result<Vec<u8>, crate::WorkspaceBridgeError> {
    decode_binary(&text, hex)
        .map_err(|reason| crate::WorkspaceBridgeError::OperationFailed { reason })
}
#[uniffi::export]
pub fn session_message_body(
    body: DomainBody,
) -> Result<SessionMessageBody, crate::WorkspaceBridgeError> {
    message_body(&body)
        .map(|(data, binary)| SessionMessageBody { data, binary })
        .map_err(|reason| crate::WorkspaceBridgeError::OperationFailed { reason })
}

#[uniffi::remote(Record)]
pub struct SessionRestoreResult {
    pub documents: Vec<SessionDocument>,
    pub snapshot: SessionTabSnapshot,
}

use wirebolt_app::domain::{DomainCollection, DomainEnvironment, DomainGroup, DomainWorkspace};
use wirebolt_app::session::workspace::{
    SessionDocumentEffect, SessionWorkspaceCommand, SessionWorkspaceEdit, SessionWorkspacePlan,
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
#[uniffi::remote(Enum)]
pub enum SessionDocumentEffect {
    Close {
        collection_id: String,
        request_id: Option<String>,
    },
    Rename {
        collection_id: String,
        request_id: String,
        name: String,
    },
    Relocate {
        collection_id: String,
        request_id: String,
        destination: String,
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
    inner: wirebolt_app::session::workspace::WorkspaceSession,
}
#[uniffi::export]
impl AppWorkspaceSession {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: wirebolt_app::session::workspace::WorkspaceSession::default(),
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
use wirebolt_app::session::persistence::SessionWorkspaceDelta;
#[uniffi::remote(Record)]
pub struct SessionWorkspaceDelta {
    pub version: u64,
    pub kind: String,
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

type SecretReplySender = tokio::sync::oneshot::Sender<Result<Option<String>, ()>>;
#[derive(Debug, uniffi::Object)]
pub struct SessionSecretReply {
    sender: Mutex<Option<SecretReplySender>>,
}
#[uniffi::export]
impl SessionSecretReply {
    pub fn finish(&self, value: Option<String>, failed: bool) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(if failed { Err(()) } else { Ok(value) });
        }
    }
}
#[uniffi::export(with_foreign)]
pub trait SessionSecretPlatform: Send + Sync {
    fn execute(
        &self,
        operation: String,
        name: String,
        value: Option<String>,
        reply: Arc<SessionSecretReply>,
    );
}
#[derive(uniffi::Object)]
pub struct AppSecretPlatform {
    workspace: Option<Arc<WorkspaceBridge>>,
    foreign: Option<Arc<dyn SessionSecretPlatform>>,
}
impl std::fmt::Debug for AppSecretPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppSecretPlatform").finish_non_exhaustive()
    }
}
#[uniffi::export]
impl AppSecretPlatform {
    #[uniffi::constructor]
    pub fn workspace(bridge: Arc<WorkspaceBridge>) -> Arc<Self> {
        Arc::new(Self {
            workspace: Some(bridge),
            foreign: None,
        })
    }
    #[uniffi::constructor]
    pub fn foreign(platform: Arc<dyn SessionSecretPlatform>) -> Arc<Self> {
        Arc::new(Self {
            workspace: None,
            foreign: Some(platform),
        })
    }
}
impl AppSecretPlatform {
    async fn execute(
        &self,
        operation: &str,
        name: String,
        value: Option<String>,
    ) -> Result<Option<String>, ()> {
        if let Some(workspace) = &self.workspace {
            let workspace = workspace.inner.clone();
            let operation = operation.to_owned();
            return tokio::task::spawn_blocking(move || match operation.as_str() {
                "read" => workspace.read_secret(&name).map_err(|_| ()),
                "save" => workspace
                    .save_secret(&name, value.as_deref().unwrap_or_default())
                    .map(|()| None)
                    .map_err(|_| ()),
                _ => workspace
                    .delete_secret(&name)
                    .map(|()| None)
                    .map_err(|_| ()),
            })
            .await
            .map_err(|_| ())?;
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.foreign.as_ref().ok_or(())?.execute(
            operation.into(),
            name,
            value,
            Arc::new(SessionSecretReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver.await.map_err(|_| ())?
    }
}
#[async_trait::async_trait]
impl wirebolt_app::session::secrets::SessionSecretPlatform for AppSecretPlatform {
    async fn read(&self, name: String) -> Result<Option<String>, ()> {
        self.execute("read", name, None).await
    }
    async fn save(&self, name: String, value: String) -> Result<(), ()> {
        self.execute("save", name, Some(value)).await.map(|_| ())
    }
    async fn remove(&self, name: String) -> Result<(), ()> {
        self.execute("remove", name, None).await.map(|_| ())
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppSessionSecrets {
    inner: Arc<wirebolt_app::session::secrets::SessionSecrets>,
}
#[uniffi::export]
impl AppSessionSecrets {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(wirebolt_app::session::secrets::SessionSecrets::default()),
        })
    }
    pub fn edit_batch(&self, values: HashMap<String, String>) {
        self.inner.edit_batch(values);
    }
    pub fn reset_editor(&self) {
        self.inner.reset_editor();
    }
    pub fn clear_observed(&self) {
        self.inner.clear_observed();
    }
    pub fn observe(&self, names: Vec<String>) {
        self.inner.observe(names);
    }
    pub fn has_observed(&self) -> bool {
        self.inner.has_observed()
    }
}
fn secret_error(
    _: wirebolt_app::session::secrets::SecretAccessError,
) -> crate::WorkspaceBridgeError {
    crate::WorkspaceBridgeError::operation("secret storage failed")
}
#[uniffi::export(async_runtime = "tokio")]
impl AppSessionSecrets {
    pub async fn load(
        &self,
        name: String,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<String, crate::WorkspaceBridgeError> {
        self.inner
            .load(name, platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn flush(
        &self,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<(), crate::WorkspaceBridgeError> {
        self.inner
            .flush(platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn save(
        &self,
        values: HashMap<String, String>,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<(), crate::WorkspaceBridgeError> {
        self.inner
            .save(values, platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn copy(
        &self,
        renames: HashMap<String, String>,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<HashMap<String, String>, crate::WorkspaceBridgeError> {
        self.inner
            .copy(renames, platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn purge(
        &self,
        in_use: Vec<String>,
        others: Vec<String>,
        platform: Arc<AppSecretPlatform>,
    ) {
        self.inner.purge(in_use, others, platform.as_ref()).await;
    }
}
#[uniffi::export]
pub fn session_scan_secrets(candidates: Vec<String>, workspaces: Vec<String>) -> Vec<String> {
    wirebolt_app::session::secrets::scan_references(&candidates, &workspaces)
        .into_iter()
        .collect()
}
#[uniffi::export]
impl AppSessionSecrets {
    pub fn cached(&self) -> HashMap<String, String> {
        self.inner.cached()
    }
    pub fn dirty(&self) -> Vec<String> {
        self.inner.dirty()
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
use wirebolt_app::session::workspace::SessionDeleteTarget;
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
#[uniffi::export]
pub fn session_tab_destination(
    tabs: Vec<String>,
    selected: Option<String>,
    index: Option<i64>,
    offset: i64,
) -> Option<String> {
    wirebolt_app::session::workspace::tab_destination(&tabs, selected.as_deref(), index, offset)
}
use wirebolt_app::session::history::SessionHistoryEntry;
#[uniffi::remote(Record)]
pub struct SessionHistoryEntry {
    pub id: String,
    pub request_id: String,
    pub created_at: f64,
    pub prepared: SessionPrepared,
    pub response_head: Option<SessionHead>,
    pub completion: Option<SessionCompletion>,
    pub failure: Option<SessionFailure>,
    pub body_path: String,
}
#[uniffi::export]
pub fn session_history_viewport(path: String) -> Vec<u8> {
    wirebolt_app::session::history::viewport(&path)
}
use wirebolt_app::session::persistence::SessionCommandBatchResult;
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
pub fn session_decode_layout(data: Vec<u8>) -> Option<AppSessionLayout> {
    wirebolt_app::session::tabs::decode_layout(&data)
}
#[uniffi::export]
pub fn session_encode_layout(layout: AppSessionLayout) -> Vec<u8> {
    wirebolt_app::session::tabs::encode_layout(&layout)
}
#[uniffi::export]
pub fn session_existing_workspace_paths(paths: Vec<String>) -> Vec<String> {
    wirebolt_app::session::tabs::existing_workspace_paths(paths)
}
#[uniffi::export]
pub fn session_publication_delay_ms() -> u64 {
    u64::try_from(wirebolt_app::session::run::PUBLICATION_INTERVAL.as_millis()).unwrap_or(16)
}

// MARK: C3 runtime operations (keep independent of workspace orchestration)
use wirebolt_app::domain::{DomainOAuthConfiguration, DomainRunInput, DomainValueSource};
use wirebolt_app::session::runtime::{
    self, RequestActivity, RunObserver, RuntimeAuthorizer, RuntimeContext, RuntimeObserver,
    RuntimeOperationError, RuntimeRuns, RuntimeSettingsPlatform, RuntimeSnapshot, RuntimeState,
    RuntimeTokenReceipt, RuntimeTransport, RuntimeTransportSettings, RuntimeTransportSnapshot,
};
#[uniffi::remote(Record)]
pub struct RuntimeContext {
    pub transport: DomainTransport,
    pub workspace_proxy: Option<DomainProxy>,
    pub app_proxy: DomainProxy,
    pub variables: HashMap<String, DomainValueSource>,
}
#[uniffi::remote(Record)]
pub struct RequestActivity {
    pub date: f64,
    pub status: Option<u16>,
}
#[uniffi::remote(Record)]
pub struct RuntimeSnapshot {
    pub history_entries: Vec<SessionHistoryEntry>,
    pub history_revision: u64,
    pub request_activity: HashMap<String, RequestActivity>,
    pub cookie_revision: u64,
}
#[uniffi::remote(Error)]
pub enum RuntimeOperationError {
    Unsupported,
    Validation { reason: String },
    Platform { reason: String },
    Workspace { reason: String },
}
#[uniffi::remote(Record)]
pub struct RuntimeTransportSnapshot {
    pub value: DomainTransport,
    pub failed: bool,
}
#[uniffi::remote(Record)]
pub struct RuntimeTokenReceipt {
    pub expires_at: Option<f64>,
    pub scope: Option<String>,
}
#[uniffi::export(with_foreign)]
pub trait SessionRuntimeObserver: Send + Sync {
    fn changed(&self);
    fn prepare(&self, reply: Arc<SessionInputReply>);
}
struct RuntimeObserverAdapter(Arc<dyn SessionRuntimeObserver>);
#[async_trait::async_trait]
impl RuntimeObserver for RuntimeObserverAdapter {
    fn changed(&self) {
        self.0.changed();
    }
    async fn prepare(&self) -> Result<Option<runtime::RuntimeInput>, ()> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0.prepare(Arc::new(SessionInputReply {
            sender: Mutex::new(Some(sender)),
        }));
        receiver.await.map_err(|_| ())?.map(Some).ok_or(())
    }
}

type ValuesReplySender = tokio::sync::oneshot::Sender<Result<Vec<String>, SessionFailure>>;
#[derive(Debug, uniffi::Object)]
pub struct SessionValuesReply {
    sender: Mutex<Option<ValuesReplySender>>,
}
#[uniffi::export]
impl SessionValuesReply {
    pub fn finish(&self, values: Vec<String>, failure: Option<SessionFailure>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(failure.map_or(Ok(values), Err));
        }
    }
}
#[derive(Debug, uniffi::Object)]
pub struct SessionRunSink {
    observer: Arc<RunObserver>,
    sender: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}
#[uniffi::export]
impl SessionRunSink {
    pub fn abandon(&self) {
        self.observer.abandon();
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(());
        }
    }
    pub fn event(&self, event: SessionRunEvent) {
        self.observer.event(event);
    }
    pub fn bytes(&self, data: Vec<u8>) {
        self.observer.bytes(&data);
    }
    pub fn cookies(&self, url: String, headers: Vec<ResponseHeaderValue>) {
        self.observer.store_cookies(&url, &headers);
    }
    pub fn finish(&self, failure: Option<SessionFailure>) {
        self.observer.finish(failure);
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(());
        }
    }
}
#[uniffi::export(with_foreign)]
pub trait SessionRuntimeTransport: Send + Sync {
    fn resolve(
        &self,
        values: Vec<DomainValueSource>,
        variables: HashMap<String, DomainValueSource>,
        reply: Arc<SessionValuesReply>,
    );
    fn execute(&self, input: DomainRunInput, id: String, sink: Arc<SessionRunSink>);
    fn cancel(&self, id: String);
}
#[derive(uniffi::Object)]
pub struct AppRuntimeTransport {
    foreign: Option<Arc<dyn SessionRuntimeTransport>>,
}
impl std::fmt::Debug for AppRuntimeTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppRuntimeTransport")
            .finish_non_exhaustive()
    }
}
#[uniffi::export]
impl AppRuntimeTransport {
    #[uniffi::constructor]
    pub fn native() -> Arc<Self> {
        Arc::new(Self { foreign: None })
    }
    #[uniffi::constructor]
    pub fn foreign(platform: Arc<dyn SessionRuntimeTransport>) -> Arc<Self> {
        Arc::new(Self {
            foreign: Some(platform),
        })
    }
}
#[cfg(target_vendor = "apple")]
fn runtime_secrets() -> impl wirebolt_core::SecretResolver + Clone {
    wirebolt_core::KeychainSecretResolver::default()
}
#[cfg(not(target_vendor = "apple"))]
fn runtime_secrets() -> impl wirebolt_core::SecretResolver + Clone {
    wirebolt_core::NoSecrets
}
#[async_trait::async_trait]
impl RuntimeTransport for AppRuntimeTransport {
    fn cancel(&self, id: &str) {
        if let Some(foreign) = &self.foreign {
            foreign.cancel(id.into());
        }
    }
    async fn resolve(
        &self,
        values: Vec<DomainValueSource>,
        variables: HashMap<String, DomainValueSource>,
    ) -> Result<Vec<String>, SessionFailure> {
        if let Some(foreign) = &self.foreign {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            foreign.resolve(
                values,
                variables,
                Arc::new(SessionValuesReply {
                    sender: Mutex::new(Some(sender)),
                }),
            );
            receiver
                .await
                .unwrap_or_else(|_| Err(SessionFailure::new("bridge")))
        } else {
            tokio::task::spawn_blocking(move || {
                runtime::resolve_values(values, variables, &runtime_secrets())
            })
            .await
            .unwrap_or_else(|_| Err(SessionFailure::new("invalid_request")))
        }
    }
    async fn execute(
        &self,
        input: DomainRunInput,
        id: String,
        cancellation: wirebolt_core::RunCancellation,
        observer: Arc<RunObserver>,
    ) {
        if let Some(foreign) = &self.foreign {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            foreign.execute(
                input,
                id,
                Arc::new(SessionRunSink {
                    observer,
                    sender: Mutex::new(Some(sender)),
                }),
            );
            let _ = receiver.await;
        } else {
            match runtime::transport_input(input) {
                Ok(input) => {
                    wirebolt_app::session::transport::execute(
                        input,
                        cancellation,
                        observer,
                        runtime_secrets(),
                    )
                    .await;
                }
                Err(failure) => observer.finish(Some(failure)),
            }
        }
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppSessionRuntime {
    state: Arc<RuntimeState>,
    runs: RuntimeRuns,
    transport: RuntimeTransportSettings,
    oauth: runtime::RuntimeOAuth,
}
#[uniffi::export]
impl AppSessionRuntime {
    #[uniffi::constructor]
    pub fn new(
        history: Arc<crate::app_response::AppRunHistory>,
        cookies: Arc<crate::app_response::AppCookieJar>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(RuntimeState::new(
                history.inner.clone(),
                cookies.inner.clone(),
            )),
            runs: RuntimeRuns::default(),
            transport: RuntimeTransportSettings::default(),
            oauth: runtime::RuntimeOAuth::default(),
        })
    }
    pub fn cookie_revision(&self) -> u64 {
        self.state.cookie_revision()
    }
    pub fn snapshot(&self) -> RuntimeSnapshot {
        self.state.snapshot()
    }
    pub fn select_environment(&self, id: Option<String>) {
        self.state.select_environment(id);
    }
    pub fn active_variables(
        &self,
        environments: Vec<DomainEnvironment>,
    ) -> HashMap<String, DomainValueSource> {
        self.state.active_variables(&environments)
    }
    pub fn configure_cookies(&self, workspace: Option<String>, directory: Option<String>) {
        self.state
            .configure_cookies(workspace.as_deref(), directory.as_deref());
    }
    pub fn cookies(&self) -> Vec<StoredCookie> {
        self.state.cookies()
    }
    pub fn delete_cookie(&self, id: String) {
        self.state.delete_cookie(&id);
    }
    pub fn clear_cookies(&self) {
        self.state.clear_cookies();
    }
    pub fn reset_workspace(&self) {
        self.state.reset_workspace();
        self.transport.reset();
        self.oauth.reset();
    }
    pub fn cancel(&self, document: Arc<AppDocumentSession>) {
        let id = lock(&document.document).id.clone();
        self.runs.cancel(&id, &document.run);
    }
    pub fn reset_transport(&self) {
        self.transport.reset();
    }
    pub fn update_transport(&self, previous: DomainTransport, value: DomainTransport) -> u64 {
        self.transport.update(previous, value)
    }
}
#[uniffi::export(async_runtime = "tokio")]
impl AppSessionRuntime {
    pub async fn send(
        &self,
        document: Arc<AppDocumentSession>,
        options: RuntimeRunOptions,
        transport: Arc<AppRuntimeTransport>,
        observer: Arc<dyn SessionRuntimeObserver>,
        secrets: Arc<AppSessionSecrets>,
        platform: Option<Arc<AppSecretPlatform>>,
    ) -> Option<SessionFailure> {
        self.runs
            .send(runtime::RuntimeSend {
                state: self.state.clone(),
                document: document.document.clone(),
                run: document.run.clone(),
                context: options.context,
                id: options.id,
                directory: options.directory.into(),
                transport,
                observer: Arc::new(RuntimeObserverAdapter(observer)),
                credentials: runtime::RuntimeCredentials {
                    secrets: secrets.inner.clone(),
                    platform: platform.map(|p| {
                        p as Arc<dyn wirebolt_app::session::secrets::SessionSecretPlatform>
                    }),
                },
            })
            .await
    }
    pub async fn history_entries(&self, id: String) -> Vec<SessionHistoryEntry> {
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || state.history_entries(&id))
            .await
            .unwrap_or_default()
    }
    pub async fn history_count(&self, id: String) -> u64 {
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || state.history_count(&id))
            .await
            .unwrap_or_default()
    }
    pub async fn load_history(&self, id: String) {
        let state = self.state.clone();
        let _ = tokio::task::spawn_blocking(move || state.load_history(&id)).await;
    }
    pub async fn clear_history(&self, id: String) {
        let state = self.state.clone();
        let _ = tokio::task::spawn_blocking(move || state.clear_history(&id)).await;
    }
    pub async fn restore_history(
        &self,
        entry: SessionHistoryEntry,
        document: Arc<AppDocumentSession>,
    ) {
        let state = self.state.clone();
        let _ = tokio::task::spawn_blocking(move || state.restore(entry, &document.run)).await;
    }
    pub async fn restore_latest(
        &self,
        document: Arc<AppDocumentSession>,
        observer: Arc<dyn SessionRuntimeObserver>,
    ) {
        let id = lock(&document.document).request_id.clone();
        self.state
            .clone()
            .restore_latest(id, document.run.clone(), &RuntimeObserverAdapter(observer))
            .await;
    }
    pub async fn curl_command(
        &self,
        draft: DomainRequestDraft,
        context: RuntimeContext,
        transport: Arc<AppRuntimeTransport>,
    ) -> Result<String, RuntimeRequestError> {
        runtime::curl_command(draft, context, transport.as_ref())
            .await
            .map_err(Into::into)
    }
}
#[uniffi::export]
impl AppSessionSecrets {
    pub fn material(&self, source: DomainValueSource) -> String {
        self.inner.material(&source)
    }
    pub fn edit(&self, name: String, value: String) {
        self.inner.edit_batch(HashMap::from([(name, value)]));
    }
}
#[uniffi::export(async_runtime = "tokio")]
impl AppSessionSecrets {
    pub async fn migrate(
        &self,
        draft: DomainRequestDraft,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<DomainRequestDraft, crate::WorkspaceBridgeError> {
        self.inner
            .migrate(draft, platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn purge_workspace(
        &self,
        workspace: DomainWorkspace,
        documents: Vec<SessionDocument>,
        current: Option<String>,
        others: Vec<String>,
        platform: Arc<AppSecretPlatform>,
    ) {
        self.inner
            .purge_workspace(workspace, documents, current, others, platform.as_ref())
            .await;
    }
}

type OperationReplySender = tokio::sync::oneshot::Sender<Result<(), RuntimeOperationError>>;
#[derive(Debug, uniffi::Object)]
pub struct SessionOperationReply {
    sender: Mutex<Option<OperationReplySender>>,
}
#[uniffi::export]
impl SessionOperationReply {
    pub fn finish(&self, reason: Option<String>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(reason.map_or(Ok(()), |reason| {
                Err(RuntimeOperationError::Platform { reason })
            }));
        }
    }
}
#[uniffi::export(with_foreign)]
pub trait SessionRuntimeSettings: Send + Sync {
    fn proxy(
        &self,
        scope: RuntimeProxyScope,
        configuration: Option<DomainProxy>,
        reply: Arc<SessionOperationReply>,
    );
    fn transport(&self, value: DomainTransport, reply: Arc<SessionOperationReply>);
}
struct RuntimeSettingsAdapter(Arc<dyn SessionRuntimeSettings>);
#[async_trait::async_trait]
impl RuntimeSettingsPlatform for RuntimeSettingsAdapter {
    async fn proxy(
        &self,
        scope: RuntimeProxyScope,
        configuration: Option<DomainProxy>,
    ) -> Result<(), RuntimeOperationError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0.proxy(
            scope,
            configuration,
            Arc::new(SessionOperationReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported))
    }
    async fn transport(&self, value: DomainTransport) -> Result<(), RuntimeOperationError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0.transport(
            value,
            Arc::new(SessionOperationReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported))
    }
}
#[uniffi::export(async_runtime = "tokio")]
impl AppSessionRuntime {
    pub async fn apply_proxy(
        &self,
        configuration: Option<DomainProxy>,
        scope: RuntimeProxyScope,
        has_session: bool,
        secrets: HashMap<String, String>,
        secret_platform: Option<Arc<AppSecretPlatform>>,
        platform: Arc<AppRuntimeSettings>,
    ) -> Result<(), RuntimeOperationError> {
        runtime::apply_proxy(
            configuration,
            scope,
            has_session,
            secrets,
            secret_platform
                .as_deref()
                .map(|s| s as &dyn wirebolt_app::session::secrets::SessionSecretPlatform),
            platform.as_ref(),
        )
        .await
    }
    pub async fn save_pending_transport(
        &self,
        generation: u64,
        platform: Arc<AppRuntimeSettings>,
    ) -> Option<RuntimeTransportSnapshot> {
        self.transport.save_pending(generation, platform).await
    }
}
type TokenReplySender =
    tokio::sync::oneshot::Sender<Result<(String, RuntimeTokenReceipt), RuntimeOperationError>>;
#[derive(Debug, uniffi::Object)]
pub struct SessionTokenReply {
    sender: Mutex<Option<TokenReplySender>>,
}
#[uniffi::export]
impl SessionTokenReply {
    pub fn finish(&self, token: String, receipt: RuntimeTokenReceipt, reason: Option<String>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(reason.map_or(Ok((token, receipt)), |reason| {
                Err(RuntimeOperationError::Platform { reason })
            }));
        }
    }
}
#[uniffi::export(with_foreign)]
pub trait SessionRuntimeAuthorizer: Send + Sync {
    fn acquire(&self, configuration: DomainOAuthConfiguration, reply: Arc<SessionTokenReply>);
}
struct RuntimeAuthorizerAdapter(Arc<dyn SessionRuntimeAuthorizer>);
#[async_trait::async_trait]
impl RuntimeAuthorizer for RuntimeAuthorizerAdapter {
    async fn acquire(
        &self,
        configuration: DomainOAuthConfiguration,
    ) -> Result<(String, RuntimeTokenReceipt), RuntimeOperationError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0.acquire(
            configuration,
            Arc::new(SessionTokenReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported))
    }
}
#[uniffi::export]
pub fn session_runtime_input(draft: DomainRequestDraft, context: RuntimeContext) -> DomainRunInput {
    context.input(draft)
}

#[uniffi::export(async_runtime = "tokio")]
impl AppSessionRuntime {
    pub async fn save_transport(
        &self,
        value: DomainTransport,
        platform: Arc<AppRuntimeSettings>,
    ) -> Result<(), RuntimeOperationError> {
        platform.transport(value).await
    }
}

#[uniffi::remote(Enum)]
pub enum RuntimeSendAction {
    Http,
    SocketConnect,
    SocketSend,
    Ignore,
}
use wirebolt_app::session::runtime::RuntimeSendAction;
#[uniffi::export]
pub fn session_send_action(
    kind: SessionKind,
    socket: SessionSocketStatus,
    running: bool,
) -> RuntimeSendAction {
    runtime::send_action(&kind, &socket, running)
}
#[uniffi::export(async_runtime = "tokio")]
impl AppSessionSecrets {
    pub async fn socket_input(
        &self,
        draft: DomainRequestDraft,
        context: RuntimeContext,
        platform: Option<Arc<AppSecretPlatform>>,
        observer: Arc<dyn SessionRuntimeObserver>,
    ) -> Result<DomainRunInput, crate::WorkspaceBridgeError> {
        runtime::socket_input(
            draft,
            context,
            runtime::RuntimeCredentials {
                secrets: self.inner.clone(),
                platform: platform
                    .map(|p| p as Arc<dyn wirebolt_app::session::secrets::SessionSecretPlatform>),
            },
            Some(&RuntimeObserverAdapter(observer)),
        )
        .await
        .map_err(secret_error)
    }
    pub async fn save_material(
        &self,
        name: String,
        value: String,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<(), crate::WorkspaceBridgeError> {
        wirebolt_app::session::secrets::SessionSecretPlatform::save(platform.as_ref(), name, value)
            .await
            .map_err(|()| secret_error(wirebolt_app::session::secrets::SecretAccessError))
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeRunOptions {
    pub context: RuntimeContext,
    pub id: String,
    pub directory: String,
}

#[uniffi::remote(Record)]
pub struct RuntimeInput {
    pub draft: DomainRequestDraft,
    pub context: RuntimeContext,
}
use wirebolt_app::session::runtime::RuntimeInput;
#[derive(Debug, uniffi::Object)]
pub struct SessionInputReply {
    sender: Mutex<Option<tokio::sync::oneshot::Sender<Option<RuntimeInput>>>>,
}
#[uniffi::export]
impl SessionInputReply {
    pub fn finish(&self, input: Option<RuntimeInput>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(input);
        }
    }
}

use wirebolt_app::session::runtime::RuntimeRequestError;
#[uniffi::remote(Error)]
pub enum RuntimeRequestError {
    Failure { failure: SessionFailure },
}
#[uniffi::export(async_runtime = "tokio")]
impl AppRuntimeTransport {
    pub async fn resolve_values(
        &self,
        values: Vec<DomainValueSource>,
        variables: HashMap<String, DomainValueSource>,
    ) -> Result<Vec<String>, RuntimeRequestError> {
        RuntimeTransport::resolve(self, values, variables)
            .await
            .map_err(Into::into)
    }
}

/// Native workspaces execute directly; only app preferences and injected test stores use the foreign relay.
#[derive(uniffi::Object)]
pub struct AppRuntimeSettings {
    workspace: Option<Arc<WorkspaceBridge>>,
    foreign: Arc<dyn SessionRuntimeSettings>,
}
impl std::fmt::Debug for AppRuntimeSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppRuntimeSettings").finish_non_exhaustive()
    }
}
#[uniffi::export]
impl AppRuntimeSettings {
    #[uniffi::constructor]
    pub fn new(
        bridge: Option<Arc<WorkspaceBridge>>,
        platform: Arc<dyn SessionRuntimeSettings>,
    ) -> Arc<Self> {
        Arc::new(Self {
            workspace: bridge,
            foreign: platform,
        })
    }
}
#[async_trait::async_trait]
impl RuntimeSettingsPlatform for AppRuntimeSettings {
    async fn proxy(
        &self,
        scope: RuntimeProxyScope,
        configuration: Option<DomainProxy>,
    ) -> Result<(), RuntimeOperationError> {
        if scope == RuntimeProxyScope::Request {
            return Ok(());
        }
        if scope == RuntimeProxyScope::Workspace
            && let Some(workspace) = &self.workspace
        {
            let workspace = workspace.inner.clone();
            return tokio::task::spawn_blocking(move || {
                workspace
                    .apply_session_command(SessionWorkspaceCommand::SaveWorkspaceProxy {
                        proxy: configuration,
                    })
                    .map(|_| ())
                    .map_err(|error| RuntimeOperationError::Workspace {
                        reason: error.to_string(),
                    })
            })
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported));
        }
        RuntimeSettingsAdapter(self.foreign.clone())
            .proxy(scope, configuration)
            .await
    }
    async fn transport(&self, value: DomainTransport) -> Result<(), RuntimeOperationError> {
        if let Some(workspace) = &self.workspace {
            let workspace = workspace.inner.clone();
            return tokio::task::spawn_blocking(move || {
                workspace
                    .apply_session_command(SessionWorkspaceCommand::SaveWorkspaceSettings {
                        transport: value,
                    })
                    .map(|_| ())
                    .map_err(|error| RuntimeOperationError::Workspace {
                        reason: error.to_string(),
                    })
            })
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported));
        }
        RuntimeSettingsAdapter(self.foreign.clone())
            .transport(value)
            .await
    }
}

use wirebolt_app::session::runtime::RuntimeRequestKey;
#[uniffi::remote(Record)]
pub struct RuntimeRequestKey {
    pub collection_id: String,
    pub request_id: String,
}
#[uniffi::export]
pub fn session_active_request_key(
    collection_id: Option<String>,
    request_id: Option<String>,
) -> Option<RuntimeRequestKey> {
    runtime::active_request_key(collection_id, request_id)
}

use wirebolt_app::session::runtime::RuntimeOAuthSnapshot;
#[uniffi::remote(Record)]
pub struct RuntimeOAuthSnapshot {
    pub busy: bool,
    pub failure: Option<RuntimeOperationError>,
    pub receipts: HashMap<String, RuntimeTokenReceipt>,
}
#[uniffi::export]
impl AppSessionRuntime {
    pub fn oauth_snapshot(&self) -> RuntimeOAuthSnapshot {
        self.oauth.snapshot()
    }
}
#[uniffi::export(async_runtime = "tokio")]
impl AppSessionRuntime {
    pub async fn acquire_token(
        &self,
        document: Arc<AppDocumentSession>,
        secrets: Arc<AppSessionSecrets>,
        platform: Arc<AppSecretPlatform>,
        authorizer: Arc<dyn SessionRuntimeAuthorizer>,
        observer: Arc<dyn SessionRuntimeObserver>,
    ) {
        self.oauth
            .acquire(
                &secrets.inner,
                &document.document,
                platform.as_ref(),
                &RuntimeAuthorizerAdapter(authorizer),
                &RuntimeObserverAdapter(observer),
            )
            .await;
    }
}

use wirebolt_app::session::runtime::RuntimeProxyScope;
#[uniffi::remote(Enum)]
pub enum RuntimeProxyScope {
    App,
    Workspace,
    Request,
}
#[uniffi::export]
impl AppDocumentSession {
    pub fn was_touched(&self) -> bool {
        lock(&self.document).touched
    }
}

// End C3 runtime operations.
