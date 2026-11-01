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
    document: Mutex<SessionDocument>,
    run: Mutex<DocumentRun>,
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
            document: Mutex::new(SessionDocument::new(
                id,
                kind,
                collection_id,
                request_id,
                draft,
                saved_draft,
                &note,
            )),
            run: Mutex::new(DocumentRun::default()),
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
    inner: Arc<wirebolt_app::session::workspace::WorkspaceSession>,
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
    inner: wirebolt_app::session::secrets::SessionSecrets,
}
#[uniffi::export]
impl AppSessionSecrets {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: wirebolt_app::session::secrets::SessionSecrets::default(),
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

// BEGIN C2 workspace application orchestration
use wirebolt_app::session::workspace::io::WorkspaceFailure;
#[uniffi::remote(Record)]
pub struct WorkspaceFailure {
    pub kind: String,
    pub reason: String,
    pub localized: bool,
}
use wirebolt_app::session::workspace::io::WorkspaceImportedEnvironment;
#[uniffi::remote(Record)]
pub struct WorkspaceImportedEnvironment {
    pub id: String,
    pub name: String,
}
use wirebolt_app::session::workspace::io::WorkspaceImportSummary;
#[uniffi::remote(Record)]
pub struct WorkspaceImportSummary {
    pub collection_names: Vec<String>,
    pub request_count: u64,
    pub group_count: u64,
    pub environments: Vec<WorkspaceImportedEnvironment>,
    pub warnings: Vec<String>,
}
use wirebolt_app::session::workspace::io::WorkspaceGitChange;
#[uniffi::remote(Record)]
pub struct WorkspaceGitChange {
    pub path: String,
    pub previous_path: Option<String>,
    pub staged: String,
    pub unstaged: String,
    pub conflicted: bool,
}
use wirebolt_app::session::workspace::io::WorkspaceGitStatus;
#[uniffi::remote(Record)]
pub struct WorkspaceGitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub changes: Vec<WorkspaceGitChange>,
    pub merging: bool,
}
use wirebolt_app::session::workspace::io::WorkspaceGitOperation;
#[uniffi::remote(Record)]
pub struct WorkspaceGitOperation {
    pub outcome: String,
    pub revision: Option<String>,
    pub status: WorkspaceGitStatus,
}
use wirebolt_app::session::workspace::io::WorkspaceGitAction;
#[uniffi::remote(Enum)]
pub enum WorkspaceGitAction {
    Status,
    Pull,
    Commit { message: String },
    Push,
    Abort,
    Initialize,
}
use wirebolt_app::session::workspace::io::WorkspaceExport;
#[uniffi::remote(Enum)]
pub enum WorkspaceExport {
    Workspace,
    Collection { id: String },
    Request { collection_id: String, id: String },
}
use wirebolt_app::session::workspace::io::WorkspaceIO;
#[uniffi::remote(Enum)]
pub enum WorkspaceIO {
    Load,
    Commands {
        commands: Vec<SessionWorkspaceCommand>,
    },
    Import {
        format: String,
        source: String,
        name: Option<String>,
    },
    Export {
        target: WorkspaceExport,
    },
    Git {
        action: WorkspaceGitAction,
    },
}
use wirebolt_app::session::workspace::io::WorkspaceReply;
#[uniffi::remote(Enum)]
pub enum WorkspaceReply {
    Loaded { workspace: DomainWorkspace },
    Commands { result: SessionCommandBatchResult },
    Imported { summary: WorkspaceImportSummary },
    Exported { content: String },
    GitStatus { status: WorkspaceGitStatus },
    GitOperation { operation: WorkspaceGitOperation },
    Failed { failure: WorkspaceFailure },
}
use wirebolt_app::session::workspace::actions::WorkspaceAction;
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
use wirebolt_app::session::workspace::operations::WorkspacePresentation;
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
    pub operation_failure: Option<String>,
}
use wirebolt_app::session::workspace::operations::WorkspaceUndoToken;
#[uniffi::remote(Record)]
pub struct WorkspaceUndoToken {
    pub token: u64,
    pub name: String,
}
use wirebolt_app::session::workspace::operations::WorkspaceMutation;
#[uniffi::remote(Record)]
pub struct WorkspaceMutation {
    pub succeeded: bool,
    pub plan: Option<SessionWorkspacePlan>,
    pub undo: Option<WorkspaceUndoToken>,
    pub created_id: Option<String>,
    pub root: Option<DomainCollection>,
}
use wirebolt_app::session::workspace::operations::WorkspaceLoad;
#[uniffi::remote(Record)]
pub struct WorkspaceLoad {
    pub workspace: DomainWorkspace,
    pub selected_environment_id: Option<String>,
    pub select: Option<DomainLocation>,
}
use wirebolt_app::session::workspace::operations::WorkspaceSave;
#[uniffi::remote(Record)]
pub struct WorkspaceSave {
    pub original_authentication: DomainAuthentication,
    pub document_id: String,
    pub draft: DomainRequestDraft,
    pub location: Option<DomainLocation>,
    pub succeeded: bool,
}

#[derive(Debug, uniffi::Object)]
pub struct WorkspaceIOReply {
    sender: Mutex<Option<tokio::sync::oneshot::Sender<WorkspaceReply>>>,
}
#[uniffi::export]
impl WorkspaceIOReply {
    pub fn complete(&self, reply: WorkspaceReply) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(reply);
        }
    }
}
#[uniffi::export(with_foreign)]
pub trait WorkspaceIOPlatform: Send + Sync {
    fn execute(&self, request: WorkspaceIO, reply: Arc<WorkspaceIOReply>);
}
#[derive(uniffi::Object)]
pub struct AppWorkspacePlatform {
    native: Option<Arc<wirebolt_app::session::persistence::SessionPersistence>>,
    foreign: Option<Arc<dyn WorkspaceIOPlatform>>,
}
impl std::fmt::Debug for AppWorkspacePlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppWorkspacePlatform")
            .finish_non_exhaustive()
    }
}
#[uniffi::export]
impl AppWorkspacePlatform {
    #[uniffi::constructor]
    pub fn workspace(bridge: Arc<WorkspaceBridge>) -> Arc<Self> {
        Arc::new(Self {
            native: Some(bridge.inner.clone()),
            foreign: None,
        })
    }
    #[uniffi::constructor]
    pub fn foreign(platform: Arc<dyn WorkspaceIOPlatform>) -> Arc<Self> {
        Arc::new(Self {
            native: None,
            foreign: Some(platform),
        })
    }
}
#[async_trait::async_trait]
impl wirebolt_app::session::workspace::io::WorkspacePlatform for AppWorkspacePlatform {
    async fn execute(&self, request: WorkspaceIO) -> WorkspaceReply {
        if let Some(native) = &self.native {
            let native = native.clone();
            return tokio::task::spawn_blocking(move || native.workspace_io(request))
                .await
                .unwrap_or_else(|_| WorkspaceReply::Failed {
                    failure: WorkspaceFailure::workspace(),
                });
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        if let Some(platform) = &self.foreign {
            platform.execute(
                request,
                Arc::new(WorkspaceIOReply {
                    sender: Mutex::new(Some(sender)),
                }),
            );
        }
        receiver.await.unwrap_or_else(|_| WorkspaceReply::Failed {
            failure: WorkspaceFailure::workspace(),
        })
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppWorkspaceOperations {
    inner: wirebolt_app::session::workspace::operations::WorkspaceOperations,
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

// C2 document lifecycle policy
use wirebolt_app::session::workspace::lifecycle::{WorkspaceDiscard, WorkspaceRestore};
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

// C2 workspace record projections
use wirebolt_app::session::workspace::interface::WorkspaceSidebarState;
#[uniffi::remote(Record)]
pub struct WorkspaceSidebarState {
    pub query: String,
    pub cursor: Option<String>,
    pub scroll_target: Option<String>,
    pub collapsed: Vec<String>,
    pub expanded: Vec<String>,
    pub collections_expanded: bool,
}
use wirebolt_app::session::workspace::interface::WorkspaceSidebarAction;
#[uniffi::remote(Enum)]
pub enum WorkspaceSidebarAction {
    Select { id: String, scroll: bool },
    Step { delta: i64 },
    Arrow { right: bool },
    Type { characters: String, time: f64 },
    SetExpanded { id: String, expanded: bool },
    Open,
    Rename,
}
use wirebolt_app::session::workspace::interface::WorkspaceSidebarResult;
#[uniffi::remote(Record)]
pub struct WorkspaceSidebarResult {
    pub state: WorkspaceSidebarState,
    pub open_request: Option<String>,
    pub rename_row: Option<String>,
    pub focus_url: bool,
    pub handled: bool,
}
use wirebolt_app::session::workspace::interface::WorkspaceDirtyClose;
#[uniffi::remote(Record)]
pub struct WorkspaceDirtyClose {
    pub scope: SessionCloseScope,
    pub group_id: String,
    pub ids: Vec<String>,
    pub titles: Vec<String>,
}
#[derive(Debug, uniffi::Object)]
pub struct AppWorkspaceInterface {
    inner: wirebolt_app::session::workspace::interface::WorkspaceInterface,
}
#[uniffi::export]
impl AppWorkspaceInterface {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: wirebolt_app::session::workspace::interface::WorkspaceInterface::default(),
        })
    }
    pub fn sidebar(
        &self,
        input: WorkspaceSidebarState,
        rows: Vec<wirebolt_app::domain::sidebar::SidebarRow>,
        selected_request: Option<String>,
        action: WorkspaceSidebarAction,
    ) -> WorkspaceSidebarResult {
        self.inner.sidebar(input, rows, selected_request, action)
    }
    pub fn reset(&self) {
        self.inner.reset();
    }
    pub fn block_close(&self, request: WorkspaceDirtyClose) {
        self.inner.block_close(request);
    }
    pub fn take_close(&self) -> Option<WorkspaceDirtyClose> {
        self.inner.take_close()
    }
    pub fn cancel_close(&self) {
        self.inner.cancel_close();
    }
    pub fn closed(&self, previous: Option<String>, active: Option<String>) -> bool {
        self.inner.closed(previous, active)
    }
    pub fn forget_closed(&self) {
        self.inner.forget_closed();
    }
    pub fn reopen(&self, has_active: bool) -> Option<String> {
        self.inner.reopen(has_active)
    }
}
#[uniffi::export]
pub fn workspace_sidebar_filtering(query: String) -> bool {
    wirebolt_app::session::workspace::interface::filtering(&query)
}
#[uniffi::export]
pub fn workspace_sidebar_selection(
    cursor: Option<String>,
    selected: Option<String>,
) -> Option<String> {
    wirebolt_app::session::workspace::interface::selection(cursor, selected)
}
#[uniffi::export]
pub fn workspace_sidebar_move_identifier(
    cursor: Option<String>,
    selected: Option<String>,
    rows: Vec<wirebolt_app::domain::sidebar::SidebarRow>,
    saved: bool,
) -> Option<String> {
    wirebolt_app::session::workspace::interface::move_identifier(
        cursor.as_deref(),
        selected.as_deref(),
        &rows,
        saved,
    )
}

// C2 workspace record projections
use wirebolt_app::session::workspace::files::WorkspaceImportFile;
#[uniffi::remote(Enum)]
pub enum WorkspaceImportFile {
    Bruno,
    Document { source: String, format: String },
    Failure { message: String },
}
#[uniffi::export]
pub fn workspace_read_import(
    path: String,
    preferred: Option<String>,
    formats: String,
) -> WorkspaceImportFile {
    wirebolt_app::session::workspace::files::read_import(Path::new(&path), preferred, &formats)
}
#[uniffi::export]
pub fn workspace_import_display_name(format: String) -> String {
    wirebolt_app::session::workspace::files::display_name(&format)
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

use wirebolt_app::session::workspace::operations::WorkspaceImport;
#[uniffi::remote(Record)]
pub struct WorkspaceImport {
    pub source: String,
    pub format: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub selected: Option<String>,
    pub pasted: bool,
}
#[uniffi::export]
impl AppWorkspaceOperations {
    pub fn take_failure(&self) -> Option<String> {
        self.inner.take_failure()
    }
}
use wirebolt_app::session::workspace::lifecycle::WorkspaceClose;
#[uniffi::remote(Record)]
pub struct WorkspaceClose {
    pub blocked: Vec<String>,
    pub removed: Vec<WorkspaceDiscard>,
    pub snapshot: SessionTabSnapshot,
}
#[uniffi::export]
impl AppSessionTabs {
    pub fn close_workspace(
        &self,
        documents: Vec<SessionDocument>,
        scope: SessionCloseScope,
        group: Option<String>,
        allow_dirty: bool,
    ) -> WorkspaceClose {
        let mut state = lock(&self.tabs);
        let result = wirebolt_app::session::workspace::lifecycle::close(
            &mut state.0,
            documents,
            &scope,
            group.as_deref(),
            allow_dirty,
        );
        let retained: std::collections::HashSet<_> = result.snapshot.session_ids.iter().collect();
        state.1.retain(|id, _| retained.contains(id));
        result
    }
}

#[uniffi::export]
impl AppWorkspaceInterface {
    pub fn enqueue_files(&self, urls: Vec<String>) -> Vec<String> {
        self.inner.enqueue_files(urls)
    }
    pub fn drain_files(&self) -> Vec<String> {
        self.inner.drain_files()
    }
}
use wirebolt_app::session::workspace::operations::WorkspaceRepositoryProblem;
#[uniffi::remote(Enum)]
pub enum WorkspaceRepositoryProblem {
    NotRepository,
    NotRepositoryRoot,
    GitUnavailable,
}
#[uniffi::export]
pub fn workspace_repository_problem(kind: String) -> Option<WorkspaceRepositoryProblem> {
    wirebolt_app::session::workspace::operations::repository_problem(&kind)
}
use wirebolt_app::session::workspace::files::WorkspaceSelectionProblem;
#[uniffi::remote(Enum)]
pub enum WorkspaceSelectionProblem {
    NotFound,
    AlreadyExists,
}
use wirebolt_app::session::workspace::interface::WorkspaceUnsavedPrompt;
#[uniffi::remote(Record)]
pub struct WorkspaceUnsavedPrompt {
    pub message: String,
    pub detail: String,
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
pub fn workspace_unsaved_prompt(titles: Vec<String>, quitting: bool) -> WorkspaceUnsavedPrompt {
    wirebolt_app::session::workspace::interface::unsaved_prompt(&titles, quitting)
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
// C2 live native-editor save boundaries
#[derive(Debug, uniffi::Object)]
pub struct WorkspaceDocumentReply {
    sender: Mutex<Option<tokio::sync::oneshot::Sender<Option<SessionDocument>>>>,
}
#[uniffi::export]
impl WorkspaceDocumentReply {
    pub fn finish(&self, document: Option<SessionDocument>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(document);
        }
    }
}
#[uniffi::export(with_foreign)]
pub trait WorkspaceDocumentPlatform: Send + Sync {
    fn read(&self, id: String, reply: Arc<WorkspaceDocumentReply>);
    fn saved(&self, result: WorkspaceSave, reply: Arc<WorkspaceDocumentReply>);
}
#[derive(uniffi::Object)]
pub struct AppWorkspaceDocuments {
    inner: Arc<dyn WorkspaceDocumentPlatform>,
}
impl std::fmt::Debug for AppWorkspaceDocuments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppWorkspaceDocuments")
            .finish_non_exhaustive()
    }
}
#[uniffi::export]
impl AppWorkspaceDocuments {
    #[uniffi::constructor]
    pub fn foreign(platform: Arc<dyn WorkspaceDocumentPlatform>) -> Arc<Self> {
        Arc::new(Self { inner: platform })
    }
}
#[async_trait::async_trait]
impl wirebolt_app::session::workspace::documents::WorkspaceDocuments for AppWorkspaceDocuments {
    async fn read(&self, id: String) -> Option<SessionDocument> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.inner.read(
            id,
            Arc::new(WorkspaceDocumentReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver.await.ok().flatten()
    }
    async fn saved(&self, result: WorkspaceSave) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.inner.saved(
            result,
            Arc::new(WorkspaceDocumentReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        let _ = receiver.await;
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

use wirebolt_app::domain::DomainAuthentication;

// END C2 workspace application orchestration
