use super::super::tabs::*;
use crate::domain::{DomainLocation, DomainRequestDraft};
use std::collections::HashMap;
fn document(id: &str) -> SessionTabDocument {
    SessionTabDocument {
        id: id.into(),
        kind: SessionKind::Http,
        collection_id: Some("api".into()),
        request_id: id.into(),
        dirty: false,
        touched: false,
    }
}
fn open(
    tabs: &mut SessionTabs,
    documents: &mut HashMap<String, SessionTabDocument>,
    id: &str,
    preview: bool,
) -> String {
    tabs.open(document(id), documents, None, false, preview).0
}
#[test]
fn preview_replaces_untouched_tab_in_place_and_prunes_history() {
    let mut tabs = SessionTabs::default();
    let mut documents = HashMap::new();
    open(&mut tabs, &mut documents, "pinned", false);
    open(&mut tabs, &mut documents, "a", true);
    open(&mut tabs, &mut documents, "b", true);
    assert_eq!(tabs.groups[0].tab_ids, ["pinned", "b"]);
    assert_eq!(tabs.groups[0].selected_tab_id.as_deref(), Some("b"));
    assert!(tabs.preview("b", &documents));
    assert!(!tabs.preview("pinned", &documents));
    assert!(!documents.contains_key("a"));
    assert!(!tabs.groups[0].backward_tab_ids.contains(&"a".into()));
}
#[test]
fn editing_or_sending_preview_keeps_it_open() {
    let mut tabs = SessionTabs::default();
    let mut documents = HashMap::new();
    open(&mut tabs, &mut documents, "a", true);
    documents.get_mut("a").unwrap().touched = true;
    assert!(!tabs.preview("a", &documents));
    open(&mut tabs, &mut documents, "b", true);
    documents.get_mut("b").unwrap().touched = true;
    open(&mut tabs, &mut documents, "c", true);
    assert_eq!(tabs.groups[0].tab_ids, ["a", "b", "c"]);
    assert!(tabs.preview("c", &documents));
}
#[test]
fn unchanged_draft_write_stays_preview() {
    let draft = DomainRequestDraft::default();
    let mut doc = SessionDocument::new(
        "a".into(),
        SessionKind::Http,
        None,
        None,
        draft.clone(),
        Some(draft),
        "",
    );
    doc.edit(doc.draft.clone());
    assert!(!doc.touched && !doc.dirty());
}
#[test]
fn pin_and_reselect_preserve_preview_and_order() {
    let mut tabs = SessionTabs::default();
    let mut documents = HashMap::new();
    open(&mut tabs, &mut documents, "a", true);
    tabs.pin("a");
    assert!(!tabs.preview("a", &documents));
    open(&mut tabs, &mut documents, "b", true);
    open(&mut tabs, &mut documents, "a", true);
    assert_eq!(tabs.groups[0].tab_ids, ["a", "b"]);
    assert!(tabs.preview("b", &documents));
    assert_eq!(tabs.groups[0].selected_tab_id.as_deref(), Some("a"));
}
#[test]
fn closing_preview_clears_it_and_explicit_open_stays_pinned() {
    let mut tabs = SessionTabs::default();
    let mut documents = HashMap::new();
    open(&mut tabs, &mut documents, "a", true);
    tabs.close(
        &SessionCloseScope::One { id: "a".into() },
        None,
        false,
        &mut documents,
    );
    assert!(tabs.groups[0].preview_tab_id.is_none());
    open(&mut tabs, &mut documents, "b", false);
    assert!(!tabs.preview("b", &documents));
    open(&mut tabs, &mut documents, "c", true);
    assert_eq!(tabs.groups[0].tab_ids, ["b", "c"]);
}
#[test]
fn layout_roundtrip_remembers_preview_selection_and_split() {
    let mut tabs = SessionTabs::default();
    let mut documents = HashMap::new();
    open(&mut tabs, &mut documents, "a", false);
    open(&mut tabs, &mut documents, "b", true);
    tabs.select("a", None);
    let layout = tabs.layout(&documents);
    assert_eq!(layout.groups[0].preview_index, Some(1));
    let decoded = serde_json::from_slice(&serde_json::to_vec(&layout).unwrap()).unwrap();
    let resolved = vec!["a", "b"]
        .into_iter()
        .map(|id| {
            Some(DomainLocation {
                collection_id: "api".into(),
                request: DomainRequestDraft {
                    id: id.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
        })
        .collect();
    let mut restored = SessionTabs::default();
    let mut docs = HashMap::new();
    restored.restore(&decoded, resolved, &mut docs);
    assert_eq!(
        restored.groups[0].selected_tab_id,
        Some(restored.groups[0].tab_ids[0].clone())
    );
    assert!(restored.preview(&restored.groups[0].tab_ids[1], &docs));
    assert!(!restored.preview(&restored.groups[0].tab_ids[0], &docs));
}
#[test]
fn legacy_layout_defaults_preview_to_none() {
    let layout:SessionLayout=serde_json::from_str(r#"{"groups":[{"tabs":[{"collectionID":"api","requestID":"a"}],"selectedIndex":0}],"activeGroupIndex":0}"#).unwrap();
    assert_eq!(layout.groups[0].preview_index, None);
}
#[test]
fn close_scope_is_atomic_when_dirty_and_prunes_navigation() {
    let mut tabs = SessionTabs::default();
    let mut documents = HashMap::new();
    for id in ["a", "b", "c"] {
        open(&mut tabs, &mut documents, id, false);
    }
    documents.get_mut("b").unwrap().dirty = true;
    assert_eq!(
        tabs.close(
            &SessionCloseScope::RightOf { id: "a".into() },
            None,
            false,
            &mut documents
        ),
        ["b"]
    );
    assert_eq!(documents.len(), 3);
    assert!(
        tabs.close(
            &SessionCloseScope::RightOf { id: "a".into() },
            None,
            true,
            &mut documents
        )
        .is_empty()
    );
    assert_eq!(tabs.groups[0].tab_ids, ["a"]);
    assert_eq!(tabs.groups[0].backward_tab_ids, ["a"]);
}
#[test]
fn back_forward_and_new_selection_clear_forward_history() {
    let mut tabs = SessionTabs::default();
    let mut documents = HashMap::new();
    for id in ["a", "b", "c"] {
        open(&mut tabs, &mut documents, id, false);
    }
    tabs.navigate(false);
    assert_eq!(tabs.groups[0].selected_tab_id.as_deref(), Some("b"));
    tabs.navigate(true);
    assert_eq!(tabs.groups[0].selected_tab_id.as_deref(), Some("c"));
    tabs.navigate(false);
    tabs.select("a", None);
    assert!(tabs.groups[0].forward_tab_ids.is_empty());
}
#[test]
fn restore_skips_deleted_requests_and_empty_groups() {
    let layout = SessionLayout {
        groups: vec![SessionLayoutGroup {
            tabs: vec![SessionLayoutTab {
                collection_id: "api".into(),
                request_id: "missing".into(),
            }],
            selected_index: Some(0),
            preview_index: None,
        }],
        active_group_index: 0,
    };
    let mut tabs = SessionTabs::default();
    let original = tabs.active_group_id.clone();
    let mut documents = HashMap::new();
    assert!(tabs.restore(&layout, vec![None], &mut documents).is_empty());
    assert_eq!(tabs.active_group_id, original);
}
#[test]
fn kind_normalization_and_saved_state_do_not_touch_document() {
    let draft = DomainRequestDraft {
        url: "wss://example.com?x=1".into(),
        ..Default::default()
    };
    let document = SessionDocument::new(
        "a".into(),
        SessionKind::Http,
        None,
        None,
        draft.clone(),
        Some(draft),
        "",
    );
    assert_eq!(document.kind, SessionKind::WebSocket);
    assert!(!document.dirty() && !document.touched);
    assert_eq!(document.draft.query.len(), 1);
}
#[test]
fn legacy_signed_layout_indices_remain_decodable_and_fall_back_to_last_tab() {
    let layout=super::super::tabs::decode_layout(br#"{"groups":[{"tabs":[{"collectionID":"api","requestID":"a"},{"collectionID":"api","requestID":"b"}],"selectedIndex":-1,"previewIndex":-1}],"activeGroupIndex":-1}"#).unwrap();
    assert_eq!(layout.groups[0].selected_index, Some(-1));
    let resolved = ["a", "b"]
        .into_iter()
        .map(|id| {
            Some(DomainLocation {
                collection_id: "api".into(),
                request: DomainRequestDraft {
                    id: id.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
        })
        .collect();
    let mut tabs = SessionTabs::default();
    let mut documents = HashMap::new();
    tabs.restore(&layout, resolved, &mut documents);
    assert_eq!(
        tabs.groups[0].selected_tab_id,
        tabs.groups[0].tab_ids.last().cloned()
    );
    assert!(tabs.groups[0].preview_tab_id.is_none());
    assert!(
        super::super::tabs::decode_layout(&super::super::tabs::encode_layout(&layout).unwrap())
            .is_ok()
    );
}
