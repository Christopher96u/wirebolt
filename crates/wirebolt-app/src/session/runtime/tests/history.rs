use super::*;
#[tokio::test]
async fn history_counts_and_clear_revision_belong_to_each_request() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let transport = Arc::new(Transport {
        body: b"history".to_vec(),
        ..Default::default()
    });
    let runs = RuntimeRuns::default();
    for id in ["first", "first", "second"] {
        runs.send(command(
            state.clone(),
            document(id),
            Arc::new(Mutex::new(DocumentRun::default())),
            transport.clone(),
            root.path(),
        ))
        .await;
    }
    assert_eq!(state.history_count("first"), 2);
    assert_eq!(state.history_count("second"), 1);
    let before = state.snapshot().history_revision;
    state.clear_history("first");
    assert_eq!(state.snapshot().history_revision, before + 1);
    assert_eq!(state.history_count("first"), 0);
    assert_eq!(state.history_count("second"), 1);
    assert!(!state.snapshot().request_activity.contains_key("first"));
}
#[tokio::test]
async fn restore_history_uses_bounded_body_and_keeps_request_draft() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let transport = Arc::new(Transport {
        body: vec![65; VIEWPORT_BYTES + 1],
        ..Default::default()
    });
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            document("a"),
            Arc::new(Mutex::new(DocumentRun::default())),
            transport,
            root.path(),
        ))
        .await;
    let entry = state.history_entries("a").remove(0);
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    state.restore(entry.clone(), &run);
    let snapshot = lock(&run).snapshot();
    assert_eq!(snapshot.data.len(), VIEWPORT_BYTES);
    assert!(snapshot.truncated);
    assert_eq!(snapshot.bytes, VIEWPORT_BYTES as u64 + 1);
    assert!(snapshot.active_run_id.is_none());
    assert_eq!(snapshot.prepared.unwrap(), entry.prepared);
    assert!(lock(&run).body().is_some());
}
#[tokio::test]
async fn latest_response_restores_activity_only_into_unused_documents() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let transport = Arc::new(Transport {
        body: b"stored".to_vec(),
        ..Default::default()
    });
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            document("a"),
            Arc::new(Mutex::new(DocumentRun::default())),
            transport,
            root.path(),
        ))
        .await;
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    state
        .clone()
        .restore_latest("a".into(), run.clone(), &Changes::default())
        .await;
    assert_eq!(lock(&run).snapshot().text, "stored");
    assert_eq!(state.snapshot().request_activity["a"].status, Some(200));
    lock(&run).begin("new", root.path());
    state
        .clone()
        .restore_latest("a".into(), run.clone(), &Changes::default())
        .await;
    assert_eq!(lock(&run).snapshot().active_run_id.as_deref(), Some("new"));
}
#[test]
fn cookies_are_sorted_deleted_cleared_and_scoped_per_workspace() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    state.configure_cookies(Some("/work/a"), Some(root.path().to_str().unwrap()));
    let old = state.cookie_jar();
    let _ = lock(&old).store(
        &[
            ResponseHeaderValue {
                name: "Set-Cookie".into(),
                value: "z=value; Path=/; Max-Age=600".into(),
            },
            ResponseHeaderValue {
                name: "Set-Cookie".into(),
                value: "a=value; Path=/; Max-Age=600".into(),
            },
        ],
        "https://example.com/",
        crate::response::unix_time(),
    );
    let cookies = state.cookies();
    assert_eq!(
        cookies.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["a", "z"]
    );
    state.delete_cookie(&cookies[0].id());
    assert_eq!(state.cookies().len(), 1);
    state.configure_cookies(Some("/work/b"), Some(root.path().to_str().unwrap()));
    assert!(state.cookies().is_empty());
    assert_eq!(lock(&old).all(crate::response::unix_time()).len(), 1);
    state.configure_cookies(Some("/work/a"), Some(root.path().to_str().unwrap()));
    assert_eq!(state.cookies().len(), 1);
    state.clear_cookies();
    assert!(state.cookies().is_empty());
    assert_eq!(state.snapshot().cookie_revision, 5);
}
#[test]
fn resetting_workspace_clears_selection_activity_and_entries_but_keeps_revisions() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    state.select_environment(Some("old".into()));
    state.clear_history("a");
    state.reset_workspace();
    assert!(state.snapshot().history_entries.is_empty());
    assert!(state.snapshot().request_activity.is_empty());
    assert_eq!(state.snapshot().history_revision, 1);
}

#[derive(Debug, Default)]
struct RestoreGate {
    ready: tokio::sync::Notify,
    release: tokio::sync::Notify,
    closed: AtomicBool,
}
#[async_trait]
impl RuntimeObserver for RestoreGate {
    fn changed(&self) {}
    async fn prepare(&self) -> Result<Option<RuntimeInput>, ()> {
        self.ready.notify_one();
        self.release.notified().await;
        if self.closed.load(Ordering::Relaxed) {
            Err(())
        } else {
            Ok(None)
        }
    }
}
#[tokio::test]
async fn latest_restore_checks_membership_after_io_and_rejects_departed_workspaces() {
    for switch_workspace in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        let transport = Arc::new(Transport {
            body: b"old workspace".to_vec(),
            ..Default::default()
        });
        RuntimeRuns::default()
            .send(command(
                state.clone(),
                document("a"),
                Arc::new(Mutex::new(DocumentRun::default())),
                transport,
                root.path(),
            ))
            .await;
        state.reset_workspace();
        let run = Arc::new(Mutex::new(DocumentRun::default()));
        let gate = Arc::new(RestoreGate::default());
        let state_for_task = state.clone();
        let run_for_task = run.clone();
        let observer = gate.clone();
        let task = tokio::spawn(async move {
            state_for_task
                .restore_latest("a".into(), run_for_task, observer.as_ref())
                .await;
        });
        gate.ready.notified().await;
        if switch_workspace {
            state.reset_workspace();
        } else {
            gate.closed.store(true, Ordering::Relaxed);
        }
        gate.release.notify_one();
        task.await.unwrap();
        assert!(state.snapshot().request_activity.is_empty());
        assert!(lock(&run).snapshot().head.is_none());
        assert!(lock(&run).body().is_none());
    }
}
