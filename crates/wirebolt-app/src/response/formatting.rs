#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "String", into = "String")]
pub enum ResponseFailureKind {
    Cancelled,
    Connection,
    ConnectTimeout,
    Timeout,
    TotalTimeout,
    ReadTimeout,
    Tls,
    TlsConfiguration,
    Proxy,
    ProxyConfiguration,
    Keychain,
    Request,
    ResponseBody,
    ResponseTooLarge,
    StreamStopped,
    Transport,
    InvalidRequest,
    InvalidInput,
    UnresolvedValue,
    Internal,
    Bridge,
    Storage,
    Export,
    Workspace,
    Other { code: String },
}
impl ResponseFailureKind {
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Connection => "connection",
            Self::ConnectTimeout => "connect_timeout",
            Self::Timeout => "timeout",
            Self::TotalTimeout => "total_timeout",
            Self::ReadTimeout => "read_timeout",
            Self::Tls => "tls",
            Self::TlsConfiguration => "tls_configuration",
            Self::Proxy => "proxy",
            Self::ProxyConfiguration => "proxy_configuration",
            Self::Keychain => "keychain",
            Self::Request => "request",
            Self::ResponseBody => "response_body",
            Self::ResponseTooLarge => "response_too_large",
            Self::StreamStopped => "stream_stopped",
            Self::Transport => "transport",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidInput => "invalid_input",
            Self::UnresolvedValue => "unresolved_value",
            Self::Internal => "internal",
            Self::Bridge => "bridge",
            Self::Storage => "storage",
            Self::Export => "export",
            Self::Workspace => "workspace",
            Self::Other { code } => code,
        }
    }
}
impl From<String> for ResponseFailureKind {
    fn from(code: String) -> Self {
        match code.as_str() {
            "cancelled" => Self::Cancelled,
            "connection" => Self::Connection,
            "connect_timeout" => Self::ConnectTimeout,
            "timeout" => Self::Timeout,
            "total_timeout" => Self::TotalTimeout,
            "read_timeout" => Self::ReadTimeout,
            "tls" => Self::Tls,
            "tls_configuration" => Self::TlsConfiguration,
            "proxy" => Self::Proxy,
            "proxy_configuration" => Self::ProxyConfiguration,
            "keychain" => Self::Keychain,
            "request" => Self::Request,
            "response_body" => Self::ResponseBody,
            "response_too_large" => Self::ResponseTooLarge,
            "stream_stopped" => Self::StreamStopped,
            "transport" => Self::Transport,
            "invalid_request" => Self::InvalidRequest,
            "invalid_input" => Self::InvalidInput,
            "unresolved_value" => Self::UnresolvedValue,
            "internal" => Self::Internal,
            "bridge" => Self::Bridge,
            "storage" => Self::Storage,
            "export" => Self::Export,
            "workspace" => Self::Workspace,
            _ => Self::Other { code },
        }
    }
}
impl From<ResponseFailureKind> for String {
    fn from(kind: ResponseFailureKind) -> Self {
        kind.code().to_owned()
    }
}

use url::Url;

#[must_use]
pub fn format_duration(seconds: f64) -> String {
    let milliseconds = seconds.max(0.0) * 1000.0;
    if milliseconds < 0.05 {
        return "<0.1 ms".into();
    }
    if milliseconds < 0.95 {
        return format!("{milliseconds:.1} ms");
    }
    if milliseconds < 999.5 {
        return format!("{:.0} ms", milliseconds.round());
    }
    let seconds = milliseconds / 1000.0;
    if seconds < 9.995 {
        return format!("{seconds:.2} s");
    }
    if seconds < 59.95 {
        return format!("{seconds:.1} s");
    }
    let whole = seconds.round();
    format!("{:.0} min {:.0} s", (whole / 60.0).floor(), whole % 60.0)
}
#[must_use]
pub fn format_elapsed(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    if seconds < 59.95 {
        return format!("{seconds:.1} s");
    }
    format!(
        "{:.0} min {:.0} s",
        (seconds / 60.0).floor(),
        seconds.floor() % 60.0
    )
}
#[must_use]
pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a Teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => fallback_phrase(status),
    }
}
fn fallback_phrase(status: u16) -> &'static str {
    match status {
        100..200 => "Informational",
        200..300 => "Success",
        300..400 => "Redirection",
        400..500 => "Client Error",
        500..600 => "Server Error",
        _ => "Response",
    }
}
#[must_use]
pub fn status_line(status: u16) -> String {
    format!("{status} {}", reason_phrase(status))
}
#[must_use]
pub fn completion_summary(
    status: Option<u16>,
    nanoseconds: Option<u64>,
    formatted_bytes: String,
) -> String {
    let mut parts = Vec::new();
    if let Some(status) = status {
        parts.push(status_line(status));
    }
    if let Some(time) = nanoseconds {
        parts.push(format_duration(
            std::time::Duration::from_nanos(time).as_secs_f64(),
        ));
    }
    parts.push(formatted_bytes);
    parts.join(", ")
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureIssue {
    pub path: String,
    pub kind: String,
    pub reference: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureCategory {
    Cancelled,
    Connection,
    Timeout,
    Tls,
    Proxy,
    Secrets,
    InvalidRequest,
    Response,
    InternalError,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureMessage {
    pub category: FailureCategory,
    pub title: String,
    pub message: String,
    pub system_image: String,
    pub suggests_network_settings: bool,
}
#[must_use]
pub fn failure_message(
    kind: &ResponseFailureKind,
    issues: &[FailureIssue],
    host: Option<&str>,
) -> FailureMessage {
    use FailureCategory as C;
    let to = host.map_or_else(String::new, |h| format!(" to {h}"));
    let at = host.map_or_else(String::new, |h| format!(" at {h}"));
    let (category, title, message) = match kind {
        ResponseFailureKind::Cancelled => (C::Cancelled, "Request Cancelled".into(), "The request was cancelled before the response finished.".into()),
        ResponseFailureKind::Connection => (C::Connection, format!("Couldn’t Connect{to}"), "The connection failed before the server responded. Check that the server is running and that the address, port, proxy and TLS settings are correct.".into()),
        ResponseFailureKind::ConnectTimeout | ResponseFailureKind::Timeout => (C::Timeout, format!("Connection{to} Timed Out"), "The server didn’t accept the connection in time. Check the address and your network or proxy settings.".into()),
        ResponseFailureKind::TotalTimeout => (C::Timeout, format!("Request{to} Timed Out"), "The response didn’t finish within the total timeout. Increase the timeout in Settings or try again.".into()),
        ResponseFailureKind::ReadTimeout => (C::Timeout, format!("Server{at} Stopped Responding"), "No data arrived within the read timeout. Increase the timeout in Settings or try again.".into()),
        ResponseFailureKind::Tls => (C::Tls, "TLS Verification Failed".into(), "The server’s certificate couldn’t be verified. Check the certificate and trust settings.".into()),
        ResponseFailureKind::TlsConfiguration => (C::Tls, "TLS Settings Couldn’t Be Loaded".into(), "The client certificate or custom CA for this request couldn’t be loaded. Check the TLS settings.".into()),
        ResponseFailureKind::Proxy | ResponseFailureKind::ProxyConfiguration => (C::Proxy, "Proxy Unavailable".into(), "The proxy for this request couldn’t be set up. Check the proxy address, port and credentials.".into()),
        ResponseFailureKind::Keychain => (C::Secrets, "Secrets Unavailable".into(), "Wirebolt couldn’t read secrets from the Keychain. Unlock the Keychain and try again.".into()),
        ResponseFailureKind::Request => (C::InvalidRequest, "Couldn’t Send the Request".into(), "The request failed while it was being sent. Check the body and any attached file.".into()),
        ResponseFailureKind::ResponseBody => (C::Response, "Couldn’t Read the Response".into(), "The response body couldn’t be received or decoded.".into()),
        ResponseFailureKind::ResponseTooLarge => (C::Response, "Response Too Large".into(), "The response exceeded the maximum size Wirebolt stores.".into()),
        ResponseFailureKind::StreamStopped => (C::Response, "Response Stopped".into(), "Receiving the response was stopped before it finished.".into()),
        ResponseFailureKind::Transport => (C::Connection, format!("Request{to} Failed"), "The HTTP connection failed while the request was in progress. Try again.".into()),
        ResponseFailureKind::InvalidRequest | ResponseFailureKind::InvalidInput | ResponseFailureKind::UnresolvedValue => (C::InvalidRequest, "Invalid Request".into(), issues.first().map_or_else(|| "Check the URL, headers and body, then try again.".into(), describe_issue)),
        ResponseFailureKind::Internal | ResponseFailureKind::Bridge => (C::InternalError, "Request Failed".into(), "Wirebolt couldn’t run the request. Try again.".into()),
        _ => (C::InternalError, "Request Failed".into(), sentence(kind.code())),
    };
    let system_image = match category {
        C::Cancelled => "xmark.circle",
        C::Connection | C::Proxy => "network.slash",
        C::Timeout => "clock.badge.exclamationmark",
        C::Tls => "lock.slash",
        C::Secrets => "key.slash",
        C::InvalidRequest => "exclamationmark.triangle",
        C::Response | C::InternalError => "exclamationmark.circle",
    }
    .into();
    FailureMessage {
        category,
        title,
        message,
        system_image,
        suggests_network_settings: matches!(
            category,
            C::Connection | C::Timeout | C::Tls | C::Proxy
        ),
    }
}
#[must_use]
pub fn failure_host(url: Option<&str>) -> Option<String> {
    let url = url?.trim_matches(|c: char| c.is_whitespace() && !matches!(c, '\n' | '\r'));
    if url.contains("{{") {
        return None;
    }
    let validation = if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.to_owned()
    };
    Url::parse(&validation).ok()?.host_str()?;
    let authority = url
        .split_once("://")
        .map(|(_, a)| a)
        .or_else(|| url.strip_prefix("//"))?
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()?;
    let (raw_host, port) = if authority.starts_with('[') {
        let end = authority.find(']')? + 1;
        (
            &authority[..end],
            authority[end..]
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok()),
        )
    } else {
        authority
            .rsplit_once(':')
            .map_or((authority, None), |(host, port)| {
                (host, port.parse::<u16>().ok())
            })
    };
    let host = percent_encoding::percent_decode_str(raw_host)
        .decode_utf8()
        .ok()?;
    let host = host
        .split('.')
        .map(|label| {
            if label.to_ascii_lowercase().starts_with("xn--") {
                idna::domain_to_unicode(label).0
            } else {
                label.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(".");
    Some(port.map_or_else(|| host.clone(), |p| format!("{host}:{p}")))
}

#[must_use]
pub fn describe_issue(issue: &FailureIssue) -> String {
    let reference = issue
        .reference
        .as_ref()
        .map_or_else(String::new, |r| format!("“{r}”"));
    let detail = match issue.kind.as_str() {
        "invalid_method" => "The HTTP method isn’t valid.".to_string(),
        "invalid_url" => "The URL isn’t valid.".to_string(),
        "invalid_header_name" => "A header name isn’t valid.".to_string(),
        "invalid_header_value" => "A header value isn’t valid.".to_string(),
        "conflicting_header" => "Two headers conflict with each other.".to_string(),
        "invalid_json" => "The JSON body isn’t valid.".to_string(),
        "invalid_body" => "The body isn’t valid.".to_string(),
        "file_unavailable" => "The body file can’t be read.".to_string(),
        "invalid_template" => "A {{variable}} template isn’t valid.".to_string(),
        "template_too_deep" => "Variables are nested too deeply.".to_string(),
        "resolved_value_too_large" => "A resolved value is too large.".to_string(),
        "missing_variable" | "unresolved_value" => {
            format!("The variable {reference} isn’t defined in the active environment.")
        }
        "cyclic_variable" => format!("The variable {reference} refers to itself."),
        "missing_secret" => format!("The secret {reference} isn’t in the Keychain."),
        _ => sentence(&issue.kind),
    };
    let location = if issue.path.is_empty() || issue.path == "values" {
        String::new()
    } else {
        format!(" ({})", issue.path)
    };
    detail.replace("  ", " ") + &location
}
#[must_use]
pub fn sentence(kind: &str) -> String {
    let words = kind.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(
        || "The request failed.".into(),
        |c| format!("{}{}.", c.to_uppercase(), chars.as_str()),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResponseBadge {
    Status { status: u16 },
    Failed { title: String },
    Cancelled,
}
impl ResponseBadge {
    #[must_use]
    pub fn new(
        status: Option<u16>,
        failure: Option<&ResponseFailureKind>,
        issues: &[FailureIssue],
        host: Option<&str>,
    ) -> Option<Self> {
        if let Some(kind) = failure {
            Some(if *kind == ResponseFailureKind::Cancelled {
                Self::Cancelled
            } else {
                Self::Failed {
                    title: failure_message(kind, issues, host).title,
                }
            })
        } else {
            status.map(|status| Self::Status { status })
        }
    }
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Status { status } => status_line(*status),
            Self::Failed { .. } => "Failed".into(),
            Self::Cancelled => "Cancelled".into(),
        }
    }
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::Status { status } => status_line(*status),
            Self::Failed { title } => title.clone(),
            Self::Cancelled => "Request Cancelled".into(),
        }
    }
}
