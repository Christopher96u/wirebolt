use std::{error::Error, fmt, io, net::SocketAddr, time::Duration};

use http::{
    HeaderMap, HeaderValue, Version,
    header::{ACCEPT, ACCEPT_ENCODING, CONTENT_ENCODING},
};
use reqwest::{Client, ClientBuilder, Proxy, redirect, retry};
use tokio::time::{Instant, sleep, sleep_until};
use tokio_util::sync::CancellationToken;

use crate::{
    NoSecrets, PreparedRequest, ProxyConfigurationError, ProxyDestination, ProxyMode, ProxyPolicy,
    ResolvedProxy, SecretResolver,
    body_decoder::{BodyDecoder, ContentEncoding},
};

pub const DEFAULT_USER_AGENT: &str = concat!("Wirebolt/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HttpVersionPolicy {
    #[default]
    Automatic,
    Http1Only,
    Http2PriorKnowledge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpEngineConfig {
    pub connect_timeout: Duration,
    /// Fails a run when the connection produces no bytes for this long,
    /// including while waiting for response headers. `None` leaves only the
    /// per-run deadlines in place.
    pub read_timeout: Option<Duration>,
    pub version_policy: HttpVersionPolicy,
    /// Sent unless the request carries its own `User-Agent`. `None` sends
    /// nothing, which some APIs reject outright.
    pub user_agent: Option<String>,
}

impl Default for HttpEngineConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            read_timeout: None,
            version_policy: HttpVersionPolicy::Automatic,
            user_agent: Some(DEFAULT_USER_AGENT.to_owned()),
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
    pub fn new(config: &HttpEngineConfig) -> Result<Self, RunError> {
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
        config: &HttpEngineConfig,
        proxy: &ResolvedProxy,
        secrets: &R,
    ) -> Result<Self, RunError>
    where
        R: SecretResolver + ?Sized,
    {
        let mut default_headers = HeaderMap::with_capacity(1);
        default_headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        let mut builder = Client::builder()
            .connect_timeout(config.connect_timeout)
            .redirect(redirect::Policy::none())
            .retry(retry::never())
            .default_headers(default_headers);
        if let Some(user_agent) = &config.user_agent {
            builder = builder.user_agent(user_agent.as_str());
        }
        if let Some(read_timeout) = config.read_timeout {
            builder = builder.read_timeout(read_timeout);
        }
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
    /// borrow their inputs only for the duration of the call. When the caller
    /// set no `Accept-Encoding`, compressed bodies are decoded on the fly and
    /// chunks carry decoded bytes; the response headers stay as received.
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
        let (method, url, mut headers, body) = request.into_parts();
        let auto_decode = options.decode_content && !headers.contains_key(ACCEPT_ENCODING);
        if auto_decode {
            headers.insert(
                ACCEPT_ENCODING,
                HeaderValue::from_static(ContentEncoding::ACCEPT),
            );
        }
        let mut request = reqwest::Request::new(method, url);
        *request.headers_mut() = headers;
        *request.body_mut() = Some(body.into());

        // One deadline timer and one cancellation future serve the whole run;
        // only the stall timer is reset per chunk.
        let deadline = sleep_until(after(started, options.total_timeout));
        tokio::pin!(deadline);
        let cancelled = cancellation.cancelled();
        tokio::pin!(cancelled);

        let mut response = tokio::select! {
            () = &mut cancelled => return Err(RunError::new(RunErrorKind::Cancelled)),
            () = &mut deadline => return Err(RunError::new(RunErrorKind::TotalTimeout)),
            result = self.client.execute(request) => result.map_err(RunError::transport)?,
        };

        let content_encoding = if auto_decode {
            response
                .headers()
                .get(CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok())
                .and_then(ContentEncoding::parse)
        } else {
            None
        };
        let head = RunHead {
            status: response.status().as_u16(),
            version: HttpVersion::from(response.version()),
            headers: response.headers().clone(),
            remote_addr: response.remote_addr(),
            content_encoding,
            time_to_headers: started.elapsed(),
        };
        on_headers(&head);
        if let (Some(limit), Some(length)) = (options.max_response_bytes, response.content_length())
            && length > limit
        {
            return Err(RunError::response_too_large(limit));
        }

        let mut decoder = match content_encoding {
            Some(encoding) => Some(BodyDecoder::new(encoding).map_err(RunError::decode)?),
            None => None,
        };
        let stall = sleep(options.read_timeout);
        tokio::pin!(stall);
        let mut bytes_received = 0_u64;
        let mut bytes_decoded = 0_u64;
        loop {
            stall
                .as_mut()
                .reset(after(Instant::now(), options.read_timeout));
            let chunk = tokio::select! {
                () = &mut cancelled => return Err(RunError::new(RunErrorKind::Cancelled)),
                () = &mut deadline => return Err(RunError::new(RunErrorKind::TotalTimeout)),
                () = &mut stall => return Err(RunError::new(RunErrorKind::ReadTimeout)),
                result = response.chunk() => result.map_err(RunError::transport)?,
            };
            let Some(chunk) = chunk else {
                break;
            };
            bytes_received = add_within_limit(bytes_received, chunk.len(), options)?;
            let delivered: &[u8] = match decoder.as_mut() {
                Some(decoder) => decoder.decode(&chunk).map_err(RunError::decode)?,
                None => &chunk,
            };
            bytes_decoded = add_within_limit(bytes_decoded, delivered.len(), options)?;
            if !delivered.is_empty() && on_chunk(delivered) == StreamControl::Stop {
                return Err(RunError::new(RunErrorKind::StreamStopped));
            }
        }
        if let Some(decoder) = decoder.as_mut() {
            let tail = decoder.finish().map_err(RunError::decode)?;
            bytes_decoded = add_within_limit(bytes_decoded, tail.len(), options)?;
            if !tail.is_empty() && on_chunk(tail) == StreamControl::Stop {
                return Err(RunError::new(RunErrorKind::StreamStopped));
            }
        }

        Ok(Run {
            status: head.status,
            version: head.version,
            headers: head.headers,
            remote_addr: head.remote_addr,
            content_encoding: head.content_encoding,
            bytes_received,
            bytes_decoded,
            time_to_headers: head.time_to_headers,
            total_time: started.elapsed(),
        })
    }
}

/// Deadlines far enough to overflow the clock are clamped to a year out,
/// which is indistinguishable from "never" for a request.
fn after(instant: Instant, duration: Duration) -> Instant {
    const ONE_YEAR: Duration = Duration::from_hours(365 * 24);
    instant
        .checked_add(duration)
        .unwrap_or_else(|| instant + ONE_YEAR)
}

fn add_within_limit(total: u64, added: usize, options: RunOptions) -> Result<u64, RunError> {
    let limit = options.max_response_bytes.unwrap_or(u64::MAX);
    let next = total
        .checked_add(u64::try_from(added).unwrap_or(u64::MAX))
        .ok_or_else(|| RunError::response_too_large(limit))?;
    if next > limit {
        return Err(RunError::response_too_large(limit));
    }
    Ok(next)
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
                    // `ManualProxy::new` already refused credentials on SOCKS4
                    // routes, where reqwest would panic instead of erroring.
                    if !route.endpoint().protocol().supports_credentials() {
                        return Err(RunError::proxy(
                            ProxyConfigurationError::unsupported_credentials(),
                        ));
                    }
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
    /// Advertise and decode compressed bodies when the request sets no
    /// `Accept-Encoding` of its own.
    pub decode_content: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            total_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(10),
            max_response_bytes: None,
            decode_content: true,
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

/// Response metadata, delivered before the first body chunk. Headers are the
/// map hyper parsed, shared by reference count rather than copied per header.
#[derive(Clone, Debug, PartialEq)]
pub struct RunHead {
    pub status: u16,
    pub version: HttpVersion,
    pub headers: HeaderMap,
    pub remote_addr: Option<SocketAddr>,
    /// The encoding the engine is decoding for this run, if any.
    pub content_encoding: Option<ContentEncoding>,
    pub time_to_headers: Duration,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub status: u16,
    pub version: HttpVersion,
    pub headers: HeaderMap,
    pub remote_addr: Option<SocketAddr>,
    pub content_encoding: Option<ContentEncoding>,
    /// Bytes read from the connection.
    pub bytes_received: u64,
    /// Bytes delivered to the chunk callback after any decoding.
    pub bytes_decoded: u64,
    pub time_to_headers: Duration,
    pub total_time: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunErrorKind {
    Cancelled,
    TotalTimeout,
    ReadTimeout,
    ConnectTimeout,
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
    Decode(io::Error),
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
        // The engine enforces its own total deadline, so a timeout reported by
        // the transport is either the connect phase or a stalled read.
        let kind = if error.is_connect() {
            if error.is_timeout() {
                RunErrorKind::ConnectTimeout
            } else {
                RunErrorKind::Connection
            }
        } else if error.is_timeout() {
            RunErrorKind::ReadTimeout
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

    const fn decode(error: io::Error) -> Self {
        Self {
            kind: RunErrorKind::ResponseBody,
            response_limit: None,
            source: Some(RunErrorSource::Decode(error)),
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
            Some(RunErrorSource::Transport(_) | RunErrorSource::Decode(_)) | None => None,
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            RunErrorKind::Cancelled => formatter.write_str("run cancelled"),
            RunErrorKind::TotalTimeout => formatter.write_str("run exceeded its total timeout"),
            RunErrorKind::ReadTimeout => formatter.write_str("response stalled"),
            RunErrorKind::ConnectTimeout => formatter.write_str("connection timed out"),
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
            Some(RunErrorSource::Decode(source)) => Some(source),
            None => None,
        }
    }
}
