use super::*;
use crate::lock;
#[tokio::test]
async fn environment_undo_and_redo_preserve_position_and_secret_rows() {
    let original = DomainEnvironment {
        id: "local".into(),
        name: "Local".into(),
        variables: vec![DomainVariable {
            id: "token".into(),
            key: "token".into(),
            value: DomainValueSource::Secret {
                secret: "env.local.token".into(),
            },
            ..Default::default()
        }],
    };
    let other = DomainEnvironment {
        id: "production".into(),
        name: "Production".into(),
        ..Default::default()
    };
    let h = Harness::new(DomainWorkspace {
        environments: vec![original.clone(), other.clone()],
        ..Default::default()
    });
    let mut changed = original.clone();
    changed.name = "Development".into();
    changed.variables.push(DomainVariable {
        id: "host".into(),
        key: "host".into(),
        value: DomainValueSource::literal("localhost"),
        ..Default::default()
    });
    let result = h
        .apply(Action::SaveEnvironment {
            environment: changed.clone(),
        })
        .await;
    assert_eq!(
        h.workspace.snapshot().environments,
        [changed.clone(), other.clone()]
    );
    let (_, redo) = h.undo(result.undo.unwrap()).await;
    assert_eq!(
        h.workspace.snapshot().environments,
        [original, other.clone()]
    );
    h.undo(redo).await;
    assert_eq!(
        h.workspace.snapshot().environments,
        [changed.clone(), other.clone()]
    );
    let result = h
        .apply(Action::DeleteEnvironment { id: "local".into() })
        .await;
    h.undo(result.undo.unwrap()).await;
    assert_eq!(h.workspace.snapshot().environments, [changed, other]);
}
#[tokio::test]
async fn folder_deletion_undo_redo_preserves_subtree_and_action_name() {
    let h = Harness::new(nested());
    let result = h
        .apply(Action::DeleteGroup {
            collection_id: "api".into(),
            id: "parent".into(),
        })
        .await;
    assert!(h.workspace.snapshot().collections[0].groups.is_empty());
    assert_eq!(result.undo.as_ref().unwrap().name, "Delete “Parent”");
    let (_, redo) = h.undo(result.undo.unwrap()).await;
    assert_eq!(sorted(h.workspace.snapshot()), sorted(nested()));
    let commands = lock(&h.store.commands).clone();
    let created: Vec<_> = commands
        .iter()
        .filter_map(|c| match c {
            SessionWorkspaceCommand::CreateGroup { group, .. } => Some(group.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(created, ["parent", "child", "grandchild"]);
    assert_eq!(
        commands
            .iter()
            .filter(|c| matches!(c, SessionWorkspaceCommand::SaveRequest { .. }))
            .count(),
        2
    );
    drop(commands);
    let (_, undo) = h.undo(redo).await;
    assert!(h.workspace.snapshot().collections[0].groups.is_empty());
    h.undo(undo).await;
    assert_eq!(sorted(h.workspace.snapshot()), sorted(nested()));
}
#[tokio::test]
async fn collection_deletion_restores_hierarchy_with_empty_collection_first() {
    let h = Harness::new(nested());
    let result = h.apply(Action::DeleteCollection { id: "api".into() }).await;
    assert!(h.workspace.snapshot().collections.is_empty());
    h.undo(result.undo.unwrap()).await;
    assert_eq!(sorted(h.workspace.snapshot()), sorted(nested()));
    assert!(
        matches!(&lock(&h.store.commands)[1], SessionWorkspaceCommand::CreateCollection { collection } if collection.groups.is_empty() && collection.requests.is_empty() && collection.order == 3)
    );
}
#[tokio::test]
async fn request_deletion_restores_saved_definition_and_redo_removes_it() {
    let h = Harness::new(nested());
    let result = h
        .apply(Action::DeleteRequest {
            collection_id: "api".into(),
            id: "deep".into(),
        })
        .await;
    assert_eq!(h.workspace.snapshot().collections[0].requests.len(), 2);
    let (_, redo) = h.undo(result.undo.unwrap()).await;
    assert!(
        h.workspace.snapshot().collections[0]
            .requests
            .contains(&nested().collections[0].requests[0])
    );
    h.undo(redo).await;
    assert_eq!(h.workspace.snapshot().collections[0].requests.len(), 2);
}
#[tokio::test]
async fn renames_restore_names_and_emit_document_rename_effect() {
    let h = Harness::new(nested());
    let mut undo = vec![];
    for action in [
        Action::RenameRequest {
            collection_id: "api".into(),
            id: "root".into(),
            name: "Renamed".into(),
        },
        Action::RenameGroup {
            collection_id: "api".into(),
            id: "child".into(),
            name: "Renamed Folder".into(),
        },
        Action::RenameCollection {
            id: "api".into(),
            name: "Renamed API".into(),
        },
    ] {
        let result = h.apply(action).await;
        assert_eq!(result.undo.as_ref().unwrap().name, "Rename");
        undo.push(result.undo.unwrap());
    }
    for token in undo.into_iter().rev() {
        h.undo(token).await;
    }
    assert_eq!(sorted(h.workspace.snapshot()), sorted(nested()));
}
#[tokio::test]
async fn creating_requests_folders_collections_and_duplicates_is_undoable() {
    let h = Harness::new(nested());
    for command in [
        Action::CreateRequest {
            web_socket: false,
            collection_id: Some("api".into()),
            group_id: Some("parent".into()),
        },
        Action::CreateGroup {
            collection_id: "api".into(),
            parent_id: Some("child".into()),
            name: "Folder".into(),
        },
        Action::CreateCollection {
            name: "Payments".into(),
        },
        Action::DuplicateRequest {
            collection_id: "api".into(),
            id: "root".into(),
        },
    ] {
        let result = h.apply(command).await;
        assert!(result.succeeded);
        h.undo(result.undo.unwrap()).await;
        assert_eq!(sorted(h.workspace.snapshot()), sorted(nested()));
    }
}
#[tokio::test]
async fn moves_restore_collection_parent_and_order() {
    let mut w = nested();
    w.collections.push(DomainCollection {
        id: "other".into(),
        ..Default::default()
    });
    let h = Harness::new(w.clone());
    let request = h
        .apply(Action::MoveTo {
            identifier: "request|api|deep".into(),
            collection_id: "other".into(),
            parent_id: None,
        })
        .await;
    assert_eq!(request.undo.as_ref().unwrap().name, "Move “Deep”");
    let group = h
        .apply(Action::MoveTo {
            identifier: "group|api|child".into(),
            collection_id: "api".into(),
            parent_id: None,
        })
        .await;
    h.undo(group.undo.unwrap()).await;
    h.undo(request.undo.unwrap()).await;
    assert_eq!(sorted(h.workspace.snapshot()), sorted(w));
}
#[tokio::test]
async fn move_up_down_persists_exact_sibling_order_and_is_reversible() {
    let h = Harness::new(nested());
    assert!(actions::neighbor(&h.workspace.snapshot(), "group|api|parent", -1).is_none());
    let result = h
        .apply(Action::MoveBy {
            identifier: "group|api|parent".into(),
            delta: 1,
        })
        .await;
    assert_eq!(
        crate::domain::sidebar::ordered_children(&h.workspace.snapshot().collections[0], None),
        ["request:root", "group:parent"]
    );
    assert!(actions::neighbor(&h.workspace.snapshot(), "group|api|parent", 1).is_none());
    let (_, redo) = h.undo(result.undo.unwrap()).await;
    assert_eq!(
        crate::domain::sidebar::ordered_children(&h.workspace.snapshot().collections[0], None),
        ["group:parent", "request:root"]
    );
    h.undo(redo).await;
    assert_eq!(
        crate::domain::sidebar::ordered_children(&h.workspace.snapshot().collections[0], None),
        ["request:root", "group:parent"]
    );
}
#[test]
fn move_to_rejects_cycles_other_collections_current_parent_and_missing_parent() {
    let mut w = nested();
    w.collections.push(DomainCollection {
        id: "other".into(),
        ..Default::default()
    });
    for (id, c, p) in [
        ("group|api|parent", "api", Some("grandchild")),
        ("group|api|parent", "other", None),
        ("group|api|parent", "api", None),
        ("request|api|root", "api", None),
        ("request|api|root", "api", Some("missing")),
    ] {
        assert!(!actions::can_move_to(&w, id, c, p));
    }
    assert!(actions::can_move_to(
        &w,
        "group|api|grandchild",
        "api",
        Some("parent")
    ));
    assert!(actions::can_move_to(&w, "request|api|root", "other", None));
}
#[tokio::test]
async fn failed_undo_reports_error_and_invalidates_every_token() {
    let h = Harness::new(nested());
    let result = h
        .apply(Action::DeleteRequest {
            collection_id: "api".into(),
            id: "root".into(),
        })
        .await;
    *lock(&h.store.fail_at) = Some(1);
    let (_, reverse) = h.undo(result.undo.unwrap()).await;
    assert_eq!(
        h.ops.presentation().operation_failure,
        Some(crate::response::ResponseFailureKind::Workspace)
    );
    assert!(h.ops.reverse_token(reverse.token).is_none());
    assert_eq!(h.workspace.snapshot().collections[0].requests.len(), 2);
}
#[tokio::test]
async fn failed_mutation_has_no_undo_and_keeps_loaded_tree() {
    let h = Harness::new(nested());
    *lock(&h.store.fail_at) = Some(0);
    let result = h
        .apply(Action::DeleteGroup {
            collection_id: "api".into(),
            id: "parent".into(),
        })
        .await;
    assert!(!result.succeeded && result.undo.is_none());
    assert_eq!(h.workspace.snapshot(), nested());
}
#[tokio::test]
async fn a_new_successful_edit_discards_redo_payloads_but_failed_edits_keep_them() {
    let h = Harness::new(nested());
    let edit = h
        .apply(Action::RenameCollection {
            id: "api".into(),
            name: "Renamed".into(),
        })
        .await;
    let (_, redo) = h.undo(edit.undo.unwrap()).await;
    *lock(&h.store.fail_at) = Some(lock(&h.store.commands).len());
    let failed = h
        .apply(Action::RenameCollection {
            id: "api".into(),
            name: "Failed".into(),
        })
        .await;
    assert!(!failed.succeeded);
    assert!(h.ops.reverse_token(redo.token).is_some());
    *lock(&h.store.fail_at) = None;
    assert!(
        h.apply(Action::RenameCollection {
            id: "api".into(),
            name: "New branch".into()
        })
        .await
        .succeeded
    );
    assert!(h.ops.reverse_token(redo.token).is_none());
}
