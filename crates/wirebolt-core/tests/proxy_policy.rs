use wirebolt_core::{
    ManualProxy, ProxyConfigurationErrorKind, ProxyDestination, ProxyEndpoint, ProxyMode,
    ProxyModeKind, ProxyPolicy, ProxyProtocol, ProxyRoute, ProxySource,
};

#[cfg(target_os = "macos")]
use wirebolt_core::{
    KeychainSecretResolver, SecretName, SecretResolutionErrorKind, SecretResolver,
};

#[test]
fn resolves_request_then_workspace_then_system_proxy_policy() {
    let system = ProxyPolicy::default().resolve(None);
    assert_eq!(system.source(), ProxySource::SystemDefault);
    assert_eq!(system.mode(), &ProxyMode::System);
    assert_eq!(system.diagnostic().mode(), ProxyModeKind::System);

    let policy = ProxyPolicy::with_workspace(ProxyMode::Direct);
    let workspace = policy.resolve(None);
    assert_eq!(workspace.source(), ProxySource::Workspace);
    assert_eq!(workspace.mode(), &ProxyMode::Direct);
    assert_eq!(workspace.diagnostic().mode(), ProxyModeKind::Direct);

    let request = policy.resolve(Some(&ProxyMode::System));
    assert_eq!(request.source(), ProxySource::Request);
    assert_eq!(request.mode(), &ProxyMode::System);
}

#[test]
fn describes_a_manual_socks_route_without_credentials() {
    let endpoint = ProxyEndpoint::new("socks5h://proxy.internal:1080").expect("proxy endpoint");
    let manual = ManualProxy::new(vec![ProxyRoute::new(ProxyDestination::All, endpoint)])
        .expect("manual proxy");
    let resolved = ProxyPolicy::with_workspace(ProxyMode::Manual(manual)).resolve(None);
    let diagnostic = resolved.diagnostic();

    assert_eq!(diagnostic.mode(), ProxyModeKind::Manual);
    assert_eq!(diagnostic.source(), ProxySource::Workspace);
    assert_eq!(diagnostic.routes().len(), 1);
    assert_eq!(diagnostic.routes()[0].destination(), ProxyDestination::All);
    assert_eq!(diagnostic.routes()[0].protocol(), ProxyProtocol::Socks5h);
    assert_eq!(
        diagnostic.routes()[0].endpoint(),
        "socks5h://proxy.internal:1080/"
    );
    assert!(!diagnostic.routes()[0].authenticated());
}

#[test]
fn rejects_proxy_endpoints_that_could_embed_secrets() {
    for endpoint in [
        "http://user:super-secret@proxy.internal:8080",
        "http://proxy.internal:8080/path?token=super-secret",
    ] {
        let error = ProxyEndpoint::new(endpoint).expect_err("unsafe endpoint must fail");
        assert!(!format!("{error:?}").contains("super-secret"));
    }
}

#[test]
fn rejects_credentials_on_socks4_routes_which_cannot_carry_them() {
    let credentials = || {
        wirebolt_core::ProxyCredentials::new(
            wirebolt_core::SecretName::new("proxy.user").expect("username"),
            wirebolt_core::SecretName::new("proxy.password").expect("password"),
        )
    };
    for endpoint in [
        "socks4://proxy.internal:1080",
        "socks4a://proxy.internal:1080",
    ] {
        let route = ProxyRoute::new(
            ProxyDestination::All,
            ProxyEndpoint::new(endpoint).expect("proxy endpoint"),
        )
        .with_credentials(credentials());
        let error = ManualProxy::new(vec![route]).expect_err("SOCKS4 credentials must fail");
        assert_eq!(
            error.kind(),
            ProxyConfigurationErrorKind::UnsupportedCredentials,
            "{endpoint}"
        );
    }

    let route = ProxyRoute::new(
        ProxyDestination::All,
        ProxyEndpoint::new("socks5h://proxy.internal:1080").expect("proxy endpoint"),
    )
    .with_credentials(credentials());
    ManualProxy::new(vec![route]).expect("SOCKS5 carries credentials");
}

#[test]
fn rejects_overlapping_manual_proxy_routes() {
    let endpoint = || ProxyEndpoint::new("http://proxy.internal:8080").expect("proxy endpoint");
    ManualProxy::new(vec![
        ProxyRoute::new(ProxyDestination::Http, endpoint()),
        ProxyRoute::new(ProxyDestination::Https, endpoint()),
    ])
    .expect("separate HTTP and HTTPS routes");

    let error = ManualProxy::new(vec![
        ProxyRoute::new(ProxyDestination::All, endpoint()),
        ProxyRoute::new(ProxyDestination::Http, endpoint()),
    ])
    .expect_err("all traffic overlaps HTTP");

    assert_eq!(error.kind(), ProxyConfigurationErrorKind::OverlappingRoutes);
}

#[cfg(target_os = "macos")]
#[test]
fn reports_a_missing_apple_keychain_secret_without_modifying_keychain() {
    let resolver = KeychainSecretResolver::new(format!(
        "local.wirebolt.tests.missing.{}",
        std::process::id()
    ));
    let name = SecretName::new("does-not-exist").expect("secret name");

    let error = resolver.resolve(&name).expect_err("secret must not exist");

    assert_eq!(error.kind(), SecretResolutionErrorKind::NotFound);
}
