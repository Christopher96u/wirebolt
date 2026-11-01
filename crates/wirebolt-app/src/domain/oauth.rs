//! OAuth 2 protocol computation over browser, credential, trust and native HTTP adapters.
mod network;
mod transport;
use super::models::{DomainOAuthConfiguration, DomainOAuthGrant};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use network::network_error;
pub use network::network_failure;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainOAuthError {
    InvalidConfiguration,
    AuthorizationCancelled,
    AuthorizationFailed,
    StateMismatch,
    MissingAuthorizationCode,
    ClientSecretUnavailable,
    TokenEndpointRejected {
        status: u16,
    },
    InvalidTokenResponse,
    TransportFailure {
        kind: DomainNetworkFailure,
        host: Option<String>,
    },
}
impl std::fmt::Display for DomainOAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration => f.write_str("OAuth 2 configuration is incomplete."),
            Self::AuthorizationCancelled => f.write_str("Authorization was cancelled."),
            Self::AuthorizationFailed => {
                f.write_str("The authorization server did not complete the sign-in.")
            }
            Self::StateMismatch => f.write_str("The OAuth 2 state did not match."),
            Self::MissingAuthorizationCode => {
                f.write_str("The authorization response did not include a code.")
            }
            Self::ClientSecretUnavailable => {
                f.write_str("The client secret is unavailable in Keychain.")
            }
            Self::TokenEndpointRejected { status } => {
                write!(f, "The token endpoint returned HTTP {status}.")
            }
            Self::TransportFailure { .. } => f.write_str("OAuth token exchange failed."),
            Self::InvalidTokenResponse => {
                f.write_str("The token endpoint returned an invalid response.")
            }
        }
    }
}
impl std::error::Error for DomainOAuthError {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainOAuthProxy {
    Direct,
    Proxy { endpoint: String },
}
#[async_trait::async_trait]
pub trait DomainOAuthPlatform: Send + Sync {
    /// Resolves only platform system settings, including destination bypass rules.
    /// Environment proxy variables are deliberately outside OAuth's transport policy.
    fn system_proxies(&self, _url: &str) -> Vec<DomainOAuthProxy> {
        vec![DomainOAuthProxy::Direct]
    }

    async fn request_headers(&self) -> DomainOAuthHeaders {
        DomainOAuthHeaders::default()
    }

    fn tls_verifier(&self) -> Option<Arc<dyn rustls::client::danger::ServerCertVerifier>> {
        None
    }

    async fn authorize(
        &self,
        url: String,
        callback_scheme: String,
    ) -> Result<String, DomainOAuthError>;
    async fn read_secret(&self, name: String) -> Option<String>;
}
#[derive(Clone, Debug, PartialEq)]
pub struct DomainTokenReceipt {
    pub token: String,
    pub expires_at: Option<f64>,
    pub scope: Option<String>,
}
#[must_use]
pub fn encode_form(form: &BTreeMap<String, String>) -> String {
    form.iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                super::url::encode_query(k).replace('+', "%2B"),
                super::url::encode_query(v).replace('+', "%2B")
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}
#[must_use]
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
/// # Errors
/// Returns an authorization failure if operating-system randomness is unavailable.
pub fn random_url_safe(count: usize) -> Result<String, DomainOAuthError> {
    let mut bytes = vec![0; count];
    getrandom::fill(&mut bytes).map_err(|_| DomainOAuthError::AuthorizationFailed)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
/// # Errors
/// Returns an invalid-configuration error for disallowed endpoints or callback URIs.
pub fn authorization_url(
    c: &DomainOAuthConfiguration,
    challenge: &str,
    state: &str,
) -> Result<(String, String), DomainOAuthError> {
    let mut url =
        Url::parse(&c.authorization_url).map_err(|_| DomainOAuthError::InvalidConfiguration)?;
    if url.scheme() != "https" && url.host_str() != Some("localhost") {
        return Err(DomainOAuthError::InvalidConfiguration);
    }
    let callback = Url::parse(&c.redirect_uri)
        .map_err(|_| DomainOAuthError::InvalidConfiguration)?
        .scheme()
        .to_owned();
    let mut items: Vec<(String, Option<String>)> = url
        .query()
        .filter(|q| !q.is_empty())
        .into_iter()
        .flat_map(|q| q.split('&'))
        .map(|item| {
            let (name, value) = item
                .split_once('=')
                .map_or((item, None), |(n, v)| (n, Some(v)));
            (
                percent_encoding::percent_decode_str(name)
                    .decode_utf8_lossy()
                    .into_owned(),
                value.map(|v| {
                    percent_encoding::percent_decode_str(v)
                        .decode_utf8_lossy()
                        .into_owned()
                }),
            )
        })
        .collect();
    items.extend(
        [
            ("response_type", "code"),
            ("client_id", &c.client_id),
            ("redirect_uri", &c.redirect_uri),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("state", state),
        ]
        .map(|(k, v)| (k.into(), Some(v.into()))),
    );
    if !c.scopes.is_empty() {
        items.push(("scope".into(), Some(c.scopes.clone())));
    }
    if !c.audience.is_empty() {
        items.push(("audience".into(), Some(c.audience.clone())));
    }
    // Match URLComponents query-item escaping, including its literal '+' behavior.
    let query = items
        .iter()
        .map(|(k, v)| {
            let name = super::url::encode_query(k);
            v.as_ref().map_or_else(
                || name.clone(),
                |value| format!("{name}={}", super::url::encode_query(value)),
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    url.set_query(Some(&query));
    Ok((url.into(), callback))
}
/// # Errors
/// Returns a failed, mismatched-state or missing-code authorization error.
pub fn authorization_code(callback: &str, state: &str) -> Result<String, DomainOAuthError> {
    let url = Url::parse(callback).map_err(|_| DomainOAuthError::AuthorizationFailed)?;
    let mut values = BTreeMap::new();
    for item in url.query().unwrap_or_default().split('&') {
        if let Some((k, v)) = item.split_once('=') {
            values
                .entry(
                    percent_encoding::percent_decode_str(k)
                        .decode_utf8_lossy()
                        .into_owned(),
                )
                .or_insert_with(|| {
                    percent_encoding::percent_decode_str(v)
                        .decode_utf8_lossy()
                        .into_owned()
                });
        }
    }
    if values.get("state").map(String::as_str) != Some(state) {
        return Err(DomainOAuthError::StateMismatch);
    }
    values
        .remove("code")
        .filter(|s| !s.is_empty())
        .ok_or(DomainOAuthError::MissingAuthorizationCode)
}
/// # Errors
/// Returns the rejected HTTP status or an invalid-token-response error.
pub fn parse_token(
    status: u16,
    body: &str,
    now: f64,
) -> Result<DomainTokenReceipt, DomainOAuthError> {
    if !(200..300).contains(&status) {
        return Err(DomainOAuthError::TokenEndpointRejected { status });
    }
    let object: serde_json::Value =
        serde_json::from_str(body).map_err(|_| DomainOAuthError::InvalidTokenResponse)?;
    let token = object
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(DomainOAuthError::InvalidTokenResponse)?
        .into();
    let seconds = object.get("expires_in").and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_bool().map(|b| f64::from(u8::from(b))))
    });
    Ok(DomainTokenReceipt {
        token,
        expires_at: seconds.map(|s| now + s),
        scope: object
            .get("scope")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}
/// # Errors
/// Returns configuration, browser, credential, transport or token-response failures.
pub async fn acquire(
    c: DomainOAuthConfiguration,
    platform: Arc<dyn DomainOAuthPlatform>,
) -> Result<DomainTokenReceipt, DomainOAuthError> {
    let token_url = Url::parse(&c.token_url).map_err(|_| DomainOAuthError::InvalidConfiguration)?;
    if (token_url.scheme() != "https"
        && !token_url
            .host_str()
            .is_some_and(|h| ["localhost", "127.0.0.1", "[::1]", "::1"].contains(&h)))
        || c.client_id.is_empty()
        || c.access_token_reference.is_empty()
    {
        return Err(DomainOAuthError::InvalidConfiguration);
    }
    let mut form = BTreeMap::from([("client_id".into(), c.client_id.clone())]);
    if !c.scopes.is_empty() {
        form.insert("scope".into(), c.scopes.clone());
    }
    if !c.audience.is_empty() {
        form.insert("audience".into(), c.audience.clone());
    }
    match c.grant {
        DomainOAuthGrant::ClientCredentials => {
            form.insert("grant_type".into(), "client_credentials".into());
            let secret = platform
                .read_secret(c.client_secret_reference.clone())
                .await
                .ok_or(DomainOAuthError::ClientSecretUnavailable)?;
            form.insert("client_secret".into(), secret);
        }
        DomainOAuthGrant::AuthorizationCodePkce => {
            let verifier = random_url_safe(48)?;
            let state = random_url_safe(24)?;
            let (url, callback) = authorization_url(&c, &pkce_challenge(&verifier), &state)?;
            let response = platform.authorize(url, callback).await?;
            let code = authorization_code(&response, &state)?;
            form.extend([
                ("grant_type".into(), "authorization_code".into()),
                ("code".into(), code),
                ("code_verifier".into(), verifier),
                ("redirect_uri".into(), c.redirect_uri),
            ]);
        }
    }
    let response = transport::exchange(token_url, &encode_form(&form), platform.as_ref()).await?;
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .await
        .map_err(|error| network_error(&error))?;
    let body = std::str::from_utf8(&body).map_err(|_| DomainOAuthError::InvalidTokenResponse)?;
    parse_token(
        status,
        body,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64(),
    )
}

/// Transport categories have native localized presentations in the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainNetworkFailure {
    Timeout,
    HostNotFound,
    CannotConnect,
    Offline,
    ConnectionLost,
    Certificate,
    Tls,
    Decode,
    Cancelled,
    TooManyRedirects,
    UnsupportedUrl,
    NoPermissionsToReadFile,
    CannotParseResponse,
    CertificateExpired,
    CertificateNotYetValid,
    Unknown,
}
impl DomainNetworkFailure {
    pub(super) fn is_tls(&self) -> bool {
        matches!(
            self,
            Self::Certificate | Self::CertificateExpired | Self::CertificateNotYetValid | Self::Tls
        )
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DomainOAuthHeaders {
    pub user_agent: String,
    pub accept_language: String,
}
