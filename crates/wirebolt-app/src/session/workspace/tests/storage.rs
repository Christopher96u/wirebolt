use super::*;
use crate::session::persistence::SessionPersistence;

fn persistence() -> (tempfile::TempDir, Arc<SessionPersistence>) {
    let root = tempfile::tempdir().unwrap();
    let store = SessionPersistence::open_or_create(
        root.path().to_string_lossy().into_owned(),
        "Storage".into(),
        Arc::new(super::files::NoSecrets),
    )
    .unwrap();
    (root, store)
}

fn imported_pair(store: &SessionPersistence) -> DomainWorkspace {
    for url in ["http://localhost/one", "http://localhost/two"] {
        store
            .commit_import(wirebolt_core::ImportFormat::Curl, &format!("curl {url}"))
            .unwrap();
    }
    store.domain_snapshot().unwrap()
}

fn apply_plan(
    store: &SessionPersistence,
    session: &WorkspaceSession,
    edit: SessionWorkspaceEdit,
) -> SessionWorkspacePlan {
    let plan = session.plan(None, edit).unwrap();
    store.apply_session_commands(plan.commands.clone()).unwrap();
    session.commit(plan.clone());
    plan
}

#[test]
fn imported_request_move_and_undo_preserve_both_requests() {
    let (_root, store) = persistence();
    let original = imported_pair(&store);
    let source = &original.collections[0];
    let target = &original.collections[1];
    assert_ne!(source.requests[0].request.id, target.requests[0].request.id);
    let session = WorkspaceSession::default();
    session.replace(original.clone());
    let plan = apply_plan(
        &store,
        &session,
        SessionWorkspaceEdit::MoveRequest {
            from_collection_id: source.id.clone(),
            id: source.requests[0].request.id.clone(),
            to_collection_id: target.id.clone(),
            group_id: None,
            order: 1,
        },
    );
    assert_eq!(
        store.domain_snapshot().unwrap().collections[1]
            .requests
            .len(),
        2
    );
    apply_plan(&store, &session, plan.inverse);
    assert_eq!(store.domain_snapshot().unwrap(), original);
}

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
    assert!(!actions::can_move_to(
        &original,
        &identifier,
        destination,
        None
    ));
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

#[test]
fn collection_delete_and_undo_preserve_attachments_and_requests() {
    let (root, store) = persistence();
    let original = imported_pair(&store);
    let collection = &original.collections[0];
    let attachment = root
        .path()
        .join("collections")
        .join(&collection.id)
        .join("attachment.bin");
    std::fs::write(&attachment, b"attachment").unwrap();
    let session = WorkspaceSession::default();
    session.replace(original.clone());
    let plan = apply_plan(
        &store,
        &session,
        SessionWorkspaceEdit::RemoveCollection {
            id: collection.id.clone(),
        },
    );
    assert_eq!(std::fs::read(&attachment).unwrap(), b"attachment");
    apply_plan(&store, &session, plan.inverse);
    assert_eq!(store.domain_snapshot().unwrap(), original);
    assert_eq!(std::fs::read(attachment).unwrap(), b"attachment");
}
