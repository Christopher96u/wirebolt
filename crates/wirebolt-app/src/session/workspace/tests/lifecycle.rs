use super::*;
use crate::lock;
use crate::session::workspace::lifecycle as policy;
use std::collections::HashMap;
#[test]
fn dirty_documents_are_unique_and_follow_tab_order() {
    let mut a = document("a", Some("api"), true);
    let mut b = document("b", Some("api"), true);
    let c = document("c", Some("api"), true);
    b.draft.url = "https://example.com/b".into();
    a.draft.url = "https://example.com/a".into();
    let groups = [SessionEditorGroup {
        tab_ids: vec!["a".into(), "b".into(), "c".into(), "a".into()],
        ..Default::default()
    }];
    assert_eq!(
        policy::dirty_ids(
            &groups,
            &[a, b, c]
                .iter()
                .map(SessionTabDocument::from_document)
                .collect::<Vec<_>>()
        ),
        ["a", "b"]
    );
}
#[tokio::test]
async fn save_all_persists_each_dirty_document_at_its_own_collection() {
    let h = Harness::new(nested());
    let mut a = document("a", Some("api"), true);
    let mut b = document("b", Some("other"), true);
    a.draft.url = "https://example.com/a".into();
    b.draft.url = "https://example.com/b".into();
    let results = h
        .ops
        .save_all(
            vec![a, b],
            Some("fallback".into()),
            &h.store,
            &h.secrets,
            &Secrets,
        )
        .await;
    assert!(results.iter().all(|r| r.succeeded));
    let saved: Vec<_> = lock(&h.store.commands)
        .iter()
        .filter_map(|c| {
            if let SessionWorkspaceCommand::SaveRequest { location } = c {
                Some((location.collection_id.clone(), location.request.url.clone()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        saved,
        [
            ("api".into(), "https://example.com/a".into()),
            ("other".into(), "https://example.com/b".into())
        ]
    );
}
#[tokio::test]
async fn failed_save_all_stops_and_reports_failure_after_successful_prefix() {
    let h = Harness::new(nested());
    *lock(&h.store.fail_at) = Some(1);
    let results = h
        .ops
        .save_all(
            vec![
                document("a", Some("api"), false),
                document("b", Some("api"), false),
                document("c", Some("api"), false),
            ],
            None,
            &h.store,
            &h.secrets,
            &Secrets,
        )
        .await;
    assert_eq!(results.len(), 2);
    assert!(results[0].succeeded);
    assert!(!results[1].succeeded);
    assert_eq!(lock(&h.store.commands).len(), 1);
    assert_eq!(
        h.ops.presentation().operation_failure,
        Some(crate::response::ResponseFailureKind::Workspace)
    );
}
#[test]
fn discard_reverts_saved_documents_and_closes_never_saved_documents() {
    let mut saved = document("a", Some("api"), true);
    saved.draft.name = "Renamed".into();
    let temporary = document("temp", None, false);
    let effects = policy::discard(vec![saved, temporary, document("clean", None, true)]);
    assert_eq!(effects.len(), 2);
    assert_eq!(effects[0].saved.as_ref().unwrap().name, "A");
    assert!(effects[1].saved.is_none());
}
fn layout() -> SessionLayout {
    SessionLayout {
        groups: vec![
            SessionLayoutGroup {
                tabs: ["a", "b", "c"]
                    .map(|id| SessionLayoutTab {
                        collection_id: "api".into(),
                        request_id: id.into(),
                    })
                    .to_vec(),
                selected_index: Some(2),
                preview_index: Some(1),
            },
            SessionLayoutGroup {
                tabs: vec![SessionLayoutTab {
                    collection_id: "api".into(),
                    request_id: "b".into(),
                }],
                selected_index: Some(0),
                preview_index: None,
            },
        ],
        active_group_index: 0,
    }
}
#[test]
fn layout_roundtrip_restores_groups_selection_and_skips_temporary_documents() {
    let mut w = nested();
    w.collections[0].requests = ["a", "b", "c"].map(|id| location(id, None, 0)).to_vec();
    let source = layout();
    let decoded =
        crate::session::tabs::decode_layout(&crate::session::tabs::encode_layout(&source).unwrap())
            .unwrap();
    let mut tabs = SessionTabs::default();
    let mut docs = HashMap::new();
    let result = policy::restore(&w, &mut tabs, &mut docs, Some(decoded));
    assert!(result.restore_history);
    assert_eq!(result.result.snapshot.groups.len(), 2);
    assert_eq!(
        result
            .result
            .documents
            .iter()
            .map(|d| d.request_id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c", "b"]
    );
    assert_eq!(tabs.layout(&docs), source);
    assert!(result.result.documents.iter().all(|d| !d.dirty()));
}
#[test]
fn stale_layout_falls_back_to_first_request_without_history_replay() {
    let mut tabs = SessionTabs::default();
    let mut docs = HashMap::new();
    let result = policy::restore(&nested(), &mut tabs, &mut docs, Some(layout()));
    assert!(!result.restore_history);
    assert_eq!(result.result.documents[0].request_id, "deep");
    assert_eq!(result.result.snapshot.groups.len(), 1);
}
#[tokio::test]
async fn loaded_state_covers_no_store_success_and_failure() {
    let h = Harness::new(nested());
    assert!(!h.ops.presentation().loaded);
    assert!(!h.ops.begin_load(false));
    assert!(h.ops.presentation().loaded);
    assert!(h.ops.begin_load(true));
    assert!(h.ops.presentation().loading);
    assert!(h.ops.load(&h.store, false).await.is_none());
    assert!(!h.ops.presentation().loading && h.ops.presentation().loaded);
    lock(&h.store.replies).push_back(WorkspaceReply::Loaded {
        workspace: nested(),
    });
    assert!(h.ops.begin_load(true));
    assert!(h.ops.load(&h.store, false).await.is_some());
}
#[test]
fn signed_layout_indices_preserve_legacy_decode() {
    let layout=crate::session::tabs::decode_layout(br#"{"groups":[{"tabs":[{"collectionID":"api","requestID":"a"}],"selectedIndex":-1,"previewIndex":-1}],"activeGroupIndex":-1}"#).unwrap();
    assert_eq!(layout.active_group_index, -1);
    assert_eq!(layout.groups[0].selected_index, Some(-1));
    assert_eq!(layout.groups[0].preview_index, Some(-1));
}
#[test]
fn close_dirty_scope_blocks_atomically_then_discards_only_closed_documents() {
    let mut tabs = SessionTabs::default();
    let mut projections = HashMap::new();
    let mut a = document("a", Some("api"), true);
    let b = document("b", Some("api"), true);
    for doc in [&a, &b] {
        tabs.open(
            SessionTabDocument::from_document(doc),
            &mut projections,
            None,
            false,
            false,
        );
    }
    a.draft.note = "Unsaved".into();
    projections.insert(a.id.clone(), SessionTabDocument::from_document(&a));
    let targets = tabs.close_targets(&SessionCloseScope::All, None);
    let mut state = (tabs, projections);
    let fetched = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
    let blocked = policy::close(
        &mut state,
        close_entries(&[a.clone(), b.clone()], &fetched),
        &targets,
        false,
    );
    assert!(
        fetched.borrow().is_empty(),
        "blocked close must not fetch saved drafts"
    );
    assert_eq!(blocked.blocked, ["a"]);
    assert!(blocked.removed.is_empty());
    assert_eq!(blocked.snapshot.session_ids.len(), 2);
    let closed = policy::close(&mut state, close_entries(&[a, b], &fetched), &targets, true);
    assert_eq!(
        *fetched.borrow(),
        ["a"],
        "clean documents must not fetch saved drafts"
    );
    assert!(closed.blocked.is_empty());
    assert_eq!(closed.removed.len(), 2);
    assert!(closed.snapshot.session_ids.is_empty());
    assert_eq!(
        closed
            .removed
            .iter()
            .find(|d| d.id == "a")
            .unwrap()
            .saved
            .as_ref()
            .unwrap()
            .note,
        ""
    );
}
#[test]
fn saved_authentication_migrates_only_unchanged_live_authentication() {
    use crate::session::workspace::documents::saved_authentication;
    let original = DomainAuthentication::Bearer {
        token: DomainValueSource::Secret {
            secret: "legacy.token".into(),
        },
    };
    let saved = DomainAuthentication::Bearer {
        token: DomainValueSource::Secret {
            secret: "wirebolt.auth.owned.token".into(),
        },
    };
    assert_eq!(
        saved_authentication(original.clone(), &original, &saved),
        saved
    );
    let current = DomainAuthentication::Basic {
        username: DomainValueSource::literal("changed"),
        password: DomainValueSource::literal(""),
    };
    assert_eq!(
        saved_authentication(current.clone(), &original, &saved),
        current
    );
}
#[tokio::test]
async fn live_save_all_reads_later_drafts_after_acknowledging_earlier_saves() {
    struct Live {
        values: Mutex<Vec<SessionDocument>>,
        results: Mutex<Vec<WorkspaceSave>>,
    }
    #[async_trait]
    impl crate::session::workspace::documents::WorkspaceDocuments for Live {
        async fn read(&self, id: String) -> Option<SessionDocument> {
            lock(&self.values).iter().find(|d| d.id == id).cloned()
        }
        async fn saved(&self, result: WorkspaceSave) {
            if result.document_id == "a" {
                lock(&self.values)[1].draft.note = "Latest B".into();
            }
            lock(&self.results).push(result);
        }
    }
    let h = Harness::new(nested());
    let live = Live {
        values: Mutex::new(vec![
            document("a", Some("api"), false),
            document("b", Some("api"), false),
        ]),
        results: Mutex::default(),
    };
    assert!(
        h.ops
            .save_all_live(
                vec!["a".into(), "b".into()],
                &h.store,
                &live,
                &h.secrets,
                &Secrets
            )
            .await
    );
    assert_eq!(lock(&live.results)[1].draft.note, "Latest B");
}

#[test]
fn close_scopes_fetch_only_dirty_removed_payloads_at_every_tab_count() {
    for count in [2, 20, 100] {
        for scope in [
            SessionCloseScope::One { id: "0".into() },
            SessionCloseScope::Others { id: "0".into() },
            SessionCloseScope::All,
        ] {
            for dirty in [false, true] {
                let mut tabs = SessionTabs::default();
                let mut projections = HashMap::new();
                let mut documents = vec![];
                for i in 0..count {
                    let mut doc = document(&i.to_string(), Some("api"), true);
                    if dirty && i == count - 1 {
                        doc.draft.note = "Edited".into();
                    }
                    let projection = SessionTabDocument::from_document(&doc);
                    tabs.open(projection, &mut projections, None, false, i == count - 1);
                    documents.push(doc);
                }
                let expected = match scope {
                    SessionCloseScope::One { .. } => 1,
                    SessionCloseScope::Others { .. } => count - 1,
                    _ => count,
                };
                let fetched = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
                let targets = tabs.close_targets(&scope, None);
                let result = policy::close(
                    &mut (tabs, projections),
                    close_entries(&documents, &fetched),
                    &targets,
                    true,
                );
                assert_eq!(result.removed.len(), expected);
                assert_eq!(result.snapshot.session_ids.len(), count - expected);
                let expected_fetch =
                    usize::from(dirty && !matches!(scope, SessionCloseScope::One { .. }));
                assert_eq!(fetched.borrow().len(), expected_fetch);
                assert_eq!(
                    result.removed.iter().filter(|d| d.saved.is_some()).count(),
                    expected_fetch
                );
                if expected_fetch > 0 {
                    assert_eq!(*fetched.borrow(), [(count - 1).to_string()]);
                }
                if count == expected {
                    assert!(result.snapshot.groups[0].preview_tab_id.is_none());
                }
            }
        }
    }
}

struct CloseProbe {
    document: SessionDocument,
    fetched: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}
impl policy::CloseDocument for CloseProbe {
    fn is_dirty(&self) -> bool {
        self.document.dirty()
    }
    fn saved_draft(&self) -> Option<DomainRequestDraft> {
        self.fetched.borrow_mut().push(self.document.id.clone());
        self.document.saved_draft.clone()
    }
}
fn close_entries(
    documents: &[SessionDocument],
    fetched: &std::rc::Rc<std::cell::RefCell<Vec<String>>>,
) -> Vec<(SessionTabDocument, CloseProbe)> {
    documents
        .iter()
        .map(|doc| {
            (
                SessionTabDocument::from_document(doc),
                CloseProbe {
                    document: doc.clone(),
                    fetched: fetched.clone(),
                },
            )
        })
        .collect()
}

#[test]
fn rust_dirty_policy_overrides_a_clean_native_projection() {
    let mut document = document("unicode", None, true);
    document.draft.note = "e\u{301}".into();
    document.saved_draft = Some(document.draft.clone());
    let projection = SessionTabDocument::from_document(&document);
    let mut state = (SessionTabs::default(), HashMap::new());
    state
        .0
        .open(projection.clone(), &mut state.1, None, false, false);
    document.draft.note = "é".into();
    let targets = state.0.close_targets(&SessionCloseScope::All, None);
    let result = policy::close(&mut state, vec![(projection, document)], &targets, false);
    assert_eq!(result.blocked, ["unicode"]);
    assert!(result.removed.is_empty());
}
