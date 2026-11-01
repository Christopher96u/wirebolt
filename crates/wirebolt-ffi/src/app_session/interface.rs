//! Typed interface application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{
    Arc, DomainBody, SessionCloseScope, WorkspaceDirtyClose, WorkspaceSidebarAction,
    WorkspaceSidebarResult, WorkspaceSidebarState, WorkspaceUnsavedPrompt,
};
#[uniffi::remote(Record)]
pub struct WorkspaceSidebarState {
    pub query: String,
    pub cursor: Option<String>,
    pub scroll_target: Option<String>,
    pub collapsed: Vec<String>,
    pub expanded: Vec<String>,
    pub collections_expanded: bool,
}

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

#[uniffi::remote(Record)]
pub struct WorkspaceSidebarResult {
    pub state: WorkspaceSidebarState,
    pub open_request: Option<String>,
    pub rename_row: Option<String>,
    pub focus_url: bool,
    pub handled: bool,
}

#[uniffi::remote(Record)]
pub struct WorkspaceDirtyClose {
    pub scope: SessionCloseScope,
    pub group_id: String,
    pub ids: Vec<String>,
    pub titles: Vec<String>,
}

#[derive(Debug, uniffi::Object)]
pub struct AppWorkspaceInterface {
    pub(super) inner: wirebolt_app::session::workspace::interface::WorkspaceInterface,
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
    pub fn close_request(&self) -> Option<WorkspaceDirtyClose> {
        self.inner.close_request()
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

#[uniffi::export]
impl AppWorkspaceInterface {
    pub fn enqueue_files(&self, urls: Vec<String>) -> Vec<String> {
        self.inner.enqueue_files(urls)
    }
    pub fn drain_files(&self) -> Vec<String> {
        self.inner.drain_files()
    }
}

#[uniffi::remote(Record)]
pub struct WorkspaceUnsavedPrompt {
    pub message: String,
    pub detail: String,
}

#[uniffi::export]
pub fn workspace_unsaved_prompt(titles: Vec<String>, quitting: bool) -> WorkspaceUnsavedPrompt {
    wirebolt_app::session::workspace::interface::unsaved_prompt(&titles, quitting)
}

#[uniffi::export]
pub fn workspace_response_section(value: String) -> String {
    wirebolt_app::session::workspace::presentation::response_section(value)
}
#[uniffi::export]
pub fn workspace_response_renderer(value: String) -> String {
    wirebolt_app::session::workspace::presentation::response_renderer(value)
}
#[uniffi::export]
pub fn workspace_request_section(body: DomainBody) -> String {
    wirebolt_app::session::workspace::presentation::request_section(&body)
}
