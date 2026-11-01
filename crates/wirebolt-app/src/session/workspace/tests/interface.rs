use super::super::interface::*;
use super::*;
fn rows() -> Vec<crate::domain::sidebar::SidebarRow> {
    crate::domain::sidebar::SidebarIndex::new(&nested().collections).rows
}
fn input() -> WorkspaceSidebarState {
    WorkspaceSidebarState {
        collections_expanded: true,
        ..Default::default()
    }
}
#[test]
fn sidebar_cursor_overrides_active_request_and_selection_opens_only_changed_requests() {
    let ui = WorkspaceInterface::default();
    assert_eq!(
        selection(Some("group:api:parent".into()), Some("api/root".into())).as_deref(),
        Some("group:api:parent")
    );
    let r = ui.sidebar(
        input(),
        rows(),
        Some("api/root".into()),
        WorkspaceSidebarAction::Select {
            id: "request:api/root".into(),
            scroll: true,
        },
    );
    assert!(r.open_request.is_none());
    assert!(r.state.cursor.is_none());
    assert_eq!(r.state.scroll_target.as_deref(), Some("request:api/root"));
    let r = ui.sidebar(
        input(),
        rows(),
        Some("api/root".into()),
        WorkspaceSidebarAction::Select {
            id: "request:api/deep".into(),
            scroll: false,
        },
    );
    assert_eq!(r.open_request.as_deref(), Some("request:api/deep"));
}
#[test]
fn sidebar_arrow_collapse_selects_container_and_filter_keeps_outline_expanded() {
    let ui = WorkspaceInterface::default();
    let mut state = input();
    state.cursor = Some("group:api:parent".into());
    state.expanded = vec!["api:parent".into()];
    let r = ui.sidebar(
        state,
        rows(),
        None,
        WorkspaceSidebarAction::Arrow { right: false },
    );
    assert!(r.state.expanded.is_empty());
    assert_eq!(r.state.cursor.as_deref(), Some("group:api:parent"));
    let mut state = r.state;
    state.query = "Deep".into();
    let r = ui.sidebar(
        state,
        rows(),
        None,
        WorkspaceSidebarAction::Arrow { right: true },
    );
    assert!(r.state.expanded.is_empty());
    assert_eq!(r.state.cursor.as_deref(), Some("group:api:child"));
}
#[test]
fn sidebar_type_ahead_has_one_second_timeout_and_rejects_leading_space() {
    let ui = WorkspaceInterface::default();
    let r = ui.sidebar(
        input(),
        rows(),
        None,
        WorkspaceSidebarAction::Type {
            characters: " ".into(),
            time: 10.0,
        },
    );
    assert!(!r.handled);
    let r = ui.sidebar(
        input(),
        rows(),
        None,
        WorkspaceSidebarAction::Type {
            characters: "P".into(),
            time: 10.1,
        },
    );
    assert_eq!(r.state.cursor.as_deref(), Some("group:api:parent"));
    let r = ui.sidebar(
        r.state,
        rows(),
        None,
        WorkspaceSidebarAction::Type {
            characters: "a".into(),
            time: 10.5,
        },
    );
    assert_eq!(r.state.cursor.as_deref(), Some("group:api:parent"));
    let r = ui.sidebar(
        r.state,
        rows(),
        None,
        WorkspaceSidebarAction::Type {
            characters: "D".into(),
            time: 12.0,
        },
    );
    assert_eq!(r.open_request.as_deref(), Some("request:api/deep"));
}
#[test]
fn sidebar_open_and_rename_return_native_effects_without_focus_policy_in_swift() {
    let ui = WorkspaceInterface::default();
    let r = ui.sidebar(
        input(),
        rows(),
        Some("api/root".into()),
        WorkspaceSidebarAction::Open,
    );
    assert!(r.focus_url);
    let r = ui.sidebar(
        input(),
        rows(),
        Some("api/root".into()),
        WorkspaceSidebarAction::Rename,
    );
    assert_eq!(r.rename_row.as_deref(), Some("request:api/root"));
    let mut state = input();
    state.cursor = Some("group:api:parent".into());
    let r = ui.sidebar(state, rows(), None, WorkspaceSidebarAction::Open);
    assert_eq!(r.state.expanded, ["api:parent"]);
}
#[test]
fn sidebar_move_identifier_tracks_cursor_and_requires_saved_document() {
    let rows = rows();
    assert_eq!(
        move_identifier(Some("group:api:parent"), None, &rows, false).as_deref(),
        Some("group|api|parent")
    );
    assert!(move_identifier(None, Some("api/root"), &rows, false).is_none());
    assert_eq!(
        move_identifier(None, Some("api/root"), &rows, true).as_deref(),
        Some("request|api|root")
    );
    assert!(filtering(" Deep "));
    assert!(!filtering(" \n"));
}
#[test]
fn dirty_close_retains_scope_group_and_document_order_until_confirm_or_cancel() {
    let ui = WorkspaceInterface::default();
    let pending = WorkspaceDirtyClose {
        scope: SessionCloseScope::All,
        group_id: "left".into(),
        ids: vec!["a".into(), "b".into()],
        titles: vec!["A".into(), "B".into()],
    };
    ui.block_close(pending.clone());
    assert_eq!(ui.take_close(), Some(pending.clone()));
    assert!(ui.take_close().is_none());
    ui.block_close(pending);
    ui.cancel_close();
    assert!(ui.take_close().is_none());
}
#[test]
fn last_document_reopens_only_with_no_active_document_and_is_cleared_by_delete_or_switch() {
    let ui = WorkspaceInterface::default();
    assert!(!ui.closed(Some("a".into()), Some("b".into())));
    assert!(ui.reopen(false).is_none());
    assert!(ui.closed(Some("a".into()), None));
    assert!(ui.reopen(true).is_none());
    assert_eq!(ui.reopen(false).as_deref(), Some("a"));
    assert!(ui.reopen(false).is_none());
    ui.closed(Some("b".into()), None);
    ui.forget_closed();
    assert!(ui.reopen(false).is_none());
    ui.closed(Some("c".into()), None);
    ui.reset();
    assert!(ui.reopen(false).is_none());
}
#[test]
fn unsaved_prompts_preserve_single_plural_and_quit_text() {
    let single = unsaved_prompt(&["A".into()], true);
    assert_eq!(
        single.message,
        "Do you want to save the changes you made to “A”?"
    );
    assert_eq!(
        single.detail,
        "Your changes will be lost if you quit without saving."
    );
    let multiple = unsaved_prompt(&["A".into(), "B".into()], false);
    assert_eq!(
        multiple.message,
        "You have unsaved changes in 2 requests. Do you want to save them?"
    );
    assert_eq!(
        multiple.detail,
        "Your changes will be lost if you don’t save them."
    );
}
