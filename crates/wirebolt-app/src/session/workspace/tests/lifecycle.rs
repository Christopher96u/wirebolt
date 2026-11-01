use super::*;
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
        h.ops.presentation().operation_failure.as_deref(),
        Some("workspace")
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
fn layout() -> AppSessionLayout {
    AppSessionLayout {
        groups: vec![
            AppSessionLayoutGroup {
                tabs: ["a", "b", "c"]
                    .map(|id| SessionLayoutTab {
                        collection_id: "api".into(),
                        request_id: id.into(),
                    })
                    .to_vec(),
                selected_index: Some(2),
                preview_index: Some(1),
            },
            AppSessionLayoutGroup {
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
        crate::session::tabs::decode_layout(&crate::session::tabs::encode_layout(&source)).unwrap();
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
    let blocked = policy::close(
        &mut tabs,
        vec![a.clone(), b.clone()],
        &SessionCloseScope::All,
        None,
        false,
    );
    assert_eq!(blocked.blocked, ["a"]);
    assert!(blocked.removed.is_empty());
    assert_eq!(blocked.snapshot.session_ids.len(), 2);
    let closed = policy::close(&mut tabs, vec![a, b], &SessionCloseScope::All, None, true);
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
