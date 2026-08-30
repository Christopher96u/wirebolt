use std::{error::Error, fmt, time::Duration};

use http::Version;
use reqwest::{Client, redirect, retry};
use tokio::time::{Instant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::PreparedRequest;

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

#[derive(Clone, Debug)]
pub struct HttpEngine {
    client: Client,
}

impl HttpEngine {
    /// Builds a pooled HTTP client with redirects, retries, and proxies disabled.
    ///
    /// Proxy behavior is deliberately added by the separate proxy policy
    /// module. TLS uses Rustls with platform certificate verification.
    ///
    /// # Errors
    ///
    /// Returns [`RunError`] when the TLS backend or system resolver cannot be
    /// initialized.
    pub fn new(config: HttpEngineConfig) -> Result<Self, RunError> {
        let mut builder = Client::builder()
            .connect_timeout(config.connect_timeout)
            .redirect(redirect::Policy::none())
            .retry(retry::never())
            .no_proxy();
        builder = match config.version_policy {
            HttpVersionPolicy::Automatic => builder,
            HttpVersionPolicy::Http1Only => builder.http1_only(),
            HttpVersionPolicy::Http2PriorKnowledge => builder.http2_prior_knowledge(),
        };

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
        mut on_chunk: F,
    ) -> Result<Run, RunError>
    where
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
        let time_to_headers = started.elapsed();
        let status = response.status().as_u16();
        let version = HttpVersion::from(response.version());
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| RunHeader {
                name: name.as_str().to_owned(),
                value: value.as_bytes().to_vec(),
            })
            .collect();

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
            status,
            version,
            headers,
            bytes_received,
            time_to_headers,
            total_time: started.elapsed(),
        })
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
    Transport,
}

#[derive(Debug)]
pub struct RunError {
    kind: RunErrorKind,
    response_limit: Option<u64>,
    source: Option<reqwest::Error>,
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
            source: Some(error.without_url()),
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
            RunErrorKind::Transport => formatter.write_str("HTTP transport failed"),
        }
    }
}

impl Error for RunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|source| source as _)
    }
}
