use std::{convert::Infallible, hint::black_box, net::SocketAddr, sync::Arc, time::Duration};

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use wirebolt_core::{
    HttpEngine, HttpEngineConfig, RequestDraft, RunCancellation, RunOptions, StreamControl,
    prepare_request,
};

const ONE_KIBIBYTE: usize = 1_024;
const ONE_MEBIBYTE: usize = 1_024 * 1_024;

fn benchmark_http_engine(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("benchmark Tokio runtime");
    let (address, shutdown, server) = runtime.block_on(spawn_server());
    let engine = HttpEngine::new(HttpEngineConfig::default()).expect("HTTP engine");
    let mut group = c.benchmark_group("http_engine_loopback");

    for size in [ONE_KIBIBYTE, ONE_MEBIBYTE] {
        let size_u64 = u64::try_from(size).expect("benchmark response size fits in u64");
        let request = prepare_request(RequestDraft {
            method: "GET".to_owned(),
            url: format!("http://{address}/{size}"),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .expect("benchmark request");
        group.throughput(Throughput::Bytes(size_u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &request, |b, request| {
            b.to_async(&runtime).iter(|| async {
                let mut received = 0_u64;
                let run = engine
                    .run(
                        request.clone(),
                        RunOptions::default(),
                        &RunCancellation::new(),
                        |chunk| {
                            received +=
                                u64::try_from(chunk.len()).expect("chunk length fits in u64");
                            StreamControl::Continue
                        },
                    )
                    .await
                    .expect("benchmark request succeeds");
                assert_eq!(received, size_u64);
                black_box(run.bytes_received)
            });
        });
    }

    group.finish();
    drop(engine);
    shutdown.cancel();
    runtime
        .block_on(server)
        .expect("benchmark server task completed");
}

async fn spawn_server() -> (SocketAddr, CancellationToken, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind benchmark server");
    let address = listener.local_addr().expect("benchmark server address");
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let small_body = Bytes::from(vec![b'x'; ONE_KIBIBYTE]);
    let large_body = Bytes::from(vec![b'x'; ONE_MEBIBYTE]);
    let bodies = Arc::new((small_body, large_body));
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
                                let body = match request.uri().path() {
                                    "/1024" => bodies.0.clone(),
                                    "/1048576" => bodies.1.clone(),
                                    path => panic!("unexpected benchmark path: {path}"),
                                };
                                Ok::<_, Infallible>(Response::new(Full::new(body)))
                            }
                        });
                        hyper::server::conn::http1::Builder::new()
                            .serve_connection(TokioIo::new(stream), service)
                            .await
                            .expect("serve benchmark connection");
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
