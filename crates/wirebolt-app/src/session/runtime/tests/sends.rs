use super::*;
use crate::lock;
#[tokio::test]
async fn streams_response_into_bounded_visible_state() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let document = document("a");
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    let transport = Arc::new(Transport {
        body: vec![65; VIEWPORT_BYTES + 128],
        ..Default::default()
    });
    assert!(
        RuntimeRuns::default()
            .send(command(
                state,
                document,
                run.clone(),
                transport,
                root.path()
            ))
            .await
            .is_none()
    );
    let snapshot = lock(&run).snapshot();
    assert_eq!(snapshot.head.unwrap().status, 200);
    assert_eq!(snapshot.bytes, (VIEWPORT_BYTES + 128) as u64);
    assert_eq!(snapshot.data.len(), VIEWPORT_BYTES);
    assert!(snapshot.truncated);
    assert!(snapshot.active_run_id.is_none());
    assert_eq!(snapshot.completion.unwrap().total_time_ns, 20);
}
#[tokio::test]
async fn sends_captured_document_and_keeps_prepared_snapshot_immutable() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let first = document("first");
    let second = document("second");
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    let transport = Arc::new(Transport {
        hold: true,
        ..Default::default()
    });
    let runs = Arc::new(RuntimeRuns::default());
    let command = command(
        state,
        first.clone(),
        run.clone(),
        transport.clone(),
        root.path(),
    );
    let task = tokio::spawn(async move { runs.send(command).await });
    transport.started.notified().await;
    lock(&first).draft.url = "https://example.com/edited".into();
    transport.release.notify_one();
    task.await.unwrap();
    assert_eq!(
        lock(&run).snapshot().prepared.unwrap().url,
        "https://example.com/first"
    );
    assert_eq!(lock(&first).draft.url, "https://example.com/edited");
    assert_eq!(lock(&second).draft.url, "https://example.com/second");
}
#[tokio::test]
async fn independent_runs_cancellation_and_late_events_stay_scoped() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let runs = Arc::new(RuntimeRuns::default());
    let (a, b) = (document("a"), document("b"));
    let (ra, rb) = (
        Arc::new(Mutex::new(DocumentRun::default())),
        Arc::new(Mutex::new(DocumentRun::default())),
    );
    let (ta, tb) = (
        Arc::new(Transport {
            hold: true,
            ..Default::default()
        }),
        Arc::new(Transport {
            hold: true,
            body: b"kept".to_vec(),
            ..Default::default()
        }),
    );
    let ca = command(state.clone(), a, ra.clone(), ta.clone(), root.path());
    let cb = command(state.clone(), b, rb.clone(), tb.clone(), root.path());
    let r = runs.clone();
    let sa = tokio::spawn(async move { r.send(ca).await });
    let r = runs.clone();
    let sb = tokio::spawn(async move { r.send(cb).await });
    ta.started.notified().await;
    tb.started.notified().await;
    runs.cancel("a", &ra);
    tb.release.notify_one();
    sa.await.unwrap();
    sb.await.unwrap();
    assert!(ta.cancelled.load(Ordering::Relaxed));
    assert!(!tb.cancelled.load(Ordering::Relaxed));
    assert_eq!(
        lock(&ra).snapshot().failure.unwrap().kind,
        crate::response::ResponseFailureKind::Cancelled
    );
    assert_eq!(lock(&ra).snapshot().bytes, 0);
    assert_eq!(lock(&rb).snapshot().text, "kept");
    assert_eq!(state.history_count("a").unwrap(), 0);
    assert_eq!(state.history_count("b").unwrap(), 1);
}
#[tokio::test]
async fn running_document_does_not_start_twice() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let doc = document("a");
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    lock(&run).begin("existing", root.path());
    let transport = Arc::new(Transport::default());
    RuntimeRuns::default()
        .send(command(state, doc, run, transport.clone(), root.path()))
        .await;
    assert!(lock(&transport.inputs).is_empty());
}
#[tokio::test]
async fn inherits_workspace_transport_until_request_override() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let doc = document("a");
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    let transport = Arc::new(Transport::default());
    let runs = RuntimeRuns::default();
    lock(&doc).draft.transport.total_timeout_ms = 999;
    let mut c = command(
        state.clone(),
        doc.clone(),
        run.clone(),
        transport.clone(),
        root.path(),
    );
    c.context.transport = DomainTransport {
        validate_tls: false,
        total_timeout_ms: 123,
        read_timeout_ms: 45,
        follow_redirects: true,
        maximum_redirects: 4,
        ..Default::default()
    };
    let context = c.context.clone();
    runs.send(c).await;
    lock(&doc).draft.inherits_workspace_transport = false;
    let mut c = command(state, doc, run, transport.clone(), root.path());
    c.context = context;
    runs.send(c).await;
    let inputs = lock(&transport.inputs);
    assert!(!inputs[0].validate_tls);
    assert_eq!(inputs[0].total_timeout_ms, 123);
    assert_eq!(inputs[0].maximum_redirects, 4);
    assert!(inputs[1].validate_tls);
    assert_eq!(inputs[1].total_timeout_ms, 999);
}
#[tokio::test]
async fn resolution_failures_preserve_issue_details_and_skip_history() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let doc = document("a");
    lock(&doc).draft.url = "https://{{missing}}".into();
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    let transport = Arc::new(Transport::default());
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            doc,
            run.clone(),
            transport.clone(),
            root.path(),
        ))
        .await;
    let failure = lock(&run).snapshot().failure.unwrap();
    assert_eq!(
        failure.kind,
        crate::response::ResponseFailureKind::InvalidRequest
    );
    assert_eq!(
        failure.issues[0].kind,
        crate::response::ResponseIssueKind::MissingVariable
    );
    assert!(lock(&transport.inputs).is_empty());
    assert_eq!(state.history_count("a").unwrap(), 0);
}
#[tokio::test]
async fn redirect_cookies_keep_response_origin_and_survive_failure() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    let transport = Arc::new(Transport {
        failure: Some(SessionFailure::new(
            crate::response::ResponseFailureKind::Connection,
        )),
        cookies: vec![
            (
                "https://original.example/start".into(),
                vec![ResponseHeaderValue {
                    name: "Set-Cookie".into(),
                    value: "first=fixture; Path=/; Secure".into(),
                }],
            ),
            (
                "https://redirected.example/final".into(),
                vec![ResponseHeaderValue {
                    name: "Set-Cookie".into(),
                    value: "second=fixture; Path=/; Secure".into(),
                }],
            ),
        ],
        ..Default::default()
    });
    RuntimeRuns::default()
        .send(command(
            state.clone(),
            document("a"),
            run.clone(),
            transport,
            root.path(),
        ))
        .await;
    let jar = state.cookie_jar();
    let mut jar = lock(&jar);
    assert_eq!(
        jar.header("https://original.example/", crate::response::unix_time())
            .as_deref(),
        Some("first=fixture")
    );
    assert_eq!(
        jar.header("https://redirected.example/", crate::response::unix_time())
            .as_deref(),
        Some("second=fixture")
    );
    assert!(
        jar.header("https://unrelated.example/", crate::response::unix_time())
            .is_none()
    );
    assert_eq!(lock(&run).snapshot().cookies.len(), 2);
    assert_eq!(
        lock(&run).snapshot().failure.unwrap().kind,
        crate::response::ResponseFailureKind::Connection
    );
    assert_eq!(state.snapshot().cookie_revision, 2);
}
#[tokio::test]
async fn cookie_injection_resolves_variables_respects_explicit_headers_and_redacts() {
    for explicit in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        let _ = lock(&state.cookie_jar()).store(
            &[ResponseHeaderValue {
                name: "Set-Cookie".into(),
                value: "session=fixture; Path=/".into(),
            }],
            "https://resolved.example/",
            crate::response::unix_time(),
        );
        let doc = document("a");
        lock(&doc).draft.url = "https://{{host}}/".into();
        if explicit {
            lock(&doc).draft.headers.push(DomainField {
                name: "cOoKiE".into(),
                value: DomainValueSource::literal("explicit"),
                enabled: true,
                ..Default::default()
            });
        }
        let transport = Arc::new(Transport::default());
        let mut c = command(
            state,
            doc,
            Arc::new(Mutex::new(DocumentRun::default())),
            transport.clone(),
            root.path(),
        );
        c.context.variables.insert(
            "host".into(),
            DomainValueSource::literal("resolved.example"),
        );
        RuntimeRuns::default().send(c).await;
        let inputs = lock(&transport.inputs);
        assert_eq!(inputs[0].headers.len(), 1);
        assert_eq!(
            inputs[0].headers[0].value.editable(),
            if explicit {
                "explicit"
            } else {
                "session=fixture"
            }
        );
        assert_eq!(inputs[0].headers[0].sensitive, !explicit);
    }
}
#[test]
fn global_variables_apply_without_selection_and_selected_values_override() {
    let variable = |key: &str, value: &str, enabled| DomainVariable {
        key: key.into(),
        value: DomainValueSource::literal(value),
        enabled,
        ..Default::default()
    };
    let environments = vec![
        DomainEnvironment {
            id: "global".into(),
            name: "Global".into(),
            variables: vec![
                variable("host", "global.example", true),
                variable("shared", "yes", true),
            ],
        },
        DomainEnvironment {
            id: "local".into(),
            name: "Local".into(),
            variables: vec![
                variable("host", "localhost", true),
                variable("shared", "disabled", false),
            ],
        },
    ];
    assert_eq!(
        active_variables(&environments, None)["host"].editable(),
        "global.example"
    );
    let selected = active_variables(&environments, Some("local"));
    assert_eq!(selected["host"].editable(), "localhost");
    assert_eq!(selected["shared"].editable(), "yes");
    assert_eq!(
        active_variables(&environments, Some("missing")),
        active_variables(&environments, None)
    );
}
#[test]
fn socket_actions_preserve_send_and_connect_rules() {
    use crate::session::websocket::SessionSocketStatus as S;
    assert_eq!(
        send_action(&SessionKind::WebSocket, &S::Connected, false),
        RuntimeSendAction::SocketSend
    );
    assert_eq!(
        send_action(&SessionKind::WebSocket, &S::Connecting, false),
        RuntimeSendAction::SocketConnect
    );
    assert_eq!(
        send_action(&SessionKind::Http, &S::Disconnected, true),
        RuntimeSendAction::Ignore
    );
    assert_eq!(
        send_action(&SessionKind::Http, &S::Disconnected, false),
        RuntimeSendAction::Http
    );
}

#[tokio::test]
async fn dropping_a_cancelled_send_releases_its_publication_task_and_observer() {
    let root = tempfile::tempdir().unwrap();
    let runs = Arc::new(RuntimeRuns::default());
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    let transport = Arc::new(Transport {
        hold: true,
        ..Default::default()
    });
    let observer = Arc::new(Changes::default());
    let weak = Arc::downgrade(&observer);
    let mut c = command(
        state(root.path()),
        document("a"),
        run.clone(),
        transport.clone(),
        root.path(),
    );
    c.observer = observer;
    let owner = runs.clone();
    let task = tokio::spawn(async move { owner.send(c).await });
    transport.started.notified().await;
    runs.cancel("a", &run);
    task.abort();
    let _ = task.await;
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while weak.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a dropped send must release its native observer and 16 ms publisher");
}
