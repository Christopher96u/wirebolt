use std::{convert::Infallible, net::SocketAddr, time::Duration};

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
    HeaderField, HttpEngine, HttpEngineConfig, HttpVersion, HttpVersionPolicy, RequestDraft,
    RunCancellation, RunErrorKind, RunOptions, StreamControl, prepare_request,
};

#[tokio::test]
async fn runs_an_http1_request_through_the_public_interface() {
    let (address, received_request) = spawn_http1_once(
        b"HTTP/1.1 201 Created\r\nContent-Length: 4\r\nX-Wirebolt-Test: yes\r\nConnection: close\r\n\r\npong",
    )
    .await;
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");
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
        run.headers.iter().any(|header| {
            header.name == "x-wirebolt-test" && header.value.as_slice() == b"yes"
        })
    );

    let raw_request = received_request.await.expect("HTTP server task");
    let raw_request = String::from_utf8(raw_request).expect("ASCII request");
    assert!(raw_request.starts_with("POST /echo?from=wirebolt HTTP/1.1\r\n"));
    assert!(raw_request.contains("x-client: wirebolt\r\n"));
    assert!(raw_request.ends_with("\r\n\r\nhello"));
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
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");
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
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");
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
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");
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
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");

    let error = engine
        .run(
            get_request(address, "/slow-body"),
            RunOptions {
                read_timeout: Duration::from_millis(20),
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
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");

    let error = engine
        .run(
            get_request(address, "/slow-headers"),
            RunOptions {
                total_timeout: Duration::from_millis(20),
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
async fn cancels_a_run_while_waiting_for_response_headers() {
    let (address, server) = spawn_stalled_http1_server().await;
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");
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
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");

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
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");

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
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");
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
    let engine = HttpEngine::new(HttpEngineConfig {
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

fn get_request(address: SocketAddr, path: &str) -> wirebolt_core::PreparedRequest {
    prepare_request(RequestDraft {
        method: "GET".to_owned(),
        url: format!("http://{address}{path}"),
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
