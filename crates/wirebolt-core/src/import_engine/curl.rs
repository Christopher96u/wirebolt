//! cURL command lines, including multi-line "Copy as cURL" output and
//! several commands in one input.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    ImportError, ImportedCollection, ImportedRequest, ImportedRequestSettings, ImportedWorkspace,
    ParsedImport, authorization_header_authentication, method_and_path_name,
};
use crate::{
    MultipartPart, MultipartPartKind, RequestAuthentication, RequestBody, RequestHeader,
    RequestValueField, TransportSettings, ValueSource,
};

pub(super) fn parse(source: &str, file_name: Option<&str>) -> Result<ParsedImport, ImportError> {
    let commands = commands(source)?;
    let mut collection = ImportedCollection {
        name: file_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Imported cURL")
            .to_owned(),
        ..ImportedCollection::default()
    };
    let mut warnings = Warnings::default();
    let mut ignored_commands = 0_usize;
    let mut last_error = None;
    let mut request_settings = BTreeMap::new();
    for words in commands {
        if !is_curl(&words[0]) {
            ignored_commands += 1;
            continue;
        }
        match Command::parse(&words[1..], &mut warnings) {
            Ok(command) => {
                let index = collection.requests.len();
                let source_id = format!("curl-{index}");
                let (request, transport) = command.into_request();
                if let Some(transport) = transport {
                    request_settings.insert(
                        source_id.clone(),
                        ImportedRequestSettings {
                            proxy_override: None,
                            transport,
                            inherits_workspace_transport: false,
                        },
                    );
                }
                collection.requests.push(ImportedRequest {
                    source_id,
                    order: i64::try_from(index).unwrap_or(i64::MAX),
                    ..request
                });
            }
            Err(error) => last_error = Some(error),
        }
    }
    if collection.requests.is_empty() {
        return Err(last_error.unwrap_or(ImportError::new(
            "The text isn't a cURL command. It must start with curl.",
        )));
    }
    if last_error.is_some() {
        warnings.push("Commands without a URL were skipped.");
    }
    if ignored_commands > 0 {
        warnings.push("Shell commands other than curl were ignored.");
    }
    collection.warnings = warnings.finish();
    Ok(ParsedImport::from(ImportedWorkspace {
        collections: vec![collection],
        environments: Vec::new(),
        request_settings,
    }))
}

fn is_curl(word: &str) -> bool {
    let program = word.rsplit(['/', '\\']).next().unwrap_or(word);
    program == "curl" || program.eq_ignore_ascii_case("curl.exe")
}

#[derive(Default)]
struct Warnings {
    messages: Vec<String>,
    ignored_options: BTreeSet<String>,
}

impl Warnings {
    fn push(&mut self, message: &str) {
        if !self.messages.iter().any(|existing| existing == message) {
            self.messages.push(message.to_owned());
        }
    }

    fn finish(mut self) -> Vec<String> {
        if !self.ignored_options.is_empty() {
            let options = self.ignored_options.into_iter().collect::<Vec<_>>();
            self.messages.push(format!(
                "These cURL options aren't supported and were ignored: {}.",
                options.join(", ")
            ));
        }
        self.messages
    }
}

enum Data {
    /// Sent as written; `@file` is read from disk by curl.
    Text { value: String, file_allowed: bool },
    /// `--data-urlencode`, encoded when the body is built.
    UrlEncoded(String),
}

struct FormPart {
    name: String,
    value: String,
    file: bool,
    file_name: Option<String>,
    content_type: Option<String>,
}

#[derive(Default)]
struct Command {
    method: Option<String>,
    urls: Vec<String>,
    url_queries: Vec<String>,
    headers: Vec<RequestHeader>,
    cookies: Vec<String>,
    data: Vec<Data>,
    form: Vec<FormPart>,
    upload_file: Option<String>,
    authentication: Option<RequestAuthentication>,
    transport: Option<TransportSettings>,
    head: bool,
    get: bool,
    json: bool,
}

/// Short options that take a value, attached (`-XPUT`) or as the next word.
const SHORT_WITH_VALUE: &str = "XHdubFoeAmxTEcKDwrUyYzCtQP";

/// Options with no effect on the saved request.
const SILENT: &[&str] = &[
    "compressed",
    "silent",
    "show-error",
    "verbose",
    "include",
    "http1.0",
    "http1.1",
    "http2",
    "http2-prior-knowledge",
    "http3",
    "http3-only",
    "output",
    "remote-name",
    "remote-name-all",
    "dump-header",
    "write-out",
    "cookie-jar",
    "output-dir",
    "create-dirs",
    "progress-bar",
    "no-progress-meter",
    "fail",
    "fail-with-body",
    "fail-early",
    "stderr",
    "no-buffer",
    "globoff",
    "path-as-is",
    "retry",
    "retry-delay",
    "retry-max-time",
    "retry-all-errors",
    "retry-connrefused",
    "connect-timeout",
    "ipv4",
    "ipv6",
    "tcp-nodelay",
    "no-keepalive",
    "keepalive-time",
    "remote-time",
    "remote-header-name",
    "trace",
    "trace-ascii",
    "trace-time",
    "styled-output",
    "no-styled-output",
    "basic",
    "raw",
    "tr-encoding",
    "no-sessionid",
    "no-alpn",
    "no-npn",
    "parallel",
    "parallel-max",
    "parallel-immediate",
    "limit-rate",
    "speed-limit",
    "speed-time",
    "max-filesize",
    "junk-session-cookies",
    "xattr",
    "ssl-no-revoke",
    "ssl-revoke-best-effort",
    "no-clobber",
    "clobber",
    "rate",
    "expect100-timeout",
    "happy-eyeballs-timeout-ms",
    "sslv2",
    "sslv3",
    "tlsv1",
    "tlsv1.0",
    "tlsv1.1",
    "tlsv1.2",
    "tlsv1.3",
    "tls-max",
    "ciphers",
    "tls13-ciphers",
];

/// Long options whose value is the next word, so it is never read as a URL.
const LONG_WITH_VALUE: &[&str] = &[
    "request",
    "header",
    "data",
    "data-raw",
    "data-binary",
    "data-ascii",
    "data-urlencode",
    "json",
    "form",
    "form-string",
    "user",
    "cookie",
    "cookie-jar",
    "output",
    "referer",
    "user-agent",
    "url",
    "url-query",
    "max-time",
    "connect-timeout",
    "proxy",
    "proxy-user",
    "cert",
    "key",
    "cacert",
    "capath",
    "upload-file",
    "write-out",
    "retry",
    "retry-delay",
    "retry-max-time",
    "range",
    "resolve",
    "connect-to",
    "limit-rate",
    "max-redirs",
    "oauth2-bearer",
    "aws-sigv4",
    "dump-header",
    "config",
    "interface",
    "local-port",
    "unix-socket",
    "abstract-unix-socket",
    "ciphers",
    "tls13-ciphers",
    "tls-max",
    "pass",
    "cert-type",
    "key-type",
    "netrc-file",
    "trace",
    "trace-ascii",
    "stderr",
    "variable",
    "expect100-timeout",
    "keepalive-time",
    "speed-limit",
    "speed-time",
    "time-cond",
    "noproxy",
    "preproxy",
    "proxy-header",
    "socks4",
    "socks4a",
    "socks5",
    "socks5-hostname",
    "doh-url",
    "dns-servers",
    "dns-interface",
    "dns-ipv4-addr",
    "dns-ipv6-addr",
    "pinnedpubkey",
    "request-target",
    "service-name",
    "login-options",
    "output-dir",
    "create-file-mode",
    "max-filesize",
    "mail-from",
    "mail-rcpt",
    "mail-auth",
    "proto",
    "proto-redir",
    "proto-default",
    "happy-eyeballs-timeout-ms",
    "hostpubmd5",
    "hostpubsha256",
    "crlfile",
    "krb",
    "delegation",
    "ftp-port",
    "ftp-method",
    "ftp-account",
    "quote",
    "telnet-option",
    "tlsuser",
    "tlspassword",
    "tlsauthtype",
    "proxy-cacert",
    "proxy-capath",
    "proxy-cert",
    "proxy-cert-type",
    "proxy-key",
    "proxy-key-type",
    "proxy-pass",
    "proxy-ciphers",
    "proxy-service-name",
    "socks5-gssapi-service",
    "sasl-authzid",
    "parallel-max",
    "rate",
    "ip-tos",
    "vlan-priority",
    "ech",
    "etag-save",
    "etag-compare",
    "alt-svc",
    "hsts",
];

impl Command {
    fn parse(arguments: &[String], warnings: &mut Warnings) -> Result<Self, ImportError> {
        let mut command = Self::default();
        let mut index = 0;
        let mut options_ended = false;
        while index < arguments.len() {
            let argument = &arguments[index];
            index += 1;
            if options_ended || !argument.starts_with('-') || argument == "-" {
                command.urls.push(argument.clone());
                continue;
            }
            if argument == "--" {
                options_ended = true;
                continue;
            }
            if let Some(name) = argument.strip_prefix("--") {
                let value = if LONG_WITH_VALUE.contains(&name) {
                    index += 1;
                    Some(arguments.get(index - 1).cloned().ok_or_else(|| {
                        ImportError::message(format!("The cURL option --{name} needs a value."))
                    })?)
                } else {
                    None
                };
                command.apply(name, value, warnings);
                continue;
            }
            // A cluster of short options such as `-sSL`, `-XPUT` or `-HAccept: x`.
            let cluster = &argument[1..];
            for (offset, flag) in cluster.char_indices() {
                if SHORT_WITH_VALUE.contains(flag) {
                    let rest = &cluster[offset + flag.len_utf8()..];
                    let value = if rest.is_empty() {
                        index += 1;
                        arguments.get(index - 1).cloned().ok_or_else(|| {
                            ImportError::message(format!("The cURL option -{flag} needs a value."))
                        })?
                    } else {
                        rest.to_owned()
                    };
                    command.apply(long_name(flag), Some(value), warnings);
                    break;
                }
                command.apply(long_name(flag), None, warnings);
            }
        }
        if command.urls.is_empty() {
            return Err(ImportError::new("The cURL command has no URL."));
        }
        if command.urls.len() > 1 {
            warnings.push("Only the first URL of a cURL command was imported.");
        }
        let data_files = command.data.iter().filter_map(|data| match data {
            Data::Text {
                value,
                file_allowed: true,
            } => value.strip_prefix('@'),
            _ => None,
        });
        let form_files = command
            .form
            .iter()
            .filter(|part| part.file)
            .map(|part| part.value.as_str());
        let mut files = data_files
            .chain(form_files)
            .chain(command.upload_file.as_deref());
        if files.clone().any(|path| path == "-") {
            warnings.push("Data read from standard input (@-) can't be imported.");
        }
        if files.any(|path| path != "-" && !path.starts_with('/')) {
            warnings.push(
                "Relative file paths depend on the folder the command ran in; check them before sending.",
            );
        }
        if !command.form.is_empty() && !command.data.is_empty() {
            warnings.push(
                "cURL -d data can't be combined with -F form fields, so the data was ignored.",
            );
        }
        Ok(command)
    }

    fn apply(&mut self, name: &str, value: Option<String>, warnings: &mut Warnings) {
        let value = value.unwrap_or_default();
        match name {
            "request" => self.method = Some(value.to_ascii_uppercase()),
            "header" => self.header(&value, warnings),
            "data" | "data-ascii" | "data-binary" => self.data.push(Data::Text {
                value,
                file_allowed: true,
            }),
            "data-raw" => self.data.push(Data::Text {
                value,
                file_allowed: false,
            }),
            "json" => {
                self.json = true;
                self.data.push(Data::Text {
                    value,
                    file_allowed: true,
                });
            }
            "data-urlencode" => self.data.push(Data::UrlEncoded(value)),
            "form" | "form-string" => self.form_part(&value, name == "form", warnings),
            "user" => {
                let (username, password) = value.split_once(':').unwrap_or_else(|| {
                    warnings
                        .push("cURL -u without a password was imported with an empty password.");
                    (value.as_str(), "")
                });
                self.authentication = Some(RequestAuthentication::Basic {
                    username: ValueSource::literal(username),
                    password: ValueSource::literal(password),
                });
            }
            "oauth2-bearer" => {
                self.authentication = Some(RequestAuthentication::Bearer {
                    token: ValueSource::literal(value),
                });
            }
            "cookie" => {
                if value.contains('=') {
                    self.cookies.push(value);
                } else {
                    warnings.push("Cookie files can't be imported, so cURL -b <file> was ignored.");
                }
            }
            "referer" => {
                let referer = value.strip_suffix(";auto").unwrap_or(&value);
                if !referer.is_empty() {
                    self.headers.push(RequestHeader::enabled(
                        "Referer",
                        ValueSource::literal(referer),
                    ));
                }
            }
            "user-agent" => self.headers.push(RequestHeader::enabled(
                "User-Agent",
                ValueSource::literal(value),
            )),
            "url" => self.urls.push(value),
            "url-query" => self.url_queries.push(value),
            "head" => self.head = true,
            "get" => self.get = true,
            "upload-file" => self.upload_file = Some(value),
            "insecure" => self.transport().validate_tls = false,
            "location" | "location-trusted" => self.transport().follow_redirects = true,
            "max-redirs" => {
                if let Ok(maximum) = value.trim().parse::<u8>() {
                    self.transport().maximum_redirects = maximum;
                }
            }
            "max-time" => {
                if let Ok(seconds) = value.trim().parse::<f64>()
                    && seconds.is_finite()
                    && seconds > 0.0
                {
                    // Bounded above, so the conversion cannot truncate meaningfully.
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "the value is positive and clamped to a day"
                    )]
                    let milliseconds = (seconds.min(86_400.0) * 1000.0).round() as u64;
                    self.transport().total_timeout_ms = milliseconds;
                }
            }
            "cacert" => self.transport().custom_ca_path = Some(value),
            name if SILENT.contains(&name) || name.starts_with("no-") => {}
            name => {
                warnings.ignored_options.insert(format!("--{name}"));
            }
        }
    }

    fn transport(&mut self) -> &mut TransportSettings {
        self.transport
            .get_or_insert_with(TransportSettings::default)
    }

    fn header(&mut self, value: &str, warnings: &mut Warnings) {
        if value.starts_with('@') {
            warnings.push("Header files can't be imported, so cURL -H @<file> was ignored.");
            return;
        }
        if let Some(name) = value.strip_suffix(';').filter(|name| !name.contains(':')) {
            // `-H 'Name;'` sends the header with an empty value.
            self.headers.push(RequestHeader::enabled(
                name.trim(),
                ValueSource::literal(""),
            ));
            return;
        }
        let Some((name, header_value)) = value.split_once(':') else {
            warnings.push("Headers without a colon were ignored.");
            return;
        };
        let header_value = header_value.trim();
        // `-H 'Name:'` only removes a header curl would add itself.
        if name.trim().is_empty() || header_value.is_empty() {
            return;
        }
        self.headers.push(RequestHeader::enabled(
            name.trim(),
            ValueSource::literal(header_value),
        ));
    }

    fn form_part(&mut self, value: &str, parse_files: bool, warnings: &mut Warnings) {
        let Some((name, content)) = value.split_once('=') else {
            warnings.push("Form fields without a name were ignored.");
            return;
        };
        let mut part = FormPart {
            name: name.to_owned(),
            value: content.to_owned(),
            file: false,
            file_name: None,
            content_type: None,
        };
        if parse_files && let Some(rest) = content.strip_prefix(['@', '<']) {
            let mut attributes = rest.split(';');
            part.file = true;
            attributes
                .next()
                .unwrap_or_default()
                .trim_matches('"')
                .clone_into(&mut part.value);
            for attribute in attributes {
                if let Some(content_type) = attribute.strip_prefix("type=") {
                    part.content_type = Some(content_type.to_owned());
                } else if let Some(file_name) = attribute.strip_prefix("filename=") {
                    part.file_name = Some(file_name.trim_matches('"').to_owned());
                }
            }
            if content.starts_with('<') {
                warnings.push(
                    "Form fields read from a file (-F name=<file) were imported as file uploads.",
                );
            }
        } else if parse_files && let Some((text, content_type)) = content.split_once(";type=") {
            text.clone_into(&mut part.value);
            part.content_type = Some(content_type.to_owned());
        }
        self.form.push(part);
    }

    fn into_request(self) -> (ImportedRequest, Option<TransportSettings>) {
        let mut headers = self.headers;
        if !self.cookies.is_empty() {
            headers.push(RequestHeader::enabled(
                "Cookie",
                ValueSource::literal(self.cookies.join("; ")),
            ));
        }
        let mut url = with_default_scheme(&self.urls[0]);
        for query in &self.url_queries {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(query);
        }
        let mut method = self.method.clone();
        let body = if !self.form.is_empty() {
            headers.retain(|header| {
                !(header.name.eq_ignore_ascii_case("content-type")
                    && literal(header).is_some_and(|value| {
                        value
                            .to_ascii_lowercase()
                            .starts_with("multipart/form-data")
                    }))
            });
            method.get_or_insert_with(|| "POST".to_owned());
            multipart(self.form)
        } else if let Some(path) = self.upload_file {
            method.get_or_insert_with(|| "PUT".to_owned());
            RequestBody::File {
                path,
                content_type: None,
            }
        } else if self.get {
            let query = self
                .data
                .iter()
                .map(|data| match data {
                    Data::Text { value, .. } => value.clone(),
                    Data::UrlEncoded(value) => url_encoded(value),
                })
                .collect::<Vec<_>>()
                .join("&");
            if !query.is_empty() {
                url.push(if url.contains('?') { '&' } else { '?' });
                url.push_str(&query);
            }
            RequestBody::Empty
        } else if self.data.is_empty() {
            RequestBody::Empty
        } else {
            method.get_or_insert_with(|| "POST".to_owned());
            if self.json {
                ensure_header(&mut headers, "Content-Type", "application/json");
                ensure_header(&mut headers, "Accept", "application/json");
            }
            data_body(&self.data, &headers)
        };
        let method = method.unwrap_or_else(|| if self.head { "HEAD" } else { "GET" }.to_owned());
        let authentication = match self.authentication {
            Some(authentication) => {
                headers.retain(|header| !header.name.eq_ignore_ascii_case("authorization"));
                authentication
            }
            None => authorization_header_authentication(&mut headers),
        };
        let request = ImportedRequest {
            name: method_and_path_name(&method, &url),
            method,
            url,
            headers,
            authentication,
            body,
            ..ImportedRequest::default()
        };
        (request, self.transport)
    }
}

fn long_name(flag: char) -> &'static str {
    match flag {
        'X' => "request",
        'H' => "header",
        'd' => "data",
        'u' => "user",
        'b' => "cookie",
        'F' => "form",
        'o' => "output",
        'e' => "referer",
        'A' => "user-agent",
        'm' => "max-time",
        'x' => "proxy",
        'T' => "upload-file",
        'E' => "cert",
        'c' => "cookie-jar",
        'K' => "config",
        'D' => "dump-header",
        'w' => "write-out",
        'r' => "range",
        'U' => "proxy-user",
        'y' => "speed-time",
        'Y' => "speed-limit",
        'z' => "time-cond",
        'C' => "continue-at",
        't' => "telnet-option",
        'Q' => "quote",
        'P' => "ftp-port",
        'I' => "head",
        'G' => "get",
        'k' => "insecure",
        'L' => "location",
        's' => "silent",
        'S' => "show-error",
        'v' => "verbose",
        'i' => "include",
        'O' => "remote-name",
        'J' => "remote-header-name",
        'R' => "remote-time",
        'f' => "fail",
        'g' => "globoff",
        'N' => "no-buffer",
        'Z' => "parallel",
        '#' => "progress-bar",
        '4' => "ipv4",
        '6' => "ipv6",
        '0' => "http1.0",
        '1' => "tlsv1",
        'n' => "netrc",
        'j' => "junk-session-cookies",
        'l' => "list-only",
        'p' => "proxytunnel",
        'q' => "disable",
        'B' => "use-ascii",
        'M' => "manual",
        'V' => "version",
        'h' => "help",
        'a' => "append",
        _ => "unknown",
    }
}

/// Builds a `-d` family body: `@file` becomes a file body, form-shaped
/// text becomes form fields, JSON becomes a JSON body.
fn data_body(data: &[Data], headers: &[RequestHeader]) -> RequestBody {
    if let [
        Data::Text {
            value,
            file_allowed: true,
        },
    ] = data
        && let Some(path) = value.strip_prefix('@')
        && path != "-"
    {
        return RequestBody::File {
            path: path.to_owned(),
            content_type: None,
        };
    }
    let has_encoded = data.iter().any(|data| matches!(data, Data::UrlEncoded(_)));
    let text = data
        .iter()
        .map(|data| match data {
            Data::Text { value, .. } => value.clone(),
            Data::UrlEncoded(value) => url_encoded(value),
        })
        .collect::<Vec<_>>()
        .join("&");
    let content_type = headers
        .iter()
        .find(|header| header.enabled && header.name.eq_ignore_ascii_case("content-type"))
        .and_then(literal)
        .map(str::to_ascii_lowercase);
    match content_type.as_deref() {
        Some(content_type) if content_type.contains("json") => RequestBody::Json { value: text },
        Some(content_type) if content_type.contains("xml") => RequestBody::Xml { value: text },
        Some(content_type) if content_type.starts_with("text/html") => {
            RequestBody::Html { value: text }
        }
        None if !has_encoded && looks_like_json(&text) => RequestBody::Json { value: text },
        None | Some("application/x-www-form-urlencoded") if is_form_shaped(&text) => {
            RequestBody::FormUrlEncoded {
                fields: url::form_urlencoded::parse(text.as_bytes())
                    .map(|(name, value)| {
                        RequestValueField::enabled(
                            name.into_owned(),
                            ValueSource::literal(value.into_owned()),
                        )
                    })
                    .collect(),
            }
        }
        // curl labels `-d` data as a form unless told otherwise.
        None => RequestBody::Text {
            content_type: Some("application/x-www-form-urlencoded".to_owned()),
            value: text,
        },
        Some(_) => RequestBody::Text {
            content_type: None,
            value: text,
        },
    }
}

fn multipart(form: Vec<FormPart>) -> RequestBody {
    RequestBody::Multipart {
        parts: form
            .into_iter()
            .enumerate()
            .map(|(index, part)| MultipartPart {
                id: format!("part-{index}"),
                name: part.name,
                kind: if part.file {
                    MultipartPartKind::File
                } else {
                    MultipartPartKind::Text
                },
                value: ValueSource::literal(if part.file {
                    String::new()
                } else {
                    part.value.clone()
                }),
                file_path: part.file.then_some(part.value),
                file_name: part.file_name,
                content_type: part.content_type,
                enabled: true,
            })
            .collect(),
    }
}

/// Applies `--data-urlencode` rules: `content`, `=content`, `name=content`.
fn url_encoded(value: &str) -> String {
    let encode =
        |text: &str| url::form_urlencoded::byte_serialize(text.as_bytes()).collect::<String>();
    match value.split_once('=') {
        Some(("", content)) => encode(content),
        Some((name, content)) => format!("{name}={}", encode(content)),
        None => encode(value),
    }
}

fn is_form_shaped(text: &str) -> bool {
    !text.is_empty()
        && text.split('&').all(|pair| {
            pair.split_once('=').is_some_and(|(name, _)| {
                !name.is_empty()
                    && !name.contains(|character: char| {
                        character.is_whitespace() || "{}[]\"".contains(character)
                    })
            })
        })
}

fn looks_like_json(value: &str) -> bool {
    let trimmed = value.trim_start();
    (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<serde::de::IgnoredAny>(value).is_ok()
}

fn ensure_header(headers: &mut Vec<RequestHeader>, name: &str, value: &str) {
    if !headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(name))
    {
        headers.push(RequestHeader::enabled(name, ValueSource::literal(value)));
    }
}

fn literal(header: &RequestHeader) -> Option<&str> {
    match &header.value {
        ValueSource::Literal(value) => Some(value),
        ValueSource::Secret { .. } => None,
    }
}

/// curl assumes `http://` when a URL has no scheme.
fn with_default_scheme(url: &str) -> String {
    if url.contains("://") || url.starts_with("{{") {
        url.to_owned()
    } else {
        format!("http://{url}")
    }
}

/// Splits shell text into commands of words. Handles quoting, `$'…'`,
/// backslash-newline continuations, comments and `;`, `&&`, `|` separators.
fn commands(source: &str) -> Result<Vec<Vec<String>>, ImportError> {
    let mut commands = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut characters = source.chars().peekable();
    let end_word = |word: &mut String, in_word: &mut bool, words: &mut Vec<String>| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    let end_command = |words: &mut Vec<String>, commands: &mut Vec<Vec<String>>| {
        if !words.is_empty() {
            commands.push(std::mem::take(words));
        }
    };
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some('\n') | None => {}
                Some('\r') => {
                    if characters.peek() == Some(&'\n') {
                        characters.next();
                    }
                }
                Some(escaped) => {
                    word.push(escaped);
                    in_word = true;
                }
            },
            // Windows `cmd` line continuation from "Copy as cURL (cmd)".
            '^' if matches!(characters.peek(), Some('\n' | '\r')) => {
                while matches!(characters.peek(), Some('\n' | '\r')) {
                    characters.next();
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match characters.next() {
                        Some('\'') => break,
                        Some(inner) => word.push(inner),
                        None => return Err(unterminated()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match characters.next() {
                        Some('"') => break,
                        Some('\\') => match characters.next() {
                            Some('\n') => {}
                            Some(escaped @ ('"' | '\\' | '$' | '`')) => word.push(escaped),
                            Some(other) => {
                                word.push('\\');
                                word.push(other);
                            }
                            None => return Err(unterminated()),
                        },
                        Some(inner) => word.push(inner),
                        None => return Err(unterminated()),
                    }
                }
            }
            '$' if characters.peek() == Some(&'\'') => {
                characters.next();
                in_word = true;
                ansi_c_quoted(&mut characters, &mut word)?;
            }
            '#' if !in_word => {
                while characters.peek().is_some_and(|next| *next != '\n') {
                    characters.next();
                }
            }
            '\n' | '\r' | ';' => {
                end_word(&mut word, &mut in_word, &mut words);
                end_command(&mut words, &mut commands);
            }
            '&' | '|' => {
                if characters.peek() == Some(&character) {
                    characters.next();
                }
                end_word(&mut word, &mut in_word, &mut words);
                end_command(&mut words, &mut commands);
            }
            character if character.is_whitespace() => {
                end_word(&mut word, &mut in_word, &mut words);
            }
            character => {
                word.push(character);
                in_word = true;
            }
        }
    }
    end_word(&mut word, &mut in_word, &mut words);
    end_command(&mut words, &mut commands);
    Ok(commands)
}

fn unterminated() -> ImportError {
    ImportError::new("The cURL command has an unterminated quote.")
}

/// Reads the rest of a bash `$'…'` string, as Chrome's "Copy as cURL" emits.
fn ansi_c_quoted(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
    word: &mut String,
) -> Result<(), ImportError> {
    loop {
        match characters.next() {
            Some('\'') => return Ok(()),
            Some('\\') => {
                let escaped = characters.next().ok_or_else(unterminated)?;
                match escaped {
                    'n' => word.push('\n'),
                    't' => word.push('\t'),
                    'r' => word.push('\r'),
                    '0' => {}
                    'a' => word.push('\u{7}'),
                    'b' => word.push('\u{8}'),
                    'e' | 'E' => word.push('\u{1b}'),
                    'f' => word.push('\u{c}'),
                    'v' => word.push('\u{b}'),
                    'x' => push_code_point(characters, word, 2),
                    'u' => push_code_point(characters, word, 4),
                    'U' => push_code_point(characters, word, 8),
                    other => word.push(other),
                }
            }
            Some(character) => word.push(character),
            None => return Err(unterminated()),
        }
    }
}

fn push_code_point(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
    word: &mut String,
    maximum_digits: usize,
) {
    let mut digits = String::new();
    while digits.len() < maximum_digits
        && let Some(digit) = characters.peek().copied().filter(char::is_ascii_hexdigit)
    {
        digits.push(digit);
        characters.next();
    }
    if let Some(character) = u32::from_str_radix(&digits, 16)
        .ok()
        .and_then(char::from_u32)
    {
        word.push(character);
    }
}

#[cfg(test)]
mod tests {
    use super::commands;

    #[test]
    fn joins_continuations_and_splits_commands() {
        let parsed =
            commands("curl 'https://a.test' \\\n  -H 'A: b'\ncurl https://b.test; echo done")
                .expect("tokenize");
        assert_eq!(
            parsed,
            vec![
                vec!["curl", "https://a.test", "-H", "A: b"],
                vec!["curl", "https://b.test"],
                vec!["echo", "done"],
            ]
        );
    }

    #[test]
    fn reads_ansi_c_and_empty_quotes() {
        let parsed = commands(r#"curl $'{"a":"it\'s\né"}' '' "x\"y""#).expect("tokenize");
        assert_eq!(parsed[0][1], "{\"a\":\"it's\né\"}");
        assert_eq!(parsed[0][2], "");
        assert_eq!(parsed[0][3], "x\"y");
    }
}
