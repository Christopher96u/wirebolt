use super::*;
#[tokio::test]
async fn staged_request_credentials_flush_before_transport_and_never_enter_draft() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path());
    let doc = document("a");
    let auth = request::new_auth(crate::domain::DomainAuthenticationKind::ApiKey);
    let name = request::auth_references(&auth).remove(0);
    lock(&doc).draft.authentication = auth;
    let transport = Arc::new(Transport::default());
    let c = command(
        state,
        doc,
        Arc::new(Mutex::new(DocumentRun::default())),
        transport.clone(),
        root.path(),
    );
    c.credentials
        .secrets
        .edit_batch(HashMap::from([(name.clone(), "fixture-key-a".into())]));
    RuntimeRuns::default().send(c).await;
    assert_eq!(
        lock(&transport.platform.values).get(&name).unwrap(),
        "fixture-key-a"
    );
    assert!(
        !serde_json::to_string(&lock(&transport.inputs)[0])
            .unwrap()
            .contains("fixture-key-a")
    );
}
#[tokio::test]
async fn keychain_failure_stops_send_without_starting_run() {
    let root = tempfile::tempdir().unwrap();
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    let transport = Arc::new(Transport::default());
    transport.platform.fails.store(true, Ordering::Relaxed);
    let c = command(
        state(root.path()),
        document("a"),
        run.clone(),
        transport.clone(),
        root.path(),
    );
    c.credentials
        .secrets
        .edit_batch(HashMap::from([("token".into(), "fixture".into())]));
    assert_eq!(
        RuntimeRuns::default().send(c).await.unwrap().kind,
        crate::response::ResponseFailureKind::Keychain
    );
    assert!(lock(&transport.inputs).is_empty());
    assert!(lock(&run).snapshot().active_run_id.is_none());
}
#[tokio::test]
async fn environment_secret_save_keeps_material_outside_workspace_and_reports_failure() {
    let platform = Platform::default();
    let secrets = SessionSecrets::default();
    let reference = request::credential_reference("staging", "env");
    secrets
        .save(
            HashMap::from([(reference.clone(), "fixture-environment-token".into())]),
            &platform,
        )
        .await
        .unwrap();
    let env = DomainEnvironment {
        id: "staging".into(),
        name: "Staging".into(),
        variables: vec![DomainVariable {
            key: "token".into(),
            value: DomainValueSource::Secret {
                secret: reference.clone(),
            },
            enabled: true,
            ..Default::default()
        }],
    };
    assert_eq!(
        secrets.material(&DomainValueSource::Secret {
            secret: reference.clone()
        }),
        "fixture-environment-token"
    );
    let json = serde_json::to_string(&env).unwrap();
    assert!(json.contains(&reference));
    assert!(!json.contains("fixture-environment-token"));
    assert_eq!(
        active_variables(&[env], Some("staging"))["token"].secret_name(),
        Some(reference.as_str())
    );
    platform.fails.store(true, Ordering::Relaxed);
    assert!(
        secrets
            .save(
                HashMap::from([("failed".into(), "fixture".into())]),
                &platform
            )
            .await
            .is_err()
    );
    assert!(
        secrets
            .material(&DomainValueSource::Secret {
                secret: "failed".into()
            })
            .is_empty()
    );
}
#[tokio::test]
async fn legacy_migration_copies_only_shared_auth_names_and_keeps_legacy_items() {
    let platform = Platform::default();
    lock(&platform.values).insert("auth.api-key".into(), "fixture-shared-key".into());
    let secrets = SessionSecrets::default();
    let draft = DomainRequestDraft {
        authentication: DomainAuthentication::ApiKey {
            placement: DomainApiKeyPlacement::Header,
            name: "X-API-Key".into(),
            value: DomainValueSource::Secret {
                secret: "auth.api-key".into(),
            },
        },
        ..Default::default()
    };
    let migrated = secrets.migrate(draft, &platform).await.unwrap();
    let references = request::auth_references(&migrated.authentication);
    assert_eq!(references.len(), 1);
    assert!(request::owned_reference(&references[0]));
    assert_eq!(lock(&platform.values)[&references[0]], "fixture-shared-key");
    assert_eq!(lock(&platform.values)["auth.api-key"], "fixture-shared-key");
    secrets.purge(vec![], vec![], &platform).await;
    assert!(lock(&platform.values).contains_key("auth.api-key"));
}
#[tokio::test]
async fn purge_retains_open_drafts_saved_versions_and_workspace_references() {
    let platform = Platform::default();
    let secrets = SessionSecrets::default();
    let names = [
        request::credential_reference("token", "auth"),
        request::credential_reference("password", "auth"),
        request::credential_reference("api-key", "auth"),
    ];
    secrets
        .save(
            names
                .iter()
                .map(|n| (n.clone(), "fixture".into()))
                .collect(),
            &platform,
        )
        .await
        .unwrap();
    let mut doc = lock(&document("a")).clone();
    doc.draft.authentication = DomainAuthentication::Bearer {
        token: DomainValueSource::Secret {
            secret: names[0].clone(),
        },
    };
    doc.saved_draft = Some(DomainRequestDraft {
        authentication: DomainAuthentication::Bearer {
            token: DomainValueSource::Secret {
                secret: names[1].clone(),
            },
        },
        ..Default::default()
    });
    secrets
        .purge_workspace(
            DomainWorkspace::default(),
            vec![doc],
            None,
            vec![],
            &platform,
        )
        .await;
    assert_eq!(*lock(&platform.deleted), [names[2].clone()]);
    assert!(lock(&platform.values).contains_key(&names[0]));
    assert!(lock(&platform.values).contains_key(&names[1]));
}
#[derive(Debug)]
struct Authorizer {
    platform: Arc<Platform>,
    configuration: Mutex<Option<DomainOAuthConfiguration>>,
}
#[async_trait]
impl RuntimeAuthorizer for Authorizer {
    async fn acquire(
        &self,
        configuration: DomainOAuthConfiguration,
    ) -> Result<(String, RuntimeTokenReceipt), RuntimeOperationError> {
        assert_eq!(
            lock(&self.platform.values)[&configuration.client_secret_reference],
            "fixture-client-secret"
        );
        *lock(&self.configuration) = Some(configuration);
        Ok((
            "fixture-new-token".into(),
            RuntimeTokenReceipt {
                expires_at: Some(60.0),
                scope: Some("read".into()),
            },
        ))
    }
}
#[tokio::test]
async fn oauth_migrates_before_acquisition_and_never_overwrites_shared_token() {
    let platform = Arc::new(Platform::default());
    lock(&platform.values).extend([
        ("oauth.client-secret".into(), "fixture-client-secret".into()),
        ("oauth.access-token".into(), "fixture-old-token".into()),
    ]);
    let secrets = SessionSecrets::default();
    let doc = document("a");
    lock(&doc).draft.authentication = DomainAuthentication::Oauth2 {
        configuration: DomainOAuthConfiguration {
            client_secret_reference: "oauth.client-secret".into(),
            access_token_reference: "oauth.access-token".into(),
            ..Default::default()
        },
    };
    let authorizer = Authorizer {
        platform: platform.clone(),
        configuration: Mutex::new(None),
    };
    let receipt = acquire_token(&secrets, &doc, platform.as_ref(), &authorizer, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.scope.as_deref(), Some("read"));
    let configuration = lock(&authorizer.configuration).clone().unwrap();
    assert!(!request::legacy_reference(
        &configuration.access_token_reference
    ));
    assert_eq!(
        lock(&platform.values)[&configuration.access_token_reference],
        "fixture-new-token"
    );
    assert_eq!(
        lock(&platform.values)["oauth.access-token"],
        "fixture-old-token"
    );
    assert!(
        !serde_json::to_string(&lock(&doc).draft)
            .unwrap()
            .contains("fixture-new-token")
    );
}
#[tokio::test]
async fn clean_preview_needs_no_secret_store_but_dirty_material_does() {
    let credentials = RuntimeCredentials {
        secrets: Arc::new(SessionSecrets::default()),
        platform: None,
    };
    assert!(credentials.flush().await.is_ok());
    credentials
        .secrets
        .edit_batch(HashMap::from([("token".into(), "fixture".into())]));
    assert!(credentials.flush().await.is_err());
    assert!(
        socket_input(DomainRequestDraft::default(), context(), credentials, None)
            .await
            .is_err()
    );
}

#[derive(Debug, Default)]
struct PausedPlatform {
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
#[async_trait]
impl SessionSecretPlatform for PausedPlatform {
    async fn read(&self, _name: String) -> Result<Option<String>, ()> {
        Ok(None)
    }
    async fn save(&self, _name: String, _value: String) -> Result<(), ()> {
        self.started.notify_one();
        self.release.notified().await;
        Ok(())
    }
    async fn remove(&self, _name: String) -> Result<(), ()> {
        Ok(())
    }
}
#[tokio::test]
async fn cancellation_during_keychain_flush_prevents_transport_start() {
    let root = tempfile::tempdir().unwrap();
    let run = Arc::new(Mutex::new(DocumentRun::default()));
    let transport = Arc::new(Transport::default());
    let platform = Arc::new(PausedPlatform::default());
    let runs = Arc::new(RuntimeRuns::default());
    let mut c = command(
        state(root.path()),
        document("a"),
        run.clone(),
        transport.clone(),
        root.path(),
    );
    c.credentials
        .secrets
        .edit_batch(HashMap::from([("token".into(), "fixture".into())]));
    c.credentials.platform = Some(platform.clone());
    let r = runs.clone();
    let task = tokio::spawn(async move { r.send(c).await });
    platform.started.notified().await;
    runs.cancel("a", &run);
    platform.release.notify_one();
    task.await.unwrap();
    assert!(lock(&transport.inputs).is_empty());
    assert!(lock(&run).snapshot().active_run_id.is_none());
}
#[derive(Debug)]
struct CurrentInput(Mutex<RuntimeInput>);
#[async_trait]
impl RuntimeObserver for CurrentInput {
    fn changed(&self) {}
    async fn prepare(&self) -> Result<Option<RuntimeInput>, ()> {
        Ok(Some(lock(&self.0).clone()))
    }
}
#[tokio::test]
async fn current_editor_and_context_are_projected_after_keychain_flush() {
    let root = tempfile::tempdir().unwrap();
    let transport = Arc::new(Transport::default());
    let platform = Arc::new(PausedPlatform::default());
    let doc = document("a");
    let observer = Arc::new(CurrentInput(Mutex::new(RuntimeInput {
        draft: lock(&doc).draft.clone(),
        context: context(),
    })));
    let mut c = command(
        state(root.path()),
        doc,
        Arc::new(Mutex::new(DocumentRun::default())),
        transport.clone(),
        root.path(),
    );
    c.observer = observer.clone();
    c.credentials
        .secrets
        .edit_batch(HashMap::from([("token".into(), "fixture".into())]));
    c.credentials.platform = Some(platform.clone());
    let task = tokio::spawn(async move { RuntimeRuns::default().send(c).await });
    platform.started.notified().await;
    lock(&observer.0).draft.url = "https://example.com/current".into();
    lock(&observer.0).context.app_proxy = DomainProxy::Direct;
    platform.release.notify_one();
    task.await.unwrap();
    let inputs = lock(&transport.inputs);
    assert_eq!(inputs[0].url, "https://example.com/current");
    assert_eq!(inputs[0].app_proxy, Some(DomainProxy::Direct));
}

#[tokio::test]
async fn oauth_command_owns_busy_failure_and_receipt_state() {
    let platform = Arc::new(Platform::default());
    lock(&platform.values).insert("chosen.client".into(), "fixture-client-secret".into());
    let document = document("a");
    lock(&document).draft.authentication = DomainAuthentication::Oauth2 {
        configuration: DomainOAuthConfiguration {
            client_secret_reference: "chosen.client".into(),
            access_token_reference: "chosen.token".into(),
            ..Default::default()
        },
    };
    let authorizer = Authorizer {
        platform: platform.clone(),
        configuration: Mutex::new(None),
    };
    let oauth = RuntimeOAuth::default();
    let secrets = SessionSecrets::default();
    let observer = Changes::default();
    oauth
        .acquire(
            &secrets,
            &document,
            platform.as_ref(),
            &authorizer,
            &observer,
        )
        .await;
    let snapshot = oauth.snapshot();
    assert!(!snapshot.busy);
    assert!(snapshot.failure.is_none());
    assert_eq!(snapshot.receipts["a"].scope.as_deref(), Some("read"));
    assert_eq!(observer.0.load(Ordering::Relaxed), 2);
    oauth.reset();
    assert!(oauth.snapshot().receipts.is_empty());
    platform.fails.store(true, Ordering::Relaxed);
    secrets.edit_batch(HashMap::from([("token".into(), "fixture".into())]));
    oauth
        .acquire(
            &secrets,
            &document,
            platform.as_ref(),
            &authorizer,
            &observer,
        )
        .await;
    assert!(!oauth.snapshot().busy);
    assert!(oauth.snapshot().failure.is_some());
    assert!(oauth.snapshot().receipts.is_empty());
}
