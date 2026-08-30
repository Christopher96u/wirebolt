use std::{collections::BTreeMap, hint::black_box, path::Path};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use wirebolt_core::{
    Collection, DocumentId, Environment, Request, RequestBody, RequestHeader, ValueSource,
    Workspace, WorkspaceDocument, WorkspaceStore,
};

const COLLECTIONS: usize = 10;
const REQUESTS_PER_COLLECTION: usize = 50;

fn id(value: &str) -> DocumentId {
    DocumentId::new(value).expect("benchmark document ID")
}

fn request(collection: usize, index: usize) -> Request {
    let mut request = Request::new(
        id(&format!("request-{index}")),
        format!("Request {index}"),
        "POST",
        format!("{{{{base_url}}}}/collections/{collection}/items/{index}"),
    );
    request.headers = vec![
        RequestHeader::enabled("accept", ValueSource::literal("application/json")),
        RequestHeader::enabled("x-trace", ValueSource::literal("{{trace}}")),
    ];
    request.body = RequestBody::Json {
        value: format!(r#"{{"index":{index},"name":"Item {index}"}}"#),
    };
    request
}

/// Ten collections with fifty requests each plus two environments.
fn populate(root: &Path) -> WorkspaceStore {
    let store = WorkspaceStore::create(root, &Workspace::new("Benchmark")).expect("workspace");
    for collection in 0..COLLECTIONS {
        let collection_id = id(&format!("collection-{collection}"));
        store
            .save(&WorkspaceDocument::Collection(Collection::new(
                collection_id.clone(),
                format!("Collection {collection}"),
            )))
            .expect("save collection");
        for index in 0..REQUESTS_PER_COLLECTION {
            store
                .save(&WorkspaceDocument::Request {
                    collection_id: collection_id.clone(),
                    request: request(collection, index),
                })
                .expect("save request");
        }
    }
    for name in ["local", "production"] {
        store
            .save(&WorkspaceDocument::Environment(Environment::new(
                id(name),
                name.to_owned(),
                BTreeMap::from([(
                    "base_url".to_owned(),
                    ValueSource::literal(format!("https://{name}.example.com")),
                )]),
            )))
            .expect("save environment");
    }
    store
}

fn benchmark_workspace_storage(c: &mut Criterion) {
    let temporary = tempfile::tempdir().expect("temporary workspace");
    let store = populate(temporary.path());
    let expected = COLLECTIONS * REQUESTS_PER_COLLECTION;

    c.bench_function("workspace_load_cold_500_requests", |b| {
        b.iter(|| {
            let fresh = WorkspaceStore::open(temporary.path()).expect("open workspace");
            let snapshot = fresh.load().expect("load workspace");
            let total: usize = snapshot
                .collections
                .iter()
                .map(|collection| collection.requests.len())
                .sum();
            assert_eq!(total, expected);
            black_box(total)
        });
    });

    c.bench_function("workspace_load_warm_500_requests", |b| {
        b.iter(|| {
            let snapshot = store.load().expect("load workspace");
            black_box(snapshot.collections.len())
        });
    });

    let collection_id = id("collection-0");
    c.bench_function("workspace_save_one_request", |b| {
        let mut revision = 0_usize;
        b.iter_batched(
            || {
                revision += 1;
                let mut request = request(0, 0);
                request.name = format!("Request 0 revision {revision}");
                request
            },
            |request| {
                let outcome = store
                    .save(&WorkspaceDocument::Request {
                        collection_id: collection_id.clone(),
                        request,
                    })
                    .expect("save request");
                black_box(outcome)
            },
            BatchSize::SmallInput,
        );
    });

    c.bench_function("workspace_save_then_reload", |b| {
        let mut revision = 0_usize;
        b.iter_batched(
            || {
                revision += 1;
                let mut request = request(1, 1);
                request.name = format!("Request 1 revision {revision}");
                request
            },
            |request| {
                store
                    .save(&WorkspaceDocument::Request {
                        collection_id: id("collection-1"),
                        request,
                    })
                    .expect("save request");
                let snapshot = store.load().expect("reload workspace");
                black_box(snapshot.collections.len())
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, benchmark_workspace_storage);
criterion_main!(benches);
