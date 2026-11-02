use super::*;
use crate::lock;
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
    assert_eq!(state.history_count(&key(&state, "first")).unwrap(), 2);
    assert_eq!(state.history_count(&key(&state, "second")).unwrap(), 1);
    let before = state.snapshot().history_revision;
    state.clear_history(&key(&state, "first")).unwrap();
    assert_eq!(state.snapshot().history_revision, before + 1);
    assert_eq!(state.history_count(&key(&state, "first")).unwrap(), 0);
    assert_eq!(state.history_count(&key(&state, "second")).unwrap(), 1);
    assert!(!state.snapshot().request_activity.contains_key("api/first"));
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
    let entry = state.history_entries(&key(&state, "a")).unwrap().remove(0);
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
        .restore_latest(key(&state, "a"), run.clone(), &Changes::default())
        .await;
    assert_eq!(lock(&run).snapshot().text, "stored");
    assert_eq!(state.snapshot().request_activity["api/a"].status, Some(200));
    lock(&run).begin("new", root.path());
    state
        .clone()
        .restore_latest(key(&state, "a"), run.clone(), &Changes::default())
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
    state.clear_history(&key(&state, "a")).unwrap();
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
                .clone()
                .restore_latest(key(&state_for_task, "a"), run_for_task, observer.as_ref())
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

#[tokio::test]
async fn corrupt_history_does_not_stop_a_run_or_replace_the_index() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("history");
    std::fs::create_dir(&directory).unwrap();
    let index = directory.join("index.json");
    std::fs::write(&index, b"corrupt index").unwrap();
    let state = state(root.path());
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            document("health"),
            Arc::new(Mutex::new(DocumentRun::default())),
            Arc::new(Transport {
                body: b"history".to_vec(),
                ..Default::default()
            }),
            root.path(),
        ))
        .await;
    assert!(state.history_count(&key(&state, "health")).is_err());
    assert!(state.clear_history(&key(&state, "health")).is_err());
    assert!(state.clear_all_history().is_err());
    assert!(state.snapshot().history_entries.is_empty());
    assert_eq!(std::fs::read(index).unwrap(), b"corrupt index");
}

#[tokio::test]
async fn clear_history_preserves_displayed_response_body() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            document("a"),
            Arc::new(Mutex::new(DocumentRun::default())),
            Arc::new(Transport {
                body: b"response".to_vec(),
                ..Default::default()
            }),
            root.path(),
        ))
        .await;
    let entry = state.history_entries(&key(&state, "a")).unwrap().remove(0);
    let run = Mutex::new(DocumentRun::default());
    state.restore(entry.clone(), &run);
    state.clear_history(&key(&state, "a")).unwrap();
    let body = lock(&run).body().unwrap();
    assert_eq!(lock(&body).viewport(0, 8).unwrap(), b"response");
    assert!(!std::path::Path::new(&entry.body_path).exists());
    let pinned = lock(&body).path();
    drop(body);
    drop(run);
    assert!(!pinned.exists());
}

#[tokio::test]
async fn request_history_does_not_cross_workspace_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let first = state(root.path());
    let second = state(root.path());
    first.configure_history_workspace(Some(root.path().join("first-workspace").to_str().unwrap()));
    second
        .configure_history_workspace(Some(root.path().join("second-workspace").to_str().unwrap()));
    RuntimeRuns::default()
        .send(command(
            first,
            document("same"),
            Arc::new(Mutex::new(DocumentRun::default())),
            Arc::new(Transport {
                body: b"first workspace".to_vec(),
                ..Default::default()
            }),
            root.path(),
        ))
        .await;
    assert_eq!(second.history_count(&key(&second, "same")).unwrap(), 0);
}

#[tokio::test]
async fn history_follows_moves_undo_and_in_flight_completions() {
    use crate::session::workspace::SessionDocumentEffect::Relocate;
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            document("same"),
            run.clone(),
            Arc::new(Transport {
                body: b"fixture".to_vec(),
                ..Default::default()
            }),
            root.path(),
        ))
        .await;
    let source = key(&state, "same");
    let destination = state.request_identity(Some("other"), "same");
    state
        .apply_workspace_effects(&[Relocate {
            collection_id: "api".into(),
            request_id: "same".into(),
            destination: "other".into(),
        }])
        .unwrap();
    assert_eq!(state.history_count(&source).unwrap(), 0);
    assert_eq!(state.history_count(&destination).unwrap(), 1);
    let snapshot = lock(&run).snapshot();
    let body = lock(&run).body().unwrap();
    state.record(&source, "in-flight", snapshot, &body);
    assert_eq!(state.history_count(&destination).unwrap(), 2);
    state
        .apply_workspace_effects(&[Relocate {
            collection_id: "other".into(),
            request_id: "same".into(),
            destination: "api".into(),
        }])
        .unwrap();
    assert_eq!(state.history_count(&destination).unwrap(), 0);
    assert_eq!(state.history_count(&source).unwrap(), 2);
    let mut reloaded = RunHistory::new(root.path().join("history"));
    assert_eq!(reloaded.count(&source).unwrap(), 2);
    assert_eq!(reloaded.count(&destination).unwrap(), 0);
}

#[tokio::test]
async fn late_history_record_does_not_publish_into_another_workspace() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            document("same"),
            run.clone(),
            Arc::new(Transport {
                body: b"old workspace".to_vec(),
                ..Default::default()
            }),
            root.path(),
        ))
        .await;
    let original = key(&state, "same");
    let snapshot = lock(&run).snapshot();
    let body = lock(&run).body().unwrap();
    state.configure_history_workspace(Some(root.path().join("other-workspace").to_str().unwrap()));
    state.reset_workspace();
    state.record(&original, "late", snapshot, &body);
    assert_eq!(state.history_count(&original).unwrap(), 2);
    assert_eq!(state.history_count(&key(&state, "same")).unwrap(), 0);
    assert!(state.snapshot().history_entries.is_empty());
    assert!(state.snapshot().request_activity.is_empty());
}

#[tokio::test]
async fn replacement_workspace_reconciles_old_move_aliases() {
    use crate::session::workspace::SessionDocumentEffect::Relocate;
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            document("same"),
            run.clone(),
            Arc::new(Transport {
                body: b"fixture".to_vec(),
                ..Default::default()
            }),
            root.path(),
        ))
        .await;
    let original = key(&state, "same");
    let moved = state.request_identity(Some("other"), "same");
    state
        .apply_workspace_effects(&[Relocate {
            collection_id: "api".into(),
            request_id: "same".into(),
            destination: "other".into(),
        }])
        .unwrap();
    state.reconcile_history(&DomainWorkspace {
        collections: vec![DomainCollection {
            id: "api".into(),
            requests: vec![DomainLocation {
                collection_id: "api".into(),
                request: DomainRequestDraft {
                    id: "same".into(),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    });
    let snapshot = lock(&run).snapshot();
    let body = lock(&run).body().unwrap();
    state.record(&original, "after-reload", snapshot, &body);
    assert_eq!(state.history_count(&original).unwrap(), 1);
    assert_eq!(state.history_count(&moved).unwrap(), 1);
    state.reset_workspace();
    let snapshot = lock(&run).snapshot();
    state.record(&original, "after-reset", snapshot, &body);
    assert_eq!(state.history_count(&original).unwrap(), 2);
}

#[tokio::test]
async fn latest_restore_retains_body_before_observer_gate() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            document("a"),
            Arc::new(Mutex::new(DocumentRun::default())),
            Arc::new(Transport {
                body: b"fixture".to_vec(),
                ..Default::default()
            }),
            root.path(),
        ))
        .await;
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    let gate = Arc::new(RestoreGate::default());
    let state_for_task = state.clone();
    let run_for_task = run.clone();
    let observer = gate.clone();
    let task = tokio::spawn(async move {
        state_for_task
            .clone()
            .restore_latest(key(&state_for_task, "a"), run_for_task, observer.as_ref())
            .await;
    });
    gate.ready.notified().await;
    state.clear_history(&key(&state, "a")).unwrap();
    gate.release.notify_one();
    task.await.unwrap();
    assert_eq!(
        lock(&lock(&run).body().unwrap()).viewport(0, 7).unwrap(),
        b"fixture"
    );
    assert!(lock(&run).snapshot().failure.is_none());
}

#[tokio::test]
async fn workspace_load_migrates_unique_legacy_history_and_restores_activity_and_response() {
    let root = tempfile::tempdir().unwrap();
    let original = state(root.path());
    for id in ["unique", "ambiguous", "missing"] {
        RuntimeRuns::default()
            .send(command(
                original.clone(),
                document(id),
                Arc::new(Mutex::new(DocumentRun::default())),
                Arc::new(Transport {
                    body: b"legacy response".to_vec(),
                    ..Default::default()
                }),
                root.path(),
            ))
            .await;
    }
    let index = root.path().join("history/index.json");
    let mut entries: Vec<crate::session::history::SessionHistoryEntry> =
        serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
    for entry in &mut entries {
        entry.identity = None;
    }
    std::fs::write(&index, serde_json::to_vec(&entries).unwrap()).unwrap();
    let workspace = DomainWorkspace {
        collections: [
            ("api", vec!["unique", "ambiguous"]),
            ("other", vec!["ambiguous"]),
        ]
        .into_iter()
        .map(|(collection, ids)| DomainCollection {
            id: collection.into(),
            requests: ids
                .into_iter()
                .map(|id| DomainLocation {
                    collection_id: collection.into(),
                    request: DomainRequestDraft {
                        id: id.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })
        .collect(),
        ..Default::default()
    };
    let reloaded = state(root.path());
    reloaded
        .load_workspace_history("fixture", &workspace)
        .unwrap();
    assert_eq!(
        reloaded.history_count(&key(&reloaded, "unique")).unwrap(),
        1
    );
    assert!(
        reloaded
            .snapshot()
            .request_activity
            .contains_key("api/unique")
    );
    assert_eq!(reloaded.legacy_history("ambiguous").len(), 1);
    assert_eq!(reloaded.legacy_history("missing").len(), 1);
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    reloaded
        .clone()
        .restore_latest(key(&reloaded, "unique"), run.clone(), &Changes::default())
        .await;
    assert_eq!(lock(&run).snapshot().text, "legacy response");
    let persisted: Vec<crate::session::history::SessionHistoryEntry> =
        serde_json::from_slice(&std::fs::read(index).unwrap()).unwrap();
    assert!(
        persisted
            .iter()
            .find(|entry| entry.request_id == "unique")
            .unwrap()
            .identity
            .is_some()
    );
    reloaded
        .clear_history(&key(&reloaded, "ambiguous"))
        .unwrap();
    assert!(reloaded.legacy_history("ambiguous").is_empty());
    reloaded.clear_all_history().unwrap();
    assert!(reloaded.legacy_history("missing").is_empty());
    assert_eq!(
        reloaded.history_count(&key(&reloaded, "unique")).unwrap(),
        0
    );
}

#[tokio::test]
async fn unsaved_workspace_history_survives_runtime_recreation() {
    let root = tempfile::tempdir().unwrap();
    let original = state(root.path());
    RuntimeRuns::default()
        .send(command(
            original.clone(),
            document("a"),
            Arc::new(Mutex::new(DocumentRun::default())),
            Arc::new(Transport {
                body: b"unsaved response".to_vec(),
                ..Default::default()
            }),
            root.path(),
        ))
        .await;
    let reloaded = state(root.path());
    assert_eq!(key(&original, "a"), key(&reloaded, "a"));
    assert_eq!(reloaded.history_count(&key(&reloaded, "a")).unwrap(), 1);
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    reloaded
        .clone()
        .restore_latest(key(&reloaded, "a"), run.clone(), &Changes::default())
        .await;
    assert_eq!(lock(&run).snapshot().text, "unsaved response");
}
