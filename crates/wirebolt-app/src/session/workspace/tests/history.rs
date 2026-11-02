use super::{persistence_fixtures::*, *};

#[tokio::test]
async fn workspace_move_and_undo_relocate_history_before_returning() {
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

#[tokio::test]
async fn first_save_relocates_unlocated_history_and_in_flight_completions() {
    use crate::response::{BodyStore, ResponseCookieJar, RunHistory};
    use crate::session::{run::*, runtime::RuntimeState};
    let (root, store) = persistence();
    let workspace = imported_pair(&store);
    let collection = workspace.collections[0].id.clone();
    let session = Arc::new(WorkspaceSession::default());
    session.replace(workspace);
    let ops = WorkspaceOperations::new(session);
    let history = Arc::new(RuntimeState::new(
        Arc::new(Mutex::new(RunHistory::new(root.path().join("history")))),
        Arc::new(Mutex::new(ResponseCookieJar::new(None))),
    ));
    ops.connect_history(history.clone());
    let document = document("unsaved", None, true);
    let from = history.document_identity(&document);
    let to = history.request_identity(Some(&collection), "unsaved");
    let mut body = BodyStore::new("run", root.path()).unwrap();
    body.append(b"fixture").unwrap();
    body.finish().unwrap();
    let body = Arc::new(Mutex::new(body));
    let snapshot = SessionRunSnapshot {
        prepared: Some(SessionPrepared {
            proxy: None,
            method: "GET".into(),
            url: "https://example.invalid".into(),
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
    let platform = SaveGate {
        store,
        ready: tokio::sync::Notify::default(),
        release: tokio::sync::Notify::default(),
        pause: std::sync::atomic::AtomicBool::new(false),
    };
    history.record(&from, "before-save", snapshot.clone(), &body);
    assert_eq!(history.history_count(&from).unwrap(), 1);
    assert!(
        ops.save(
            document,
            Some(collection),
            &platform,
            &SessionSecrets::default(),
            &Secrets
        )
        .await
        .succeeded
    );
    assert_eq!(history.history_count(&from).unwrap(), 0);
    assert_eq!(history.history_count(&to).unwrap(), 1);
    assert!(
        history
            .snapshot()
            .request_activity
            .contains_key(&to.activity_key())
    );
    history.record(&from, "in-flight", snapshot, &body);
    assert_eq!(history.history_count(&to).unwrap(), 2);
    assert_eq!(
        RunHistory::new(root.path().join("history"))
            .count(&to)
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn workspace_folder_rename_and_move_preserve_request_history() {
    use crate::response::{BodyStore, ResponseCookieJar, RunHistory};
    use crate::session::{persistence::SessionPersistence, run::*, runtime::RuntimeState};
    let root = tempfile::tempdir().unwrap();
    let original_path = root.path().join("original");
    let persistence = SessionPersistence::open_or_create(
        original_path.to_string_lossy().into_owned(),
        "Fixture".into(),
        Arc::new(super::files::NoSecrets),
    )
    .unwrap();
    let workspace = imported_pair(&persistence);
    let request = &workspace.collections[0].requests[0];
    let make_history = || {
        RuntimeState::new(
            Arc::new(Mutex::new(RunHistory::new(root.path().join("history")))),
            Arc::new(Mutex::new(ResponseCookieJar::new(None))),
        )
    };
    let history = make_history();
    history.load_workspace_history(&workspace).unwrap();
    let identity = history.request_identity(Some(&request.collection_id), &request.request.id);
    let mut body = BodyStore::new("run", root.path()).unwrap();
    body.append(b"follows workspace").unwrap();
    body.finish().unwrap();
    history.record(
        &identity,
        "run",
        SessionRunSnapshot {
            prepared: Some(SessionPrepared {
                proxy: None,
                method: "GET".into(),
                url: request.request.url.clone(),
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
        },
        &Arc::new(Mutex::new(body)),
    );
    drop(persistence);
    let mut previous = original_path;
    std::fs::create_dir(root.path().join("another-parent")).unwrap();
    for path in [
        root.path().join("renamed"),
        root.path().join("another-parent/moved"),
    ] {
        std::fs::rename(&previous, &path).unwrap();
        let store = SessionPersistence::open_or_create(
            path.to_string_lossy().into_owned(),
            "Fixture".into(),
            Arc::new(super::files::NoSecrets),
        )
        .unwrap();
        let reloaded = make_history();
        reloaded
            .load_workspace_history(&store.domain_snapshot().unwrap())
            .unwrap();
        let entries = reloaded.history_entries(&identity).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            std::fs::read(&entries[0].body_path).unwrap(),
            b"follows workspace"
        );
        assert!(
            reloaded
                .snapshot()
                .request_activity
                .contains_key(&identity.activity_key())
        );
        previous = path;
    }
}

#[tokio::test]
async fn requests_added_after_load_publish_history_and_activity_immediately() {
    use crate::response::{BodyStore, ResponseCookieJar, RunHistory};
    use crate::session::{run::*, runtime::RuntimeState};
    for creation in ["command", "draft", "save"] {
        let (root, store) = persistence();
        let workspace = imported_pair(&store);
        let collection = workspace.collections[0].id.clone();
        let session = Arc::new(WorkspaceSession::default());
        let ops = WorkspaceOperations::new(session);
        let history = Arc::new(RuntimeState::new(
            Arc::new(Mutex::new(RunHistory::new(root.path().join("history")))),
            Arc::new(Mutex::new(ResponseCookieJar::new(None))),
        ));
        ops.connect_history(history.clone());
        let platform = SaveGate {
            store,
            ready: tokio::sync::Notify::default(),
            release: tokio::sync::Notify::default(),
            pause: std::sync::atomic::AtomicBool::new(false),
        };
        ops.load(&platform, false).await.unwrap();
        let request_id = match creation {
            "command" => ops
                .mutate(
                    Action::CreateRequest {
                        web_socket: false,
                        collection_id: Some(collection.clone()),
                        group_id: None,
                    },
                    None,
                    &platform,
                    &SessionSecrets::default(),
                    &Secrets,
                )
                .await
                .created_id
                .unwrap(),
            "draft" => ops.new_request(false, collection.clone(), None).request.id,
            _ => {
                let saved = ops
                    .save(
                        document("saved", Some(&collection), true),
                        None,
                        &platform,
                        &SessionSecrets::default(),
                        &Secrets,
                    )
                    .await;
                assert!(saved.succeeded);
                saved.location.unwrap().request.id
            }
        };
        let identity = history.request_identity(Some(&collection), &request_id);
        let mut body = BodyStore::new("run", root.path()).unwrap();
        body.append(b"new request").unwrap();
        body.finish().unwrap();
        let before = history.snapshot().history_revision;
        history.record(
            &identity,
            "run",
            SessionRunSnapshot {
                prepared: Some(SessionPrepared {
                    proxy: None,
                    method: "GET".into(),
                    url: "https://example.invalid/new".into(),
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
            },
            &Arc::new(Mutex::new(body)),
        );
        let snapshot = history.snapshot();
        assert_eq!(snapshot.history_entries.len(), 1, "{creation}");
        assert!(snapshot.history_revision > before, "{creation}");
        assert!(
            snapshot
                .request_activity
                .contains_key(&identity.activity_key()),
            "{creation}"
        );
    }
}
