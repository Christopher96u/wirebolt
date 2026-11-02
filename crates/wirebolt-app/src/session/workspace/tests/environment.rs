use super::super::environment::EnvironmentCommit as Commit;
use super::*;
use std::collections::HashMap;

#[derive(Debug, Default)]
struct Keychain(Mutex<HashMap<String, String>>);
#[async_trait]
impl SessionSecretPlatform for Keychain {
    async fn read(&self, name: String) -> Result<Option<String>, ()> {
        Ok(lock(&self.0).get(&name).cloned())
    }
    async fn save(&self, name: String, value: String) -> Result<(), ()> {
        lock(&self.0).insert(name, value);
        Ok(())
    }
    async fn remove(&self, name: String) -> Result<(), ()> {
        lock(&self.0).remove(&name);
        Ok(())
    }
}
fn fixture() -> DomainWorkspace {
    DomainWorkspace {
        environments: vec![
            DomainEnvironment {
                id: "global".into(),
                name: "Global Environment".into(),
                ..Default::default()
            },
            DomainEnvironment {
                id: "local".into(),
                name: "Local".into(),
                variables: vec![
                    DomainVariable {
                        id: "url".into(),
                        key: "base_url".into(),
                        value: DomainValueSource::Literal {
                            value: "http://localhost".into(),
                        },
                        enabled: true,
                        order: 0,
                    },
                    DomainVariable {
                        id: "token".into(),
                        key: "token".into(),
                        value: DomainValueSource::Secret {
                            secret: "env.local.token".into(),
                        },
                        enabled: true,
                        order: 1,
                    },
                ],
            },
            DomainEnvironment {
                id: "other".into(),
                name: "Other".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}
async fn commit(
    h: &Harness,
    keychain: &Keychain,
    id: &str,
    edit: Commit,
) -> EnvironmentCommitOutcome {
    h.ops
        .commit_environment(
            id.into(),
            edit,
            Some("local".into()),
            &h.store,
            &h.secrets,
            keychain,
        )
        .await
}

fn live_cases() -> Vec<(Commit, &'static str)> {
    vec![
        (
            Commit::Rename {
                name: "Renamed".into(),
            },
            "Rename Environment",
        ),
        (
            Commit::VariableName {
                id: "url".into(),
                name: "service_url".into(),
            },
            "Rename Variable",
        ),
        (
            Commit::VariableValue {
                id: "url".into(),
                value: "http://127.0.0.1:8080".into(),
            },
            "Edit Variable",
        ),
        (
            Commit::VariableValue {
                id: "token".into(),
                value: "new-secret-material".into(),
            },
            "Edit Variable",
        ),
        (
            Commit::VariableEnabled {
                id: "url".into(),
                enabled: false,
            },
            "Edit Variable",
        ),
        (
            Commit::VariableSecret {
                id: "url".into(),
                secret: true,
            },
            "Edit Variable Secret",
        ),
        (
            Commit::VariableSecret {
                id: "token".into(),
                secret: false,
            },
            "Edit Variable Secret",
        ),
        (Commit::AddVariable, "Add Variable"),
        (
            Commit::DeleteVariable { id: "token".into() },
            "Delete Variable",
        ),
        (Commit::ClearVariables, "Delete Variables"),
        (Commit::Create, "Add Environment"),
        (Commit::Delete, "Delete Environment"),
    ]
}

#[tokio::test]
async fn every_live_commit_has_one_named_undo_and_symmetric_redo() {
    for (edit, label) in live_cases() {
        let original = fixture();
        let h = Harness::new(original.clone());
        let keychain = Keychain::default();
        lock(&keychain.0).insert("env.local.token".into(), "previous-secret-material".into());
        let result = commit(&h, &keychain, "local", edit).await;
        assert_eq!(result.problem, None, "{label}");
        let mutation = result.mutation;
        assert!(mutation.succeeded, "{label}");
        let undo = mutation.undo.unwrap();
        assert_eq!(undo.name, label);
        let changed = h.workspace.snapshot();
        assert!(
            changed
                .environments
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id),
            "{label}"
        );
        assert_ne!(changed, original);
        let redo = h.ops.reverse_token(undo.token).unwrap();
        let undone = h
            .ops
            .undo(
                undo.token,
                redo.token,
                Some("local".into()),
                &h.store,
                &h.secrets,
                &keychain,
            )
            .await;
        assert!(undone.succeeded, "{label}");
        assert_eq!(h.workspace.snapshot(), original, "{label}");
        if label == "Delete Environment" {
            assert_eq!(
                undone.plan.unwrap().selected_environment_id.as_deref(),
                Some("local")
            );
        }
        let reverse = h.ops.reverse_token(redo.token).unwrap();
        let redone = h
            .ops
            .undo(
                redo.token,
                reverse.token,
                Some("local".into()),
                &h.store,
                &h.secrets,
                &keychain,
            )
            .await;
        assert!(redone.succeeded, "{label}");
        assert_eq!(h.workspace.snapshot(), changed, "{label}");
        assert_eq!(
            h.secrets
                .load("env.local.token".into(), &keychain)
                .await
                .unwrap(),
            "previous-secret-material"
        );
    }
}

#[tokio::test]
async fn invalid_field_commits_never_write_or_register_undo() {
    for edit in [
        Commit::Rename { name: " \n".into() },
        Commit::Rename {
            name: " other ".into(),
        },
        Commit::VariableName {
            id: "url".into(),
            name: String::new(),
        },
        Commit::VariableName {
            id: "url".into(),
            name: "two words".into(),
        },
        Commit::VariableName {
            id: "url".into(),
            name: "{{url}}".into(),
        },
        Commit::VariableName {
            id: "url".into(),
            name: "token".into(),
        },
    ] {
        let h = Harness::new(fixture());
        let result = commit(&h, &Keychain::default(), "local", edit).await;
        assert!(result.problem.is_some());
        assert!(!result.mutation.succeeded && result.mutation.undo.is_none());
        assert!(lock(&h.store.commands).is_empty());
        assert_eq!(h.workspace.snapshot(), fixture());
    }
}

#[tokio::test]
async fn secret_field_commits_keep_material_out_of_workspace_commands_and_undo() {
    let h = Harness::new(fixture());
    let keychain = Keychain::default();
    lock(&keychain.0).insert("env.local.token".into(), "previous-secret-material".into());
    let edit = Commit::VariableValue {
        id: "token".into(),
        value: "new-secret-material".into(),
    };
    assert!(!format!("{edit:?}").contains("new-secret-material"));
    let result = commit(&h, &keychain, "local", edit).await.mutation;
    let saved = h.workspace.snapshot();
    let reference = saved.environments[1].variables[1]
        .value
        .secret_name()
        .unwrap();
    assert_ne!(reference, "env.local.token");
    assert_eq!(
        h.secrets.load(reference.into(), &keychain).await.unwrap(),
        "new-secret-material"
    );
    for payload in [
        serde_json::to_string(&saved).unwrap(),
        format!("{:?}", result.plan),
        format!("{:?}", lock(&h.store.commands)),
    ] {
        assert!(
            !payload.contains("new-secret-material")
                && !payload.contains("previous-secret-material")
        );
    }
    let undo = result.undo.unwrap();
    let redo = h.ops.reverse_token(undo.token).unwrap();
    assert!(
        h.ops
            .undo(
                undo.token, redo.token, None, &h.store, &h.secrets, &keychain
            )
            .await
            .succeeded
    );
    assert_eq!(h.workspace.snapshot(), fixture());
    let reverse = h.ops.reverse_token(redo.token).unwrap();
    assert!(
        h.ops
            .undo(
                redo.token,
                reverse.token,
                None,
                &h.store,
                &h.secrets,
                &keychain
            )
            .await
            .succeeded
    );
    assert_eq!(h.workspace.snapshot(), saved);
    assert_eq!(
        h.secrets.load(reference.into(), &keychain).await.unwrap(),
        "new-secret-material"
    );
    let unchanged = commit(
        &h,
        &keychain,
        "local",
        Commit::VariableValue {
            id: "token".into(),
            value: "new-secret-material".into(),
        },
    )
    .await;
    assert!(unchanged.mutation.succeeded && unchanged.mutation.undo.is_none());
}

#[tokio::test]
async fn globals_commit_live_but_cannot_be_renamed_or_deleted() {
    let h = Harness::new(fixture());
    let added = commit(&h, &Keychain::default(), "global", Commit::AddVariable).await;
    assert_eq!(added.mutation.undo.unwrap().name, "Add Variable");
    for edit in [
        Commit::Rename {
            name: "Other globals".into(),
        },
        Commit::Delete,
    ] {
        assert!(
            commit(&h, &Keychain::default(), "global", edit)
                .await
                .problem
                .is_some()
        );
    }
}

#[tokio::test]
async fn repeated_additions_get_unique_valid_names_and_noop_has_no_undo() {
    let h = Harness::new(fixture());
    let keychain = Keychain::default();
    for _ in 0..3 {
        assert!(
            commit(&h, &keychain, "local", Commit::AddVariable)
                .await
                .mutation
                .succeeded
        );
        assert!(
            commit(&h, &keychain, "local", Commit::Create)
                .await
                .mutation
                .succeeded
        );
    }
    let snapshot = h.workspace.snapshot();
    for env in &snapshot.environments {
        super::super::environment::validate_name(&snapshot, env).unwrap();
        for row in &env.variables {
            super::super::environment::validate_key(env, &row.id, &row.key).unwrap();
        }
    }
    let unchanged = commit(
        &h,
        &keychain,
        "local",
        Commit::Rename {
            name: "Local".into(),
        },
    )
    .await;
    assert!(unchanged.mutation.succeeded && unchanged.mutation.undo.is_none());
}

#[derive(Debug)]
struct DiskStore(Arc<crate::session::persistence::SessionPersistence>);
#[async_trait]
impl WorkspacePlatform for DiskStore {
    async fn execute(&self, request: WorkspaceIO) -> WorkspaceReply {
        self.0.workspace_io(request)
    }
}

#[tokio::test]
async fn live_environment_edits_keep_id_order_and_secret_references_on_disk_reload() {
    let root = tempfile::tempdir().unwrap();
    let disk = DiskStore(
        crate::session::persistence::SessionPersistence::open_or_create(
            root.path().to_string_lossy().into_owned(),
            "Fixture".into(),
            Arc::new(super::files::NoSecrets),
        )
        .unwrap(),
    );
    let h = Harness::new(fixture());
    let keychain = Keychain::default();
    for environment in fixture().environments {
        assert!(
            matches!(disk.0.workspace_io(WorkspaceIO::Commands { commands: vec![SessionWorkspaceCommand::SaveEnvironment { environment }] }), WorkspaceReply::Commands { result } if result.failure_reason.is_none())
        );
    }
    lock(&keychain.0).insert("env.local.token".into(), "previous-secret-material".into());
    let saved = h
        .ops
        .commit_environment(
            "local".into(),
            Commit::VariableValue {
                id: "token".into(),
                value: "new-secret-material".into(),
            },
            Some("local".into()),
            &disk,
            &h.secrets,
            &keychain,
        )
        .await;
    assert!(saved.mutation.succeeded);
    let WorkspaceReply::Loaded { workspace } = disk.0.workspace_io(WorkspaceIO::Load) else {
        panic!("load failed")
    };
    assert_eq!(
        workspace
            .environments
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["global", "local", "other"]
    );
    for file in std::fs::read_dir(root.path().join("environments")).unwrap() {
        let source = std::fs::read_to_string(file.unwrap().path()).unwrap();
        assert!(
            !source.contains("new-secret-material") && !source.contains("previous-secret-material")
        );
    }
    assert_eq!(workspace.environments, h.workspace.snapshot().environments);
    let token = saved.mutation.undo.unwrap();
    let redo = h.ops.reverse_token(token.token).unwrap();
    assert!(
        h.ops
            .undo(
                token.token,
                redo.token,
                Some("local".into()),
                &disk,
                &h.secrets,
                &keychain
            )
            .await
            .succeeded
    );
    let WorkspaceReply::Loaded { workspace } = disk.0.workspace_io(WorkspaceIO::Load) else {
        panic!("load failed")
    };
    assert_eq!(
        workspace
            .environments
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["global", "local", "other"]
    );
}

#[tokio::test]
async fn environment_creation_deletion_and_undo_match_reload_id_order() {
    let root = tempfile::tempdir().unwrap();
    let disk = DiskStore(
        crate::session::persistence::SessionPersistence::open_or_create(
            root.path().to_string_lossy().into_owned(),
            "Fixture".into(),
            Arc::new(super::files::NoSecrets),
        )
        .unwrap(),
    );
    let h = Harness::new(DomainWorkspace {
        name: "Fixture".into(),
        ..Default::default()
    });
    let keychain = Keychain::default();
    // Deliberately insert IDs out of order and use names with a different order.
    for (id, name) in [("zulu", "Alpha"), ("alpha", "Zulu"), ("global", "Globals")] {
        let saved = h
            .ops
            .save_environment(
                DomainEnvironment {
                    id: id.into(),
                    name: name.into(),
                    ..Default::default()
                },
                HashMap::new(),
                None,
                &disk,
                &h.secrets,
                &keychain,
            )
            .await;
        assert!(saved.succeeded);
        assert_environment_reload_matches(&h, &disk);
    }
    for edit in [
        Commit::Create,
        Commit::Rename {
            name: "Renamed".into(),
        },
        Commit::Delete,
    ] {
        let result = h
            .ops
            .commit_environment("alpha".into(), edit, None, &disk, &h.secrets, &keychain)
            .await;
        assert!(result.mutation.succeeded);
        assert_environment_reload_matches(&h, &disk);
        let undo = result.mutation.undo.unwrap();
        let redo = h.ops.reverse_token(undo.token).unwrap();
        assert!(
            h.ops
                .undo(undo.token, redo.token, None, &disk, &h.secrets, &keychain)
                .await
                .succeeded
        );
        assert_environment_reload_matches(&h, &disk);
        let reverse = h.ops.reverse_token(redo.token).unwrap();
        assert!(
            h.ops
                .undo(
                    redo.token,
                    reverse.token,
                    None,
                    &disk,
                    &h.secrets,
                    &keychain
                )
                .await
                .succeeded
        );
        assert_environment_reload_matches(&h, &disk);
    }
}

fn assert_environment_reload_matches(h: &Harness, disk: &DiskStore) {
    let WorkspaceReply::Loaded { workspace } = disk.0.workspace_io(WorkspaceIO::Load) else {
        panic!("load failed")
    };
    let environments = h.workspace.snapshot().environments;
    assert!(environments.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert_eq!(environments, workspace.environments);
    let inspector =
        super::super::environment::inspector_environments(environments.into_iter().rev().collect());
    assert_eq!(inspector[0].id, "global");
    assert_eq!(inspector.iter().filter(|e| e.id == "global").count(), 1);
    assert!(
        inspector[1..]
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id)
    );
}

#[test]
fn inspector_browsing_preserves_active_environment_until_explicit_use() {
    use crate::session::workspace::environment::EnvironmentInspector;
    let active = Some("staging".to_owned());
    let mut inspector = EnvironmentInspector::new(active.clone());
    for id in ["production", wirebolt_core::GLOBAL_ENVIRONMENT_ID, "local"] {
        inspector.browse(id.into());
        assert_eq!(active.as_deref(), Some("staging"));
        assert_eq!(inspector.editing_id, id);
    }
    assert_eq!(inspector.use_for_requests().as_deref(), Some("local"));
    inspector.browse(wirebolt_core::GLOBAL_ENVIRONMENT_ID.into());
    assert_eq!(inspector.use_for_requests(), None);
}
