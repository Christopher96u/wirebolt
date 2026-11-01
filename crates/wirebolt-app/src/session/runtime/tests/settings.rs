use super::*;
use crate::lock;
#[derive(Debug, Default)]
struct Settings {
    writes: Mutex<Vec<(RuntimeProxyScope, Option<DomainProxy>)>>,
    transports: Mutex<Vec<DomainTransport>>,
    fails: AtomicBool,
}
#[async_trait]
impl RuntimeSettingsPlatform for Settings {
    async fn proxy(
        &self,
        scope: RuntimeProxyScope,
        configuration: Option<DomainProxy>,
    ) -> Result<(), RuntimeOperationError> {
        if self.fails.load(Ordering::Relaxed) {
            return Err(RuntimeOperationError::Unsupported);
        }
        lock(&self.writes).push((scope, configuration));
        Ok(())
    }
    async fn transport(&self, value: DomainTransport) -> Result<(), RuntimeOperationError> {
        if self.fails.load(Ordering::Relaxed) {
            return Err(RuntimeOperationError::Unsupported);
        }
        lock(&self.transports).push(value);
        Ok(())
    }
}
#[tokio::test]
async fn proxy_applies_only_selected_scope_and_persists_workspace_reset() {
    let platform = Settings::default();
    for (scope, configuration) in [
        (RuntimeProxyScope::App, Some(DomainProxy::Direct)),
        (RuntimeProxyScope::Workspace, Some(DomainProxy::System)),
        (RuntimeProxyScope::Request, Some(DomainProxy::Direct)),
        (RuntimeProxyScope::Workspace, None),
    ] {
        apply_proxy(
            configuration.clone(),
            scope,
            true,
            HashMap::new(),
            None,
            &platform,
        )
        .await
        .unwrap();
        assert_eq!(
            lock(&platform.writes).last().unwrap(),
            &(scope, configuration)
        );
    }
    assert!(
        apply_proxy(
            None,
            RuntimeProxyScope::App,
            true,
            HashMap::new(),
            None,
            &platform
        )
        .await
        .is_err()
    );
    assert!(
        apply_proxy(
            None,
            RuntimeProxyScope::Request,
            false,
            HashMap::new(),
            None,
            &platform
        )
        .await
        .is_err()
    );
    assert_eq!(lock(&platform.writes).len(), 4);
}
#[tokio::test]
async fn proxy_keychain_failure_and_failed_persistence_never_activate_policy() {
    let platform = Settings::default();
    let secrets = Platform::default();
    secrets.fails.store(true, Ordering::Relaxed);
    assert!(
        apply_proxy(
            Some(DomainProxy::Direct),
            RuntimeProxyScope::Workspace,
            true,
            HashMap::from([("proxy.user".into(), "fixture".into())]),
            Some(&secrets),
            &platform
        )
        .await
        .is_err()
    );
    assert!(lock(&platform.writes).is_empty());
    secrets.fails.store(false, Ordering::Relaxed);
    apply_proxy(
        Some(DomainProxy::Direct),
        RuntimeProxyScope::Workspace,
        true,
        HashMap::from([("proxy.user".into(), "fixture".into())]),
        Some(&secrets),
        &platform,
    )
    .await
    .unwrap();
    assert_eq!(lock(&secrets.values)["proxy.user"], "fixture");
    platform.fails.store(true, Ordering::Relaxed);
    assert!(
        apply_proxy(
            None,
            RuntimeProxyScope::Workspace,
            true,
            HashMap::new(),
            Some(&secrets),
            &platform
        )
        .await
        .is_err()
    );
    assert_eq!(lock(&platform.writes).len(), 1);
}
#[tokio::test]
async fn invalid_proxy_fails_before_any_platform_write() {
    let platform = Settings::default();
    let error = apply_proxy(
        Some(DomainProxy::Manual { routes: vec![] }),
        RuntimeProxyScope::Workspace,
        true,
        HashMap::new(),
        None,
        &platform,
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "Add at least one proxy route.");
    assert!(lock(&platform.writes).is_empty());
}
#[tokio::test]
async fn transport_writes_coalesce_and_rollback_on_failure() {
    for fails in [false, true] {
        let settings = Arc::new(RuntimeTransportSettings::default());
        let platform = Arc::new(Settings::default());
        platform.fails.store(fails, Ordering::Relaxed);
        let before = DomainTransport::default();
        let first = DomainTransport {
            total_timeout_ms: 4321,
            ..before.clone()
        };
        let latest = DomainTransport {
            follow_redirects: true,
            ..first.clone()
        };
        let old = settings.update(before.clone(), first.clone());
        let generation = settings.update(first, latest.clone());
        assert!(settings.save_pending(old, platform.clone()).await.is_none());
        let snapshot = settings
            .save_pending(generation, platform.clone())
            .await
            .unwrap();
        assert_eq!(snapshot.failed, fails);
        assert_eq!(snapshot.value, if fails { before } else { latest.clone() });
        if !fails {
            assert_eq!(*lock(&platform.transports), [latest]);
        }
    }
}
#[tokio::test]
async fn resolved_curl_exports_keychain_references_and_effective_proxy() {
    let transport = Transport::default();
    lock(&transport.platform.values).extend([
        ("token".into(), "fixture\n".into()),
        ("proxy.user".into(), "user".into()),
        ("proxy.password".into(), "pass".into()),
    ]);
    let mut context = context();
    context.app_proxy = DomainProxy::Manual {
        routes: vec![DomainProxyRoute {
            destination: "all".into(),
            endpoint: "http://localhost:8080".into(),
            credentials: Some(DomainProxyCredentials {
                username: "proxy.user".into(),
                password: "proxy.password".into(),
            }),
        }],
    };
    context
        .variables
        .insert("host".into(), DomainValueSource::literal("example.com"));
    let draft = DomainRequestDraft {
        url: "https://{{host}}/".into(),
        authentication: DomainAuthentication::Bearer {
            token: DomainValueSource::Secret {
                secret: "token".into(),
            },
        },
        ..Default::default()
    };
    let command = curl_command(draft, context, &transport).await.unwrap();
    assert!(command.contains("https://example.com/"));
    assert!(command.contains("localhost:8080"));
    assert!(command.contains("security find-generic-password"));
    assert!(command.contains("xxd -r -p"));
    assert!(command.contains("proxy.user"));
    assert!(!command.contains("fixture"));
}
#[tokio::test]
async fn curl_missing_secret_preserves_validation_failure() {
    let error = curl_command(
        DomainRequestDraft {
            url: "https://example.com".into(),
            authentication: DomainAuthentication::Bearer {
                token: DomainValueSource::Secret {
                    secret: "missing".into(),
                },
            },
            ..Default::default()
        },
        context(),
        &Transport::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.kind,
        crate::response::ResponseFailureKind::InvalidRequest
    );
    assert_eq!(
        error.issues[0].kind,
        crate::response::ResponseIssueKind::MissingSecret
    );
}
#[tokio::test]
async fn socket_captures_policy_and_inherits_transport_until_reconnect() {
    let mut context = context();
    context.app_proxy = DomainProxy::Direct;
    context.transport.total_timeout_ms = 123;
    let input = socket_input(
        DomainRequestDraft {
            url: "wss://example.com".into(),
            ..Default::default()
        },
        context.clone(),
        RuntimeCredentials {
            secrets: Arc::new(SessionSecrets::default()),
            platform: None,
        },
        None,
    )
    .await
    .unwrap();
    context.app_proxy = DomainProxy::System;
    assert_eq!(input.app_proxy, Some(DomainProxy::Direct));
    assert_eq!(input.total_timeout_ms, 123);
    assert_eq!(
        context.input(DomainRequestDraft::default()).app_proxy,
        Some(DomainProxy::System)
    );
}
