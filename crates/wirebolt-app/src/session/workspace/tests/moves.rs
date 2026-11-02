use super::{persistence_fixtures::*, *};

#[test]
fn legacy_move_collision_is_rejected_by_policy_plan_and_persistence() {
    let (_root, store) = persistence();
    let mut original = imported_pair(&store);
    let source = original.collections[0].requests[0].clone();
    let target = &mut original.collections[1];
    let mut duplicate = target.requests[0].clone();
    store
        .apply_session_command(SessionWorkspaceCommand::DeleteRequest {
            collection_id: target.id.clone(),
            id: duplicate.request.id.clone(),
        })
        .unwrap();
    duplicate.request.id.clone_from(&source.request.id);
    store
        .apply_session_command(SessionWorkspaceCommand::SaveRequest {
            location: duplicate.clone(),
        })
        .unwrap();
    target.requests = vec![duplicate];
    let original = store.domain_snapshot().unwrap();
    let destination = &original.collections[1].id;
    let session = WorkspaceSession::default();
    session.replace(original.clone());
    let identifier = format!("request|{}|{}", source.collection_id, source.request.id);
    assert!(!actions::move_eligibility(&original, &identifier, destination, None).is_allowed());
    assert!(
        session
            .plan(
                None,
                SessionWorkspaceEdit::MoveRequest {
                    from_collection_id: source.collection_id.clone(),
                    id: source.request.id.clone(),
                    to_collection_id: destination.clone(),
                    group_id: None,
                    order: 1
                }
            )
            .is_none()
    );
    let result = store.apply_session_command(SessionWorkspaceCommand::MoveRequest {
        from_collection_id: source.collection_id,
        request_id: source.request.id,
        to_collection_id: destination.clone(),
        group_id: None,
        order: 1,
    });
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("already contains a request with this ID")
    );
    assert_eq!(store.domain_snapshot().unwrap(), original);
}

#[tokio::test]
async fn move_refusals_use_one_rust_message_for_menu_drop_and_persistence() {
    let (_root, store) = persistence();
    let mut workspace = imported_pair(&store);
    let source = workspace.collections[0].requests[0].clone();
    let destination = workspace.collections[1].id.clone();
    workspace.collections[1].requests[0]
        .request
        .id
        .clone_from(&source.request.id);
    let session = Arc::new(WorkspaceSession::default());
    session.replace(workspace);
    let ops = WorkspaceOperations::new(session);
    let identifier = format!("request|{}|{}", source.collection_id, source.request.id);
    let eligibility = ops.move_eligibility(identifier.clone(), destination.clone(), None);
    assert!(
        matches!(eligibility, actions::MoveEligibility::Refused {reason, ..} if reason == actions::MOVE_COLLISION_REASON)
    );
    let platform = Store::default();
    for command in [
        Action::MoveTo {
            identifier,
            collection_id: destination.clone(),
            parent_id: None,
        },
        Action::MoveRequest {
            from_collection_id: source.collection_id,
            id: source.request.id,
            to_collection_id: destination,
            group_id: None,
            order: 0,
        },
    ] {
        assert!(
            !ops.mutate(
                command,
                None,
                &platform,
                &SessionSecrets::default(),
                &Secrets
            )
            .await
            .succeeded
        );
        assert_eq!(
            ops.presentation().operation_failure_reason.as_deref(),
            Some(actions::MOVE_COLLISION_REASON)
        );
    }
}

#[test]
fn move_destination_labels_follow_collection_folder_and_top_level_vocabulary() {
    let workspace = DomainWorkspace {
        collections: vec![
            DomainCollection {
                id: "source".into(),
                requests: vec![DomainLocation {
                    collection_id: "source".into(),
                    request: DomainRequestDraft {
                        id: "request".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            },
            DomainCollection {
                id: actions::ROOT_COLLECTION.into(),
                ..Default::default()
            },
            DomainCollection {
                id: "target".into(),
                name: "API".into(),
                groups: vec![DomainGroup {
                    id: "folder".into(),
                    name: "Endpoints".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    for (collection, parent, expected) in [
        (actions::ROOT_COLLECTION, None, "Top Level"),
        ("target", None, "API"),
        ("target", Some("folder"), "Endpoints"),
    ] {
        assert_eq!(
            actions::move_eligibility(&workspace, "request|source|request", collection, parent),
            actions::MoveEligibility::Allowed {
                label: expected.into()
            }
        );
    }
}
