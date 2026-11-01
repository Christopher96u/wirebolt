//! Proxy editor policy and isolated connection-test messages.
use super::models::{
    DomainProxy, DomainProxyCredentials, DomainProxyRoute, DomainRequestDraft, DomainTransport,
};
use std::collections::HashSet;
use url::Url;

pub const PROTOCOLS: [&str; 6] = ["http", "https", "socks4", "socks4a", "socks5", "socks5h"];
/// # Errors
/// Returns a user-facing explanation when proxy routes overlap or an endpoint is invalid.
pub fn validate(proxy: &DomainProxy) -> Result<(), String> {
    let DomainProxy::Manual { routes } = proxy else {
        return Ok(());
    };
    if routes.is_empty() {
        return Err("Add at least one proxy route.".into());
    }
    let mut destinations = HashSet::new();
    for route in routes {
        if !["all", "http", "https"].contains(&route.destination.as_str()) {
            return Err("Choose All, HTTP or HTTPS destinations.".into());
        }
        let covered = if route.destination == "all" {
            vec!["http", "https"]
        } else {
            vec![route.destination.as_str()]
        };
        for destination in covered {
            if !destinations.insert(destination) {
                return Err("Proxy routes overlap. Use one route for all traffic, or separate HTTP and HTTPS routes.".into());
            }
        }
        let raw_scheme = route
            .endpoint
            .split_once(':')
            .map_or("", |(scheme, _)| scheme);
        let authority = route
            .endpoint
            .split_once("://")
            .map_or("", |(_, rest)| rest);
        let raw_path = authority.find('/').map_or("", |i| &authority[i..]);
        if !PROTOCOLS.contains(&raw_scheme)
            || route.endpoint.chars().any(char::is_whitespace)
            || (!raw_path.is_empty() && raw_path != "/")
            || route.endpoint.contains(['?', '#'])
        {
            return Err(
                "Use a valid proxy host and port, without a path or embedded credentials.".into(),
            );
        }
        let valid = Url::parse(&route.endpoint).ok().filter(|u| {
            PROTOCOLS.contains(&u.scheme())
                && u.host_str()
                    .is_some_and(|s| !s.is_empty() && !s.chars().any(char::is_whitespace))
                && u.username().is_empty()
                && u.password().is_none()
                && (u.path().is_empty() || u.path() == "/")
                && u.query().is_none()
                && u.fragment().is_none()
                && u.port() != Some(0)
                && !route.endpoint.contains('@')
        });
        let Some(url) = valid else {
            return Err(
                "Use a valid proxy host and port, without a path or embedded credentials.".into(),
            );
        };
        if route.credentials.is_some() && ["socks4", "socks4a"].contains(&url.scheme()) {
            return Err(
                "SOCKS4 does not support username/password authentication. Choose SOCKS5.".into(),
            );
        }
    }
    Ok(())
}
#[must_use]
pub fn resolve(
    request: Option<DomainProxy>,
    workspace: Option<DomainProxy>,
    app: DomainProxy,
) -> (DomainProxy, String) {
    request
        .map(|p| (p, "request".into()))
        .or_else(|| workspace.map(|p| (p, "workspace".into())))
        .unwrap_or((app, "app_default".into()))
}
#[must_use]
pub fn summary(proxy: &DomainProxy, url: Option<&str>) -> String {
    match proxy {
        DomainProxy::System => "System proxy".into(),
        DomainProxy::Direct => "Direct · No proxy".into(),
        DomainProxy::Manual { routes } => {
            let scheme = url.and_then(url_scheme).map(|s| match s.as_str() {
                "ws" => "http".into(),
                "wss" => "https".into(),
                _ => s,
            });
            let route = scheme
                .as_ref()
                .and_then(|s| {
                    routes
                        .iter()
                        .find(|r| r.destination == "all" || r.destination == *s)
                })
                .or_else(|| {
                    if scheme.is_none() {
                        routes.first()
                    } else {
                        None
                    }
                });
            let Some(route) = route else {
                return "Direct · No matching proxy route".into();
            };
            let Ok(endpoint) = Url::parse(&route.endpoint) else {
                return "Proxy configuration error".into();
            };
            let Some(host) = endpoint.host_str() else {
                return "Proxy configuration error".into();
            };
            // URLComponents preserves an explicitly written default port.
            let port = explicit_port(&route.endpoint).map_or(String::new(), |p| format!(":{p}"));
            format!("{} · {host}{port}", endpoint.scheme().to_uppercase())
        }
    }
}
fn explicit_port(endpoint: &str) -> Option<u16> {
    let authority = endpoint.split_once("://")?.1.split('/').next()?;
    let (_, port) = authority.rsplit_once(':')?;
    port.parse().ok()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyRouteInput {
    pub id: String,
    pub destination: String,
    pub scheme: String,
    pub host: String,
    pub port: String,
    pub authenticated: bool,
    pub replace_credentials: bool,
    pub username: String,
    pub password: String,
    pub credentials: Option<DomainProxyCredentials>,
}
#[must_use]
pub fn route_input(route: Option<DomainProxyRoute>) -> ProxyRouteInput {
    let mut input = ProxyRouteInput {
        id: super::request::unique_id(),
        destination: "all".into(),
        scheme: "http".into(),
        host: String::new(),
        port: "8080".into(),
        authenticated: false,
        replace_credentials: false,
        username: String::new(),
        password: String::new(),
        credentials: None,
    };
    if let Some(r) = route {
        let u = Url::parse(&r.endpoint).ok();
        input.destination = r.destination;
        input.scheme = u.as_ref().map_or("http", Url::scheme).into();
        input.host = u
            .as_ref()
            .and_then(Url::host_str)
            .unwrap_or_default()
            .into();
        input.port = explicit_port(&r.endpoint).map_or(String::new(), |p| p.to_string());
        input.authenticated = r.credentials.is_some();
        input.credentials = r.credentials;
    }
    input
}
#[must_use]
pub fn needs_credentials(input: &ProxyRouteInput) -> bool {
    input.authenticated && (input.credentials.is_none() || input.replace_credentials)
}
#[must_use]
pub fn credential_references(input: &ProxyRouteInput) -> Option<DomainProxyCredentials> {
    if !input.authenticated {
        return None;
    }
    if !needs_credentials(input) {
        return input.credentials.clone();
    }
    Some(DomainProxyCredentials {
        username: format!("proxy.{}.user", input.id.to_lowercase()),
        password: format!("proxy.{}.password", input.id.to_lowercase()),
    })
}
/// # Errors
/// Returns a user-facing explanation for invalid host, port, credentials or proxy policy.
pub fn route_document(input: &ProxyRouteInput) -> Result<DomainProxyRoute, String> {
    let host = input.host.trim();
    if host.is_empty()
        || host
            .chars()
            .any(|c| "/@?#".contains(c) || c.is_whitespace())
        || host.contains("://")
    {
        return Err("Enter a host name or IP address, without a protocol or path.".into());
    }
    if needs_credentials(input) && (input.username.is_empty() || input.password.is_empty()) {
        return Err("Enter both the proxy username and password.".into());
    }
    if !input.port.is_empty() && !input.port.parse::<u16>().is_ok_and(|p| p > 0) {
        return Err("Port must be between 1 and 65535.".into());
    }
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.into()
    };
    let port = if input.port.is_empty() {
        String::new()
    } else {
        format!(":{}", input.port)
    };
    let route = DomainProxyRoute {
        destination: input.destination.clone(),
        endpoint: format!("{}://{host}{port}", input.scheme),
        credentials: credential_references(input),
    };
    validate(&DomainProxy::Manual {
        routes: vec![route.clone()],
    })?;
    Ok(route)
}
#[must_use]
pub fn valid_target(value: &str) -> bool {
    !value.contains("{{")
        && Url::parse(value).is_ok_and(|u| {
            ["http", "https"].contains(&u.scheme())
                && u.host_str().is_some_and(|h| !h.is_empty())
                && u.username().is_empty()
                && u.password().is_none()
                && !value.contains('@')
        })
}
#[must_use]
pub fn test_result(status: Option<u16>, failure: Option<&str>) -> (bool, String) {
    if let Some(category) = failure {
        return (
            false,
            match category {
                "timeout" => "Connection timed out after 5 seconds.",
                "tls" => "TLS verification failed. Check the certificate and trust settings.",
                "proxy" | "secrets" => "Proxy configuration or credentials could not be loaded.",
                "cancelled" => "Test cancelled.",
                _ => "Connection failed. Check the proxy address, port and destination.",
            }
            .into(),
        );
    }
    match status {
        Some(407) => (
            false,
            "Proxy authentication failed (HTTP 407). Check your credentials.".into(),
        ),
        Some(s) => (true, format!("Reached destination · HTTP {s}")),
        None => (false, "No HTTP response received.".into()),
    }
}
/// # Errors
/// Returns a user-facing explanation for an invalid destination or proxy policy.
pub fn test_draft(configuration: DomainProxy, url: String) -> Result<DomainRequestDraft, String> {
    if !valid_target(&url) || validate(&configuration).is_err() {
        return Err("Enter a valid HTTP or HTTPS URL and proxy configuration.".into());
    }
    Ok(DomainRequestDraft {
        method: "HEAD".into(),
        url,
        proxy: Some(configuration),
        transport: DomainTransport {
            total_timeout_ms: 5000,
            read_timeout_ms: 5000,
            ..DomainTransport::default()
        },
        ..DomainRequestDraft::default()
    })
}
#[must_use]
pub fn route_secrets(input: &ProxyRouteInput) -> std::collections::HashMap<String, String> {
    if !needs_credentials(input) {
        return std::collections::HashMap::new();
    }
    credential_references(input).map_or_else(std::collections::HashMap::new, |c| {
        std::collections::HashMap::from([
            (c.username, input.username.clone()),
            (c.password, input.password.clone()),
        ])
    })
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ProbeState {
    pub is_running: bool,
    pub message: Option<String>,
    pub succeeded: bool,
    pub run_id: Option<String>,
}
#[derive(Debug, Default)]
pub struct ConnectionProbe {
    pub state: ProbeState,
}
impl ConnectionProbe {
    pub fn start(&mut self, configuration: DomainProxy, url: String) -> Option<DomainRequestDraft> {
        self.cancel();
        match test_draft(configuration, url) {
            Ok(draft) => {
                self.state = ProbeState {
                    is_running: true,
                    message: None,
                    succeeded: false,
                    run_id: Some(super::request::unique_id()),
                };
                Some(draft)
            }
            Err(message) => {
                self.state.message = Some(message);
                None
            }
        }
    }
    pub fn cancel(&mut self) {
        if self.state.run_id.take().is_some() {
            self.state.message = Some("Test cancelled.".into());
            self.state.succeeded = false;
        }
        self.state.is_running = false;
    }
    pub fn reset(&mut self) {
        self.cancel();
        self.state.message = None;
        self.state.succeeded = false;
    }
    #[must_use]
    pub fn active(&self, id: &str) -> bool {
        self.state.run_id.as_deref() == Some(id)
    }
    pub fn complete(&mut self, id: &str, status: Option<u16>, failure: Option<&str>) {
        if !self.active(id) {
            return;
        }
        let (succeeded, message) = test_result(status, failure);
        self.state = ProbeState {
            is_running: false,
            message: Some(message),
            succeeded,
            run_id: None,
        };
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyFormInput {
    pub mode: String,
    pub routes: Vec<ProxyRouteInput>,
}
#[must_use]
pub fn form_input(configuration: Option<DomainProxy>) -> ProxyFormInput {
    let (mode, routes) = match configuration {
        None => ("inherit", vec![route_input(None)]),
        Some(DomainProxy::System) => ("system", vec![route_input(None)]),
        Some(DomainProxy::Direct) => ("direct", vec![route_input(None)]),
        Some(DomainProxy::Manual { routes }) => (
            "manual",
            routes.into_iter().map(|r| route_input(Some(r))).collect(),
        ),
    };
    ProxyFormInput {
        mode: mode.into(),
        routes,
    }
}
/// # Errors
/// Returns a user-facing explanation for an invalid route or overlapping proxy routes.
pub fn form_document(input: &ProxyFormInput) -> Result<Option<DomainProxy>, String> {
    match input.mode.as_str() {
        "system" => Ok(Some(DomainProxy::System)),
        "direct" => Ok(Some(DomainProxy::Direct)),
        "manual" => {
            let proxy = DomainProxy::Manual {
                routes: input
                    .routes
                    .iter()
                    .map(route_document)
                    .collect::<Result<_, _>>()?,
            };
            validate(&proxy)?;
            Ok(Some(proxy))
        }
        _ => Ok(None),
    }
}
#[must_use]
pub fn form_secrets(input: &ProxyFormInput) -> std::collections::HashMap<String, String> {
    let mut secrets = std::collections::HashMap::new();
    if input.mode == "manual" {
        for r in &input.routes {
            secrets.extend(route_secrets(r));
        }
    }
    secrets
}
#[must_use]
pub fn needs_indicator(configuration: &DomainProxy, source: &str) -> bool {
    validate(configuration).is_err() || source == "request" || source == "workspace"
}
#[must_use]
pub fn scope_title(scope: &str) -> String {
    match scope {
        "workspace" => "Workspace",
        "request" => "Request",
        "system_default" => "System default",
        _ => "App default",
    }
    .into()
}

#[must_use]
pub fn url_scheme(value: &str) -> Option<String> {
    let (scheme, _) = value.split_once(':')?;
    let mut bytes = scheme.bytes();
    if !bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        || !bytes.all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))
    {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}
