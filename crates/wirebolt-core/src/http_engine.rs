use std::{error::Error, fmt, time::Duration};

use http::Version;
use reqwest::{Client, ClientBuilder, Proxy, redirect, retry};
use tokio::time::{Instant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::{
    NoSecrets, PreparedRequest, ProxyConfigurationError, ProxyDestination, ProxyMode, ProxyPolicy,
    ResolvedProxy, SecretResolver,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HttpVersionPolicy {
    #[default]
    Automatic,
    Http1Only,
    Http2PriorKnowledge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttpEngineConfig {
    pub connect_timeout: Duration,
    pub version_policy: HttpVersionPolicy,
}

impl Default for HttpEngineConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            version_policy: HttpVersionPolicy::Automatic,
        }
    }
}

#[derive(Clone)]
pub struct HttpEngine {
    client: Client,
}

impl fmt::Debug for HttpEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpEngine")
            .field("client", &"[REDACTED]")
            .finish()
    }
}

impl HttpEngine {
    /// Builds a pooled HTTP client in direct mode.
    ///
    /// # Errors
    ///
    /// Returns [`RunError`] when the TLS backend or resolver cannot be initialized.
    pub fn new(config: HttpEngineConfig) -> Result<Self, RunError> {
        let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);
        Self::with_proxy(config, &proxy, &NoSecrets)
    }

    /// Builds a pooled HTTP client for one resolved proxy policy.
    ///
    /// # Errors
    ///
    /// Returns [`RunError`] when proxy configuration, TLS, or the resolver
    /// cannot be initialized.
    pub fn with_proxy<R>(
        config: HttpEngineConfig,
        proxy: &ResolvedProxy,
        secrets: &R,
    ) -> Result<Self, RunError>
    where
        R: SecretResolver + ?Sized,
    {
        let mut builder = Client::builder()
            .connect_timeout(config.connect_timeout)
            .redirect(redirect::Policy::none())
            .retry(retry::never());
        builder = match config.version_policy {
            HttpVersionPolicy::Automatic => builder,
            HttpVersionPolicy::Http1Only => builder.http1_only(),
            HttpVersionPolicy::Http2PriorKnowledge => builder.http2_prior_knowledge(),
        };
        builder = configure_proxy(builder, proxy, secrets)?;

        builder
            .build()
            .map(|client| Self { client })
            .map_err(RunError::transport)
    }

    /// Executes one request and forwards each response chunk synchronously.
    ///
    /// The chunk callback must return quickly and must not retain the borrowed
    /// slice. Returning [`StreamControl::Stop`] ends the run without reading the
    /// remaining body. The engine never accumulates response bytes.
    ///
    /// # Errors
    ///
    /// Returns [`RunError`] for cancellation, timeouts, transport failures,
    /// callback stops, or a configured response-size limit.
    pub async fn run<F>(
        &self,
        request: PreparedRequest,
        options: RunOptions,
        cancellation: &RunCancellation,
        on_chunk: F,
    ) -> Result<Run, RunError>
    where
        F: FnMut(&[u8]) -> StreamControl,
    {
        self.run_observed(request, options, cancellation, |_| {}, on_chunk)
            .await
    }

    /// Executes one request while exposing response metadata before body chunks.
    ///
    /// The metadata callback is invoked exactly once after response headers are
    /// available and before the first body chunk is delivered. Both callbacks
    /// borrow their inputs only for the duration of the call.
    ///
    /// # Errors
    ///
    /// Returns [`RunError`] for cancellation, timeouts, transport failures,
    /// callback stops, or a configured response-size limit.
    pub async fn run_observed<H, F>(
        &self,
        request: PreparedRequest,
        options: RunOptions,
        cancellation: &RunCancellation,
        mut on_headers: H,
        mut on_chunk: F,
    ) -> Result<Run, RunError>
    where
        H: FnMut(&RunHead),
        F: FnMut(&[u8]) -> StreamControl,
    {
        let started = Instant::now();
        let deadline = started + options.total_timeout;
        let (method, uri, headers, body) = request.into_parts();
        let request = self
            .client
            .request(method, uri.to_string())
            .headers(headers)
            .body(body)
            .build()
            .map_err(RunError::transport)?;

        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(RunError::new(RunErrorKind::Cancelled)),
            result = timeout_at(deadline, self.client.execute(request)) => {
                result
                    .map_err(|_| RunError::new(RunErrorKind::TotalTimeout))?
                    .map_err(RunError::transport)?
            }
        };
        let head = RunHead {
            status: response.status().as_u16(),
            version: HttpVersion::from(response.version()),
            headers: response
                .headers()
                .iter()
                .map(|(name, value)| RunHeader {
                    name: name.as_str().to_owned(),
                    value: value.as_bytes().to_vec(),
                })
                .collect(),
            time_to_headers: started.elapsed(),
        };
        on_headers(&head);

        let mut response = response;
        let mut bytes_received = 0_u64;
        loop {
            let chunk = tokio::select! {
                () = cancellation.cancelled() => {
                    return Err(RunError::new(RunErrorKind::Cancelled));
                }
                result = timeout_at(deadline, timeout(options.read_timeout, response.chunk())) => {
                    let result = result
                        .map_err(|_| RunError::new(RunErrorKind::TotalTimeout))?;
                    result
                        .map_err(|_| RunError::new(RunErrorKind::ReadTimeout))?
                        .map_err(RunError::transport)?
                }
            };

            let Some(chunk) = chunk else {
                break;
            };
            let chunk_bytes = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            let next_bytes = bytes_received.checked_add(chunk_bytes).ok_or_else(|| {
                RunError::response_too_large(options.max_response_bytes.unwrap_or(u64::MAX))
            })?;
            if options
                .max_response_bytes
                .is_some_and(|limit| next_bytes > limit)
            {
                return Err(RunError::response_too_large(
                    options.max_response_bytes.unwrap_or(u64::MAX),
                ));
            }

            if on_chunk(&chunk) == StreamControl::Stop {
                return Err(RunError::new(RunErrorKind::StreamStopped));
            }
            bytes_received = next_bytes;
        }

        Ok(Run {
            status: head.status,
            version: head.version,
            headers: head.headers,
            bytes_received,
            time_to_headers: head.time_to_headers,
            total_time: started.elapsed(),
        })
    }
}

fn configure_proxy<R>(
    builder: ClientBuilder,
    resolved: &ResolvedProxy,
    secrets: &R,
) -> Result<ClientBuilder, RunError>
where
    R: SecretResolver + ?Sized,
{
    match resolved.mode() {
        ProxyMode::System => Ok(builder),
        ProxyMode::Direct => Ok(builder.no_proxy()),
        ProxyMode::Manual(manual) => {
            let mut builder = builder.no_proxy();
            for route in manual.routes() {
                let endpoint = route.endpoint().normalized();
                let mut proxy = match route.destination() {
                    ProxyDestination::All => Proxy::all(&endpoint),
                    ProxyDestination::Http => Proxy::http(&endpoint),
                    ProxyDestination::Https => Proxy::https(&endpoint),
                }
                .map_err(|_| RunError::proxy(ProxyConfigurationError::invalid_endpoint()))?;
                if let Some(credentials) = route.credentials() {
                    let username = secrets.resolve(credentials.username()).map_err(|error| {
                        RunError::proxy(ProxyConfigurationError::secret(
                            credentials.username(),
                            &error,
                        ))
                    })?;
                    let password = secrets.resolve(credentials.password()).map_err(|error| {
                        RunError::proxy(ProxyConfigurationError::secret(
                            credentials.password(),
                            &error,
                        ))
                    })?;
                    proxy = proxy.basic_auth(username.expose(), password.expose());
                }
                builder = builder.proxy(proxy);
            }
            Ok(builder)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunOptions {
    pub total_timeout: Duration,
    pub read_timeout: Duration,
    pub max_response_bytes: Option<u64>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            total_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(10),
            max_response_bytes: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RunCancellation {
    token: CancellationToken,
}

impl RunCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    async fn cancelled(&self) {
        self.token.cancelled().await;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamControl {
    Continue,
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpVersion {
    Http09,
    Http10,
    Http11,
    Http2,
    Http3,
    Other,
}

impl From<Version> for HttpVersion {
    fn from(version: Version) -> Self {
        match version {
            Version::HTTP_09 => Self::Http09,
            Version::HTTP_10 => Self::Http10,
            Version::HTTP_11 => Self::Http11,
            Version::HTTP_2 => Self::Http2,
            Version::HTTP_3 => Self::Http3,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunHeader {
    pub name: String,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunHead {
    pub status: u16,
    pub version: HttpVersion,
    pub headers: Vec<RunHeader>,
    pub time_to_headers: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Run {
    pub status: u16,
    pub version: HttpVersion,
    pub headers: Vec<RunHeader>,
    pub bytes_received: u64,
    pub time_to_headers: Duration,
    pub total_time: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunErrorKind {
    Cancelled,
    TotalTimeout,
    ReadTimeout,
    StreamStopped,
    ResponseTooLarge,
    Connection,
    Request,
    ResponseBody,
    Proxy,
    Transport,
}

#[derive(Debug)]
pub struct RunError {
    kind: RunErrorKind,
    response_limit: Option<u64>,
    source: Option<RunErrorSource>,
}

#[derive(Debug)]
enum RunErrorSource {
    Transport(reqwest::Error),
    Proxy(ProxyConfigurationError),
}

impl RunError {
    const fn new(kind: RunErrorKind) -> Self {
        Self {
            kind,
            response_limit: None,
            source: None,
        }
    }

    const fn response_too_large(limit: u64) -> Self {
        Self {
            kind: RunErrorKind::ResponseTooLarge,
            response_limit: Some(limit),
            source: None,
        }
    }

    fn transport(error: reqwest::Error) -> Self {
        let kind = if error.is_timeout() {
            RunErrorKind::TotalTimeout
        } else if error.is_connect() {
            RunErrorKind::Connection
        } else if error.is_request() {
            RunErrorKind::Request
        } else if error.is_body() || error.is_decode() {
            RunErrorKind::ResponseBody
        } else {
            RunErrorKind::Transport
        };
        Self {
            kind,
            response_limit: None,
            source: Some(RunErrorSource::Transport(error.without_url())),
        }
    }

    const fn proxy(error: ProxyConfigurationError) -> Self {
        Self {
            kind: RunErrorKind::Proxy,
            response_limit: None,
            source: Some(RunErrorSource::Proxy(error)),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> RunErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn response_limit(&self) -> Option<u64> {
        self.response_limit
    }

    #[must_use]
    pub const fn proxy_configuration_error(&self) -> Option<&ProxyConfigurationError> {
        match self.source.as_ref() {
            Some(RunErrorSource::Proxy(error)) => Some(error),
            Some(RunErrorSource::Transport(_)) | None => None,
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            RunErrorKind::Cancelled => formatter.write_str("run cancelled"),
            RunErrorKind::TotalTimeout => formatter.write_str("run exceeded its total timeout"),
            RunErrorKind::ReadTimeout => formatter.write_str("response body stalled"),
            RunErrorKind::StreamStopped => formatter.write_str("response stream stopped by caller"),
            RunErrorKind::ResponseTooLarge => write!(
                formatter,
                "response exceeded the configured {} byte limit",
                self.response_limit.unwrap_or(0)
            ),
            RunErrorKind::Connection => formatter.write_str("connection failed"),
            RunErrorKind::Request => formatter.write_str("request failed before sending"),
            RunErrorKind::ResponseBody => {
                formatter.write_str("response body failed while streaming")
            }
            RunErrorKind::Proxy => formatter.write_str("proxy setup failed"),
            RunErrorKind::Transport => formatter.write_str("HTTP transport failed"),
        }
    }
}

impl Error for RunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self.source.as_ref() {
            Some(RunErrorSource::Transport(source)) => Some(source),
            Some(RunErrorSource::Proxy(source)) => Some(source),
            None => None,
        }
    }
}
