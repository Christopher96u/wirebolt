//! Interface workflow policy; native focus and editor object identity remain adapters.
use crate::domain::sidebar::{self, SidebarOutcome, SidebarRow};
use crate::lock;
use crate::session::tabs::SessionCloseScope;
use std::{collections::HashSet, sync::Mutex};
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct WorkspaceSidebarState {
    pub query: String,
    pub cursor: Option<String>,
    pub scroll_target: Option<String>,
    pub collapsed: Vec<String>,
    pub expanded: Vec<String>,
    pub collections_expanded: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum WorkspaceSidebarAction {
    Select { id: String, scroll: bool },
    Step { delta: i64 },
    Arrow { right: bool },
    Type { characters: String, time: f64 },
    SetExpanded { id: String, expanded: bool },
    Open,
    Rename,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSidebarResult {
    pub state: WorkspaceSidebarState,
    pub open_request: Option<String>,
    pub rename_row: Option<String>,
    pub focus_url: bool,
    pub handled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceDirtyClose {
    pub scope: SessionCloseScope,
    pub group_id: String,
    pub ids: Vec<String>,
    pub titles: Vec<String>,
}
#[derive(Debug, Default)]
struct State {
    sidebar: WorkspaceSidebarState,
    type_buffer: String,
    type_time: f64,
    dirty_close: Option<WorkspaceDirtyClose>,
    last_closed: Option<String>,
    files: Vec<String>,
}
#[derive(Debug, Default)]
pub struct WorkspaceInterface {
    state: Mutex<State>,
}
#[must_use]
pub fn filtering(query: &str) -> bool {
    !query.trim().is_empty()
}
#[must_use]
pub fn selection(cursor: Option<String>, selected_request: Option<String>) -> Option<String> {
    cursor.or_else(|| selected_request.map(|id| format!("request:{id}")))
}
fn expanded(row: &SidebarRow, state: &WorkspaceSidebarState) -> bool {
    row.request_index.is_none()
        && (filtering(&state.query)
            || if row.group_index.is_none() {
                !state.collapsed.contains(&row.collection_id)
            } else {
                state.expanded.contains(&format!(
                    "{}:{}",
                    row.collection_id,
                    row.id.split(':').next_back().unwrap_or_default()
                ))
            })
}
fn set_expanded(row: &SidebarRow, state: &mut WorkspaceSidebarState, value: bool) {
    let (values, key, insert) = if row.request_index.is_some() {
        return;
    } else if row.group_index.is_none() {
        (&mut state.collapsed, row.collection_id.clone(), !value)
    } else {
        (
            &mut state.expanded,
            row.id.strip_prefix("group:").unwrap_or_default().into(),
            value,
        )
    };
    values.retain(|v| v != &key);
    if insert {
        values.push(key);
    }
}
fn select(
    id: &str,
    scroll: bool,
    rows: &[SidebarRow],
    result: &mut WorkspaceSidebarResult,
    selected: Option<&str>,
) {
    let Some(row) = rows.iter().find(|r| r.id == id) else {
        return;
    };
    if row.request_index.is_some() {
        result.state.cursor = None;
        if row.id.strip_prefix("request:") != selected {
            result.open_request = Some(row.id.clone());
        }
    } else {
        result.state.cursor = Some(row.id.clone());
    }
    if scroll {
        result.state.scroll_target = Some(row.id.clone());
    }
}
#[allow(
    clippy::needless_pass_by_value,
    reason = "Interface actions carry lifted command snapshots into retained state."
)]
impl WorkspaceInterface {
    #[must_use]
    pub fn sidebar(
        &self,
        input: WorkspaceSidebarState,
        rows: Vec<SidebarRow>,
        selected_request: Option<String>,
        action: WorkspaceSidebarAction,
    ) -> WorkspaceSidebarResult {
        let mut s = lock(&self.state);
        let current = selection(input.cursor.clone(), selected_request.clone());
        let mut result = WorkspaceSidebarResult {
            state: input,
            open_request: None,
            rename_row: None,
            focus_url: false,
            handled: true,
        };
        match action {
            WorkspaceSidebarAction::Select { id, scroll } => {
                select(&id, scroll, &rows, &mut result, selected_request.as_deref());
            }
            WorkspaceSidebarAction::Step { delta } => {
                if let Some(id) = sidebar::step(&rows, current.as_deref(), delta) {
                    select(&id, true, &rows, &mut result, selected_request.as_deref());
                }
            }
            WorkspaceSidebarAction::Arrow { right } => {
                let is_expanded = current
                    .as_ref()
                    .and_then(|id| rows.iter().find(|r| &r.id == id))
                    .is_some_and(|r| expanded(r, &result.state));
                match sidebar::arrow(&rows, current.as_deref(), right, is_expanded) {
                    SidebarOutcome::Select { id } => {
                        select(&id, true, &rows, &mut result, selected_request.as_deref());
                    }
                    SidebarOutcome::Expand { id } | SidebarOutcome::Collapse { id } => {
                        if let Some(row) = rows.iter().find(|r| r.id == id) {
                            if is_expanded {
                                result.state.cursor = Some(id);
                            }
                            set_expanded(row, &mut result.state, !is_expanded);
                        }
                    }
                    SidebarOutcome::None => {}
                }
            }
            WorkspaceSidebarAction::Type { characters, time } => {
                if time - s.type_time > 1.0 {
                    s.type_buffer.clear();
                }
                if s.type_buffer.is_empty() && characters == " " {
                    result.handled = false;
                } else {
                    s.type_buffer.push_str(&characters);
                    s.type_time = time;
                    if let Some(id) =
                        sidebar::type_select(&rows, current.as_deref(), &s.type_buffer)
                    {
                        select(&id, true, &rows, &mut result, selected_request.as_deref());
                    }
                }
            }
            WorkspaceSidebarAction::SetExpanded { id, expanded } => {
                if let Some(row) = rows.iter().find(|r| r.id == id) {
                    set_expanded(row, &mut result.state, expanded);
                }
            }
            WorkspaceSidebarAction::Open => {
                if let Some(row) = current
                    .as_ref()
                    .and_then(|id| rows.iter().find(|r| &r.id == id))
                {
                    if row.request_index.is_some() {
                        result.focus_url = true;
                    } else {
                        let value = !expanded(row, &result.state);
                        set_expanded(row, &mut result.state, value);
                    }
                }
            }
            WorkspaceSidebarAction::Rename => {
                result.rename_row = current.filter(|id| rows.iter().any(|r| &r.id == id));
            }
        }
        s.sidebar = result.state.clone();
        result
    }
    pub fn reset(&self) {
        *lock(&self.state) = State::default();
    }
    pub fn block_close(&self, request: WorkspaceDirtyClose) {
        lock(&self.state).dirty_close = Some(request);
    }
    #[must_use]
    pub fn close_request(&self) -> Option<WorkspaceDirtyClose> {
        lock(&self.state).dirty_close.clone()
    }
    #[must_use]
    pub fn take_close(&self) -> Option<WorkspaceDirtyClose> {
        lock(&self.state).dirty_close.take()
    }
    pub fn cancel_close(&self) {
        lock(&self.state).dirty_close = None;
    }
    pub fn closed(&self, previous: Option<String>, active: Option<String>) -> bool {
        if active.is_some() {
            return false;
        }
        lock(&self.state).last_closed = previous;
        true
    }
    pub fn forget_closed(&self) {
        lock(&self.state).last_closed = None;
    }
    #[must_use]
    pub fn reopen(&self, has_active: bool) -> Option<String> {
        if has_active {
            None
        } else {
            lock(&self.state).last_closed.take()
        }
    }
}
#[must_use]
pub fn move_identifier(
    cursor: Option<&str>,
    selected: Option<&str>,
    rows: &[SidebarRow],
    saved_request: bool,
) -> Option<String> {
    if let Some(cursor) = cursor {
        return rows
            .iter()
            .find(|r| r.id == cursor)?
            .move_identifier
            .clone();
    }
    if !saved_request {
        return None;
    }
    let (collection, id) = selected?.split_once('/')?;
    Some(format!("request|{collection}|{id}"))
}
#[must_use]
pub fn retained_presentations(remaining: Vec<String>, presentations: Vec<String>) -> Vec<String> {
    let remaining: HashSet<_> = remaining.into_iter().collect();
    presentations
        .into_iter()
        .filter(|id| remaining.contains(id))
        .collect()
}

impl WorkspaceInterface {
    pub fn enqueue_files(&self, urls: Vec<String>) -> Vec<String> {
        let mut s = lock(&self.state);
        s.files.extend(
            urls.into_iter()
                .filter(|s| url::Url::parse(s).is_ok_and(|u| u.scheme() == "file")),
        );
        s.files.clone()
    }
    #[must_use]
    pub fn drain_files(&self) -> Vec<String> {
        std::mem::take(&mut lock(&self.state).files)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceUnsavedPrompt {
    pub message: String,
    pub detail: String,
}
#[must_use]
pub fn unsaved_prompt(titles: &[String], quitting: bool) -> WorkspaceUnsavedPrompt {
    WorkspaceUnsavedPrompt {
        message: if titles.len() == 1 {
            format!(
                "Do you want to save the changes you made to “{}”?",
                titles[0]
            )
        } else {
            format!(
                "You have unsaved changes in {} requests. Do you want to save them?",
                titles.len()
            )
        },
        detail: if quitting {
            "Your changes will be lost if you quit without saving."
        } else {
            "Your changes will be lost if you don’t save them."
        }
        .into(),
    }
}
