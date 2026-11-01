//! Shell-safe cURL exports retain Keychain references, never secret material.
use super::models::{DomainMultipartPart, DomainTransport};
use super::{
    models::{
        DomainApiKeyPlacement, DomainAuthentication, DomainBody, DomainField, DomainMultipartKind,
        DomainProxy, DomainProxyRoute, DomainRequestDraft, DomainValueSource,
    },
    request::{display_url, encode_query},
    variables,
};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
};
const SERVICE: &str = "io.github.christopher96u.wirebolt";
#[must_use]
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}
#[must_use]
pub fn prints_as_hex(s: &str) -> bool {
    !s.bytes().all(|b| (0x20..=0x7e).contains(&b))
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Text(String),
    Secret(String),
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Text(Vec<Part>);
impl Text {
    fn plain(&self) -> Option<String> {
        let mut s = String::new();
        for p in &self.0 {
            match p {
                Part::Text(t) => s.push_str(t),
                Part::Secret(_) => return None,
            }
        }
        Some(s)
    }
    fn append(&mut self, p: Part) {
        if let Part::Text(t) = &p {
            if t.is_empty() {
                return;
            }
            if let Some(Part::Text(last)) = self.0.last_mut() {
                last.push_str(t);
                return;
            }
        }
        self.0.push(p);
    }
    fn join(mut self, other: Self) -> Self {
        for p in other.0 {
            self.append(p);
        }
        self
    }
    fn literal(s: impl Into<String>) -> Self {
        let mut result = Self::default();
        result.append(Part::Text(s.into()));
        result
    }
    fn secret(s: impl Into<String>) -> Self {
        Self(vec![Part::Secret(s.into())])
    }
}
#[derive(Debug)]
struct Export<'a> {
    variables: &'a HashMap<String, DomainValueSource>,
    hex: &'a HashSet<String>,
    names: Vec<String>,
    streams: Vec<String>,
}
impl Export<'_> {
    fn keychain_read(&self, name: &str) -> String {
        format!(
            "security find-generic-password -s {} -a {} -w{}",
            shell_quote(SERVICE),
            shell_quote(name),
            if self.hex.contains(name) {
                " | xxd -r -p"
            } else {
                ""
            }
        )
    }
    fn expand_template(&self, text: &str, resolving: &HashSet<String>) -> Text {
        let chars: Vec<u16> = text.encode_utf16().collect();
        let mut location = 0;
        let mut result = Text::default();
        for r in variables::references(text) {
            let start = usize::try_from(r.location).unwrap_or(usize::MAX);
            let end = start + usize::try_from(r.length).unwrap_or(usize::MAX);
            result = result.join(Text::literal(String::from_utf16_lossy(
                &chars[location..start],
            )));
            let whole = String::from_utf16_lossy(&chars[start..end]);
            let part = match self.variables.get(&r.name) {
                Some(DomainValueSource::Secret { secret }) => Text::secret(secret),
                Some(DomainValueSource::Literal { value })
                    if !resolving.contains(&r.name) && resolving.len() < 64 =>
                {
                    let mut next = resolving.clone();
                    next.insert(r.name);
                    self.expand_template(value, &next)
                }
                _ => Text::literal(whole),
            };
            result = result.join(part);
            location = end;
        }
        result.join(Text::literal(String::from_utf16_lossy(&chars[location..])))
    }
    fn expand(&mut self, value: &DomainValueSource) -> Text {
        let text = match value {
            DomainValueSource::Secret { secret } => Text::secret(secret),
            DomainValueSource::Literal { value } => self.expand_template(value, &HashSet::new()),
        };
        self.names.extend(text.0.iter().filter_map(|p| {
            if let Part::Secret(s) = p {
                Some(s.clone())
            } else {
                None
            }
        }));
        text
    }
    fn template(&mut self, text: &str) -> Text {
        self.expand(&DomainValueSource::literal(text))
    }
    fn word(&self, text: &Text) -> String {
        let rendered = text
            .0
            .iter()
            .map(|p| match p {
                Part::Text(s) => shell_quote(s),
                Part::Secret(s) => format!("\"$({})\"", self.keychain_read(s)),
            })
            .collect::<String>();
        if rendered.is_empty() {
            "''".into()
        } else {
            rendered
        }
    }
    fn stream(&mut self, command: &str) -> String {
        let fd = self.streams.len() + 3;
        self.streams.push(format!("{fd}< <({command})"));
        format!("/dev/fd/{fd}")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurlExport {
    pub command: String,
    pub secret_names: Vec<String>,
}
fn form_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "The application uses the standard hasher; no caller supplies custom hashing."
)]
pub fn export(
    d: &DomainRequestDraft,
    variables: &HashMap<String, DomainValueSource>,
    hex: &HashSet<String>,
) -> CurlExport {
    let mut e = Export {
        variables,
        hex,
        names: vec![],
        streams: vec![],
    };
    let mut query: Vec<_> = d
        .query
        .iter()
        .filter(|f| f.enabled)
        .map(|f| (e.template(&f.name), e.expand(&f.value)))
        .collect();
    let auth = authentication_arguments(&mut e, &d.authentication, &mut query);
    let mut args = vec!["curl".into(), "--request".into(), shell_quote(&d.method)];
    let address = e.template(&d.url);
    let plain: Option<Vec<_>> = query
        .iter()
        .map(|(n, v)| Some((n.plain()?, v.plain()?)))
        .collect();
    if query.is_empty() {
        args.push(e.word(&address));
    } else if let (Some(base), Some(query)) = (address.plain(), plain) {
        let mut d = d.clone();
        d.url = base;
        d.query = query
            .into_iter()
            .map(|(name, value)| DomainField {
                id: String::new(),
                name,
                value: DomainValueSource::literal(value),
                enabled: true,
                sensitive: false,
            })
            .collect();
        args.push(shell_quote(&display_url(&d)));
    } else {
        args.push(e.word(&address));
        for (n, v) in query {
            let n = n.plain().map_or_else(
                || n.clone(),
                |s| Text::literal(encode_query(&s).replace('+', "%2B")),
            );
            args.extend([
                "--url-query".into(),
                e.word(&n.join(Text::literal("=")).join(v)),
            ]);
        }
    }
    args.extend(auth);
    transport_arguments(&mut e, &mut args, &d.transport);
    let has_type = header_arguments(&mut e, &mut args, d);
    body_arguments(&mut e, &mut args, &d.body, has_type);
    args.extend(e.streams);
    let mut seen = HashSet::new();
    CurlExport {
        command: args.join(" "),
        secret_names: e
            .names
            .into_iter()
            .filter(|n| seen.insert(n.clone()))
            .collect(),
    }
}
#[must_use]
pub fn proxy_route(proxy: &DomainProxy, url: &str) -> Option<DomainProxyRoute> {
    let DomainProxy::Manual { routes } = proxy else {
        return None;
    };
    let scheme = super::proxy::url_scheme(url.trim())?;
    routes
        .iter()
        .find(|r| r.destination == "all" || r.destination == scheme)
        .cloned()
}
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "The application uses the standard hasher; no caller supplies custom hashing."
)]
pub fn proxy_arguments(proxy: &DomainProxy, url: &str, hex: &HashSet<String>) -> String {
    match proxy {
        DomainProxy::System => String::new(),
        DomainProxy::Direct => " --noproxy '*'".into(),
        DomainProxy::Manual { .. } => {
            let Some(r) = proxy_route(proxy, url) else {
                return " --noproxy '*'".into();
            };
            let mut args = format!(" --proxy {} --noproxy ''", shell_quote(&r.endpoint));
            if let Some(c) = r.credentials {
                let variables = HashMap::new();
                let e = Export {
                    variables: &variables,
                    hex,
                    names: vec![],
                    streams: vec![],
                };
                let _ = write!(
                    args,
                    " --proxy-user {}",
                    e.word(
                        &Text::secret(c.username)
                            .join(Text::literal(":"))
                            .join(Text::secret(c.password))
                    )
                );
            }
            args
        }
    }
}
#[must_use]
pub fn value_sources(d: &DomainRequestDraft) -> Vec<DomainValueSource> {
    fn fields(sources: &mut Vec<DomainValueSource>, fields: &[DomainField]) {
        for f in fields.iter().filter(|f| f.enabled) {
            sources.push(DomainValueSource::literal(&f.name));
            sources.push(f.value.clone());
        }
    }
    let mut sources = vec![DomainValueSource::literal(&d.url)];
    fields(&mut sources, &d.query);
    fields(&mut sources, &d.headers);
    match &d.authentication {
        DomainAuthentication::None => {}
        DomainAuthentication::Basic { username, password } => {
            sources.extend([username.clone(), password.clone()]);
        }
        DomainAuthentication::Bearer { token } => sources.push(token.clone()),
        DomainAuthentication::ApiKey { name, value, .. } => {
            sources.extend([DomainValueSource::literal(name), value.clone()]);
        }
        DomainAuthentication::Oauth2 { configuration } => sources.push(DomainValueSource::Secret {
            secret: configuration.access_token_reference.clone(),
        }),
    }
    match &d.body {
        DomainBody::Empty => {}
        DomainBody::Json { value } | DomainBody::Xml { value } | DomainBody::Html { value } => {
            sources.push(DomainValueSource::literal(value));
        }
        DomainBody::Text {
            content_type,
            value,
        }
        | DomainBody::Raw {
            content_type,
            value,
        } => {
            if let Some(m) = content_type {
                sources.push(DomainValueSource::literal(m));
            }
            sources.push(DomainValueSource::literal(value));
        }
        DomainBody::File { content_type, .. } => {
            if let Some(m) = content_type {
                sources.push(DomainValueSource::literal(m));
            }
        }
        DomainBody::FormUrlEncoded { fields: f } => fields(&mut sources, f),
        DomainBody::Multipart { parts } => {
            for p in parts.iter().filter(|p| p.enabled) {
                sources.push(DomainValueSource::literal(&p.name));
                if p.kind != DomainMultipartKind::File {
                    sources.push(p.value.clone());
                }
            }
        }
    }
    if let Some(identity) = &d.transport.client_certificate_reference {
        sources.push(DomainValueSource::Secret {
            secret: identity.clone(),
        });
    }
    sources
}

fn authentication_arguments(
    e: &mut Export<'_>,
    authentication: &DomainAuthentication,
    query: &mut Vec<(Text, Text)>,
) -> Vec<String> {
    let mut auth = vec![];
    match authentication {
        DomainAuthentication::None => {}
        DomainAuthentication::Basic { username, password } => {
            let t = e
                .expand(username)
                .join(Text::literal(":"))
                .join(e.expand(password));
            auth.extend(["--user".into(), e.word(&t)]);
        }
        DomainAuthentication::Bearer { token } => {
            let t = Text::literal("Authorization: Bearer ").join(e.expand(token));
            auth.extend(["--header".into(), e.word(&t)]);
        }
        DomainAuthentication::ApiKey {
            placement,
            name,
            value,
        } => {
            let (n, v) = (e.template(name), e.expand(value));
            if *placement == DomainApiKeyPlacement::Query {
                query.push((n, v));
            } else {
                auth.extend([
                    "--header".into(),
                    e.word(&n.join(Text::literal(": ")).join(v)),
                ]);
            }
        }
        DomainAuthentication::Oauth2 { configuration } => {
            let t = Text::literal("Authorization: Bearer ").join(e.expand(
                &DomainValueSource::Secret {
                    secret: configuration.access_token_reference.clone(),
                },
            ));
            auth.extend(["--header".into(), e.word(&t)]);
        }
    }
    auth
}

fn transport_arguments(e: &mut Export<'_>, args: &mut Vec<String>, t: &DomainTransport) {
    if t.follow_redirects {
        args.extend([
            "--location".into(),
            "--max-redirs".into(),
            t.maximum_redirects.to_string(),
        ]);
    }
    if !t.validate_tls {
        args.push("--insecure".into());
    }
    if t.total_timeout_ms != 30_000 {
        let time = super::request::milliseconds_as_seconds(t.total_timeout_ms);
        let mut value = time.to_string();
        if !value.contains('.') {
            value.push_str(".0");
        }
        args.extend(["--max-time".into(), value]);
    }
    if let Some(path) = &t.custom_ca_path {
        args.extend(["--cacert".into(), shell_quote(path)]);
    }
    if let Some(identity) = &t.client_certificate_reference {
        e.names.push(identity.clone());
        let command = e.keychain_read(identity);
        args.extend([
            "--cert".into(),
            e.stream(&command),
            "--key".into(),
            e.stream(&command),
        ]);
    }
}

fn header_arguments(e: &mut Export<'_>, args: &mut Vec<String>, d: &DomainRequestDraft) -> bool {
    let headers: Vec<_> = d
        .headers
        .iter()
        .filter(|f| f.enabled)
        .map(|f| (e.template(&f.name), e.expand(&f.value)))
        .collect();
    let is_type = |n: &Text| {
        n.plain()
            .is_some_and(|n| n.eq_ignore_ascii_case("Content-Type"))
    };
    let has_type = headers.iter().any(|(n, _)| is_type(n));
    for (n, v) in headers {
        if matches!(d.body, DomainBody::Multipart { .. }) && is_type(&n) {
            continue;
        }
        args.extend([
            "--header".into(),
            e.word(&n.join(Text::literal(": ")).join(v)),
        ]);
    }
    has_type
}

fn body_arguments(e: &mut Export<'_>, args: &mut Vec<String>, body: &DomainBody, has_type: bool) {
    let content_type = |args: &mut Vec<String>, value: Option<Text>, e: &Export<'_>| {
        if !has_type && let Some(value) = value {
            args.extend([
                "--header".into(),
                e.word(&Text::literal("Content-Type: ").join(value)),
            ]);
        }
    };
    match body {
        DomainBody::Empty => {}
        DomainBody::Json { value } | DomainBody::Xml { value } | DomainBody::Html { value } => {
            let mime = match body {
                DomainBody::Json { .. } => "application/json",
                DomainBody::Xml { .. } => "application/xml",
                _ => "text/html",
            };
            content_type(args, Some(Text::literal(mime)), e);
            let text = e.template(value);
            args.extend(["--data-raw".into(), e.word(&text)]);
        }
        DomainBody::Text {
            content_type: mime,
            value,
        }
        | DomainBody::Raw {
            content_type: mime,
            value,
        } => {
            let mime = mime.as_ref().map(|m| e.template(m)).unwrap_or_default();
            content_type(args, Some(mime), e);
            let text = e.template(value);
            args.extend(["--data-raw".into(), e.word(&text)]);
        }
        DomainBody::File {
            path,
            content_type: mime,
        } => {
            let mime = mime.as_ref().map(|m| e.template(m));
            content_type(args, mime, e);
            args.extend(["--data-binary".into(), shell_quote(&format!("@{path}"))]);
        }
        DomainBody::FormUrlEncoded { fields } => {
            content_type(
                args,
                Some(Text::literal("application/x-www-form-urlencoded")),
                e,
            );
            for f in fields.iter().filter(|f| f.enabled) {
                let text = e
                    .template(&f.name)
                    .join(Text::literal("="))
                    .join(e.expand(&f.value));
                args.extend(["--data-urlencode".into(), e.word(&text)]);
            }
        }
        DomainBody::Multipart { parts } => multipart_arguments(e, args, parts),
    }
}

fn multipart_arguments(e: &mut Export<'_>, args: &mut Vec<String>, parts: &[DomainMultipartPart]) {
    for p in parts.iter().filter(|p| p.enabled) {
        let value = e.expand(&p.value);
        let mut form = match p.kind {
            DomainMultipartKind::Text => {
                if let Some(plain) = value.plain() {
                    Text::literal(form_quote(&plain))
                } else {
                    let command = format!("printf %s {}", e.word(&value));
                    Text::literal(format!("<{}", e.stream(&command)))
                }
            }
            DomainMultipartKind::File => Text::literal(format!(
                "@{}",
                form_quote(p.file_path.as_deref().unwrap_or_default())
            )),
            DomainMultipartKind::Binary => {
                let command = format!("printf %s {} | base64 --decode", e.word(&value));
                Text::literal(format!("@{}", e.stream(&command)))
            }
        };
        if let Some(mime) = p.content_type.as_ref().filter(|s| !s.is_empty()) {
            form = form.join(Text::literal(format!(";type={}", form_quote(mime))));
        }
        if let Some(name) = &p.file_name {
            form = form.join(Text::literal(format!(";filename={}", form_quote(name))));
        }
        let text = e.template(&p.name).join(Text::literal("=")).join(form);
        args.extend(["--form".into(), e.word(&text)]);
    }
}
