use std::{
    convert::Infallible, hint::black_box, io::Write as _, net::SocketAddr, sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming, service::service_fn};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use wirebolt_core::{
    HttpEngine, HttpEngineConfig, HttpVersionPolicy, RequestDraft, RunCancellation, RunOptions,
    StreamControl, prepare_request,
};

const ONE_KIBIBYTE: usize = 1_024;
const ONE_MEBIBYTE: usize = 1_024 * 1_024;
const EIGHT_MEBIBYTES: usize = 8 * ONE_MEBIBYTE;

#[derive(Clone, Copy)]
enum Protocol {
    Http1,
    Http2,
}

fn benchmark_http_engine(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("benchmark Tokio runtime");
    let bodies = Arc::new(Bodies::new());

    for protocol in [Protocol::Http1, Protocol::Http2] {
        let (address, shutdown, server) =
            runtime.block_on(spawn_server(protocol, Arc::clone(&bodies)));
        let engine = HttpEngine::new(&HttpEngineConfig {
            version_policy: match protocol {
                Protocol::Http1 => HttpVersionPolicy::Http1Only,
                Protocol::Http2 => HttpVersionPolicy::Http2PriorKnowledge,
            },
            ..HttpEngineConfig::default()
        })
        .expect("HTTP engine");
        let group_name = match protocol {
            Protocol::Http1 => "http_engine_loopback_h1",
            Protocol::Http2 => "http_engine_loopback_h2",
        };
        let mut group = c.benchmark_group(group_name);

        for size in [ONE_KIBIBYTE, ONE_MEBIBYTE, EIGHT_MEBIBYTES] {
            let size_u64 = u64::try_from(size).expect("benchmark response size fits in u64");
            let request = get(address, &format!("/plain/{size}"));
            group.throughput(Throughput::Bytes(size_u64));
            group.bench_with_input(BenchmarkId::new("plain", size), &request, |b, request| {
                b.to_async(&runtime)
                    .iter(|| stream_and_count(&engine, request, size_u64));
            });
        }

        // Compressible text is what APIs actually send; the decoded size is
        // the throughput that matters to the user.
        let text_len = u64::try_from(bodies.text.len()).expect("size fits");
        let request = get(address, "/gzip");
        group.throughput(Throughput::Bytes(text_len));
        group.bench_with_input(
            BenchmarkId::new("gzip", bodies.text.len()),
            &request,
            |b, request| {
                b.to_async(&runtime)
                    .iter(|| stream_and_count(&engine, request, text_len));
            },
        );

        group.finish();
        drop(engine);
        shutdown.cancel();
        runtime
            .block_on(server)
            .expect("benchmark server task completed");
    }
}

async fn stream_and_count(
    engine: &HttpEngine,
    request: &wirebolt_core::PreparedRequest,
    expected: u64,
) -> u64 {
    let mut received = 0_u64;
    let run = engine
        .run(
            request.clone(),
            RunOptions::default(),
            &RunCancellation::new(),
            |chunk| {
                received += u64::try_from(chunk.len()).expect("chunk length fits in u64");
                StreamControl::Continue
            },
        )
        .await
        .expect("benchmark request succeeds");
    assert_eq!(received, expected);
    black_box(run.bytes_decoded)
}

fn get(address: SocketAddr, path: &str) -> wirebolt_core::PreparedRequest {
    prepare_request(RequestDraft {
        method: "GET".to_owned(),
        url: format!("http://{address}{path}"),
        headers: Vec::new(),
        body: Vec::new(),
    })
    .expect("benchmark request")
}

struct Bodies {
    small: Bytes,
    medium: Bytes,
    large: Bytes,
    text: Bytes,
    gzip: Bytes,
}

impl Bodies {
    fn new() -> Self {
        let mut text = String::new();
        for index in 0..20_000 {
            use std::fmt::Write as _;
            let _ = write!(
                text,
                r#"{{"id":{index},"name":"Item {index}","active":true}},"#
            );
        }
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(text.as_bytes())
            .expect("gzip benchmark body");
        let gzip = encoder.finish().expect("finish gzip benchmark body");
        Self {
            small: Bytes::from(vec![b'x'; ONE_KIBIBYTE]),
            medium: Bytes::from(vec![b'x'; ONE_MEBIBYTE]),
            large: Bytes::from(vec![b'x'; EIGHT_MEBIBYTES]),
            text: Bytes::from(text),
            gzip: Bytes::from(gzip),
        }
    }

    fn respond(&self, path: &str) -> Response<Full<Bytes>> {
        let body = match path {
            "/plain/1024" => self.small.clone(),
            "/plain/1048576" => self.medium.clone(),
            "/plain/8388608" => self.large.clone(),
            "/gzip" => {
                return Response::builder()
                    .header("content-encoding", "gzip")
                    .body(Full::new(self.gzip.clone()))
                    .expect("gzip response");
            }
            path => panic!("unexpected benchmark path: {path}"),
        };
        Response::new(Full::new(body))
    }
}

async fn spawn_server(
    protocol: Protocol,
    bodies: Arc<Bodies>,
) -> (SocketAddr, CancellationToken, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind benchmark server");
    let address = listener.local_addr().expect("benchmark server address");
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let server = tokio::spawn(async move {
        let mut connections = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                () = server_shutdown.cancelled() => break,
                accepted = listener.accept() => {
                    let (stream, _) = accepted.expect("accept benchmark request");
                    let bodies = Arc::clone(&bodies);
                    connections.spawn(async move {
                        let service = service_fn(move |request: Request<Incoming>| {
                            let bodies = Arc::clone(&bodies);
                            async move {
                                Ok::<_, Infallible>(bodies.respond(request.uri().path()))
                            }
                        });
                        match protocol {
                            Protocol::Http1 => hyper::server::conn::http1::Builder::new()
                                .serve_connection(TokioIo::new(stream), service)
                                .await
                                .expect("serve benchmark connection"),
                            Protocol::Http2 => {
                                hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                                    .serve_connection(TokioIo::new(stream), service)
                                    .await
                                    .expect("serve benchmark connection");
                            }
                        }
                    });
                }
            }
        }
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    });
    (address, shutdown, server)
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .sample_size(30)
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = benchmark_http_engine
}
criterion_main!(benches);
