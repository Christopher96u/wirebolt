//! HTTP preparation, pooled engines and streamed execution. The interface only
//! supplies platform secret resolution and observes coarse events.
use crate::response::FailureIssue;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use wirebolt_core::{
    DocumentId, Environment, HttpEngine, HttpEngineConfig, HttpVersion, ProxyMode, ProxyPolicy,
    Request, RequestAuthentication, RequestBody, RequestHeader, RequestPipeline, RequestValueField,
    ResolvedProxy, RunCancellation, RunError, RunErrorKind, RunOptions, SecretName, SecretResolver,
    StreamControl, TransportSettings, ValueSource,
};
static SHARED_HTTP_ENGINES: LazyLock<HttpEngineCache> = LazyLock::new(HttpEngineCache::default);
pub fn reset_http_engines() {
    SHARED_HTTP_ENGINES.reset();
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunInput {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub query: Vec<RequestValueField>,
    #[serde(default)]
    pub headers: Vec<RequestHeader>,
    #[serde(default)]
    pub authentication: RequestAuthentication,
    #[serde(default = "empty_body")]
    pub body: RequestBody,
    #[serde(default)]
    pub variables: BTreeMap<String, ValueSource>,
    #[serde(default)]
    pub workspace_proxy: Option<ProxyMode>,
    #[serde(default)]
    pub app_proxy: Option<ProxyMode>,
    #[serde(default)]
    pub request_proxy: Option<ProxyMode>,
    #[serde(default = "default_total_timeout_ms")]
    pub total_timeout_ms: u64,
    #[serde(default = "default_read_timeout_ms")]
    pub read_timeout_ms: u64,
    #[serde(default)]
    pub max_response_bytes: Option<u64>,
    #[serde(default = "default_decode_content")]
    pub decode_content: bool,
    #[serde(default = "default_validate_tls")]
    pub validate_tls: bool,
    #[serde(default)]
    pub follow_redirects: bool,
    #[serde(default = "default_maximum_redirects")]
    pub maximum_redirects: u8,
    #[serde(default)]
    pub client_certificate_reference: Option<SecretName>,
    #[serde(default)]
    pub custom_ca_path: Option<String>,
}

const fn empty_body() -> RequestBody {
    RequestBody::Empty
}

const fn default_total_timeout_ms() -> u64 {
    30_000
}

const fn default_read_timeout_ms() -> u64 {
    10_000
}

const fn default_decode_content() -> bool {
    true
}

const fn default_validate_tls() -> bool {
    true
}
const fn default_maximum_redirects() -> u8 {
    10
}

/// Zero disables the deadline; the engine then arms no timer at all.
#[must_use]
fn timeout_from_millis(milliseconds: u64) -> Option<Duration> {
    (milliseconds != 0).then(|| Duration::from_millis(milliseconds))
}

#[derive(Debug, Serialize)]
pub struct PreparedRunSnapshotDocument {
    pub method: String,
    pub url: String,
    pub headers: Vec<PreparedHeaderDocument>,
    pub body: PreparedBodyDocument,
    pub transport: TransportSettings,
    pub proxy: crate::domain::proxy::ResolvedDomainProxy,
}

#[derive(Debug, Serialize)]
pub struct PreparedHeaderDocument {
    pub name: String,
    pub value: String,
    pub redacted: bool,
}

#[derive(Debug, Serialize)]
pub struct PreparedBodyDocument {
    pub byte_count: u64,
    pub content_type: Option<String>,
    pub text_preview: Option<String>,
    pub redacted: bool,
}

#[derive(Debug, Serialize)]
pub struct RunFailureDocument {
    pub kind: crate::response::ResponseFailureKind,
    pub issues: Vec<FailureIssue>,
}

pub type PreparedRun = (
    wirebolt_core::PreparedRequest,
    Arc<HttpEngine>,
    PreparedRunSnapshotDocument,
);

/// # Errors
/// Returns redacted request preparation failures.
pub fn prepare_run<R: SecretResolver + ?Sized>(
    input: RunInput,
    secrets: &R,
) -> Result<PreparedRun, RunFailureDocument> {
    prepare_protocol_run(input, secrets, false)
}

/// # Errors
/// Returns redacted request preparation or transport configuration failures.
/// # Panics
/// The internal static document identifiers are validated by the core.
pub fn prepare_protocol_run<R: SecretResolver + ?Sized>(
    input: RunInput,
    secrets: &R,
    websocket: bool,
) -> Result<PreparedRun, RunFailureDocument> {
    let mut request = Request::new(
        DocumentId::new("draft").expect("static document ID"),
        "Draft",
        input.method,
        input.url,
    );
    request.query = input.query;
    request.headers = input.headers;
    request.authentication = input.authentication;
    request.body = input.body;
    request.proxy_override = input.request_proxy;
    request.transport = TransportSettings {
        validate_tls: input.validate_tls,
        follow_redirects: input.follow_redirects,
        maximum_redirects: input.maximum_redirects.min(10),
        total_timeout_ms: input.total_timeout_ms,
        read_timeout_ms: input.read_timeout_ms,
        client_certificate_reference: input.client_certificate_reference,
        custom_ca_path: input.custom_ca_path,
    };
    let environment = Environment::new(
        DocumentId::new("active").expect("static document ID"),
        "Active".to_owned(),
        input.variables,
    );
    let pipeline = RequestPipeline::new(Some(&environment), secrets);
    let prepared = if websocket {
        pipeline.prepare_websocket(&request)
    } else {
        pipeline.prepare(&request)
    }
    .map_err(|error| RunFailureDocument {
        kind: crate::response::ResponseFailureKind::InvalidRequest,
        issues: error.issues.into_iter().map(Into::into).collect(),
    })?;
    let proxy = ProxyPolicy::new(input.workspace_proxy)
        .with_app_default(input.app_proxy)
        .resolve(request.proxy_override.as_ref());
    let config = HttpEngineConfig {
        validate_tls: request.transport.validate_tls,
        maximum_redirects: request
            .transport
            .follow_redirects
            .then_some(request.transport.maximum_redirects),
        ..HttpEngineConfig::default()
    }
    .resolve_tls(
        request.transport.client_certificate_reference.as_ref(),
        request.transport.custom_ca_path.as_deref(),
        secrets,
    )
    .map_err(|_| RunFailureDocument::new(crate::response::ResponseFailureKind::TlsConfiguration))?;
    let engine = shared_http_engine(&proxy, &config, secrets)
        .map_err(|error| RunFailureDocument::from_run_error(&error))?;
    let snapshot = prepared_run_snapshot(&prepared, request.transport, &proxy);
    Ok((prepared, engine, snapshot))
}

/// # Errors
/// Returns a transport initialization failure.
pub fn shared_http_engine<R: SecretResolver + ?Sized>(
    proxy: &ResolvedProxy,
    config: &HttpEngineConfig,
    secrets: &R,
) -> Result<Arc<HttpEngine>, RunError> {
    SHARED_HTTP_ENGINES.engine_with_config(proxy.mode(), config, || {
        HttpEngine::with_proxy(config, proxy, secrets)
    })
}

fn prepared_run_snapshot(
    prepared: &wirebolt_core::PreparedRequest,
    transport: TransportSettings,
    proxy: &ResolvedProxy,
) -> PreparedRunSnapshotDocument {
    let headers = if prepared.header_names_are_sensitive() {
        vec![PreparedHeaderDocument {
            name: "[REDACTED]".to_owned(),
            value: "••••••••".to_owned(),
            redacted: true,
        }]
    } else {
        prepared
            .headers()
            .iter()
            .map(|(name, value)| PreparedHeaderDocument {
                name: name.as_str().to_owned(),
                value: if value.is_sensitive() {
                    "••••••••".to_owned()
                } else {
                    String::from_utf8_lossy(value.as_bytes()).into_owned()
                },
                redacted: value.is_sensitive(),
            })
            .collect()
    };
    let redacted = prepared.body_is_sensitive();
    let text_preview = if redacted || prepared.body_is_file_backed() {
        None
    } else {
        std::str::from_utf8(prepared.body())
            .ok()
            .map(|text| text.chars().take(32 * 1024).collect())
    };
    PreparedRunSnapshotDocument {
        method: prepared.method().as_str().to_owned(),
        url: if prepared.url_is_sensitive() {
            "[REDACTED]".to_owned()
        } else {
            prepared.url().as_str().to_owned()
        },
        headers,
        body: PreparedBodyDocument {
            byte_count: prepared.body_byte_count(),
            content_type: prepared
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            text_preview,
            redacted,
        },
        transport,
        proxy: crate::domain::proxy::ResolvedDomainProxy {
            configuration: match proxy.mode() {
                ProxyMode::Direct => crate::domain::DomainProxy::Direct,
                ProxyMode::System => crate::domain::DomainProxy::System,
                ProxyMode::Manual(_) => crate::domain::DomainProxy::Manual {
                    routes: proxy
                        .diagnostic()
                        .routes()
                        .iter()
                        .map(|route| crate::domain::DomainProxyRoute {
                            destination: route.destination(),
                            endpoint: route.endpoint().into(),
                            credentials: None,
                        })
                        .collect(),
                },
            },
            source: proxy.source(),
        },
    }
}

pub fn clear_manual_http_engines() {
    SHARED_HTTP_ENGINES.clear_manual();
}

/// Pooled HTTP engines keyed by proxy mode. One lock guards every slot, so a
/// reset is observed as a whole; clients are built outside the lock, and a
/// build that a reset overtook serves its own run but is never pooled.
#[derive(Debug, Default)]
struct HttpEngineCache {
    state: Mutex<HttpEngineCacheState>,
}

#[derive(Debug, Default)]
struct HttpEngineCacheState {
    /// Bumped by every reset; a build that started under an older generation
    /// may have captured proxy or secret state the reset meant to discard.
    generation: u64,
    system: Option<(HttpEngineConfig, Arc<HttpEngine>)>,
    direct: Option<(HttpEngineConfig, Arc<HttpEngine>)>,
    /// Manual engines are keyed by proxy mode alone: the same configuration
    /// reached through a workspace policy or a request override shares one
    /// pool. Least recently created is evicted first.
    manual: Vec<(ProxyMode, HttpEngineConfig, Arc<HttpEngine>)>,
}

const MAX_CACHED_MANUAL_ENGINES: usize = 8;

impl HttpEngineCache {
    /// # Errors
    /// Returns the client builder failure.
    #[cfg(test)]
    fn engine(
        &self,
        mode: &ProxyMode,
        build: impl FnOnce() -> Result<HttpEngine, RunError>,
    ) -> Result<Arc<HttpEngine>, RunError> {
        self.engine_with_config(mode, &HttpEngineConfig::default(), build)
    }

    fn engine_with_config(
        &self,
        mode: &ProxyMode,
        config: &HttpEngineConfig,
        build: impl FnOnce() -> Result<HttpEngine, RunError>,
    ) -> Result<Arc<HttpEngine>, RunError> {
        let generation = {
            let state = crate::lock(&self.state);
            if let Some(engine) = state.lookup(mode, config) {
                return Ok(engine);
            }
            state.generation
        };
        let engine = Arc::new(build()?);
        let mut state = crate::lock(&self.state);
        if state.generation != generation {
            return Ok(engine);
        }
        // Another run may have built the same engine meanwhile; keep the
        // pooled one so both share its connections.
        Ok(state.insert(mode, config.clone(), engine))
    }

    fn reset(&self) {
        let mut state = crate::lock(&self.state);
        state.generation += 1;
        state.system = None;
        state.direct = None;
        state.manual.clear();
    }

    /// Manual engines embed proxy credentials, so a changed secret retires
    /// them; direct and system pools carry nothing secret and stay.
    fn clear_manual(&self) {
        let mut state = crate::lock(&self.state);
        state.generation += 1;
        state.manual.clear();
    }
}

impl HttpEngineCacheState {
    fn lookup(&self, mode: &ProxyMode, config: &HttpEngineConfig) -> Option<Arc<HttpEngine>> {
        match mode {
            ProxyMode::System => self
                .system
                .as_ref()
                .filter(|(stored, _)| stored == config)
                .map(|(_, engine)| Arc::clone(engine)),
            ProxyMode::Direct => self
                .direct
                .as_ref()
                .filter(|(stored, _)| stored == config)
                .map(|(_, engine)| Arc::clone(engine)),
            ProxyMode::Manual(_) => self
                .manual
                .iter()
                .find(|(candidate, stored, _)| candidate == mode && stored == config)
                .map(|(_, _, engine)| Arc::clone(engine)),
        }
    }

    fn insert(
        &mut self,
        mode: &ProxyMode,
        config: HttpEngineConfig,
        engine: Arc<HttpEngine>,
    ) -> Arc<HttpEngine> {
        if let Some(existing) = self.lookup(mode, &config) {
            return existing;
        }
        match mode {
            ProxyMode::System => self.system = Some((config, Arc::clone(&engine))),
            ProxyMode::Direct => self.direct = Some((config, Arc::clone(&engine))),
            ProxyMode::Manual(_) => {
                if self.manual.len() == MAX_CACHED_MANUAL_ENGINES {
                    self.manual.remove(0);
                }
                self.manual
                    .push((mode.clone(), config, Arc::clone(&engine)));
            }
        }
        engine
    }
}

impl RunFailureDocument {
    #[must_use]
    pub const fn new(kind: crate::response::ResponseFailureKind) -> Self {
        Self {
            kind,
            issues: Vec::new(),
        }
    }

    #[must_use]
    pub fn from_run_error(error: &RunError) -> Self {
        Self::new(run_error_kind(error.kind()))
    }
}

const fn run_error_kind(kind: RunErrorKind) -> crate::response::ResponseFailureKind {
    match kind {
        RunErrorKind::Cancelled => crate::response::ResponseFailureKind::Cancelled,
        RunErrorKind::TotalTimeout => crate::response::ResponseFailureKind::TotalTimeout,
        RunErrorKind::ReadTimeout => crate::response::ResponseFailureKind::ReadTimeout,
        RunErrorKind::ConnectTimeout => crate::response::ResponseFailureKind::ConnectTimeout,
        RunErrorKind::StreamStopped => crate::response::ResponseFailureKind::StreamStopped,
        RunErrorKind::ResponseTooLarge => crate::response::ResponseFailureKind::ResponseTooLarge,
        RunErrorKind::Connection => crate::response::ResponseFailureKind::Connection,
        RunErrorKind::Request => crate::response::ResponseFailureKind::Request,
        RunErrorKind::ResponseBody => crate::response::ResponseFailureKind::ResponseBody,
        RunErrorKind::Proxy => crate::response::ResponseFailureKind::Proxy,
        RunErrorKind::Transport => crate::response::ResponseFailureKind::Transport,
    }
}

#[must_use]
pub const fn http_version(version: HttpVersion) -> &'static str {
    match version {
        HttpVersion::Http09 => "HTTP/0.9",
        HttpVersion::Http10 => "HTTP/1.0",
        HttpVersion::Http11 => "HTTP/1.1",
        HttpVersion::Http2 => "HTTP/2",
        HttpVersion::Http3 => "HTTP/3",
        HttpVersion::Other => "HTTP/?",
    }
}

#[must_use]
pub fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

pub trait TransportObserver: Send + Sync {
    fn prepared(&self, snapshot: &PreparedRunSnapshotDocument);
    fn head(&self, head: &wirebolt_core::RunHead, warnings: &[FailureIssue]);
    fn chunk(&self, bytes: &[u8]) -> bool;
    fn cookies(&self, url: &str, headers: Vec<(String, String)>);
    fn complete(&self, bytes_received: u64, bytes_decoded: u64, total_time: Duration);
    fn failure(&self, failure: &RunFailureDocument);
}
pub async fn execute<R>(
    input: RunInput,
    cancellation: RunCancellation,
    observer: Arc<dyn TransportObserver>,
    secrets: R,
) where
    R: SecretResolver + Clone + Send + 'static,
{
    let options = RunOptions {
        total_timeout: timeout_from_millis(input.total_timeout_ms),
        read_timeout: timeout_from_millis(input.read_timeout_ms),
        max_response_bytes: input.max_response_bytes,
        decode_content: input.decode_content,
    };
    let prepared = tokio::task::spawn_blocking(move || prepare_run(input, &secrets)).await;
    let (prepared, engine, snapshot) = match prepared {
        Ok(Ok(value)) => value,
        Ok(Err(failure)) => {
            observer.failure(&failure);
            return;
        }
        Err(_) => {
            observer.failure(&RunFailureDocument::new(
                crate::response::ResponseFailureKind::Internal,
            ));
            return;
        }
    };
    observer.prepared(&snapshot);
    let warnings: Vec<FailureIssue> = prepared
        .warnings()
        .iter()
        .cloned()
        .map(Into::into)
        .collect();
    let batch = Arc::new(StreamBatch {
        state: Mutex::new(BatchState::default()),
        observer: observer.clone(),
    });
    let result = engine
        .run_observed_with_cookies(
            prepared,
            options,
            &cancellation,
            |head| observer.head(head, &warnings),
            |chunk| {
                if batch.append(chunk) {
                    StreamControl::Continue
                } else {
                    StreamControl::Stop
                }
            },
            |url, headers| {
                observer.cookies(
                    url.as_str(),
                    headers
                        .iter()
                        .map(|(name, value)| {
                            (
                                name.as_str().to_owned(),
                                String::from_utf8_lossy(value.as_bytes()).into_owned(),
                            )
                        })
                        .collect(),
                );
            },
        )
        .await;
    batch.flush(true);
    match result {
        Ok(run) => observer.complete(run.bytes_received, run.bytes_decoded, run.total_time),
        Err(error) => observer.failure(&RunFailureDocument::from_run_error(&error)),
    }
}
#[derive(Debug, Default)]
struct BatchState {
    bytes: Vec<u8>,
    scheduled: bool,
    terminated: bool,
    stopped: bool,
}
struct StreamBatch {
    state: Mutex<BatchState>,
    observer: Arc<dyn TransportObserver>,
}
impl std::fmt::Debug for StreamBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamBatch").finish_non_exhaustive()
    }
}
impl StreamBatch {
    fn append(self: &Arc<Self>, bytes: &[u8]) -> bool {
        let mut state = crate::lock(&self.state);
        if state.terminated || state.stopped {
            return false;
        }
        state.bytes.extend_from_slice(bytes);
        if !state.scheduled {
            state.scheduled = true;
            let batch = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(super::run::PUBLICATION_INTERVAL).await;
                batch.flush(false);
            });
        }
        if state.bytes.len() >= 32 * 1024 {
            state.stopped = !self.observer.chunk(&state.bytes);
            state.bytes.clear();
        }
        !state.stopped
    }
    fn flush(&self, terminal: bool) {
        let mut state = crate::lock(&self.state);
        state.scheduled = false;
        if state.terminated {
            return;
        }
        if !state.bytes.is_empty() {
            state.stopped = !self.observer.chunk(&state.bytes);
            state.bytes.clear();
        }
        if terminal {
            state.terminated = true;
        }
    }
}

#[cfg(test)]
mod tests;
