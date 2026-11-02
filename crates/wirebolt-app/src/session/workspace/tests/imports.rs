use super::{persistence_fixtures::*, *};

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
