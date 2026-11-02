use super::{persistence_fixtures::*, *};

#[tokio::test]
async fn save_and_delete_serialize_without_recreating_deleted_request() {
    let (_root, store) = persistence();
    let original = imported_pair(&store);
    let source = original.collections[0].requests[0].clone();
    let session = Arc::new(WorkspaceSession::default());
    session.replace(original);
    let ops = Arc::new(WorkspaceOperations::new(session.clone()));
    let gate = Arc::new(SaveGate {
        store: store.clone(),
        ready: tokio::sync::Notify::default(),
        release: tokio::sync::Notify::default(),
        pause: std::sync::atomic::AtomicBool::new(true),
    });
    let mut draft = source.request.clone();
    draft.note = "Edited".into();
    let document = SessionDocument::new(
        "tab".into(),
        SessionKind::Http,
        Some(source.collection_id.clone()),
        None,
        draft,
        Some(source.request.clone()),
        "",
    );
    let save_ops = ops.clone();
    let save_gate = gate.clone();
    let saving = tokio::spawn(async move {
        save_ops
            .save(
                document,
                None,
                save_gate.as_ref(),
                &SessionSecrets::default(),
                &Secrets,
            )
            .await
    });
    gate.ready.notified().await;
    let secrets = SessionSecrets::default();
    let deleting = ops.mutate(
        Action::DeleteRequest {
            collection_id: source.collection_id.clone(),
            id: source.request.id.clone(),
        },
        None,
        gate.as_ref(),
        &secrets,
        &Secrets,
    );
    tokio::pin!(deleting);
    let early = tokio::select! {biased; result = &mut deleting => Some(result), () = std::future::ready(()) => None};
    let completed_early = early.is_some();
    gate.release.notify_one();
    assert!(saving.await.unwrap().succeeded);
    let deleted = match early {
        Some(result) => result,
        None => deleting.await,
    };
    assert!(deleted.succeeded);
    let disk = store.domain_snapshot().unwrap();
    assert!(
        !disk
            .collections
            .iter()
            .flat_map(|c| &c.requests)
            .any(|r| r.collection_id == source.collection_id && r.request.id == source.request.id),
        "Save recreated a request after Delete completed"
    );
    assert_eq!(session.snapshot(), disk);
    assert!(!completed_early, "Delete passed a paused Save");
}

#[tokio::test]
async fn stale_save_after_move_or_delete_cannot_recreate_the_source() {
    for move_request in [false, true] {
        let (_root, store) = persistence();
        let original = imported_pair(&store);
        let source = original.collections[0].requests[0].clone();
        let destination = original.collections[1].id.clone();
        let session = Arc::new(WorkspaceSession::default());
        session.replace(original);
        let ops = WorkspaceOperations::new(session.clone());
        let platform = SaveGate {
            store: store.clone(),
            ready: tokio::sync::Notify::default(),
            release: tokio::sync::Notify::default(),
            pause: std::sync::atomic::AtomicBool::new(false),
        };
        let action = if move_request {
            Action::MoveRequest {
                from_collection_id: source.collection_id.clone(),
                id: source.request.id.clone(),
                to_collection_id: destination,
                group_id: None,
                order: 1,
            }
        } else {
            Action::DeleteRequest {
                collection_id: source.collection_id.clone(),
                id: source.request.id.clone(),
            }
        };
        assert!(
            ops.mutate(
                action,
                None,
                &platform,
                &SessionSecrets::default(),
                &Secrets
            )
            .await
            .succeeded
        );
        let before = store.domain_snapshot().unwrap();
        let mut draft = source.request.clone();
        draft.note = "Stale edit".into();
        let document = SessionDocument::new(
            "tab".into(),
            SessionKind::Http,
            Some(source.collection_id),
            None,
            draft,
            Some(source.request),
            "",
        );
        assert!(
            !ops.save(
                document,
                None,
                &platform,
                &SessionSecrets::default(),
                &Secrets
            )
            .await
            .succeeded
        );
        assert_eq!(store.domain_snapshot().unwrap(), before);
        assert_eq!(sorted(session.snapshot()), sorted(before));
    }
}

#[tokio::test]
async fn oversized_save_reports_limit_and_keeps_previous_request() {
    let (_root, store) = persistence();
    let original = imported_pair(&store);
    let source = original.collections[0].requests[0].clone();
    let session = Arc::new(WorkspaceSession::default());
    session.replace(original.clone());
    let ops = WorkspaceOperations::new(session);
    let platform = SaveGate {
        store: store.clone(),
        ready: tokio::sync::Notify::default(),
        release: tokio::sync::Notify::default(),
        pause: std::sync::atomic::AtomicBool::new(false),
    };
    let mut draft = source.request.clone();
    draft.body = DomainBody::Raw {
        content_type: None,
        value: "x".repeat(16 * 1024 * 1024),
    };
    let document = SessionDocument::new(
        "tab".into(),
        SessionKind::Http,
        Some(source.collection_id),
        None,
        draft,
        Some(source.request),
        "",
    );
    assert!(
        !ops.save(
            document,
            None,
            &platform,
            &SessionSecrets::default(),
            &Secrets
        )
        .await
        .succeeded
    );
    assert!(
        ops.presentation()
            .operation_failure_reason
            .unwrap()
            .contains("16 MiB")
    );
    assert_eq!(store.domain_snapshot().unwrap(), original);
}

#[tokio::test]
async fn clean_unsaved_document_can_be_saved_to_its_first_collection() {
    let (_root, store) = persistence();
    let original = imported_pair(&store);
    let destination = original.collections[0].id.clone();
    let session = Arc::new(WorkspaceSession::default());
    session.replace(original);
    let ops = WorkspaceOperations::new(session);
    let platform = SaveGate {
        store: store.clone(),
        ready: tokio::sync::Notify::default(),
        release: tokio::sync::Notify::default(),
        pause: std::sync::atomic::AtomicBool::new(false),
    };
    // Native clean tabs retain a saved projection even before they have a location.
    let document = document("new", None, true);
    assert!(
        ops.save(
            document,
            Some(destination.clone()),
            &platform,
            &SessionSecrets::default(),
            &Secrets
        )
        .await
        .succeeded
    );
    assert!(
        store
            .domain_snapshot()
            .unwrap()
            .collections
            .iter()
            .find(|collection| collection.id == destination)
            .unwrap()
            .requests
            .iter()
            .any(|location| location.request.id == "new")
    );
}

#[tokio::test]
async fn internal_save_failure_codes_are_not_used_as_alert_text() {
    let workspace = Arc::new(WorkspaceSession::default());
    workspace.replace(DomainWorkspace {
        collections: vec![DomainCollection {
            id: "api".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let ops = WorkspaceOperations::new(workspace);
    let store = Store::default();
    *lock(&store.fail_at) = Some(0);
    let result = ops
        .save(
            document("draft", None, true),
            Some("api".into()),
            &store,
            &SessionSecrets::default(),
            &Secrets,
        )
        .await;
    assert!(!result.succeeded);
    assert_eq!(
        ops.presentation().operation_failure_reason.as_deref(),
        Some("The workspace change could not be saved. Check the workspace folder and try again.")
    );
}

#[tokio::test]
async fn external_request_deletion_can_be_saved_before_and_after_reload() {
    for reload in [false, true] {
        let (_root, store) = persistence();
        let original = imported_pair(&store);
        let mut source = original.collections[0].requests[0].clone();
        store
            .apply_session_command(SessionWorkspaceCommand::CreateGroup {
                collection_id: source.collection_id.clone(),
                group: DomainGroup {
                    id: "folder".into(),
                    name: "Folder".into(),
                    parent_id: None,
                    order: 0,
                },
            })
            .unwrap();
        source.group_id = Some("folder".into());
        source.order = 7;
        store
            .apply_session_command(SessionWorkspaceCommand::SaveRequest {
                location: source.clone(),
            })
            .unwrap();
        let session = Arc::new(WorkspaceSession::default());
        session.replace(store.domain_snapshot().unwrap());
        let ops = WorkspaceOperations::new(session.clone());
        let platform = SaveGate {
            store: store.clone(),
            ready: tokio::sync::Notify::default(),
            release: tokio::sync::Notify::default(),
            pause: std::sync::atomic::AtomicBool::new(false),
        };
        // Persistence here stands for a file removed by git, outside WorkspaceOperations.
        store
            .apply_session_command(SessionWorkspaceCommand::DeleteRequest {
                collection_id: source.collection_id.clone(),
                id: source.request.id.clone(),
            })
            .unwrap();
        if reload {
            ops.load(&platform, false).await.unwrap();
        }
        let mut draft = source.request.clone();
        draft.note = "Recover external deletion".into();
        let document = SessionDocument::new(
            "tab".into(),
            SessionKind::Http,
            Some(source.collection_id.clone()),
            None,
            draft.clone(),
            Some(source.request.clone()),
            "",
        );
        let result = ops
            .save(
                document,
                None,
                &platform,
                &SessionSecrets::default(),
                &Secrets,
            )
            .await;
        assert!(result.succeeded);
        let disk = store.domain_snapshot().unwrap();
        let saved =
            super::super::location(&disk, &source.collection_id, &source.request.id).unwrap();
        assert_eq!(saved.request, draft);
        assert_eq!(saved.order, source.order);
        assert_eq!(saved.group_id, source.group_id);
        assert_eq!(session.snapshot(), disk);
    }
}

#[test]
fn workspace_failure_messages_follow_typed_errors_and_never_expose_internal_reasons() {
    use crate::session::persistence::WorkspaceError;
    assert!(
        WorkspaceError::MissingDestination
            .user_message()
            .starts_with("The destination no longer exists.")
    );
    assert!(
        WorkspaceError::MissingRequest
            .user_message()
            .starts_with("The request no longer exists.")
    );
    assert!(
        WorkspaceError::RequestTooLarge
            .user_message()
            .contains("16 MiB")
    );
    assert_eq!(
        WorkspaceError::operation("Uppercase private detail").user_message(),
        WorkspaceError::operation("lowercase private detail").user_message()
    );
    assert!(
        !WorkspaceError::operation("Uppercase private detail")
            .user_message()
            .contains("private detail")
    );
}
