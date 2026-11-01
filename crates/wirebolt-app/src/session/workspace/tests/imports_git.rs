use super::*;
fn status() -> WorkspaceGitStatus {
    WorkspaceGitStatus {
        branch: Some("main".into()),
        upstream: Some("origin/main".into()),
        ahead: 1,
        behind: 0,
        changes: vec![],
        merging: false,
    }
}
fn operation(outcome: &str) -> WorkspaceReply {
    WorkspaceReply::GitOperation {
        operation: WorkspaceGitOperation {
            outcome: outcome.into(),
            revision: Some("abc".into()),
            status: status(),
        },
    }
}
fn summary() -> WorkspaceImportSummary {
    WorkspaceImportSummary {
        collection_names: vec!["Shop API".into()],
        request_count: 9,
        group_count: 3,
        environments: vec![WorkspaceImportedEnvironment {
            id: "import-env-0".into(),
            name: "Shop API".into(),
        }],
        warnings: vec!["Scripts weren't imported.".into()],
    }
}
#[test]
fn committed_import_summary_decodes_delta_document_without_losing_fields() {
    #[derive(serde::Deserialize)]
    struct Commit {
        version: u64,
        affected_ids: Vec<String>,
        summary: WorkspaceImportSummary,
    }
    let result:Commit=serde_json::from_str(r#"{"version":7,"kind":"collection","affected_ids":["import-1-0"],"summary":{"collection_name":"Shop API","collection_names":["Shop API"],"request_count":9,"group_count":3,"environments":[{"id":"import-1-env-0","name":"Shop API"}],"warnings":["Scripts aren't supported and weren't imported: Shop API (pre-request)."]}}"#).unwrap();
    assert_eq!(result.version, 7);
    assert_eq!(result.affected_ids, ["import-1-0"]);
    assert_eq!(result.summary.request_count, 9);
    assert_eq!(result.summary.environments[0].id, "import-1-env-0");
    assert_eq!(
        crate::domain::request::import_headline(&result.summary.collection_names, 9, 3),
        "Imported 9 requests in 3 folders into “Shop API”."
    );
}
#[test]
fn clean_import_summary_and_plural_collection_headlines() {
    assert_eq!(
        crate::domain::request::import_headline(&["Imported cURL".into()], 1, 0),
        "Imported 1 request into “Imported cURL”."
    );
    assert_eq!(
        crate::domain::request::import_headline(&["A".into(), "B".into()], 2, 1),
        "Imported 2 requests in 1 folder into 2 collections."
    );
}
#[test]
fn import_failure_messages_preserve_reason_and_fallback() {
    assert_eq!(
        import_failure_message(Some("legacy collection has no nodes"), Some("old.json")),
        "“old.json” couldn’t be imported. Legacy collection has no nodes."
    );
    assert_eq!(
        import_failure_message(None, None),
        "The import failed. Check its format and contents."
    );
}
#[test]
fn import_detection_includes_commented_curl_and_known_formats() {
    for (ext, source, expected) in [
        (
            "sh",
            "#!/bin/sh\n# Seed data\ncurl https://api.example.test",
            Some("curl"),
        ),
        ("sh", "# only a comment", None),
        ("har", r#"{"log":{"entries":[]}}"#, Some("har")),
        (
            "json",
            r#"{"info":{"schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[]}"#,
            Some("postman_v2"),
        ),
        (
            "json",
            r#"{"version":1,"nodes":[]}"#,
            Some("legacy_workspace_v1"),
        ),
        ("json", r#"{"hello":"world"}"#, None),
        ("txt", "hello", None),
    ] {
        assert_eq!(
            crate::domain::imports::detect(ext, source).as_deref(),
            expected
        );
    }
}
#[tokio::test]
async fn successful_import_publishes_summary_and_selects_created_environment_and_request() {
    let h = Harness::new(DomainWorkspace::default());
    let mut w = nested();
    w.collections[0].id = "import-0".into();
    w.environments = vec![
        DomainEnvironment {
            id: "existing".into(),
            ..Default::default()
        },
        DomainEnvironment {
            id: "import-env-0".into(),
            ..Default::default()
        },
    ];
    lock(&h.store.replies).extend([
        WorkspaceReply::Imported { summary: summary() },
        WorkspaceReply::Loaded { workspace: w },
    ]);
    assert!(h.ops.begin_import(true, false));
    let loaded = h
        .ops
        .import(
            WorkspaceImport {
                source: "{}".into(),
                format: "postman_v2".into(),
                name: Some("shop".into()),
                display_name: Some("shop.json".into()),
                selected: None,
                pasted: false,
            },
            &h.store,
        )
        .await
        .unwrap();
    assert_eq!(
        loaded.selected_environment_id.as_deref(),
        Some("import-env-0")
    );
    assert_eq!(loaded.select.unwrap().request.id, "deep");
    assert_eq!(h.ops.presentation().import_summary, Some(summary()));
    assert!(!h.ops.presentation().importing);
}
#[tokio::test]
async fn failed_import_and_paste_report_reason_without_summary() {
    let h = Harness::new(nested());
    for (pasted, name, expected) in [
        (
            false,
            Some("capture.har".into()),
            "“capture.har” couldn’t be imported. The HAR file has no HTTP requests to import.",
        ),
        (
            true,
            None,
            "The import failed. The HAR file has no HTTP requests to import.",
        ),
    ] {
        lock(&h.store.replies).push_back(WorkspaceReply::Failed {
            failure: WorkspaceFailure {
                kind: "import".into(),
                reason: "The HAR file has no HTTP requests to import.".into(),
                localized: true,
            },
        });
        assert!(h.ops.begin_import(true, pasted));
        assert!(
            h.ops
                .import(
                    WorkspaceImport {
                        source: "{}".into(),
                        format: "har".into(),
                        name: None,
                        display_name: name,
                        selected: None,
                        pasted
                    },
                    &h.store
                )
                .await
                .is_none()
        );
        assert_eq!(
            h.ops.presentation().import_failure.as_deref(),
            Some(expected)
        );
        assert!(h.ops.presentation().import_summary.is_none());
    }
}
#[tokio::test]
async fn import_keeps_existing_environment_and_existing_tabs_selection_policy() {
    let mut w = nested();
    w.environments = vec![
        DomainEnvironment {
            id: "keep".into(),
            ..Default::default()
        },
        DomainEnvironment {
            id: "import-env-0".into(),
            ..Default::default()
        },
    ];
    let h = Harness::new(w.clone());
    lock(&h.store.replies).extend([
        WorkspaceReply::Imported { summary: summary() },
        WorkspaceReply::Loaded { workspace: w },
    ]);
    let result = h
        .ops
        .import(
            WorkspaceImport {
                source: "{}".into(),
                format: "har".into(),
                name: None,
                display_name: None,
                selected: Some("keep".into()),
                pasted: false,
            },
            &h.store,
        )
        .await
        .unwrap();
    assert_eq!(result.selected_environment_id.as_deref(), Some("keep"));
    assert!(result.select.is_none());
}
#[test]
fn git_file_status_words_preserve_all_cases() {
    for (staged, unstaged, conflict, expected) in [
        ("none", "untracked", false, "New"),
        ("added", "modified", false, "New"),
        ("modified", "none", false, "Modified"),
        ("none", "deleted", false, "Deleted"),
        ("renamed", "none", false, "Renamed"),
        ("renamed", "modified", false, "Renamed"),
        ("unmerged", "unmerged", false, "Conflict"),
        ("modified", "modified", true, "Conflict"),
    ] {
        assert_eq!(
            crate::domain::request::git_change_kind(staged, unstaged, conflict),
            expected
        );
    }
}
#[test]
fn git_merge_status_defaults_and_merge_abort_document_decode() {
    let older: WorkspaceGitStatus = serde_json::from_str(
        r#"{"branch":"main","upstream":null,"ahead":0,"behind":0,"changes":[]}"#,
    )
    .unwrap();
    assert!(!older.merging);
    let merging:WorkspaceGitStatus=serde_json::from_str(r#"{"branch":"main","upstream":"origin/main","ahead":1,"behind":1,"merging":true,"changes":[{"path":"wirebolt.toml","previous_path":null,"staged":"unmerged","unstaged":"unmerged","conflicted":true},{"path":"environments/dev.toml","previous_path":null,"staged":"modified","unstaged":"none","conflicted":false}]}"#).unwrap();
    assert!(merging.merging);
    assert_eq!(
        merging
            .changes
            .iter()
            .filter(|c| c.conflicted)
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        ["wirebolt.toml"]
    );
    let aborted:WorkspaceGitOperation=serde_json::from_str(r#"{"outcome":"merge_aborted","revision":"abc","status":{"branch":"main","upstream":null,"ahead":0,"behind":0,"changes":[],"merging":false}}"#).unwrap();
    assert_eq!(aborted.outcome, "merge_aborted");
}
#[tokio::test]
async fn abort_reloads_and_initialize_clears_previous_operation() {
    let h = Harness::new(nested());
    let mut w = nested();
    w.name = "Reloaded".into();
    lock(&h.store.replies).extend([
        operation("merge_aborted"),
        WorkspaceReply::Loaded { workspace: w },
        WorkspaceReply::GitStatus {
            status: WorkspaceGitStatus {
                upstream: None,
                ..status()
            },
        },
    ]);
    assert!(h.ops.begin_git(true, WorkspaceGitAction::Abort, false));
    let loaded = h
        .ops
        .git(WorkspaceGitAction::Abort, &h.store, Some(&h.store))
        .await
        .unwrap();
    assert_eq!(loaded.workspace.name, "Reloaded");
    assert_eq!(
        h.ops.presentation().git_operation.unwrap().outcome,
        "merge_aborted"
    );
    assert!(h.ops.begin_git(true, WorkspaceGitAction::Initialize, false));
    h.ops
        .git(WorkspaceGitAction::Initialize, &h.store, Some(&h.store))
        .await;
    let p = h.ops.presentation();
    assert!(p.git_operation.is_none() && p.git_failure.is_none());
    assert!(p.git_status.unwrap().upstream.is_none());
}
#[tokio::test]
async fn unsupported_git_recovery_preserves_error_kind() {
    let h = Harness::new(nested());
    assert!(h.ops.begin_git(true, WorkspaceGitAction::Abort, false));
    h.ops.git(WorkspaceGitAction::Abort, &h.store, None).await;
    assert_eq!(
        h.ops.presentation().git_failure.unwrap().kind,
        "unsupported"
    );
}
#[tokio::test]
async fn git_runs_only_when_requested_and_pull_reloads_only_updated_outcome() {
    let h = Harness::new(nested());
    assert!(h.ops.presentation().git_status.is_none());
    for (action, outcome) in [
        (
            WorkspaceGitAction::Commit {
                message: "Save".into(),
            },
            "committed",
        ),
        (WorkspaceGitAction::Push, "pushed"),
        (WorkspaceGitAction::Pull, "up_to_date"),
    ] {
        lock(&h.store.replies).push_back(operation(outcome));
        assert!(h.ops.begin_git(true, action.clone(), false));
        assert!(h.ops.git(action, &h.store, Some(&h.store)).await.is_none());
        assert_eq!(h.ops.presentation().git_operation.unwrap().outcome, outcome);
    }
    let mut w = nested();
    w.name = "Updated".into();
    lock(&h.store.replies).extend([
        operation("updated"),
        WorkspaceReply::Loaded { workspace: w },
    ]);
    assert!(h.ops.begin_git(true, WorkspaceGitAction::Pull, false));
    assert_eq!(
        h.ops
            .git(WorkspaceGitAction::Pull, &h.store, Some(&h.store))
            .await
            .unwrap()
            .workspace
            .name,
        "Updated"
    );
}
#[test]
fn pulling_refuses_unsaved_edits_and_unconfigured_git_has_exact_guidance() {
    let h = Harness::new(nested());
    assert!(!h.ops.begin_git(true, WorkspaceGitAction::Pull, true));
    assert_eq!(
        h.ops.presentation().git_failure.unwrap().reason,
        "Save or discard request edits before pulling."
    );
    assert!(!h.ops.begin_git(false, WorkspaceGitAction::Status, false));
    assert_eq!(
        h.ops.presentation().git_failure.unwrap().reason,
        "Open a workspace before using Git collaboration."
    );
}
#[tokio::test]
async fn git_reload_failure_reports_distinct_failure_and_finishes_busy_state() {
    let h = Harness::new(nested());
    lock(&h.store.replies).push_back(operation("updated"));
    assert!(h.ops.begin_git(true, WorkspaceGitAction::Pull, false));
    assert!(!h.ops.begin_git(true, WorkspaceGitAction::Status, false));
    h.ops
        .git(WorkspaceGitAction::Pull, &h.store, Some(&h.store))
        .await;
    let p = h.ops.presentation();
    assert!(!p.git_busy);
    assert_eq!(p.git_failure.unwrap().kind, "workspace_reload");
}
#[tokio::test]
async fn workspace_rename_trims_persists_once_and_rejects_empty_names() {
    let h = Harness::new(nested());
    assert_eq!(
        h.ops
            .rename("  Payments API  ".into(), &h.store)
            .await
            .as_deref(),
        Some("Payments API")
    );
    assert!(h.ops.rename("  ".into(), &h.store).await.is_none());
    assert_eq!(
        h.ops
            .rename("Payments API".into(), &h.store)
            .await
            .as_deref(),
        Some("Payments API")
    );
    assert_eq!(
        *lock(&h.store.commands),
        [SessionWorkspaceCommand::RenameWorkspace {
            name: "Payments API".into()
        }]
    );
    assert_eq!(h.workspace.snapshot().name, "Payments API");
}

#[test]
fn push_availability_and_repository_guidance_preserve_cases() {
    use crate::domain::request::{PushAvailability, push_availability};
    assert_eq!(
        push_availability(Some("main".into()), Some("origin/main"), 2),
        PushAvailability::Push
    );
    assert_eq!(
        push_availability(Some("main".into()), None, 0),
        PushAvailability::Publish {
            branch: "main".into()
        }
    );
    assert!(matches!(
        push_availability(Some("main".into()), Some("origin/main"), 0),
        PushAvailability::Unavailable { .. }
    ));
    assert!(matches!(
        push_availability(None, None, 0),
        PushAvailability::Unavailable { .. }
    ));
}

#[test]
fn repository_problems_recognize_only_guidance_kinds() {
    assert_eq!(
        repository_problem("not_repository"),
        Some(WorkspaceRepositoryProblem::NotRepository)
    );
    assert_eq!(
        repository_problem("workspace_not_repository_root"),
        Some(WorkspaceRepositoryProblem::NotRepositoryRoot)
    );
    assert_eq!(
        repository_problem("git_unavailable"),
        Some(WorkspaceRepositoryProblem::GitUnavailable)
    );
    assert_eq!(repository_problem("authentication_required"), None);
}
