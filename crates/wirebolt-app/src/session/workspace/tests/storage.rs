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

#[test]
fn partial_workspace_load_reports_skipped_files_without_contents() {
    let (root, store) = persistence();
    let original = imported_pair(&store);
    let requests = root
        .path()
        .join("collections")
        .join(&original.collections[0].id)
        .join("requests");
    std::fs::write(
        requests.join("broken.toml"),
        r#"name = "private-fixture-value"#,
    )
    .unwrap();
    std::fs::write(
        requests.join("future.toml"),
        "schema_version = 4294967295\n",
    )
    .unwrap();
    std::fs::File::create(requests.join("oversized.toml"))
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    let result = store.load_result().unwrap();
    assert_eq!(result.workspace, original);
    assert_eq!(result.diagnostics.problems.len(), 3);
    for kind in [
        crate::session::persistence::WorkspaceDocumentProblemKind::InvalidToml,
        crate::session::persistence::WorkspaceDocumentProblemKind::UnsupportedSchema,
        crate::session::persistence::WorkspaceDocumentProblemKind::DocumentTooLarge,
    ] {
        assert!(
            result
                .diagnostics
                .problems
                .iter()
                .any(|problem| problem.kind == kind)
        );
    }
    let reply = store.workspace_io(WorkspaceIO::Load);
    let diagnostics = format!("{reply:?}");
    for name in ["broken.toml", "future.toml", "oversized.toml"] {
        assert!(diagnostics.contains(name));
    }
    assert!(!diagnostics.contains("private-fixture-value"));
    for index in 0..20 {
        std::fs::write(requests.join(format!("bad-{index}\n.toml")), "invalid = [").unwrap();
    }
    let result = store.load_result().unwrap();
    assert_eq!(result.diagnostics.problems.len(), 10);
    assert_eq!(result.diagnostics.omitted, 13);
    assert!(
        result
            .diagnostics
            .problems
            .iter()
            .all(|problem| !problem.path.chars().any(char::is_control))
    );
    let notice = result.diagnostics.notice.unwrap();
    assert!(notice.len() < 4000 && notice.contains("13 more files"));
}

struct SaveGate {
    store: Arc<SessionPersistence>,
    ready: tokio::sync::Notify,
    release: tokio::sync::Notify,
    pause: std::sync::atomic::AtomicBool,
}
#[async_trait]
impl WorkspacePlatform for SaveGate {
    async fn execute(&self, request: WorkspaceIO) -> WorkspaceReply {
        if matches!(&request, WorkspaceIO::Commands {commands} if commands.iter().any(|c| matches!(c, SessionWorkspaceCommand::SaveRequest {..})))
            && self.pause.swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.ready.notify_one();
            self.release.notified().await;
        }
        self.store.workspace_io(request)
    }
}

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
