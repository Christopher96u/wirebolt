use std::{error::Error, fmt};

use http::Uri;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::SecretName;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProxyMode {
    System,
    Direct,
    Manual(ManualProxy),
}

impl Serialize for ProxyMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::System => ProxyModeRef::System.serialize(serializer),
            Self::Direct => ProxyModeRef::Direct.serialize(serializer),
            Self::Manual(manual) => ProxyModeRef::Manual {
                routes: &manual.routes,
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ProxyMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match ProxyModeDocument::deserialize(deserializer)? {
            ProxyModeDocument::System => Ok(Self::System),
            ProxyModeDocument::Direct => Ok(Self::Direct),
            ProxyModeDocument::Manual { routes } => ManualProxy::new(routes)
                .map(Self::Manual)
                .map_err(de::Error::custom),
        }
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "mode")]
enum ProxyModeRef<'a> {
    System,
    Direct,
    Manual { routes: &'a [ProxyRoute] },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "mode")]
enum ProxyModeDocument {
    System,
    Direct,
    Manual { routes: Vec<ProxyRoute> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyModeKind {
    System,
    Direct,
    Manual,
}

impl From<&ProxyMode> for ProxyModeKind {
    fn from(mode: &ProxyMode) -> Self {
        match mode {
            ProxyMode::System => Self::System,
            ProxyMode::Direct => Self::Direct,
            ProxyMode::Manual(_) => Self::Manual,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProxyPolicy {
    workspace: Option<ProxyMode>,
}

impl ProxyPolicy {
    #[must_use]
    pub const fn new(workspace: Option<ProxyMode>) -> Self {
        Self { workspace }
    }

    #[must_use]
    pub fn with_workspace(mode: ProxyMode) -> Self {
        Self::new(Some(mode))
    }

    #[must_use]
    pub fn resolve(&self, request: Option<&ProxyMode>) -> ResolvedProxy {
        if let Some(mode) = request {
            return ResolvedProxy {
                mode: mode.clone(),
                source: ProxySource::Request,
            };
        }
        if let Some(mode) = &self.workspace {
            return ResolvedProxy {
                mode: mode.clone(),
                source: ProxySource::Workspace,
            };
        }
        ResolvedProxy {
            mode: ProxyMode::System,
            source: ProxySource::SystemDefault,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxySource {
    SystemDefault,
    Workspace,
    Request,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedProxy {
    mode: ProxyMode,
    source: ProxySource,
}

impl ResolvedProxy {
    #[must_use]
    pub const fn mode(&self) -> &ProxyMode {
        &self.mode
    }

    #[must_use]
    pub const fn source(&self) -> ProxySource {
        self.source
    }

    #[must_use]
    pub fn diagnostic(&self) -> ProxyDiagnostic {
        ProxyDiagnostic {
            mode: (&self.mode).into(),
            source: self.source,
            routes: match &self.mode {
                ProxyMode::Manual(manual) => manual
                    .routes
                    .iter()
                    .map(ProxyRouteDiagnostic::from)
                    .collect(),
                ProxyMode::System | ProxyMode::Direct => Vec::new(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyDiagnostic {
    mode: ProxyModeKind,
    source: ProxySource,
    routes: Vec<ProxyRouteDiagnostic>,
}

impl ProxyDiagnostic {
    #[must_use]
    pub const fn mode(&self) -> ProxyModeKind {
        self.mode
    }

    #[must_use]
    pub const fn source(&self) -> ProxySource {
        self.source
    }

    #[must_use]
    pub fn routes(&self) -> &[ProxyRouteDiagnostic] {
        &self.routes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualProxy {
    routes: Vec<ProxyRoute>,
}

impl ManualProxy {
    /// Creates a manual proxy with at least one route.
    ///
    /// # Errors
    ///
    /// Returns [`ProxyConfigurationError`] when no routes are provided, routes
    /// overlap, or credentials are attached to a SOCKS4 endpoint, which cannot
    /// carry them.
    pub fn new(routes: Vec<ProxyRoute>) -> Result<Self, ProxyConfigurationError> {
        if routes.is_empty() {
            return Err(ProxyConfigurationError::new(
                ProxyConfigurationErrorKind::NoRoutes,
            ));
        }
        let mut covers_http = false;
        let mut covers_https = false;
        for route in &routes {
            if route.credentials.is_some() && !route.endpoint.protocol().supports_credentials() {
                return Err(ProxyConfigurationError::new(
                    ProxyConfigurationErrorKind::UnsupportedCredentials,
                ));
            }
            let overlaps = match route.destination {
                ProxyDestination::All => covers_http || covers_https,
                ProxyDestination::Http => covers_http,
                ProxyDestination::Https => covers_https,
            };
            if overlaps {
                return Err(ProxyConfigurationError::new(
                    ProxyConfigurationErrorKind::OverlappingRoutes,
                ));
            }
            match route.destination {
                ProxyDestination::All => {
                    covers_http = true;
                    covers_https = true;
                }
                ProxyDestination::Http => covers_http = true,
                ProxyDestination::Https => covers_https = true,
            }
        }
        Ok(Self { routes })
    }

    pub(crate) fn routes(&self) -> &[ProxyRoute] {
        &self.routes
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyDestination {
    All,
    Http,
    Https,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyRoute {
    destination: ProxyDestination,
    endpoint: ProxyEndpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credentials: Option<ProxyCredentials>,
}

impl ProxyRoute {
    #[must_use]
    pub const fn new(destination: ProxyDestination, endpoint: ProxyEndpoint) -> Self {
        Self {
            destination,
            endpoint,
            credentials: None,
        }
    }

    #[must_use]
    pub fn with_credentials(mut self, credentials: ProxyCredentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub(crate) const fn destination(&self) -> ProxyDestination {
        self.destination
    }

    pub(crate) const fn endpoint(&self) -> &ProxyEndpoint {
        &self.endpoint
    }

    pub(crate) const fn credentials(&self) -> Option<&ProxyCredentials> {
        self.credentials.as_ref()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyCredentials {
    username: SecretName,
    password: SecretName,
}

impl ProxyCredentials {
    #[must_use]
    pub const fn new(username: SecretName, password: SecretName) -> Self {
        Self { username, password }
    }

    pub(crate) const fn username(&self) -> &SecretName {
        &self.username
    }

    pub(crate) const fn password(&self) -> &SecretName {
        &self.password
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyEndpoint {
    uri: Uri,
    protocol: ProxyProtocol,
}

impl Serialize for ProxyEndpoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.normalized())
    }
}

impl<'de> Deserialize<'de> for ProxyEndpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(&String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl ProxyEndpoint {
    /// Validates a proxy endpoint URI.
    ///
    /// # Errors
    ///
    /// Returns [`ProxyConfigurationError`] for an invalid or unsupported URI.
    pub fn new(value: &str) -> Result<Self, ProxyConfigurationError> {
        let uri = value.parse::<Uri>().map_err(|_| {
            ProxyConfigurationError::new(ProxyConfigurationErrorKind::InvalidEndpoint)
        })?;
        let protocol = match uri.scheme_str() {
            Some("http") => ProxyProtocol::Http,
            Some("https") => ProxyProtocol::Https,
            Some("socks4") => ProxyProtocol::Socks4,
            Some("socks4a") => ProxyProtocol::Socks4a,
            Some("socks5") => ProxyProtocol::Socks5,
            Some("socks5h") => ProxyProtocol::Socks5h,
            _ => {
                return Err(ProxyConfigurationError::new(
                    ProxyConfigurationErrorKind::InvalidEndpoint,
                ));
            }
        };
        let has_embedded_credentials = uri
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'));
        let has_path = !matches!(uri.path(), "" | "/");
        let has_query = uri
            .path_and_query()
            .and_then(http::uri::PathAndQuery::query)
            .is_some();
        if uri.host().is_none() || has_embedded_credentials || has_path || has_query {
            return Err(ProxyConfigurationError::new(
                ProxyConfigurationErrorKind::InvalidEndpoint,
            ));
        }
        Ok(Self { uri, protocol })
    }

    #[must_use]
    pub const fn protocol(&self) -> ProxyProtocol {
        self.protocol
    }

    pub(crate) fn normalized(&self) -> String {
        let mut value = self.uri.to_string();
        if self.uri.path().is_empty() {
            value.push('/');
        }
        value
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyProtocol {
    Http,
    Https,
    Socks4,
    Socks4a,
    Socks5,
    Socks5h,
}

impl ProxyProtocol {
    /// SOCKS4 has no authentication exchange, so credentials cannot be sent.
    #[must_use]
    pub const fn supports_credentials(self) -> bool {
        !matches!(self, Self::Socks4 | Self::Socks4a)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyRouteDiagnostic {
    destination: ProxyDestination,
    protocol: ProxyProtocol,
    endpoint: String,
    authenticated: bool,
}

impl ProxyRouteDiagnostic {
    #[must_use]
    pub const fn destination(&self) -> ProxyDestination {
        self.destination
    }

    #[must_use]
    pub const fn protocol(&self) -> ProxyProtocol {
        self.protocol
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    #[must_use]
    pub const fn authenticated(&self) -> bool {
        self.authenticated
    }
}

impl From<&ProxyRoute> for ProxyRouteDiagnostic {
    fn from(route: &ProxyRoute) -> Self {
        Self {
            destination: route.destination,
            protocol: route.endpoint.protocol(),
            endpoint: route.endpoint.normalized(),
            authenticated: route.credentials.is_some(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyConfigurationErrorKind {
    InvalidEndpoint,
    NoRoutes,
    OverlappingRoutes,
    UnsupportedCredentials,
    SecretNotFound,
    SecretUnavailable,
    InvalidSecretEncoding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProxyConfigurationError {
    kind: ProxyConfigurationErrorKind,
    secret_name: Option<SecretName>,
}

impl ProxyConfigurationError {
    const fn new(kind: ProxyConfigurationErrorKind) -> Self {
        Self {
            kind,
            secret_name: None,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> ProxyConfigurationErrorKind {
        self.kind
    }

    pub(crate) const fn invalid_endpoint() -> Self {
        Self::new(ProxyConfigurationErrorKind::InvalidEndpoint)
    }

    pub(crate) const fn unsupported_credentials() -> Self {
        Self::new(ProxyConfigurationErrorKind::UnsupportedCredentials)
    }

    pub(crate) fn secret(name: &SecretName, error: &SecretResolutionError) -> Self {
        let kind = match error.kind() {
            SecretResolutionErrorKind::NotFound => ProxyConfigurationErrorKind::SecretNotFound,
            SecretResolutionErrorKind::Unavailable => {
                ProxyConfigurationErrorKind::SecretUnavailable
            }
            SecretResolutionErrorKind::InvalidEncoding => {
                ProxyConfigurationErrorKind::InvalidSecretEncoding
            }
        };
        Self {
            kind,
            secret_name: Some(name.clone()),
        }
    }

    #[must_use]
    pub const fn secret_name(&self) -> Option<&SecretName> {
        self.secret_name.as_ref()
    }
}

impl fmt::Display for ProxyConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ProxyConfigurationErrorKind::InvalidEndpoint => {
                formatter.write_str("invalid proxy endpoint")
            }
            ProxyConfigurationErrorKind::NoRoutes => {
                formatter.write_str("manual proxy requires at least one route")
            }
            ProxyConfigurationErrorKind::OverlappingRoutes => {
                formatter.write_str("manual proxy routes overlap")
            }
            ProxyConfigurationErrorKind::UnsupportedCredentials => {
                formatter.write_str("SOCKS4 proxies cannot carry credentials")
            }
            ProxyConfigurationErrorKind::SecretNotFound => write!(
                formatter,
                "proxy secret '{}' was not found",
                self.secret_name
                    .as_ref()
                    .map_or("unknown", SecretName::as_str)
            ),
            ProxyConfigurationErrorKind::SecretUnavailable => write!(
                formatter,
                "proxy secret '{}' is unavailable",
                self.secret_name
                    .as_ref()
                    .map_or("unknown", SecretName::as_str)
            ),
            ProxyConfigurationErrorKind::InvalidSecretEncoding => write!(
                formatter,
                "proxy secret '{}' is not valid UTF-8",
                self.secret_name
                    .as_ref()
                    .map_or("unknown", SecretName::as_str)
            ),
        }
    }
}

impl Error for ProxyConfigurationError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretResolutionErrorKind {
    NotFound,
    Unavailable,
    InvalidEncoding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretResolutionError {
    kind: SecretResolutionErrorKind,
}

impl SecretResolutionError {
    #[must_use]
    pub const fn new(kind: SecretResolutionErrorKind) -> Self {
        Self { kind }
    }

    #[must_use]
    pub const fn kind(&self) -> SecretResolutionErrorKind {
        self.kind
    }
}

impl fmt::Display for SecretResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            SecretResolutionErrorKind::NotFound => formatter.write_str("secret not found"),
            SecretResolutionErrorKind::Unavailable => {
                formatter.write_str("secret store unavailable")
            }
            SecretResolutionErrorKind::InvalidEncoding => {
                formatter.write_str("secret is not valid UTF-8")
            }
        }
    }
}

impl Error for SecretResolutionError {}

pub trait SecretResolver: Send + Sync {
    /// Resolves a named secret without exposing it through diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`SecretResolutionError`] when the value cannot be retrieved.
    fn resolve(&self, name: &SecretName) -> Result<ResolvedSecret, SecretResolutionError>;
}

#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedSecret(String);

impl ResolvedSecret {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ResolvedSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResolvedSecret([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoSecrets;

impl SecretResolver for NoSecrets {
    fn resolve(&self, _name: &SecretName) -> Result<ResolvedSecret, SecretResolutionError> {
        Err(SecretResolutionError::new(
            SecretResolutionErrorKind::NotFound,
        ))
    }
}

#[cfg(target_vendor = "apple")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeychainSecretResolver {
    service: String,
}

#[cfg(target_vendor = "apple")]
impl KeychainSecretResolver {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

#[cfg(target_vendor = "apple")]
impl Default for KeychainSecretResolver {
    fn default() -> Self {
        Self::new("local.wirebolt.app")
    }
}

#[cfg(target_vendor = "apple")]
impl SecretResolver for KeychainSecretResolver {
    fn resolve(&self, name: &SecretName) -> Result<ResolvedSecret, SecretResolutionError> {
        use security_framework::passwords::{PasswordOptions, generic_password};
        use security_framework_sys::base::errSecItemNotFound;

        let options = PasswordOptions::new_generic_password(&self.service, name.as_str());
        match generic_password(options) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(ResolvedSecret::new)
                .map_err(|_| {
                    SecretResolutionError::new(SecretResolutionErrorKind::InvalidEncoding)
                }),
            Err(error) if error.code() == errSecItemNotFound => Err(SecretResolutionError::new(
                SecretResolutionErrorKind::NotFound,
            )),
            Err(_) => Err(SecretResolutionError::new(
                SecretResolutionErrorKind::Unavailable,
            )),
        }
    }
}

#[cfg(target_vendor = "apple")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeychainSecretStore {
    service: String,
}

#[cfg(target_vendor = "apple")]
impl KeychainSecretStore {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Creates or replaces a named secret in the user's Keychain.
    ///
    /// # Errors
    ///
    /// Returns [`SecretResolutionError`] when Keychain is unavailable.
    pub fn save(&self, name: &SecretName, value: &str) -> Result<(), SecretResolutionError> {
        security_framework::passwords::set_generic_password(
            &self.service,
            name.as_str(),
            value.as_bytes(),
        )
        .map_err(|_| SecretResolutionError::new(SecretResolutionErrorKind::Unavailable))
    }
}

#[cfg(target_vendor = "apple")]
impl Default for KeychainSecretStore {
    fn default() -> Self {
        Self::new("local.wirebolt.app")
    }
}
