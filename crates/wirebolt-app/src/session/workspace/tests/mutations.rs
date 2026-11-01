use super::*;
use crate::lock;
#[tokio::test]
async fn failed_workspace_switch_leaves_previous_workspace_intact() {
    let h = Harness::new(nested());
    assert!(h.ops.begin_load(true));
    assert!(h.ops.load(&h.store, true).await.is_none());
    assert_eq!(h.workspace.snapshot(), nested());
    assert!(!h.ops.presentation().loaded);
    assert!(!h.ops.presentation().loading);
}
#[tokio::test]
async fn workspace_switch_resets_environment_choice_git_and_undo_registry() {
    let h = Harness::new(nested());
    let undo = h
        .apply(Action::DeleteRequest {
            collection_id: "api".into(),
            id: "root".into(),
        })
        .await
        .undo
        .unwrap();
    lock(&h.store.replies).push_back(WorkspaceReply::Loaded {
        workspace: DomainWorkspace {
            name: "Mutations".into(),
            ..Default::default()
        },
    });
    assert!(h.ops.begin_load(true));
    let result = h.ops.load(&h.store, true).await.unwrap();
    assert!(result.selected_environment_id.is_none());
    assert!(h.ops.reverse_token(undo.token).is_none());
    assert_eq!(h.workspace.snapshot().name, "Mutations");
    assert!(h.ops.presentation().loaded);
}
#[tokio::test]
async fn failed_hierarchy_mutations_keep_tree_and_emit_no_document_effects() {
    let h = Harness::new(nested());
    *lock(&h.store.fail_at) = Some(0);
    for action in [
        Action::DeleteGroup {
            collection_id: "api".into(),
            id: "parent".into(),
        },
        Action::MoveRequest {
            from_collection_id: "api".into(),
            id: "root".into(),
            to_collection_id: "missing".into(),
            group_id: None,
            order: 0,
        },
    ] {
        let result = h.apply(action).await;
        assert!(result.plan.is_none());
        assert_eq!(h.workspace.snapshot(), nested());
    }
}
#[tokio::test]
async fn moved_document_save_keeps_destination_order_and_unsaved_edits() {
    let mut w = nested();
    w.collections.push(DomainCollection {
        id: "target".into(),
        ..Default::default()
    });
    let h = Harness::new(w);
    let moved = h
        .apply(Action::MoveRequest {
            from_collection_id: "api".into(),
            id: "deep".into(),
            to_collection_id: "target".into(),
            group_id: None,
            order: 4,
        })
        .await;
    assert!(
        matches!(&moved.plan.unwrap().effects[0],SessionDocumentEffect::Relocate{destination,..} if destination=="target")
    );
    let mut document = document("deep", Some("target"), true);
    document.draft.note = "Unsaved split".into();
    let result = h
        .ops
        .save(document, Some("api".into()), &h.store, &h.secrets, &Secrets)
        .await;
    let saved = result.location.unwrap();
    assert_eq!(saved.collection_id, "target");
    assert_eq!(saved.order, 4);
    assert_eq!(saved.request.note, "Unsaved split");
}
#[tokio::test]
async fn deleting_folder_emits_close_effects_for_each_descendant_request() {
    let h = Harness::new(nested());
    let result = h
        .apply(Action::DeleteGroup {
            collection_id: "api".into(),
            id: "parent".into(),
        })
        .await;
    let plan = result.plan.unwrap();
    assert_eq!(plan.effects.len(), 2);
    assert!(plan.effects.contains(&SessionDocumentEffect::Close {
        collection_id: "api".into(),
        request_id: Some("deep".into())
    }));
    assert_eq!(h.workspace.snapshot().collections[0].requests.len(), 1);
}
#[tokio::test]
async fn mixed_reorder_preserves_nested_order_and_saved_request_position() {
    let mut w = nested();
    w.collections[0].requests = vec![
        location("a", None, 0),
        location("b", None, 9),
        location("nested", Some("parent"), 42),
    ];
    let h = Harness::new(w);
    let result = h
        .apply(Action::Reorder {
            identifier: "request|api|b".into(),
            target: "group:parent".into(),
            after: false,
            collection_id: "api".into(),
            parent_id: None,
        })
        .await;
    assert!(result.succeeded);
    let w = h.workspace.snapshot();
    assert_eq!(
        crate::domain::sidebar::ordered_children(&w.collections[0], None),
        ["request:b", "group:parent", "request:a"]
    );
    assert_eq!(
        w.collections[0]
            .requests
            .iter()
            .find(|r| r.request.id == "nested")
            .unwrap()
            .order,
        42
    );
    let mut doc = document("b", Some("api"), true);
    doc.draft.note = "Unsaved edit".into();
    let saved = h
        .ops
        .save(doc, None, &h.store, &h.secrets, &Secrets)
        .await
        .location
        .unwrap();
    assert_eq!(saved.order, 0);
    assert_eq!(saved.request.note, "Unsaved edit");
}
#[tokio::test]
async fn invalid_self_cross_folder_and_adjacent_reorders_are_noops() {
    let mut w = nested();
    w.collections[0].requests = vec![
        location("a", Some("folder"), 0),
        location("b", Some("folder"), 1),
        location("outside", None, 0),
    ];
    let h = Harness::new(w);
    for identifier in [
        "invalid",
        "request|other|a",
        "request|api|outside",
        "request|api|b",
        "group|api|a",
        "request|api|a",
    ] {
        let r = h
            .apply(Action::Reorder {
                identifier: identifier.into(),
                target: "request:b".into(),
                after: false,
                collection_id: "api".into(),
                parent_id: Some("folder".into()),
            })
            .await;
        assert!(!r.succeeded);
    }
    assert!(lock(&h.store.commands).is_empty());
    let result = h
        .apply(Action::Reorder {
            identifier: "request|api|a".into(),
            target: "request:b".into(),
            after: true,
            collection_id: "api".into(),
            parent_id: Some("folder".into()),
        })
        .await;
    assert!(result.succeeded);
    assert_eq!(lock(&h.store.commands).len(), 1);
}
#[tokio::test]
async fn failed_reorder_leaves_displayed_order_unchanged() {
    let h = Harness::new(nested());
    *lock(&h.store.fail_at) = Some(0);
    let result = h
        .apply(Action::MoveBy {
            identifier: "request|api|root".into(),
            delta: -1,
        })
        .await;
    assert!(!result.succeeded);
    assert_eq!(h.workspace.snapshot(), nested());
}
#[tokio::test]
async fn root_creation_and_contextual_websocket_request_preserve_location_and_order() {
    let h = Harness::new(nested());
    let root = h
        .apply(Action::CreateRequest {
            web_socket: false,
            collection_id: None,
            group_id: None,
        })
        .await;
    assert!(root.succeeded);
    assert_eq!(root.root.unwrap().id, "workspace-root");
    let socket = h
        .apply(Action::CreateRequest {
            web_socket: true,
            collection_id: Some("api".into()),
            group_id: Some("parent".into()),
        })
        .await;
    let location = socket.plan.unwrap().collections[0]
        .requests
        .last()
        .unwrap()
        .clone();
    assert_eq!(location.group_id.as_deref(), Some("parent"));
    assert!(location.request.web_socket);
    assert!(location.request.url.is_empty());
    assert_eq!(lock(&h.store.commands).len(), 3);
}
#[tokio::test]
async fn save_uses_document_collection_before_fallback_and_retains_group() {
    let h = Harness::new(nested());
    let result = h
        .ops
        .save(
            document("deep", Some("api"), true),
            Some("first".into()),
            &h.store,
            &h.secrets,
            &Secrets,
        )
        .await;
    let r = result.location.unwrap();
    assert_eq!(r.collection_id, "api");
    assert_eq!(r.group_id.as_deref(), Some("grandchild"));
    assert_eq!(r.order, 4);
}
#[tokio::test]
async fn renamed_saved_definition_has_only_saved_fields_and_document_effect() {
    let h = Harness::new(nested());
    let result = h
        .apply(Action::RenameRequest {
            collection_id: "api".into(),
            id: "root".into(),
            name: "Renamed".into(),
        })
        .await;
    let plan = result.plan.unwrap();
    assert_eq!(
        plan.effects,
        [SessionDocumentEffect::Rename {
            collection_id: "api".into(),
            request_id: "root".into(),
            name: "Renamed".into()
        }]
    );
    assert!(
        plan.collections[0]
            .requests
            .iter()
            .all(|r| r.request.note.is_empty())
    );
}
#[tokio::test]
async fn workspace_mutations_apply_deltas_without_loading_entire_workspace() {
    let h = Harness::new(nested());
    h.apply(Action::CreateCollection {
        name: "Other".into(),
    })
    .await;
    h.apply(Action::CreateGroup {
        collection_id: "api".into(),
        parent_id: None,
        name: "Folder".into(),
    })
    .await;
    assert_eq!(h.workspace.snapshot().collections.len(), 2);
    assert_eq!(h.workspace.snapshot().collections[0].groups.len(), 4);
    assert!(lock(&h.store.replies).is_empty());
}
#[tokio::test]
async fn export_reports_failure_kind_and_preserves_saved_only_contract() {
    let h = Harness::new(nested());
    lock(&h.store.replies).push_back(WorkspaceReply::Exported {
        content: "saved definition".into(),
    });
    assert_eq!(
        h.ops
            .export(
                WorkspaceExport::Request {
                    collection_id: "api".into(),
                    id: "root".into()
                },
                &h.store
            )
            .await
            .as_deref(),
        Some("saved definition")
    );
    assert!(
        h.ops
            .export(WorkspaceExport::Workspace, &h.store)
            .await
            .is_none()
    );
    assert_eq!(
        h.ops.presentation().operation_failure,
        Some(crate::response::ResponseFailureKind::Export)
    );
}
#[test]
fn new_environment_and_temporary_request_have_unique_ids_and_expected_defaults() {
    let a = actions::new_environment();
    let b = actions::new_environment();
    assert_ne!(a.id, b.id);
    assert_eq!(a.name, "New Environment");
    assert!(a.variables.is_empty());
    let r = actions::new_request(&nested(), false, "api".into(), Some("parent".into()));
    assert_eq!(r.order, -1);
    assert_eq!(r.request.name, "Untitled Request");
    assert_eq!(r.group_id.as_deref(), Some("parent"));
}
#[test]
fn next_child_order_includes_mixed_siblings_and_handles_empty_containers() {
    assert_eq!(actions::next_order(&nested(), "api", Some("parent")), 8);
    assert_eq!(actions::next_order(&nested(), "missing", None), 0);
}
#[test]
fn request_search_caches_normalization_and_matches_case_diacritics_and_url() {
    let h = Harness::new(nested());
    let request = crate::domain::sidebar::DomainSidebarRequest {
        id: "status".into(),
        collection_id: "api".into(),
        group_id: None,
        order: 0,
        name: "État Service".into(),
        method: "GET".into(),
        url: "https://example.com/health".into(),
    };
    assert!(
        h.ops
            .request_matches(request.clone(), &crate::domain::sidebar::normalize("ETAT"))
    );
    assert!(h.ops.request_matches(request.clone(), "example.com/health"));
    let mut renamed = request;
    renamed.name = "Renamed".into();
    assert!(!h.ops.request_matches(renamed, "etat"));
}
