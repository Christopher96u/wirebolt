use super::{persistence_fixtures::*, *};

#[tokio::test]
async fn workspace_move_and_undo_migrate_history_before_returning() {
    use crate::response::{BodyStore, ResponseCookieJar, RunHistory};
    use crate::session::{run::*, runtime::RuntimeState};
    let (root, store) = persistence();
    let original = imported_pair(&store);
    let source = original.collections[0].requests[0].clone();
    let destination = original.collections[1].id.clone();
    let session = Arc::new(WorkspaceSession::default());
    session.replace(original);
    let ops = WorkspaceOperations::new(session);
    let history = Arc::new(RuntimeState::new(
        Arc::new(Mutex::new(RunHistory::new(root.path().join("history")))),
        Arc::new(Mutex::new(ResponseCookieJar::new(None))),
    ));
    ops.connect_history(history.clone());
    let identity = history.request_identity(Some(&source.collection_id), &source.request.id);
    let target = history.request_identity(Some(&destination), &source.request.id);
    let mut body = BodyStore::new("run", root.path()).unwrap();
    body.append(b"fixture").unwrap();
    body.finish().unwrap();
    let snapshot = SessionRunSnapshot {
        prepared: Some(SessionPrepared {
            proxy: None,
            method: "GET".into(),
            url: source.request.url.clone(),
            headers: vec![],
            body: SessionPreparedBody {
                byte_count: 0,
                content_type: None,
                text_preview: None,
                redacted: false,
            },
            transport: DomainTransport::default(),
        }),
        ..Default::default()
    };
    history.record(&identity, "run", snapshot, &Arc::new(Mutex::new(body)));
    let platform = SaveGate {
        store,
        ready: tokio::sync::Notify::default(),
        release: tokio::sync::Notify::default(),
        pause: std::sync::atomic::AtomicBool::new(false),
    };
    let secrets = SessionSecrets::default();
    let result = ops
        .mutate(
            Action::MoveRequest {
                from_collection_id: source.collection_id,
                id: source.request.id,
                to_collection_id: destination,
                group_id: None,
                order: 1,
            },
            None,
            &platform,
            &secrets,
            &Secrets,
        )
        .await;
    assert!(result.succeeded);
    assert_eq!(history.history_count(&identity).unwrap(), 0);
    assert_eq!(history.history_count(&target).unwrap(), 1);
    let token = result.undo.unwrap();
    let reverse = ops.reverse_token(token.token).unwrap();
    assert!(
        ops.undo(
            token.token,
            reverse.token,
            None,
            &platform,
            &secrets,
            &Secrets
        )
        .await
        .succeeded
    );
    assert_eq!(history.history_count(&identity).unwrap(), 1);
    assert_eq!(history.history_count(&target).unwrap(), 0);
}
