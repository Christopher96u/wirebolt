use super::{persistence_fixtures::*, *};

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
