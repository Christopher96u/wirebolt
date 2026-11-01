use super::*;
use crate::{
    lock,
    session::run::{SessionFailure, SessionPrepared},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
use wirebolt_core::ProxyPolicy;
use wirebolt_core::RunHead;

/// Each test owns its cache, so resets in one test cannot race lookups
/// in another the way a shared static would.
fn engine_for(cache: &HttpEngineCache, proxy: &ResolvedProxy) -> Arc<HttpEngine> {
    cache
        .engine(proxy.mode(), || {
            HttpEngine::with_proxy(
                &HttpEngineConfig::default(),
                proxy,
                &wirebolt_core::NoSecrets,
            )
        })
        .expect("HTTP engine")
}

fn manual_proxy_mode() -> ProxyMode {
    let route = wirebolt_core::ProxyRoute::new(
        wirebolt_core::ProxyDestination::All,
        wirebolt_core::ProxyEndpoint::new("http://proxy.internal:8080").expect("proxy endpoint"),
    );
    ProxyMode::Manual(wirebolt_core::ManualProxy::new(vec![route]).expect("manual proxy"))
}

#[test]
fn shared_direct_engine_reuses_the_same_client_pool() {
    let cache = HttpEngineCache::default();
    let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);

    let first = engine_for(&cache, &proxy);
    let second = engine_for(&cache, &proxy);

    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn shared_manual_engine_is_keyed_by_mode_and_reset_on_demand() {
    let cache = HttpEngineCache::default();
    let manual = manual_proxy_mode();
    let from_workspace = ProxyPolicy::with_workspace(manual.clone()).resolve(None);
    let from_request = ProxyPolicy::default().resolve(Some(&manual));

    let first = engine_for(&cache, &from_workspace);
    let second = engine_for(&cache, &from_request);
    assert!(Arc::ptr_eq(&first, &second));

    cache.reset();
    let replacement = engine_for(&cache, &from_workspace);
    assert!(!Arc::ptr_eq(&first, &replacement));
}

#[test]
fn reset_retires_every_slot_at_once() {
    let cache = HttpEngineCache::default();
    let direct = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);
    let system = ProxyPolicy::with_workspace(ProxyMode::System).resolve(None);
    let manual = ProxyPolicy::with_workspace(manual_proxy_mode()).resolve(None);
    let before = [
        engine_for(&cache, &direct),
        engine_for(&cache, &system),
        engine_for(&cache, &manual),
    ];

    cache.reset();

    let after = [
        engine_for(&cache, &direct),
        engine_for(&cache, &system),
        engine_for(&cache, &manual),
    ];
    for (old, new) in before.iter().zip(&after) {
        assert!(!Arc::ptr_eq(old, new));
    }
    for (old, new) in after.iter().zip([
        engine_for(&cache, &direct),
        engine_for(&cache, &system),
        engine_for(&cache, &manual),
    ]) {
        assert!(Arc::ptr_eq(old, &new));
    }
}

#[test]
fn an_engine_built_while_a_reset_ran_is_used_once_but_never_pooled() {
    let cache = HttpEngineCache::default();
    let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);

    let raced = cache
        .engine(proxy.mode(), || {
            // Simulates a network change landing while the client is
            // still being built with the pre-change proxy settings.
            cache.reset();
            HttpEngine::with_proxy(
                &HttpEngineConfig::default(),
                &proxy,
                &wirebolt_core::NoSecrets,
            )
        })
        .expect("engine for this run");

    let pooled = engine_for(&cache, &proxy);
    assert!(!Arc::ptr_eq(&raced, &pooled));
    assert!(Arc::ptr_eq(&pooled, &engine_for(&cache, &proxy)));
}

#[test]
fn concurrent_lookups_and_resets_never_hand_out_a_retired_engine() {
    let cache = Arc::new(HttpEngineCache::default());
    let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let proxy = proxy.clone();
            thread::spawn(move || {
                for _ in 0..50 {
                    drop(engine_for(&cache, &proxy));
                }
            })
        })
        .collect();
    for _ in 0..20 {
        cache.reset();
        thread::sleep(Duration::from_millis(1));
    }
    for worker in workers {
        worker.join().expect("worker thread");
    }

    let retired = engine_for(&cache, &proxy);
    cache.reset();
    let fresh = engine_for(&cache, &proxy);
    assert!(!Arc::ptr_eq(&retired, &fresh));
    assert!(Arc::ptr_eq(&fresh, &engine_for(&cache, &proxy)));
}

#[test]
fn zero_timeouts_disable_the_deadline() {
    assert_eq!(timeout_from_millis(0), None);
    assert_eq!(timeout_from_millis(250), Some(Duration::from_millis(250)));
}

#[test]
fn a_failed_app_proxy_never_retries_the_destination_directly() {
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let unused = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_address = unused.local_addr().unwrap();
    drop(unused);
    let input = serde_json::json!({"method":"GET", "url":format!("http://{}/must-not-arrive", destination.local_addr().unwrap()),
        "app_proxy":{"mode":"manual", "routes":[{"destination":"all", "endpoint":format!("http://{proxy_address}")}]}});
    let (events, state) = run_to_completion(&input.to_string());
    assert_eq!(events, ["prepared", "error"]);
    assert_eq!(
        state.failure.lock().unwrap().as_ref().unwrap().kind,
        crate::response::ResponseFailureKind::Connection
    );
    assert!(
        matches!(destination.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}

#[test]
fn app_proxy_routes_and_request_direct_bypasses_it() {
    for direct in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "proxy was never reached"
                );
                thread::sleep(Duration::from_millis(5));
            };
            // The accepted socket inherits the listener's non-blocking mode on macOS.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0_u8; 4096];
            let count = stream.read(&mut bytes).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npong")
                .unwrap();
            String::from_utf8_lossy(&bytes[..count]).into_owned()
        });
        let endpoint = if direct {
            "http://127.0.0.1:9".to_owned()
        } else {
            format!("http://{address}")
        };
        let url = if direct {
            format!("http://{address}/proxy-test")
        } else {
            "http://127.0.0.1:9/proxy-test".to_owned()
        };
        let mut input = serde_json::json!({"method":"GET","url":url,"app_proxy":{"mode":"manual","routes":[{"destination":"all","endpoint":endpoint}]}});
        if direct {
            input["request_proxy"] = serde_json::json!({"mode":"direct"});
        }
        let (events, state) = run_to_completion(&input.to_string());
        assert_eq!(events, ["prepared", "head", "chunk:pong", "complete"]);
        let snapshot = state.prepared.lock().unwrap().clone().unwrap();
        assert_eq!(
            snapshot.proxy.as_ref().unwrap().source,
            if direct {
                crate::domain::proxy::ProxySource::Request
            } else {
                crate::domain::proxy::ProxySource::AppDefault
            }
        );
        assert_eq!(
            matches!(
                snapshot.proxy.unwrap().configuration,
                crate::domain::DomainProxy::Direct
            ),
            direct
        );
        let request = server.join().unwrap();
        assert!(request.starts_with(if direct {
            "GET /proxy-test "
        } else {
            "GET http://127.0.0.1:9/proxy-test "
        }));
    }
}

#[test]
fn transport_observer_delivers_head_chunks_and_completion_in_order() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback server");
    let address = listener.local_addr().expect("loopback address");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept HTTP client");
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).expect("read HTTP request");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nX-Wirebolt: ffi\r\nConnection: close\r\n\r\npong",
            )
            .expect("write HTTP response");
    });
    let input = serde_json::json!({
        "method": "GET",
        "url": format!("http://{address}/ffi"),
        "workspace_proxy": { "mode": "direct" }
    })
    .to_string();
    let (events, _state) = run_to_completion(&input);
    server.join().expect("HTTP server");
    assert_eq!(events, ["prepared", "head", "chunk:pong", "complete"]);
}

#[test]
fn transport_failures_retain_kind_and_empty_issues() {
    let input = serde_json::json!({
        "method": "GET",
        "url": "http://127.0.0.1:1/refused",
        "workspace_proxy": { "mode": "direct" }
    })
    .to_string();
    let (events, state) = run_to_completion(&input);

    assert_eq!(events, ["prepared", "error"]);
    let failure = state
        .failure
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .expect("failure document");
    assert!(failure.issues.is_empty());
    assert_eq!(
        failure.kind,
        crate::response::ResponseFailureKind::Connection
    );
}

fn run_to_completion(input: &str) -> (Vec<String>, Arc<TestTransportObserver>) {
    let observer = Arc::new(TestTransportObserver::default());
    let input: RunInput = serde_json::from_str(input).unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(execute(
        input,
        RunCancellation::new(),
        observer.clone(),
        wirebolt_core::NoSecrets,
    ));
    let events = lock(&observer.events).clone();
    assert!(
        events
            .last()
            .is_some_and(|event| matches!(event.as_str(), "complete" | "error"))
    );
    (events, observer)
}
#[derive(Debug, Default)]
struct TestTransportObserver {
    events: Mutex<Vec<String>>,
    failure: Mutex<Option<SessionFailure>>,
    prepared: Mutex<Option<SessionPrepared>>,
}
impl TransportObserver for TestTransportObserver {
    fn prepared(&self, snapshot: &PreparedRunSnapshotDocument) {
        assert_eq!(snapshot.method, "GET");
        *lock(&self.prepared) = Some(SessionPrepared::from(snapshot));
        lock(&self.events).push("prepared".into());
    }
    fn head(&self, head: &RunHead, _: &[FailureIssue]) {
        assert_eq!(head.status, 200);
        lock(&self.events).push("head".into());
    }
    fn chunk(&self, bytes: &[u8]) -> bool {
        lock(&self.events).push(format!("chunk:{}", String::from_utf8_lossy(bytes)));
        true
    }
    fn cookies(&self, _: &str, _: Vec<(String, String)>) {}
    fn complete(&self, received: u64, decoded: u64, _: Duration) {
        assert_eq!(received, 4);
        assert_eq!(decoded, 4);
        lock(&self.events).push("complete".into());
    }
    fn failure(&self, failure: &RunFailureDocument) {
        *lock(&self.failure) = Some(SessionFailure::from(failure));
        lock(&self.events).push("error".into());
    }
}

#[test]
fn workspace_secret_writes_and_deletes_retire_manual_http_engines() {
    use crate::session::persistence::{SessionPersistence, WorkspaceError, WorkspaceSecrets};
    #[derive(Debug)]
    struct Secrets;
    impl WorkspaceSecrets for Secrets {
        fn read(&self, _: &str) -> Result<Option<String>, WorkspaceError> {
            Ok(None)
        }
        fn save(&self, _: &str, _: &str) -> Result<(), WorkspaceError> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), WorkspaceError> {
            Err(WorkspaceError::operation("already missing"))
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = SessionPersistence::open_or_create(
        dir.path().to_string_lossy().into_owned(),
        "Secrets".into(),
        Arc::new(Secrets),
    )
    .unwrap();
    let proxy = ProxyPolicy::with_workspace(manual_proxy_mode()).resolve(None);
    let engine = || {
        shared_http_engine(
            &proxy,
            &HttpEngineConfig::default(),
            &wirebolt_core::NoSecrets,
        )
        .unwrap()
    };
    let before_save = engine();
    store.save_secret("owned.secret", "new material").unwrap();
    let after_save = engine();
    assert!(!Arc::ptr_eq(&before_save, &after_save));
    store.delete_secret("owned.secret").unwrap();
    assert!(!Arc::ptr_eq(&after_save, &engine()));
}
