use super::*;
use crate::session::workspace::files::*;
#[test]
fn unrecognized_files_include_openapi_guidance_and_supported_formats() {
    assert!(
        unrecognized(
            "pets.yaml",
            "openapi: 3.1.0\ninfo:\n  title: Pets",
            "formats"
        )
        .contains("OpenAPI import isn’t supported yet")
    );
    assert!(unrecognized("pets.json", r#"{"swagger":"2.0"}"#, "formats").contains("OpenAPI"));
    let other = unrecognized("notes.txt", "hello", "cURL, HAR and Postman Collection v2");
    assert!(other.starts_with("“notes.txt” isn’t a format Wirebolt can import."));
    assert!(other.contains("Postman Collection v2"));
}
#[test]
fn external_file_routing_detects_contents_before_preferred_format_and_reports_read_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("request.txt");
    std::fs::write(&path, "curl https://example.com").unwrap();
    assert!(
        matches!(read_import(&path,Some(wirebolt_core::ImportFormat::Har),"formats"),WorkspaceImportFile::Document{format,..} if format==wirebolt_core::ImportFormat::Curl)
    );
    assert!(
        matches!(read_import(&dir.path().join("missing"),None,"formats"),WorkspaceImportFile::Failure{message} if message.contains("couldn’t be read"))
    );
    let unknown = dir.path().join("unknown");
    std::fs::write(&unknown, "hello").unwrap();
    assert!(matches!(
        read_import(&unknown, None, "formats"),
        WorkspaceImportFile::Failure { .. }
    ));
    assert!(
        matches!(read_import(&unknown,Some(wirebolt_core::ImportFormat::Har),"formats"),WorkspaceImportFile::Document{format,..} if format==wirebolt_core::ImportFormat::Har)
    );
}
#[test]
fn bruno_directory_and_single_file_routing_keep_exact_errors() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        matches!(read_import(dir.path(),None,""),WorkspaceImportFile::Failure{message} if message.ends_with("isn’t a Bruno collection folder. Choose the folder that contains bruno.json."))
    );
    std::fs::write(dir.path().join("bruno.json"), "{}").unwrap();
    assert_eq!(
        read_import(dir.path(), None, ""),
        WorkspaceImportFile::Bruno
    );
    assert_eq!(
        read_import(&dir.path().join("single.BRU"), None, ""),
        WorkspaceImportFile::Bruno
    );
}
#[test]
fn recent_workspaces_are_deduplicated_pruned_and_limited() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..12)
        .map(|i| {
            let p = dir.path().join(i.to_string());
            std::fs::create_dir(&p).unwrap();
            p.to_string_lossy().into_owned()
        })
        .collect();
    let mut old = vec!["/missing/workspace".into(), paths[0].clone()];
    old.extend(paths.clone());
    let updated = recent(&paths[0], old);
    assert_eq!(updated.len(), 10);
    assert_eq!(updated[0], paths[0]);
    assert_eq!(updated[1], paths[1]);
    assert_eq!(default_name(dir.path(), dir.path()), "Wirebolt");
    assert_eq!(
        default_name(&dir.path().join("Payments"), dir.path()),
        "Payments"
    );
}
#[test]
fn external_file_queue_retains_order_filters_nonfiles_and_drains_once() {
    let ui = crate::session::workspace::interface::WorkspaceInterface::default();
    assert_eq!(
        ui.enqueue_files(vec![
            "file:///tmp/a".into(),
            "https://example.com".into(),
            "file:///tmp/b".into()
        ]),
        ["file:///tmp/a", "file:///tmp/b"]
    );
    assert_eq!(ui.drain_files(), ["file:///tmp/a", "file:///tmp/b"]);
    assert!(ui.drain_files().is_empty());
}
#[derive(Debug)]
pub(super) struct NoSecrets;
impl crate::session::persistence::WorkspaceSecrets for NoSecrets {
    fn read(&self, _: &str) -> Result<Option<String>, crate::session::persistence::WorkspaceError> {
        Ok(None)
    }
    fn save(&self, _: &str, _: &str) -> Result<(), crate::session::persistence::WorkspaceError> {
        Ok(())
    }
    fn remove(&self, _: &str) -> Result<(), crate::session::persistence::WorkspaceError> {
        Ok(())
    }
}
#[test]
fn native_workspace_io_executes_typed_load_commands_import_export_and_git() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::session::persistence::SessionPersistence::open_or_create(
        dir.path().to_string_lossy().into_owned(),
        "Native".into(),
        Arc::new(NoSecrets),
    )
    .unwrap();
    assert!(
        matches!(store.workspace_io(WorkspaceIO::Load),WorkspaceReply::Loaded{workspace} if workspace.name=="Native")
    );
    let reply = store.workspace_io(WorkspaceIO::Commands {
        commands: vec![SessionWorkspaceCommand::RenameWorkspace {
            name: "Renamed".into(),
        }],
    });
    assert!(
        matches!(reply,WorkspaceReply::Commands{result} if result.completed==1 && result.failure_reason.is_none())
    );
    let reply = store.workspace_io(WorkspaceIO::Import {
        format: wirebolt_core::ImportFormat::Curl,
        source: "curl https://example.com".into(),
        name: Some("shop".into()),
    });
    assert!(matches!(reply,WorkspaceReply::Imported{summary} if summary.request_count==1));
    assert!(
        matches!(store.workspace_io(WorkspaceIO::Export{target:WorkspaceExport::Workspace}),WorkspaceReply::Exported{content} if content.contains("example.com"))
    );
    assert!(
        matches!(store.workspace_io(WorkspaceIO::Git{action:WorkspaceGitAction::Initialize}),WorkspaceReply::GitStatus{status} if !status.merging)
    );
}

#[test]
fn typed_workspace_modes_preserve_open_create_and_open_or_create_checks() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        selection_problem(dir.path(), WorkspaceOpenMode::Open),
        Some(WorkspaceSelectionProblem::NotFound)
    );
    for mode in [WorkspaceOpenMode::Create, WorkspaceOpenMode::OpenOrCreate] {
        assert_eq!(selection_problem(dir.path(), mode), None);
    }
    std::fs::write(dir.path().join("wirebolt.toml"), "").unwrap();
    assert_eq!(
        selection_problem(dir.path(), WorkspaceOpenMode::Create),
        Some(WorkspaceSelectionProblem::AlreadyExists)
    );
    for mode in [WorkspaceOpenMode::Open, WorkspaceOpenMode::OpenOrCreate] {
        assert_eq!(selection_problem(dir.path(), mode), None);
    }
}

#[derive(Debug)]
struct RemovalSecrets {
    existing: Result<Option<String>, crate::session::persistence::WorkspaceError>,
    remove_fails: bool,
}
impl crate::session::persistence::WorkspaceSecrets for RemovalSecrets {
    fn read(&self, _: &str) -> Result<Option<String>, crate::session::persistence::WorkspaceError> {
        self.existing.clone()
    }
    fn save(&self, _: &str, _: &str) -> Result<(), crate::session::persistence::WorkspaceError> {
        Ok(())
    }
    fn remove(&self, _: &str) -> Result<(), crate::session::persistence::WorkspaceError> {
        if self.remove_fails {
            Err(crate::session::persistence::WorkspaceError::operation(
                "delete failed",
            ))
        } else {
            Ok(())
        }
    }
}
#[test]
fn secret_removal_treats_only_confirmed_missing_items_as_removed() {
    use crate::session::persistence::{SessionPersistence, WorkspaceError};
    for (existing, remove_fails, succeeds) in [
        (Ok(None), true, true),
        (Ok(Some("still present".into())), true, false),
        (Err(WorkspaceError::operation("read failed")), true, false),
        (Err(WorkspaceError::operation("read failed")), false, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionPersistence::open_or_create(
            dir.path().to_string_lossy().into_owned(),
            "Secrets".into(),
            Arc::new(RemovalSecrets {
                existing,
                remove_fails,
            }),
        )
        .unwrap();
        let result = store.delete_secret("owned.secret");
        assert_eq!(result.is_ok(), succeeds);
        if !succeeds {
            assert_eq!(result.unwrap_err().to_string(), "delete failed");
        }
    }
}
