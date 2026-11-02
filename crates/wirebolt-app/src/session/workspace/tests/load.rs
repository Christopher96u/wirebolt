use super::{persistence_fixtures::*, *};

#[test]
fn partial_workspace_load_reports_skipped_files_without_contents() {
    let (root, store) = persistence();
    let original = imported_pair(&store);
    let requests = root
        .path()
        .join("collections")
        .join(&original.collections[0].id)
        .join("requests");
    std::fs::write(
        requests.join("broken.toml"),
        r#"name = "private-fixture-value"#,
    )
    .unwrap();
    std::fs::write(
        requests.join("future.toml"),
        "schema_version = 4294967295\n",
    )
    .unwrap();
    std::fs::File::create(requests.join("oversized.toml"))
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    let result = store.load_result().unwrap();
    assert_eq!(result.workspace, original);
    assert_eq!(result.diagnostics.problems.len(), 3);
    for kind in [
        crate::session::persistence::WorkspaceDocumentProblemKind::InvalidToml,
        crate::session::persistence::WorkspaceDocumentProblemKind::UnsupportedSchema,
        crate::session::persistence::WorkspaceDocumentProblemKind::DocumentTooLarge,
    ] {
        assert!(
            result
                .diagnostics
                .problems
                .iter()
                .any(|problem| problem.kind == kind)
        );
    }
    let reply = store.workspace_io(WorkspaceIO::Load);
    let diagnostics = format!("{reply:?}");
    for name in ["broken.toml", "future.toml", "oversized.toml"] {
        assert!(diagnostics.contains(name));
    }
    assert!(!diagnostics.contains("private-fixture-value"));
    for index in 0..20 {
        std::fs::write(requests.join(format!("bad-{index}\n.toml")), "invalid = [").unwrap();
    }
    let result = store.load_result().unwrap();
    assert_eq!(result.diagnostics.problems.len(), 10);
    assert_eq!(result.diagnostics.omitted, 13);
    assert!(
        result
            .diagnostics
            .problems
            .iter()
            .all(|problem| !problem.path.chars().any(char::is_control))
    );
    let notice = result.diagnostics.notice.unwrap();
    assert!(notice.len() < 4000 && notice.contains("13 more files"));
}

#[tokio::test]
async fn damaged_history_is_reported_during_workspace_load_without_hiding_requests() {
    use crate::response::{ResponseCookieJar, RunHistory};
    use crate::session::runtime::RuntimeState;
    let (root, store) = persistence();
    let original = imported_pair(&store);
    let history_root = root.path().join("history");
    std::fs::create_dir(&history_root).unwrap();
    let index = history_root.join("index.json");
    std::fs::write(&index, b"damaged index").unwrap();
    let runtime = Arc::new(RuntimeState::new(
        Arc::new(Mutex::new(RunHistory::new(history_root))),
        Arc::new(Mutex::new(ResponseCookieJar::new(None))),
    ));
    let ops = WorkspaceOperations::new(Arc::new(WorkspaceSession::default()));
    ops.connect_history(runtime);
    let platform = SaveGate {
        store,
        ready: tokio::sync::Notify::default(),
        release: tokio::sync::Notify::default(),
        pause: std::sync::atomic::AtomicBool::new(false),
    };
    let loaded = ops.load(&platform, false).await.unwrap();
    assert_eq!(loaded.workspace, original);
    assert_eq!(
        ops.presentation().operation_failure,
        Some(crate::response::ResponseFailureKind::Storage)
    );
    let reason = ops.presentation().operation_failure_reason.unwrap();
    assert!(reason.contains("History could not be loaded."));
    assert_eq!(std::fs::read(index).unwrap(), b"damaged index");
}
