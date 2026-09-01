use std::{hint::black_box, path::Path, process::Command, time::Duration};

use criterion::{Criterion, criterion_group, criterion_main};
use wirebolt_core::{
    Collection, DocumentId, GitWorkspace, Request, Workspace, WorkspaceDocument, WorkspaceStore,
};

fn git(root: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .output()
        .expect("run Git");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A committed workspace with 200 requests and a handful of dirty files, the
/// shape of a status check while editing.
fn repository(root: &Path) {
    let store = WorkspaceStore::create(root, &Workspace::new("Benchmark")).expect("workspace");
    for collection in 0..4 {
        let collection_id = DocumentId::new(format!("collection-{collection}")).expect("ID");
        store
            .save(&WorkspaceDocument::Collection(Collection::new(
                collection_id.clone(),
                format!("Collection {collection}"),
            )))
            .expect("save collection");
        for index in 0..50 {
            store
                .save(&WorkspaceDocument::Request {
                    collection_id: collection_id.clone(),
                    request: Request::new(
                        DocumentId::new(format!("request-{index}")).expect("ID"),
                        format!("Request {index}"),
                        "GET",
                        format!("https://example.com/{collection}/{index}"),
                    ),
                })
                .expect("save request");
        }
    }
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Wirebolt Bench"]);
    git(root, &["config", "user.email", "bench@example.invalid"]);
    git(root, &["add", "--all"]);
    git(root, &["commit", "-q", "-m", "benchmark workspace"]);
    for index in 0..3 {
        store
            .save(&WorkspaceDocument::Request {
                collection_id: DocumentId::new("collection-0").expect("ID"),
                request: Request::new(
                    DocumentId::new(format!("request-{index}")).expect("ID"),
                    format!("Request {index} edited"),
                    "GET",
                    format!("https://example.com/0/{index}"),
                ),
            })
            .expect("edit request");
    }
}

fn benchmark_git_status(c: &mut Criterion) {
    let temporary = tempfile::tempdir().expect("temporary repository");
    repository(temporary.path());
    let workspace = GitWorkspace::open(temporary.path()).expect("Git workspace");

    c.bench_function("git_status_200_requests_3_dirty", |b| {
        b.iter(|| {
            let status = workspace.status().expect("Git status");
            assert_eq!(status.changes.len(), 3);
            black_box(status.changes.len())
        });
    });
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(3))
        .sample_size(20)
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = benchmark_git_status
}
criterion_main!(benches);
