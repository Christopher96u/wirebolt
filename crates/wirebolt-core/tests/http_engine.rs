use std::{convert::Infallible, net::SocketAddr, process::Command, time::Duration};

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming, service::service_fn};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use wirebolt_core::{
    ContentEncoding, DocumentId, HeaderField, HttpEngine, HttpEngineConfig, HttpVersion,
    HttpVersionPolicy, ManualProxy, NoSecrets, ProxyConfigurationErrorKind, ProxyCredentials,
    ProxyDestination, ProxyEndpoint, ProxyMode, ProxyPolicy, ProxyRoute, Request as SavedRequest,
    RequestBody as SavedRequestBody, RequestDraft, RequestPipeline, ResolvedSecret,
    RunCancellation, RunErrorKind, RunOptions, SecretName, SecretResolutionError,
    SecretResolutionErrorKind, SecretResolver, StreamControl, prepare_request,
};

#[tokio::test]
async fn reports_response_headers_before_streaming_body_chunks() {
    let (address, server) = spawn_http1_script(
        b"HTTP/1.1 202 Accepted\r\nTransfer-Encoding: chunked\r\nX-Wirebolt-Test: observed\r\nConnection: close\r\n\r\n",
        vec![
            (Duration::ZERO, b"4\r\nbody\r\n"),
            (Duration::ZERO, b"0\r\n\r\n"),
        ],
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let events = std::cell::RefCell::new(Vec::new());

    let run = engine
        .run_observed(
            get_request(address, "/observed"),
            RunOptions::default(),
            &RunCancellation::new(),
            |head| {
                assert_eq!(head.status, 202);
                assert_eq!(head.version, HttpVersion::Http11);
                assert!(
                    head.headers
                        .get("x-wirebolt-test")
                        .is_some_and(|value| value == "observed")
                );
                events.borrow_mut().push("headers");
            },
            |chunk| {
                assert_eq!(chunk, b"body");
                events.borrow_mut().push("chunk");
                StreamControl::Continue
            },
        )
        .await
        .expect("observed response");

    assert_eq!(run.bytes_received, 4);
    assert_eq!(*events.borrow(), ["headers", "chunk"]);
    server.await.expect("HTTP server task");
}

#[tokio::test]
async fn runs_an_http1_request_through_the_public_interface() {
    let (address, received_request) = spawn_http1_once(
        b"HTTP/1.1 201 Created\r\nContent-Length: 4\r\nX-Wirebolt-Test: yes\r\nConnection: close\r\n\r\npong",
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let request = prepare_request(RequestDraft {
        method: "POST".to_owned(),
        url: format!("http://{address}/echo?from=wirebolt"),
        headers: vec![HeaderField {
            name: "x-client".to_owned(),
            value: "wirebolt".to_owned(),
        }],
        body: b"hello".to_vec(),
    })
    .expect("prepared request");
    let mut body = Vec::new();

    let run = engine
        .run(
            request,
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                body.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect("successful run");

    assert_eq!(run.status, 201);
    assert_eq!(run.version, HttpVersion::Http11);
    assert_eq!(run.bytes_received, 4);
    assert_eq!(body, b"pong");
    assert!(run.time_to_headers <= run.total_time);
    assert!(
        run.headers
            .get("x-wirebolt-test")
            .is_some_and(|value| value == "yes")
    );
    assert_eq!(run.bytes_decoded, 4);
    assert_eq!(run.content_encoding, None);
    assert_eq!(run.remote_addr, Some(address));

    let raw_request = received_request.await.expect("HTTP server task");
    let raw_request = String::from_utf8(raw_request).expect("ASCII request");
    assert!(raw_request.starts_with("POST /echo?from=wirebolt HTTP/1.1\r\n"));
    assert!(raw_request.contains("x-client: wirebolt\r\n"));
    assert!(raw_request.contains("user-agent: Wirebolt/0.1.0\r\n"));
    assert!(raw_request.contains("accept: */*\r\n"));
    assert!(raw_request.contains("accept-encoding: gzip, deflate, br, zstd\r\n"));
    assert!(raw_request.ends_with("\r\n\r\nhello"));
}

#[tokio::test]
async fn streams_a_file_upload_from_disk() {
    let (address, received_request) =
        spawn_http1_once(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await;
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("upload.bin");
    let payload = vec![0x5a; 512 * 1024];
    std::fs::write(&path, &payload).expect("write upload fixture");
    let mut request = SavedRequest::new(
        DocumentId::new("file-upload").expect("request ID"),
        "File upload",
        "PUT",
        format!("http://{address}/upload"),
    );
    request.body = SavedRequestBody::File {
        path: path.to_string_lossy().into_owned(),
        content_type: Some("application/octet-stream".to_owned()),
    };
    let prepared = RequestPipeline::new(None, &NoSecrets)
        .prepare(&request)
        .expect("prepare streamed upload");

    HttpEngine::new(&HttpEngineConfig::default())
        .expect("HTTP engine")
        .run(
            prepared,
            RunOptions::default(),
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect("send streamed upload");

    let raw = received_request.await.expect("HTTP server task");
    assert!(raw.ends_with(&payload));
    assert!(
        String::from_utf8_lossy(&raw[..raw.len() - payload.len()])
            .contains("content-length: 524288\r\n")
    );
}

#[tokio::test]
async fn streams_a_chunked_response_without_building_a_body() {
    let (address, server) = spawn_http1_script(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        vec![
            (Duration::ZERO, b"5\r\nalpha\r\n"),
            (Duration::from_millis(20), b"4\r\nbeta\r\n"),
            (Duration::from_millis(20), b"0\r\n\r\n"),
        ],
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let request = get_request(address, "/stream");
    let mut chunks = Vec::new();

    let run = engine
        .run(
            request,
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                chunks.push(chunk.to_vec());
                StreamControl::Continue
            },
        )
        .await
        .expect("successful streamed run");

    assert_eq!(run.bytes_received, 9);
    assert_eq!(chunks.concat(), b"alphabeta");
    assert!(chunks.len() >= 2, "delayed body chunks were not streamed");
    server.await.expect("HTTP server task");
}

#[tokio::test]
async fn rejects_a_response_before_delivering_a_chunk_past_the_byte_limit() {
    let (address, server) = spawn_http1_script(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        vec![
            (Duration::ZERO, b"4\r\n1234\r\n"),
            (Duration::from_millis(20), b"4\r\n5678\r\n"),
            (Duration::ZERO, b"0\r\n\r\n"),
        ],
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let mut delivered = Vec::new();

    let error = engine
        .run(
            get_request(address, "/bounded"),
            RunOptions {
                max_response_bytes: Some(5),
                ..RunOptions::default()
            },
            &RunCancellation::new(),
            |chunk| {
                delivered.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect_err("response must exceed the limit");

    assert_eq!(error.kind(), RunErrorKind::ResponseTooLarge);
    assert_eq!(error.response_limit(), Some(5));
    assert_eq!(delivered, b"1234");
    server.await.expect("HTTP server task");
}

#[tokio::test]
async fn stops_streaming_when_the_caller_requests_it() {
    let (address, server) = spawn_http1_script(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        vec![
            (Duration::ZERO, b"5\r\nfirst\r\n"),
            (Duration::from_millis(20), b"6\r\nsecond\r\n"),
            (Duration::ZERO, b"0\r\n\r\n"),
        ],
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let mut callback_count = 0;

    let error = engine
        .run(
            get_request(address, "/stop"),
            RunOptions::default(),
            &RunCancellation::new(),
            |_| {
                callback_count += 1;
                StreamControl::Stop
            },
        )
        .await
        .expect_err("caller stopped the response stream");

    assert_eq!(error.kind(), RunErrorKind::StreamStopped);
    assert_eq!(callback_count, 1);
    server.await.expect("HTTP server task");
}

#[tokio::test]
async fn enforces_the_read_timeout_between_body_chunks() {
    let (address, server) = spawn_http1_script(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        vec![
            (Duration::ZERO, b"5\r\nfirst\r\n"),
            (Duration::from_millis(100), b"6\r\nsecond\r\n"),
        ],
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");

    let error = engine
        .run(
            get_request(address, "/slow-body"),
            RunOptions {
                read_timeout: Some(Duration::from_millis(20)),
                ..RunOptions::default()
            },
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("body must time out");

    assert_eq!(error.kind(), RunErrorKind::ReadTimeout);
    server.abort();
    assert!(server.await.expect_err("server was aborted").is_cancelled());
}

#[tokio::test]
async fn enforces_the_total_timeout_before_response_headers() {
    let (address, server) = spawn_stalled_http1_server().await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");

    let error = engine
        .run(
            get_request(address, "/slow-headers"),
            RunOptions {
                total_timeout: Some(Duration::from_millis(20)),
                ..RunOptions::default()
            },
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("request must reach its total timeout");

    assert_eq!(error.kind(), RunErrorKind::TotalTimeout);
    server.abort();
    assert!(server.await.expect_err("server was aborted").is_cancelled());
}

#[tokio::test]
async fn the_run_read_timeout_covers_the_wait_for_headers() {
    let (address, server) = spawn_stalled_http1_server().await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");

    let error = engine
        .run(
            get_request(address, "/silent-headers"),
            RunOptions {
                read_timeout: Some(Duration::from_millis(30)),
                total_timeout: Some(Duration::from_secs(5)),
                ..RunOptions::default()
            },
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("a silent server must trip the per-run stall guard");

    assert_eq!(error.kind(), RunErrorKind::ReadTimeout);
    server.abort();
    let _ = server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_that_keeps_moving_is_not_a_stall() {
    // Larger than the most the kernel can buffer for one loopback socket
    // pair (macOS caps it at 8 MiB), so most of the upload is gated by the
    // server's reads rather than absorbed before the first read.
    const BODY_BYTES: usize = 32 * 1024 * 1024;
    let (address, server) = spawn_slow_reading_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nuploaded",
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let request = prepare_request(RequestDraft {
        method: "PUT".to_owned(),
        url: format!("http://{address}/upload"),
        headers: Vec::new(),
        body: vec![b'x'; BODY_BYTES],
    })
    .expect("prepared request");
    let mut body = Vec::new();

    let run = engine
        .run(
            request,
            RunOptions {
                read_timeout: Some(Duration::from_millis(250)),
                total_timeout: Some(Duration::from_secs(30)),
                ..RunOptions::default()
            },
            &RunCancellation::new(),
            |chunk| {
                body.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect("a slow but moving upload must not time out");

    assert_eq!(body, b"uploaded");
    assert!(
        run.time_to_headers > Duration::from_millis(250),
        "the upload finished too quickly to prove anything: {:?}",
        run.time_to_headers
    );
    assert_eq!(server.await.expect("slow server task"), BODY_BYTES);
}

#[tokio::test]
async fn disabled_timeouts_leave_cancellation_in_charge() {
    let (address, server) = spawn_stalled_http1_server().await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let cancellation = RunCancellation::new();
    let trigger = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.cancel();
    });

    let error = engine
        .run(
            get_request(address, "/no-deadline"),
            RunOptions {
                read_timeout: None,
                total_timeout: None,
                ..RunOptions::default()
            },
            &cancellation,
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("only cancellation can end the run");

    assert_eq!(error.kind(), RunErrorKind::Cancelled);
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn cancels_a_run_while_waiting_for_response_headers() {
    let (address, server) = spawn_stalled_http1_server().await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let cancellation = RunCancellation::new();
    let cancellation_trigger = cancellation.clone();
    let trigger = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancellation_trigger.cancel();
    });

    let error = engine
        .run(
            get_request(address, "/cancel"),
            RunOptions::default(),
            &cancellation,
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("request must be cancelled");

    assert_eq!(error.kind(), RunErrorKind::Cancelled);
    assert!(cancellation.is_cancelled());
    trigger.await.expect("cancellation task");
    server.abort();
    assert!(server.await.expect_err("server was aborted").is_cancelled());
}

#[tokio::test]
async fn leaves_redirects_visible_to_the_caller() {
    let (address, server) = spawn_http1_once(
        b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/not-followed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");

    let run = engine
        .run(
            get_request(address, "/redirect"),
            RunOptions::default(),
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect("redirect response");

    assert_eq!(run.status, 302);
    server.await.expect("HTTP server task");
}

#[tokio::test]
async fn classifies_a_refused_connection_without_exposing_the_url() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve loopback address");
    let address = listener.local_addr().expect("loopback address");
    drop(listener);
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");

    let error = engine
        .run(
            get_request(address, "/private-value"),
            RunOptions::default(),
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("connection must be refused");

    assert_eq!(error.kind(), RunErrorKind::Connection);
    assert!(!format!("{error:?}").contains("private-value"));
}

#[tokio::test]
async fn rejects_an_invalid_tls_peer_without_exposing_the_url() {
    let (address, server) = spawn_invalid_tls_server().await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let request = prepare_request(RequestDraft {
        method: "GET".to_owned(),
        url: format!("https://{address}/private-value"),
        headers: Vec::new(),
        body: Vec::new(),
    })
    .expect("prepared HTTPS request");

    let error = engine
        .run(
            request,
            RunOptions::default(),
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("invalid TLS peer must fail");

    assert_eq!(error.kind(), RunErrorKind::Connection);
    assert!(!format!("{error:?}").contains("private-value"));
    server.await.expect("invalid TLS server task");
}

#[tokio::test]
async fn runs_an_http2_prior_knowledge_request() {
    let (address, server) = spawn_http2_once().await;
    let engine = HttpEngine::new(&HttpEngineConfig {
        version_policy: HttpVersionPolicy::Http2PriorKnowledge,
        ..HttpEngineConfig::default()
    })
    .expect("HTTP/2 engine");
    let mut body = Vec::new();

    let run = engine
        .run(
            get_request(address, "/http2"),
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                body.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect("HTTP/2 run");

    assert_eq!(run.status, 200);
    assert_eq!(run.version, HttpVersion::Http2);
    assert_eq!(body, b"h2-pong");
    server.abort();
    assert!(
        server
            .await
            .expect_err("pooled HTTP/2 connection was closed")
            .is_cancelled()
    );
}

#[tokio::test]
async fn routes_an_http_request_through_a_manual_proxy() {
    let (proxy_address, proxy) = spawn_http1_once(
        b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nproxy-ok",
    )
    .await;
    let manual = ManualProxy::new(vec![ProxyRoute::new(
        ProxyDestination::All,
        ProxyEndpoint::new(&format!("http://{proxy_address}")).expect("proxy endpoint"),
    )])
    .expect("manual proxy");
    let resolved = ProxyPolicy::with_workspace(ProxyMode::Manual(manual)).resolve(None);
    let engine = HttpEngine::with_proxy(&HttpEngineConfig::default(), &resolved, &NoSecrets)
        .expect("proxied HTTP engine");
    let mut body = Vec::new();

    let run = engine
        .run(
            get_request("127.0.0.1:9".parse().expect("unused target"), "/proxied"),
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                body.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect("manual proxy response");

    assert_eq!(run.status, 200);
    assert_eq!(body, b"proxy-ok");
    let raw_request =
        String::from_utf8(proxy.await.expect("HTTP proxy task")).expect("ASCII proxy request");
    assert!(raw_request.starts_with("GET http://127.0.0.1:9/proxied HTTP/1.1\r\n"));
}

#[tokio::test]
async fn direct_request_override_bypasses_the_workspace_proxy() {
    let proxy_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unused proxy");
    let proxy_address = proxy_listener.local_addr().expect("unused proxy address");
    let (target_address, target) = spawn_http1_once(
        b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\ndirect-ok",
    )
    .await;
    let manual = ManualProxy::new(vec![ProxyRoute::new(
        ProxyDestination::All,
        ProxyEndpoint::new(&format!("http://{proxy_address}")).expect("proxy endpoint"),
    )])
    .expect("manual proxy");
    let policy = ProxyPolicy::with_workspace(ProxyMode::Manual(manual));
    let resolved = policy.resolve(Some(&ProxyMode::Direct));
    let engine = HttpEngine::with_proxy(&HttpEngineConfig::default(), &resolved, &NoSecrets)
        .expect("direct HTTP engine");
    let mut body = Vec::new();

    engine
        .run(
            get_request(target_address, "/direct"),
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                body.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect("direct response");

    assert_eq!(body, b"direct-ok");
    target.await.expect("direct target task");
    assert!(
        tokio::time::timeout(Duration::from_millis(20), proxy_listener.accept())
            .await
            .is_err(),
        "direct mode contacted the proxy"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn system_mode_honors_process_proxy_settings() {
    let (proxy_address, proxy) = spawn_http1_once(
        b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nsystem-ok",
    )
    .await;
    let test_binary = std::env::current_exe().expect("integration test binary");
    let child = tokio::task::spawn_blocking(move || {
        Command::new(test_binary)
            .args(["--exact", "system_proxy_child", "--nocapture"])
            .env(
                "WIREBOLT_SYSTEM_PROXY_TEST_URL",
                "http://wirebolt.invalid/system",
            )
            .env("http_proxy", format!("http://{proxy_address}"))
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("https_proxy")
            .env_remove("ALL_PROXY")
            .env_remove("all_proxy")
            .env_remove("NO_PROXY")
            .env_remove("no_proxy")
            .env_remove("REQUEST_METHOD")
            .output()
            .expect("run isolated system proxy test")
    })
    .await
    .expect("system proxy child task");

    assert!(
        child.status.success(),
        "system proxy child failed:\n{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    let raw_request =
        String::from_utf8(proxy.await.expect("system proxy task")).expect("ASCII proxy request");
    assert!(raw_request.starts_with("GET http://wirebolt.invalid/system HTTP/1.1\r\n"));
}

#[tokio::test]
async fn system_proxy_child() {
    let Ok(url) = std::env::var("WIREBOLT_SYSTEM_PROXY_TEST_URL") else {
        return;
    };
    let resolved = ProxyPolicy::default().resolve(None);
    let engine = HttpEngine::with_proxy(&HttpEngineConfig::default(), &resolved, &NoSecrets)
        .expect("system proxy engine");
    let mut body = Vec::new();

    engine
        .run(
            get_url(&url),
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                body.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect("system proxy response");

    assert_eq!(body, b"system-ok");
}

#[tokio::test]
async fn loads_manual_proxy_credentials_without_exposing_them() {
    let (proxy_address, proxy) =
        spawn_http1_once(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await;
    let credentials = ProxyCredentials::new(
        SecretName::new("proxy.username").expect("username secret name"),
        SecretName::new("proxy.password").expect("password secret name"),
    );
    let route = ProxyRoute::new(
        ProxyDestination::All,
        ProxyEndpoint::new(&format!("http://{proxy_address}")).expect("proxy endpoint"),
    )
    .with_credentials(credentials);
    let manual = ManualProxy::new(vec![route]).expect("manual proxy");
    let resolved = ProxyPolicy::with_workspace(ProxyMode::Manual(manual)).resolve(None);
    let engine = HttpEngine::with_proxy(&HttpEngineConfig::default(), &resolved, &TestSecrets)
        .expect("authenticated proxy engine");

    engine
        .run(
            get_request(
                "127.0.0.1:9".parse().expect("unused target"),
                "/authenticated",
            ),
            RunOptions::default(),
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect("authenticated proxy response");

    let raw_request =
        String::from_utf8(proxy.await.expect("HTTP proxy task")).expect("ASCII proxy request");
    assert!(
        raw_request.contains("proxy-authorization: Basic cHJveHktdXNlcjpzdXBlci1zZWNyZXQ=\r\n")
    );
    assert!(resolved.diagnostic().routes()[0].authenticated());
    assert!(!format!("{resolved:?}{engine:?}").contains("super-secret"));
}

#[test]
fn reports_the_missing_proxy_secret_by_reference_name() {
    let username = SecretName::new("missing.proxy.username").expect("username secret name");
    let credentials = ProxyCredentials::new(
        username.clone(),
        SecretName::new("missing.proxy.password").expect("password secret name"),
    );
    let route = ProxyRoute::new(
        ProxyDestination::All,
        ProxyEndpoint::new("http://127.0.0.1:8080").expect("proxy endpoint"),
    )
    .with_credentials(credentials);
    let manual = ManualProxy::new(vec![route]).expect("manual proxy");
    let resolved = ProxyPolicy::with_workspace(ProxyMode::Manual(manual)).resolve(None);

    let error = HttpEngine::with_proxy(&HttpEngineConfig::default(), &resolved, &NoSecrets)
        .expect_err("missing proxy secret must fail");

    assert_eq!(error.kind(), RunErrorKind::Proxy);
    let proxy_error = error
        .proxy_configuration_error()
        .expect("proxy configuration error");
    assert_eq!(
        proxy_error.kind(),
        ProxyConfigurationErrorKind::SecretNotFound
    );
    assert_eq!(proxy_error.secret_name(), Some(&username));
}

#[tokio::test]
async fn routes_a_request_through_a_socks5_proxy_with_remote_dns() {
    let (proxy_address, proxy) = spawn_socks5_once().await;
    let manual = ManualProxy::new(vec![ProxyRoute::new(
        ProxyDestination::All,
        ProxyEndpoint::new(&format!("socks5h://{proxy_address}")).expect("SOCKS endpoint"),
    )])
    .expect("manual SOCKS proxy");
    let resolved = ProxyPolicy::with_workspace(ProxyMode::Manual(manual)).resolve(None);
    let engine = HttpEngine::with_proxy(&HttpEngineConfig::default(), &resolved, &NoSecrets)
        .expect("SOCKS HTTP engine");
    let mut body = Vec::new();

    engine
        .run(
            get_url("http://wirebolt.invalid:8080/socks"),
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                body.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect("SOCKS response");

    assert_eq!(body, b"socks-ok");
    let (host, port, request) = proxy.await.expect("SOCKS proxy task");
    assert_eq!(host, "wirebolt.invalid");
    assert_eq!(port, 8080);
    assert!(request.starts_with(b"GET /socks HTTP/1.1\r\n"));
}

#[tokio::test]
async fn tunnels_an_https_request_through_the_https_proxy_route() {
    let (proxy_address, proxy) = spawn_connect_proxy_once().await;
    let manual = ManualProxy::new(vec![ProxyRoute::new(
        ProxyDestination::Https,
        ProxyEndpoint::new(&format!("http://{proxy_address}")).expect("proxy endpoint"),
    )])
    .expect("manual HTTPS proxy");
    let resolved = ProxyPolicy::with_workspace(ProxyMode::Manual(manual)).resolve(None);
    let engine = HttpEngine::with_proxy(&HttpEngineConfig::default(), &resolved, &NoSecrets)
        .expect("HTTPS proxy engine");

    let error = engine
        .run(
            get_url("https://wirebolt.invalid/through-connect"),
            RunOptions::default(),
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("test proxy refuses the tunnel");

    assert_eq!(error.kind(), RunErrorKind::Connection);
    let request = proxy.await.expect("CONNECT proxy task");
    assert!(request.starts_with(b"CONNECT wirebolt.invalid:443 HTTP/1.1\r\n"));
}

#[tokio::test]
async fn decodes_gzip_bodies_while_keeping_the_wire_headers() {
    let wire = gzip(b"compressed body");
    let (address, received_request) = spawn_http1_once(gzip_response(&wire)).await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let mut body = Vec::new();

    let run = engine
        .run(
            get_request(address, "/gzip"),
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                body.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect("decoded run");

    assert_eq!(body, b"compressed body");
    assert_eq!(run.content_encoding, Some(ContentEncoding::Gzip));
    assert_eq!(run.bytes_received, wire.len() as u64);
    assert_eq!(run.bytes_decoded, 15);
    assert_eq!(run.headers["content-encoding"], "gzip");
    assert_eq!(run.headers["content-length"], wire.len().to_string());
    let raw_request = String::from_utf8(received_request.await.expect("HTTP server task"))
        .expect("ASCII request");
    assert!(raw_request.contains("accept-encoding: gzip, deflate, br, zstd\r\n"));
}

#[tokio::test]
async fn an_explicit_accept_encoding_keeps_the_body_verbatim() {
    let wire = gzip(b"compressed body");
    let (address, _) = spawn_http1_once(gzip_response(&wire)).await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let request = prepare_request(RequestDraft {
        method: "GET".to_owned(),
        url: format!("http://{address}/raw"),
        headers: vec![HeaderField {
            name: "accept-encoding".to_owned(),
            value: "gzip".to_owned(),
        }],
        body: Vec::new(),
    })
    .expect("prepared request");
    let mut body = Vec::new();

    let run = engine
        .run(
            request,
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                body.extend_from_slice(chunk);
                StreamControl::Continue
            },
        )
        .await
        .expect("verbatim run");

    assert_eq!(body, wire);
    assert_eq!(run.content_encoding, None);
    assert_eq!(run.bytes_decoded, run.bytes_received);
}

#[tokio::test]
async fn rejects_an_oversized_content_length_before_reading_the_body() {
    let (address, server) = spawn_http1_script(
        b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n",
        vec![(Duration::from_millis(50), b"0123456789")],
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let mut saw_headers = false;
    let mut delivered = 0;

    let error = engine
        .run_observed(
            get_request(address, "/huge"),
            RunOptions {
                max_response_bytes: Some(10),
                ..RunOptions::default()
            },
            &RunCancellation::new(),
            |head| {
                assert_eq!(head.status, 200);
                saw_headers = true;
            },
            |_| {
                delivered += 1;
                StreamControl::Continue
            },
        )
        .await
        .expect_err("declared length exceeds the limit");

    assert_eq!(error.kind(), RunErrorKind::ResponseTooLarge);
    assert!(saw_headers, "headers are still reported before failing");
    assert_eq!(
        delivered, 0,
        "no body chunk is read once the length is known"
    );
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn a_connect_timeout_is_not_reported_as_the_total_timeout() {
    let engine = HttpEngine::new(&HttpEngineConfig {
        connect_timeout: Duration::from_millis(50),
        ..HttpEngineConfig::default()
    })
    .expect("HTTP engine");

    // TEST-NET-1 is never routable; the connect either times out or is
    // refused by the local stack, never reaching the 5 s total deadline.
    let error = engine
        .run(
            get_url("http://192.0.2.1:81/blackhole"),
            RunOptions {
                total_timeout: Some(Duration::from_secs(5)),
                ..RunOptions::default()
            },
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("connect must fail");

    assert!(
        matches!(
            error.kind(),
            RunErrorKind::ConnectTimeout | RunErrorKind::Connection
        ),
        "{:?}",
        error.kind()
    );
}

#[tokio::test]
async fn the_engine_read_timeout_covers_the_wait_for_headers() {
    let (address, server) = spawn_stalled_http1_server().await;
    let engine = HttpEngine::new(&HttpEngineConfig {
        read_timeout: Some(Duration::from_millis(30)),
        ..HttpEngineConfig::default()
    })
    .expect("HTTP engine");

    let error = engine
        .run(
            get_request(address, "/silent"),
            RunOptions {
                total_timeout: Some(Duration::from_secs(5)),
                read_timeout: None,
                ..RunOptions::default()
            },
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect_err("silent server must trip the stall guard");

    assert_eq!(error.kind(), RunErrorKind::ReadTimeout);
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn decodes_brotli_and_zstd_bodies_split_across_chunks() {
    for (encoding, wire, expected) in [
        ("br", brotli_fixture(), &b"wirebolt brotli body"[..]),
        (
            "zstd",
            zstd::encode_all(&b"wirebolt zstd body"[..], 3).expect("zstd encode"),
            &b"wirebolt zstd body"[..],
        ),
    ] {
        let split = wire.len() / 2;
        let (head, tail) = wire.split_at(split);
        let (address, server) = spawn_http1_script(
            leak(format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Encoding: {encoding}\r\nConnection: close\r\n\r\n",
                wire.len()
            )),
            vec![
                (Duration::ZERO, leak(head.to_vec())),
                (Duration::from_millis(20), leak(tail.to_vec())),
            ],
        )
        .await;
        let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
        let mut body = Vec::new();

        let run = engine
            .run(
                get_request(address, "/split"),
                RunOptions::default(),
                &RunCancellation::new(),
                |chunk| {
                    body.extend_from_slice(chunk);
                    StreamControl::Continue
                },
            )
            .await
            .expect("decoded run");

        assert_eq!(body, expected, "{encoding}");
        assert_eq!(run.bytes_received, wire.len() as u64);
        assert_eq!(run.bytes_decoded, expected.len() as u64);
        server.await.expect("HTTP server task");
    }
}

#[tokio::test]
async fn a_truncated_compressed_body_is_a_response_body_error() {
    for (encoding, wire) in [
        ("gzip", gzip(b"a gzip body that is long enough to cut")),
        ("br", brotli_fixture()),
        (
            "zstd",
            zstd::encode_all(&b"a zstd body that is long enough to cut"[..], 3).expect("encode"),
        ),
    ] {
        let truncated = &wire[..wire.len() - 4];
        let response = leak(
            [
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Encoding: {encoding}\r\nConnection: close\r\n\r\n",
                    truncated.len()
                )
                .into_bytes(),
                truncated.to_vec(),
            ]
            .concat(),
        );
        let (address, _) = spawn_http1_once(response).await;
        let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");

        let error = engine
            .run(
                get_request(address, "/truncated"),
                RunOptions::default(),
                &RunCancellation::new(),
                |_| StreamControl::Continue,
            )
            .await
            .expect_err("a truncated stream must not look complete");

        assert_eq!(error.kind(), RunErrorKind::ResponseBody, "{encoding}");
    }
}

#[tokio::test]
async fn a_compression_bomb_fails_at_the_cap_not_after_expanding() {
    const DECODED_BYTES: usize = 32 * 1024 * 1024;
    const CAP: u64 = 1024 * 1024;
    let wire = gzip(&vec![0; DECODED_BYTES]);
    assert!(wire.len() < 128 * 1024, "fixture is not a bomb");
    let (address, _) = spawn_http1_once(gzip_response(&wire)).await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let mut delivered = 0_u64;

    let error = engine
        .run(
            get_request(address, "/bomb"),
            RunOptions {
                max_response_bytes: Some(CAP),
                ..RunOptions::default()
            },
            &RunCancellation::new(),
            |chunk| {
                delivered += chunk.len() as u64;
                StreamControl::Continue
            },
        )
        .await
        .expect_err("decoded size must exceed the cap");

    assert_eq!(error.kind(), RunErrorKind::ResponseTooLarge);
    assert_eq!(error.response_limit(), Some(CAP));
    assert!(delivered <= CAP, "delivered {delivered} bytes past the cap");
}

#[tokio::test]
async fn bodyless_responses_with_a_content_encoding_are_not_decode_failures() {
    for (status, response) in [
        (
            204,
            &b"HTTP/1.1 204 No Content\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\n"[..],
        ),
        (
            304,
            &b"HTTP/1.1 304 Not Modified\r\nContent-Encoding: br\r\nContent-Length: 512\r\nConnection: close\r\n\r\n"[..],
        ),
    ] {
        let (address, server) = spawn_http1_once(response).await;
        let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");

        let run = engine
            .run(
                get_request(address, "/empty"),
                RunOptions::default(),
                &RunCancellation::new(),
                |_| StreamControl::Continue,
            )
            .await
            .expect("a response without a body has nothing to decode");

        assert_eq!(run.status, status);
        assert_eq!(run.bytes_received, 0);
        assert_eq!(run.bytes_decoded, 0);
        server.await.expect("HTTP server task");
    }
}

#[tokio::test]
async fn a_head_response_with_a_content_encoding_is_not_a_decode_failure() {
    let (address, server) = spawn_http1_once(
        b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n",
    )
    .await;
    let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");
    let request = prepare_request(RequestDraft {
        method: "HEAD".to_owned(),
        url: format!("http://{address}/head"),
        headers: Vec::new(),
        body: Vec::new(),
    })
    .expect("prepared request");

    let run = engine
        .run(
            request,
            RunOptions::default(),
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .expect("HEAD carries no body to decode");

    assert_eq!(run.status, 200);
    assert_eq!(run.bytes_received, 0);
    server.await.expect("HTTP server task");
}

#[tokio::test]
async fn a_compressed_body_cut_before_its_first_byte_is_truncated() {
    let cases = [
        ("gzip", ""),
        ("deflate", ""),
        ("br", ""),
        ("zstd", ""),
        // A declared empty body is still no compressed stream at all.
        ("zstd", "Content-Length: 0\r\n"),
        ("gzip", "Content-Length: 0\r\n"),
    ];
    for (encoding, length) in cases {
        let response = leak(format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: {encoding}\r\n{length}Connection: close\r\n\r\n"
        ));
        let (address, server) = spawn_http1_once(response).await;
        let engine = HttpEngine::new(&HttpEngineConfig::default()).expect("HTTP engine");

        let error = engine
            .run(
                get_request(address, "/cut"),
                RunOptions::default(),
                &RunCancellation::new(),
                |_| StreamControl::Continue,
            )
            .await
            .expect_err("a 200 whose compressed body never arrived is truncated");

        assert_eq!(
            error.kind(),
            RunErrorKind::ResponseBody,
            "{encoding} {length:?}"
        );
        server.await.expect("HTTP server task");
    }
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write as _;

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).expect("gzip body");
    encoder.finish().expect("finish gzip body")
}

/// `brotli -c` of `wirebolt brotli body`; brotli-decompressor ships no encoder.
fn brotli_fixture() -> Vec<u8> {
    vec![
        0x8f, 0x09, 0x80, 0x77, 0x69, 0x72, 0x65, 0x62, 0x6f, 0x6c, 0x74, 0x20, 0x62, 0x72, 0x6f,
        0x74, 0x6c, 0x69, 0x20, 0x62, 0x6f, 0x64, 0x79, 0x03,
    ]
}

fn leak(bytes: impl Into<Vec<u8>>) -> &'static [u8] {
    Box::leak(bytes.into().into_boxed_slice())
}

fn gzip_response(wire: &[u8]) -> &'static [u8] {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\n",
        wire.len()
    );
    leak([head.as_bytes(), wire].concat())
}

#[derive(Debug)]
struct TestSecrets;

impl SecretResolver for TestSecrets {
    fn resolve(&self, name: &SecretName) -> Result<ResolvedSecret, SecretResolutionError> {
        match name.as_str() {
            "proxy.username" => Ok(ResolvedSecret::new("proxy-user")),
            "proxy.password" => Ok(ResolvedSecret::new("super-secret")),
            _ => Err(SecretResolutionError::new(
                SecretResolutionErrorKind::NotFound,
            )),
        }
    }
}

async fn spawn_http1_once(response: &'static [u8]) -> (SocketAddr, JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback server");
    let address = listener.local_addr().expect("server address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        let request = read_http1_request(&mut stream).await;
        stream.write_all(response).await.expect("write response");
        stream.shutdown().await.expect("close response");
        request
    });
    (address, task)
}

async fn spawn_http1_script(
    response_headers: &'static [u8],
    writes: Vec<(Duration, &'static [u8])>,
) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback server");
    let address = listener.local_addr().expect("server address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        read_http1_request(&mut stream).await;
        stream
            .write_all(response_headers)
            .await
            .expect("write response headers");
        for (delay, bytes) in writes {
            tokio::time::sleep(delay).await;
            if stream.write_all(bytes).await.is_err() {
                return;
            }
        }
        let _ = stream.shutdown().await;
    });
    (address, task)
}

/// Reads the request body 256 KiB at a time with a pause between reads, so
/// the client's upload is throttled by backpressure rather than finishing
/// into kernel buffers at once. Returns the body bytes it consumed.
async fn spawn_slow_reading_server(response: &'static [u8]) -> (SocketAddr, JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback server");
    let address = listener.local_addr().expect("server address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        let mut buffer = vec![0_u8; 256 * 1024];
        let mut received = Vec::new();
        let mut body_start = None;
        let mut content_length = 0;
        let mut total = 0;
        loop {
            let read = stream.read(&mut buffer).await.expect("read request");
            assert!(read > 0, "connection closed before the upload completed");
            total += read;
            if body_start.is_none() {
                received.extend_from_slice(&buffer[..read]);
                if let Some(end) = received.windows(4).position(|window| window == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&received[..end]);
                    content_length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .expect("content-length");
                    body_start = Some(end + 4);
                }
            }
            if body_start.is_some_and(|start| total >= start + content_length) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        stream.write_all(response).await.expect("write response");
        let _ = stream.shutdown().await;
        total - body_start.expect("request head")
    });
    (address, task)
}

async fn spawn_stalled_http1_server() -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback server");
    let address = listener.local_addr().expect("server address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        read_http1_request(&mut stream).await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    (address, task)
}

async fn spawn_http2_once() -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind HTTP/2 loopback server");
    let address = listener.local_addr().expect("HTTP/2 server address");
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept HTTP/2 request");
        let service = service_fn(|request: Request<Incoming>| async move {
            assert_eq!(request.version(), hyper::Version::HTTP_2);
            assert_eq!(request.uri().path(), "/http2");
            Ok::<_, Infallible>(
                Response::builder()
                    .header("x-wirebolt-protocol", "h2")
                    .body(Full::new(Bytes::from_static(b"h2-pong")))
                    .expect("HTTP/2 response"),
            )
        });
        hyper::server::conn::http2::Builder::new(TokioExecutor::new())
            .serve_connection(TokioIo::new(stream), service)
            .await
            .expect("serve HTTP/2 connection");
    });
    (address, task)
}

async fn spawn_invalid_tls_server() -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind invalid TLS server");
    let address = listener.local_addr().expect("invalid TLS server address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept TLS connection");
        let mut client_hello = [0_u8; 4_096];
        let read = stream
            .read(&mut client_hello)
            .await
            .expect("read TLS client hello");
        assert!(read > 0, "TLS client sent no bytes");
        stream
            .write_all(b"this is not TLS")
            .await
            .expect("write invalid TLS response");
        let _ = stream.shutdown().await;
    });
    (address, task)
}

async fn spawn_socks5_once() -> (SocketAddr, JoinHandle<(String, u16, Vec<u8>)>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind SOCKS5 proxy");
    let address = listener.local_addr().expect("SOCKS5 proxy address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept SOCKS5 client");

        let mut greeting = [0_u8; 2];
        stream
            .read_exact(&mut greeting)
            .await
            .expect("read SOCKS5 greeting");
        assert_eq!(greeting[0], 5);
        let mut methods = vec![0_u8; usize::from(greeting[1])];
        stream
            .read_exact(&mut methods)
            .await
            .expect("read SOCKS5 methods");
        assert!(methods.contains(&0), "SOCKS5 client omitted no-auth method");
        stream
            .write_all(&[5, 0])
            .await
            .expect("select SOCKS5 no-auth");

        let mut request_head = [0_u8; 4];
        stream
            .read_exact(&mut request_head)
            .await
            .expect("read SOCKS5 request");
        assert_eq!(&request_head[..3], &[5, 1, 0]);
        assert_eq!(request_head[3], 3, "socks5h must resolve DNS remotely");
        let domain_length = stream.read_u8().await.expect("read SOCKS5 domain length");
        let mut domain = vec![0_u8; usize::from(domain_length)];
        stream
            .read_exact(&mut domain)
            .await
            .expect("read SOCKS5 domain");
        let port = stream.read_u16().await.expect("read SOCKS5 port");
        stream
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
            .await
            .expect("accept SOCKS5 connection");

        let request = read_http1_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nsocks-ok")
            .await
            .expect("write SOCKS5 response");
        let _ = stream.shutdown().await;
        (
            String::from_utf8(domain).expect("ASCII SOCKS5 domain"),
            port,
            request,
        )
    });
    (address, task)
}

async fn spawn_connect_proxy_once() -> (SocketAddr, JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind CONNECT proxy");
    let address = listener.local_addr().expect("CONNECT proxy address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept CONNECT client");
        let request = read_http1_request(&mut stream).await;
        stream
            .write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("refuse CONNECT tunnel");
        let _ = stream.shutdown().await;
        request
    });
    (address, task)
}

fn get_request(address: SocketAddr, path: &str) -> wirebolt_core::PreparedRequest {
    get_url(&format!("http://{address}{path}"))
}

fn get_url(url: &str) -> wirebolt_core::PreparedRequest {
    prepare_request(RequestDraft {
        method: "GET".to_owned(),
        url: url.to_owned(),
        headers: Vec::new(),
        body: Vec::new(),
    })
    .expect("prepared request")
}

async fn read_http1_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4_096];
    loop {
        let read = stream.read(&mut buffer).await.expect("read request");
        assert!(read > 0, "connection closed before request completed");
        request.extend_from_slice(&buffer[..read]);

        if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = header_end + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .unwrap_or(0);
            if request.len() >= header_end + content_length {
                return request;
            }
        }
    }
}

#[tokio::test]
async fn redirect_cookies_are_sent_captured_and_isolated_between_runs() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in [
            &b"HTTP/1.1 302 Found\r\nLocation: /final\r\nSet-Cookie: session=fixture; Path=/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"[..],
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_http1_request(&mut stream).await);
            stream.write_all(response).await.unwrap();
            stream.shutdown().await.unwrap();
        }
        requests
    });
    let engine = HttpEngine::new(&HttpEngineConfig {
        maximum_redirects: Some(3),
        ..Default::default()
    })
    .unwrap();
    let mut updates = Vec::new();
    engine
        .run_observed_with_cookies(
            get_request(address, "/start"),
            RunOptions::default(),
            &RunCancellation::new(),
            |_| {},
            |_| StreamControl::Continue,
            |url, headers| updates.push((url.clone(), headers.clone())),
        )
        .await
        .unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0.path(), "/start");
    assert_eq!(updates[0].1["set-cookie"], "session=fixture; Path=/");
    engine
        .run(
            get_request(address, "/separate-run"),
            RunOptions::default(),
            &RunCancellation::new(),
            |_| StreamControl::Continue,
        )
        .await
        .unwrap();
    let requests = server.await.unwrap();
    assert!(String::from_utf8_lossy(&requests[1]).contains("cookie: session=fixture"));
    assert!(!String::from_utf8_lossy(&requests[2]).contains("cookie:"));
}

#[tokio::test]
async fn cookies_are_reported_even_when_redirect_limit_fails() {
    let (address, server) = spawn_http1_once(b"HTTP/1.1 302 Found\r\nLocation: /again\r\nSet-Cookie: session=fixture; Path=/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    let engine = HttpEngine::new(&HttpEngineConfig {
        maximum_redirects: Some(0),
        ..Default::default()
    })
    .unwrap();
    let mut count = 0;
    let result = engine
        .run_observed_with_cookies(
            get_request(address, "/start"),
            RunOptions::default(),
            &RunCancellation::new(),
            |_| {},
            |_| StreamControl::Continue,
            |_, _| count += 1,
        )
        .await;
    assert!(result.is_err());
    assert_eq!(count, 1);
    server.await.unwrap();
}

#[tokio::test]
async fn concurrent_redirects_do_not_share_cookies_on_a_pooled_engine() {
    async fn fixture(value: &str) -> (SocketAddr, JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: /final\r\nSet-Cookie: isolated={value}; Path=/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let task = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            read_http1_request(&mut first).await;
            first.write_all(response.as_bytes()).await.unwrap();
            first.shutdown().await.unwrap();
            let (mut second, _) = listener.accept().await.unwrap();
            let request = read_http1_request(&mut second).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            second
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            request
        });
        (address, task)
    }
    let (first, first_server) = fixture("a").await;
    let (second, second_server) = fixture("b").await;
    let engine = HttpEngine::new(&HttpEngineConfig {
        maximum_redirects: Some(3),
        ..Default::default()
    })
    .unwrap();
    let cancellation = RunCancellation::new();
    let (a, b) = tokio::join!(
        engine.run(
            get_request(first, "/start"),
            RunOptions::default(),
            &cancellation,
            |_| StreamControl::Continue
        ),
        engine.run(
            get_request(second, "/start"),
            RunOptions::default(),
            &cancellation,
            |_| StreamControl::Continue
        ),
    );
    a.unwrap();
    b.unwrap();
    let a = String::from_utf8(first_server.await.unwrap()).unwrap();
    let b = String::from_utf8(second_server.await.unwrap()).unwrap();
    assert!(a.contains("cookie: isolated=a\r\n"));
    assert!(b.contains("cookie: isolated=b\r\n"));
}
