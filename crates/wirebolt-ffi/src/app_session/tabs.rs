//! Typed tabs application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::run::AppDocumentSession;
use super::{
    Arc, DomainLocation, DomainRequestDraft, HashMap, Mutex, SessionCloseScope, SessionDocument,
    SessionDocumentEffect, SessionEditorGroup, SessionKind, SessionLayout, SessionLayoutGroup,
    SessionLayoutTab, SessionRestoreResult, SessionTabDocument, SessionTabSnapshot, SessionTabs,
    WorkspaceClose,
};
use crate::lock;
use wirebolt_app::session::tabs::SessionCloseTargets;

/// A native editor's bounded projection and the Rust document that owns its
/// saved state. Draft bodies never enter the close-decision boundary.
#[derive(Debug, uniffi::Record)]
pub struct AppCloseDocument {
    pub document: Arc<AppDocumentSession>,
    pub projection: SessionTabDocument,
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
pub struct SessionLayoutGroup {
    tabs: Vec<SessionLayoutTab>,
    selected_index: Option<i64>,
    preview_index: Option<i64>,
}

#[uniffi::remote(Record)]
#[uniffi(name = "RustSessionLayout")]
pub struct SessionLayout {
    groups: Vec<SessionLayoutGroup>,
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
pub struct SessionCloseTargets {
    group_id: String,
    ids: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct SessionTabSnapshot {
    groups: Vec<SessionEditorGroup>,
    active_group_id: String,
    session_ids: Vec<String>,
}

#[derive(Debug, uniffi::Object)]
pub struct AppSessionTabs {
    pub(super) tabs: Mutex<(SessionTabs, HashMap<String, SessionTabDocument>)>,
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
    pub fn layout(&self) -> SessionLayout {
        let state = lock(&self.tabs);
        state.0.layout(&state.1)
    }
    pub fn restore(
        &self,
        layout: SessionLayout,
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
}

#[uniffi::remote(Record)]
pub struct SessionRestoreResult {
    pub documents: Vec<SessionDocument>,
    pub snapshot: SessionTabSnapshot,
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

#[uniffi::export]
pub fn session_tab_destination(
    tabs: Vec<String>,
    selected: Option<String>,
    index: Option<i64>,
    offset: i64,
) -> Option<String> {
    wirebolt_app::session::workspace::tab_destination(&tabs, selected.as_deref(), index, offset)
}

#[uniffi::export]
pub fn session_decode_layout(data: Vec<u8>) -> Result<SessionLayout, crate::WorkspaceBridgeError> {
    wirebolt_app::session::tabs::decode_layout(&data)
}

#[uniffi::export]
pub fn session_encode_layout(
    layout: SessionLayout,
) -> Result<Vec<u8>, crate::WorkspaceBridgeError> {
    wirebolt_app::session::tabs::encode_layout(&layout)
}

#[uniffi::export]
impl AppSessionTabs {
    pub fn close_workspace(
        &self,
        documents: Vec<AppCloseDocument>,
        targets: SessionCloseTargets,
        allow_dirty: bool,
    ) -> WorkspaceClose {
        wirebolt_app::session::workspace::lifecycle::close(
            &mut lock(&self.tabs),
            documents
                .iter()
                .map(|entry| (entry.projection.clone(), entry.document.as_ref()))
                .collect(),
            &targets,
            allow_dirty,
        )
    }
    pub fn close_targets(
        &self,
        scope: SessionCloseScope,
        group: Option<String>,
    ) -> SessionCloseTargets {
        lock(&self.tabs).0.close_targets(&scope, group.as_deref())
    }
}
