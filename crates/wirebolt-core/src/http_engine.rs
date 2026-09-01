use std::{
    convert::Infallible,
    error::Error,
    fmt, fs,
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use futures_util::TryStreamExt;
use http::{
    HeaderMap, HeaderValue, Method, Version,
    header::{ACCEPT, ACCEPT_ENCODING, CONTENT_ENCODING},
};
use http_body::{Frame, SizeHint};
use reqwest::{Certificate, Client, ClientBuilder, Identity, Proxy, redirect, retry};
use tokio::time::{Instant, Sleep, sleep_until};
use tokio_util::{io::ReaderStream, sync::CancellationToken};

use crate::{
    NoSecrets, PreparedRequest, ProxyConfigurationError, ProxyDestination, ProxyMode, ProxyPolicy,
    ResolvedProxy, SecretName, SecretResolver,
    body_decoder::{BodyStream, ChunkSink, ContentEncoding, SinkRefusal},
    request::PreparedBodySource,
};

pub const DEFAULT_USER_AGENT: &str = concat!("Wirebolt/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HttpVersionPolicy {
    #[default]
    Automatic,
    Http1Only,
    Http2PriorKnowledge,
}

#[derive(Clone, Eq, PartialEq)]
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
    pub validate_tls: bool,
    pub maximum_redirects: Option<u8>,
    /// PEM certificate chain plus private key loaded from Keychain for mTLS.
    /// This material participates in pool identity but is always redacted from Debug.
    pub client_identity_pem: Option<Vec<u8>>,
    /// PEM root bundle selected for this request.
    pub root_certificates_pem: Option<Vec<u8>>,
}

impl fmt::Debug for HttpEngineConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpEngineConfig")
            .field("connect_timeout", &self.connect_timeout)
            .field("read_timeout", &self.read_timeout)
            .field("version_policy", &self.version_policy)
            .field("user_agent", &self.user_agent)
            .field("validate_tls", &self.validate_tls)
            .field("maximum_redirects", &self.maximum_redirects)
            .field(
                "client_identity_pem",
                &self.client_identity_pem.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "root_certificates_pem",
                &self.root_certificates_pem.as_ref().map(|_| "[CONFIGURED]"),
            )
            .finish()
    }
}

impl Default for HttpEngineConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            read_timeout: None,
            version_policy: HttpVersionPolicy::Automatic,
            user_agent: Some(DEFAULT_USER_AGENT.to_owned()),
            validate_tls: true,
            maximum_redirects: None,
            client_identity_pem: None,
            root_certificates_pem: None,
        }
    }
}

impl HttpEngineConfig {
    /// Resolves mTLS identity material from the secret store and reads an optional
    /// public custom CA bundle. Neither source is included in diagnostics.
    ///
    /// # Errors
    ///
    /// Returns a redacted configuration error when either source is unavailable.
    pub fn resolve_tls<R: SecretResolver + ?Sized>(
        mut self,
        identity: Option<&SecretName>,
        custom_ca_path: Option<&str>,
        secrets: &R,
    ) -> Result<Self, TlsConfigurationError> {
        if let Some(identity) = identity {
            self.client_identity_pem = Some(
                secrets
                    .resolve(identity)
                    .map_err(|_| TlsConfigurationError::ClientIdentityUnavailable)?
                    .expose()
                    .as_bytes()
                    .to_vec(),
            );
        }
        if let Some(path) = custom_ca_path.filter(|path| !path.is_empty()) {
            self.root_certificates_pem =
                Some(fs::read(path).map_err(|_| TlsConfigurationError::CustomCaUnavailable)?);
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TlsConfigurationError {
    ClientIdentityUnavailable,
    CustomCaUnavailable,
}

impl fmt::Display for TlsConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientIdentityUnavailable => {
                formatter.write_str("client identity is unavailable")
            }
            Self::CustomCaUnavailable => formatter.write_str("custom CA bundle is unavailable"),
        }
    }
}

impl Error for TlsConfigurationError {}

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
            .redirect(
                config
                    .maximum_redirects
                    .map_or_else(redirect::Policy::none, |maximum| {
                        redirect::Policy::limited(usize::from(maximum.min(10)))
                    }),
            )
            .danger_accept_invalid_certs(!config.validate_tls)
            .retry(retry::never())
            .default_headers(default_headers);
        if let Some(user_agent) = &config.user_agent {
            builder = builder.user_agent(user_agent.as_str());
        }
        if let Some(read_timeout) = config.read_timeout {
            builder = builder.read_timeout(read_timeout);
        }
        if let Some(pem) = &config.client_identity_pem {
            let identity = Identity::from_pem(pem).map_err(RunError::transport)?;
            builder = builder.identity(identity);
        }
        if let Some(pem) = &config.root_certificates_pem {
            for certificate in Certificate::from_pem_bundle(pem).map_err(RunError::transport)? {
                builder = builder.add_root_certificate(certificate);
            }
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
        on_chunk: F,
    ) -> Result<Run, RunError>
    where
        H: FnMut(&RunHead),
        F: FnMut(&[u8]) -> StreamControl,
    {
        let started = Instant::now();
        let (method, url, mut headers, body) = request.into_parts();
        let head_request = method == Method::HEAD;
        let auto_decode = options.decode_content && !headers.contains_key(ACCEPT_ENCODING);
        if auto_decode {
            headers.insert(
                ACCEPT_ENCODING,
                HeaderValue::from_static(ContentEncoding::ACCEPT),
            );
        }
        let progress = Arc::new(SendProgress::new(started));
        let mut request = reqwest::Request::new(method, url);
        *request.headers_mut() = headers;
        *request.body_mut() = Some(upload_body(body, Arc::clone(&progress)).await?);

        // One deadline timer and one cancellation future serve the whole run;
        // only the stall timer is re-armed as bytes move in either direction.
        let deadline = options
            .total_timeout
            .map(|timeout| sleep_until(after(started, timeout)));
        tokio::pin!(deadline);
        let stall = options
            .read_timeout
            .map(|timeout| sleep_until(after(started, timeout)));
        tokio::pin!(stall);
        let cancelled = cancellation.cancelled();
        tokio::pin!(cancelled);

        let mut response = self
            .send(
                request,
                RunTimers {
                    read_timeout: options.read_timeout,
                    progress: &progress,
                    deadline: deadline.as_mut(),
                    stall: stall.as_mut(),
                    cancelled: cancelled.as_mut(),
                },
            )
            .await?;

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
        let bodyless = is_bodyless(head_request, head.status);

        // Decoded pieces flow straight from the decoder into the callback;
        // nothing is buffered beyond the decoder's fixed working buffer.
        let sink = ChunkSink::new(on_chunk, options.max_response_bytes);
        let mut body = BodyStream::new(content_encoding, sink).map_err(RunError::decode)?;
        let mut bytes_received = 0_u64;
        loop {
            if let Some(timeout) = options.read_timeout {
                rearm(stall.as_mut(), after(Instant::now(), timeout));
            }
            let chunk = tokio::select! {
                () = &mut cancelled => return Err(RunError::new(RunErrorKind::Cancelled)),
                () = fired(deadline.as_mut()) => return Err(RunError::new(RunErrorKind::TotalTimeout)),
                () = fired(stall.as_mut()) => return Err(RunError::new(RunErrorKind::ReadTimeout)),
                result = response.chunk() => result.map_err(RunError::transport)?,
            };
            let Some(chunk) = chunk else {
                break;
            };
            bytes_received = add_within_limit(bytes_received, chunk.len(), options)?;
            if let Err(error) = body.write_chunk(&chunk) {
                return Err(body_error(body.sink().refusal(), error, options));
            }
        }
        if !bodyless && let Err(error) = body.finish() {
            return Err(body_error(body.sink().refusal(), error, options));
        }

        Ok(Run {
            status: head.status,
            version: head.version,
            headers: head.headers,
            remote_addr: head.remote_addr,
            content_encoding: head.content_encoding,
            bytes_received,
            bytes_decoded: body.sink().delivered(),
            time_to_headers: head.time_to_headers,
            total_time: started.elapsed(),
        })
    }
}

impl HttpEngine {
    /// Sends the request and waits for response headers under the run's
    /// timers. Request bytes the transport is still taking count as
    /// progress, so only a genuine stall trips the read timeout.
    async fn send<C: Future<Output = ()>>(
        &self,
        request: reqwest::Request,
        mut timers: RunTimers<'_, C>,
    ) -> Result<reqwest::Response, RunError> {
        let execute = self.client.execute(request);
        tokio::pin!(execute);
        loop {
            tokio::select! {
                () = &mut timers.cancelled => return Err(RunError::new(RunErrorKind::Cancelled)),
                () = fired(timers.deadline.as_mut()) => {
                    return Err(RunError::new(RunErrorKind::TotalTimeout));
                }
                () = fired(timers.stall.as_mut()) => {
                    let due = timers.read_timeout.map_or(Instant::now(), |timeout| {
                        after(timers.progress.last_activity(), timeout)
                    });
                    if due <= Instant::now() {
                        return Err(RunError::new(RunErrorKind::ReadTimeout));
                    }
                    rearm(timers.stall.as_mut(), due);
                }
                result = &mut execute => return result.map_err(RunError::transport),
            }
        }
    }
}

/// The per-run timers, borrowed for the send phase.
struct RunTimers<'a, C> {
    read_timeout: Option<Duration>,
    progress: &'a SendProgress,
    deadline: Pin<&'a mut Option<Sleep>>,
    stall: Pin<&'a mut Option<Sleep>>,
    cancelled: Pin<&'a mut C>,
}

/// Deadlines far enough to overflow the clock are clamped to a year out,
/// which is indistinguishable from "never" for a request.
fn after(instant: Instant, duration: Duration) -> Instant {
    const ONE_YEAR: Duration = Duration::from_hours(365 * 24);
    instant
        .checked_add(duration)
        .unwrap_or_else(|| instant + ONE_YEAR)
}

/// Whether a response carries no body by definition: a `HEAD` reply or a
/// 1xx/204/304 status. Only such a response may end a compressed stream at
/// zero bytes; any other one, a declared zero length included, was cut
/// short of its end marker.
const fn is_bodyless(head_request: bool, status: u16) -> bool {
    head_request || matches!(status, 100..=199 | 204 | 304)
}

/// Resolves when an armed timer elapses; a disabled timer never resolves.
async fn fired(timer: Pin<&mut Option<Sleep>>) {
    match timer.as_pin_mut() {
        Some(sleep) => sleep.await,
        None => std::future::pending().await,
    }
}

fn rearm(timer: Pin<&mut Option<Sleep>>, at: Instant) {
    if let Some(sleep) = timer.as_pin_mut() {
        sleep.reset(at);
    }
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

/// Maps a decoder failure back to what actually happened at the sink.
fn body_error(refusal: Option<SinkRefusal>, error: io::Error, options: RunOptions) -> RunError {
    match refusal {
        Some(SinkRefusal::Stopped) => RunError::new(RunErrorKind::StreamStopped),
        Some(SinkRefusal::TooLarge) => {
            RunError::response_too_large(options.max_response_bytes.unwrap_or(u64::MAX))
        }
        None => RunError::decode(error),
    }
}

/// When the transport last pulled request bytes, as nanoseconds after the
/// run started. Lets the stall timer treat an upload still moving as
/// progress while waiting for response headers.
#[derive(Debug)]
struct SendProgress {
    started: Instant,
    last_activity_nanos: AtomicU64,
}

async fn upload_body(
    source: PreparedBodySource,
    progress: Arc<SendProgress>,
) -> Result<reqwest::Body, RunError> {
    match source {
        PreparedBodySource::Bytes(bytes) => Ok(reqwest::Body::wrap(RequestBody {
            remaining: Bytes::from(bytes),
            progress,
        })),
        PreparedBodySource::File { path, .. } => {
            let file = tokio::fs::File::open(path)
                .await
                .map_err(RunError::upload)?;
            let stream = ReaderStream::new(file).inspect_ok(move |_chunk| progress.touch());
            Ok(reqwest::Body::wrap_stream(stream))
        }
    }
}

impl SendProgress {
    const fn new(started: Instant) -> Self {
        Self {
            started,
            last_activity_nanos: AtomicU64::new(0),
        }
    }

    fn touch(&self) {
        let elapsed = u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.last_activity_nanos.store(elapsed, Ordering::Relaxed);
    }

    fn last_activity(&self) -> Instant {
        self.started + Duration::from_nanos(self.last_activity_nanos.load(Ordering::Relaxed))
    }
}

/// The request body handed to hyper in bounded frames, so each frame the
/// transport pulls is a progress signal rather than one opaque blob.
struct RequestBody {
    remaining: Bytes,
    progress: Arc<SendProgress>,
}

const REQUEST_FRAME_BYTES: usize = 64 * 1024;

impl http_body::Body for RequestBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let this = self.get_mut();
        if this.remaining.is_empty() {
            return Poll::Ready(None);
        }
        let take = this.remaining.len().min(REQUEST_FRAME_BYTES);
        let frame = this.remaining.split_to(take);
        this.progress.touch();
        Poll::Ready(Some(Ok(Frame::data(frame))))
    }

    fn is_end_stream(&self) -> bool {
        self.remaining.is_empty()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(u64::try_from(self.remaining.len()).unwrap_or(u64::MAX))
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

/// Per-run limits. A timeout of `None` is disabled outright: the run then
/// ends only through cancellation, the other deadline, or the transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunOptions {
    /// Bounds the whole run from send to the last body byte.
    pub total_timeout: Option<Duration>,
    /// Fails the run when nothing moves for this long: no request bytes
    /// taken by the transport while waiting for response headers, and no
    /// body bytes arriving between chunks.
    pub read_timeout: Option<Duration>,
    pub max_response_bytes: Option<u64>,
    /// Advertise and decode compressed bodies when the request sets no
    /// `Accept-Encoding` of its own.
    pub decode_content: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            total_timeout: Some(Duration::from_secs(30)),
            read_timeout: Some(Duration::from_secs(10)),
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
    Upload(io::Error),
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

    const fn upload(error: io::Error) -> Self {
        Self {
            kind: RunErrorKind::Request,
            response_limit: None,
            source: Some(RunErrorSource::Upload(error)),
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
            Some(
                RunErrorSource::Transport(_)
                | RunErrorSource::Decode(_)
                | RunErrorSource::Upload(_),
            )
            | None => None,
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
            Some(RunErrorSource::Decode(source) | RunErrorSource::Upload(source)) => Some(source),
            None => None,
        }
    }
}
