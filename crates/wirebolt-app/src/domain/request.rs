//! Draft editing, stable row identity, credential references and transport labels.
use super::models::{
    DomainApiKeyPlacement, DomainAuthentication, DomainBody, DomainEnvironment, DomainField,
    DomainOAuthConfiguration, DomainOAuthGrant, DomainProxy, DomainRequestDraft, DomainTransport,
    DomainValueSource, DomainVariable, DomainWorkspace,
};
use percent_encoding::percent_decode_str;
use std::fmt::Write as _;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

#[must_use]
/// # Panics
/// Panics if the operating system cannot provide random bytes.
pub fn unique_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("operating system randomness");
    b[6] = (b[6] & 15) | 64;
    b[8] = (b[8] & 63) | 128;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}
#[must_use]
pub fn normalize_method(value: &str) -> Option<String> {
    let s = value.trim().to_uppercase();
    (!s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-'))
    .then_some(s)
}
#[must_use]
pub fn credential_reference(role: &str, scope: &str) -> String {
    let role = sanitized(role);
    let text = format!(
        "{scope}.{}.{}",
        unique_id(),
        if role.is_empty() { "value" } else { &role }
    );
    text.chars().take(128).collect()
}
#[must_use]
pub fn legacy_reference(name: &str) -> bool {
    matches!(
        name,
        "auth.api-key" | "oauth.client-secret" | "oauth.access-token"
    )
}
#[must_use]
pub fn owned_reference(name: &str) -> bool {
    static PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\A(?:(?:auth|env|request)\.[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\.[A-Za-z0-9._-]+|import-[0-9a-f]+-[0-9a-f]+-[0-9a-f]+-request-[0-9]+-[a-z-]+)\z").expect("credential reference pattern")
    });
    PATTERN.is_match(name)
}
#[must_use]
pub fn auth_references(auth: &DomainAuthentication) -> Vec<String> {
    match auth {
        DomainAuthentication::None => vec![],
        DomainAuthentication::Basic { username, password } => [username, password]
            .into_iter()
            .filter_map(|v| v.secret_name().map(str::to_owned))
            .collect(),
        DomainAuthentication::Bearer { token } => {
            token.secret_name().into_iter().map(str::to_owned).collect()
        }
        DomainAuthentication::ApiKey { value, .. } => {
            value.secret_name().into_iter().map(str::to_owned).collect()
        }
        DomainAuthentication::Oauth2 { configuration: c } => {
            [&c.client_secret_reference, &c.access_token_reference]
                .into_iter()
                .filter(|n| !n.is_empty())
                .cloned()
                .collect()
        }
    }
}
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "Credential rename maps come from the typed application bridge."
)]
pub fn rename_auth(
    mut auth: DomainAuthentication,
    names: &HashMap<String, String>,
) -> DomainAuthentication {
    fn rename(v: &mut DomainValueSource, n: &HashMap<String, String>) {
        if let DomainValueSource::Secret { secret } = v
            && let Some(new) = n.get(secret)
        {
            *secret = new.clone();
        }
    }
    match &mut auth {
        DomainAuthentication::None => {}
        DomainAuthentication::Basic { username, password } => {
            rename(username, names);
            rename(password, names);
        }
        DomainAuthentication::Bearer { token } => rename(token, names),
        DomainAuthentication::ApiKey { value, .. } => rename(value, names),
        DomainAuthentication::Oauth2 { configuration: c } => {
            if let Some(new) = names.get(&c.client_secret_reference) {
                c.client_secret_reference = new.clone();
            }
            if let Some(new) = names.get(&c.access_token_reference) {
                c.access_token_reference = new.clone();
            }
        }
    }
    auth
}
#[must_use]
pub fn own_auth(
    auth: DomainAuthentication,
    only_legacy: bool,
) -> (DomainAuthentication, HashMap<String, String>) {
    let mut names = HashMap::new();
    for name in auth_references(&auth) {
        if !only_legacy || legacy_reference(&name) {
            let role = name.rsplit_once('.').map_or("secret", |(_, r)| r);
            names
                .entry(name.clone())
                .or_insert_with(|| credential_reference(role, "auth"));
        }
    }
    (rename_auth(auth, &names), names)
}
#[must_use]
pub fn auth_kind(auth: &DomainAuthentication) -> &'static str {
    match auth {
        DomainAuthentication::None => "none",
        DomainAuthentication::Basic { .. } => "basic",
        DomainAuthentication::Bearer { .. } => "bearer",
        DomainAuthentication::ApiKey { .. } => "apiKey",
        DomainAuthentication::Oauth2 { .. } => "oauth2",
    }
}
#[must_use]
pub fn new_auth(kind: &str) -> DomainAuthentication {
    match kind {
        "basic" => DomainAuthentication::Basic {
            username: DomainValueSource::literal(""),
            password: DomainValueSource::Secret {
                secret: credential_reference("password", "auth"),
            },
        },
        "bearer" => DomainAuthentication::Bearer {
            token: DomainValueSource::Secret {
                secret: credential_reference("token", "auth"),
            },
        },
        "apiKey" => DomainAuthentication::ApiKey {
            placement: DomainApiKeyPlacement::Header,
            name: "X-API-Key".into(),
            value: DomainValueSource::Secret {
                secret: credential_reference("api-key", "auth"),
            },
        },
        "oauth2" => DomainAuthentication::Oauth2 {
            configuration: DomainOAuthConfiguration {
                grant: DomainOAuthGrant::AuthorizationCodePkce,
                authorization_url: String::new(),
                token_url: String::new(),
                client_id: String::new(),
                client_secret_reference: credential_reference("oauth-client-secret", "auth"),
                scopes: String::new(),
                audience: String::new(),
                redirect_uri: "wirebolt://oauth/callback".into(),
                access_token_reference: credential_reference("oauth-access-token", "auth"),
            },
        },
        _ => DomainAuthentication::None,
    }
}
// URLComponents query encoding: query punctuation is legal except the item delimiters.
#[must_use]
pub fn encode_query(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~!$'()*+,/:;?@".contains(&b) {
            out.push(char::from(b));
        } else {
            use std::fmt::Write;
            write!(out, "%{b:02X}").expect("String write");
        }
    }
    out
}
#[must_use]
pub fn display_url(draft: &DomainRequestDraft) -> String {
    let query: Vec<_> = draft
        .query
        .iter()
        .filter(|f| f.enabled)
        .map(|f| {
            format!(
                "{}={}",
                encode_query(&f.name),
                encode_query(f.value.editable())
            )
        })
        .collect();
    if query.is_empty() {
        return draft.url.clone();
    }
    let (prefix, fragment) = draft
        .url
        .split_once('#')
        .map_or((draft.url.as_str(), String::new()), |(p, f)| {
            (p, format!("#{f}"))
        });
    format!(
        "{prefix}{}{}{fragment}",
        if prefix.contains('?') { '&' } else { '?' },
        query
            .join("&")
            .replace("%7B%7B", "{{")
            .replace("%7D%7D", "}}")
    )
}
#[must_use]
pub fn edit_url(mut draft: DomainRequestDraft, text: &str) -> DomainRequestDraft {
    let (prefix, fragment) = text
        .split_once('#')
        .map_or((text, String::new()), |(p, f)| (p, format!("#{f}")));
    let Some((base, query)) = prefix.split_once('?') else {
        draft.url = text.into();
        draft.query.retain(|f| !f.enabled);
        return draft;
    };
    let mut previous: Vec<_> = draft.query.iter().filter(|f| f.enabled).cloned().collect();
    let disabled: Vec<_> = draft.query.into_iter().filter(|f| !f.enabled).collect();
    draft.query = if query.is_empty() {
        vec![]
    } else {
        query
            .split('&')
            .map(|item| {
                let (n, v) = item.split_once('=').unwrap_or((item, ""));
                let name = percent_decode_str(n).decode_utf8_lossy().into_owned();
                let value = percent_decode_str(v).decode_utf8_lossy().into_owned();
                let mut field = previous.iter().position(|f| f.name == name).map_or_else(
                    || DomainField {
                        id: unique_id(),
                        name: name.clone(),
                        value: DomainValueSource::literal(&value),
                        enabled: true,
                        sensitive: false,
                    },
                    |i| previous.remove(i),
                );
                if field.value.editable() != value {
                    field.value = DomainValueSource::literal(value);
                }
                field
            })
            .collect()
    };
    draft.query.extend(disabled);
    draft.url = format!("{base}{fragment}");
    draft
}
#[must_use]
pub fn parse_bulk(source: &str, previous: Vec<DomainField>) -> Vec<DomainField> {
    let mut unused = previous;
    source
        .split('\n')
        .filter(|s| !s.is_empty())
        .filter_map(|line| {
            let (enabled, text) = line
                .strip_prefix('#')
                .map_or((true, line), |s| (false, s.trim_matches([' ', '\t'])));
            let (name, value) = text.split_once(':')?;
            let (name, value) = (
                name.trim_matches([' ', '\t']),
                value.trim_matches([' ', '\t']),
            );
            let i = unused
                .iter()
                .position(|f| f.name == name && f.value.editable() == value)
                .or_else(|| unused.iter().position(|f| f.name == name))
                .or_else(|| {
                    unused.iter().position(|f| {
                        f.value.editable() == value
                            && (f.value.secret_name().is_some() || f.sensitive)
                    })
                });
            let mut f = i.map_or_else(
                || DomainField {
                    id: unique_id(),
                    name: String::new(),
                    value: DomainValueSource::literal(""),
                    enabled: true,
                    sensitive: false,
                },
                |i| unused.remove(i),
            );
            f.name = name.into();
            f.enabled = enabled;
            f.value = if f.value.secret_name().is_some() {
                DomainValueSource::Secret {
                    secret: value.into(),
                }
            } else {
                DomainValueSource::literal(value)
            };
            Some(f)
        })
        .collect()
}
#[must_use]
pub fn request_references(d: &DomainRequestDraft) -> HashSet<String> {
    let mut refs: HashSet<_> = auth_references(&d.authentication).into_iter().collect();
    let mut values: Vec<_> = d.query.iter().chain(&d.headers).map(|f| &f.value).collect();
    match &d.body {
        DomainBody::FormUrlEncoded { fields } => values.extend(fields.iter().map(|f| &f.value)),
        DomainBody::Multipart { parts } => values.extend(parts.iter().map(|p| &p.value)),
        _ => {}
    }
    refs.extend(
        values
            .into_iter()
            .filter_map(|v| v.secret_name().map(str::to_owned)),
    );
    refs.extend(proxy_references(d.proxy.as_ref()));
    refs
}
#[must_use]
pub fn proxy_references(proxy: Option<&DomainProxy>) -> Vec<String> {
    if let Some(DomainProxy::Manual { routes }) = proxy {
        routes
            .iter()
            .filter_map(|r| r.credentials.as_ref())
            .flat_map(|c| [c.username.clone(), c.password.clone()])
            .collect()
    } else {
        vec![]
    }
}
#[must_use]
pub fn workspace_references(w: &DomainWorkspace) -> Vec<String> {
    let mut refs: HashSet<_> = w
        .environments
        .iter()
        .flat_map(|e| {
            e.variables
                .iter()
                .filter_map(|v| v.value.secret_name().map(str::to_owned))
        })
        .collect();
    for c in &w.collections {
        for r in &c.requests {
            refs.extend(request_references(&r.request));
        }
    }
    refs.extend(proxy_references(w.proxy.as_ref()));
    refs.into_iter().collect()
}
#[must_use]
pub fn workspace_name(name: &str) -> String {
    if name.trim().is_empty() {
        "Wirebolt".into()
    } else {
        name.into()
    }
}
#[must_use]
pub fn window_title(workspace: &str, request: Option<&str>) -> String {
    let w = workspace_name(workspace);
    request
        .filter(|r| !r.is_empty())
        .map_or_else(|| w.clone(), |r| format!("{w} — {r}"))
}
#[must_use]
pub fn tab_description(method: &str, ws: bool, dirty: bool, preview: bool) -> String {
    let mut p = vec![if ws { "WebSocket" } else { method }];
    if dirty {
        p.push("Edited");
    }
    if preview {
        p.push("Preview");
    }
    p.join(", ")
}
#[derive(Clone, Debug, PartialEq)]
pub struct TimeoutPresentation {
    pub text: Option<String>,
    pub seconds: Option<f64>,
    pub suffix: String,
}
#[must_use]
pub fn timeout_presentation(ms: u64) -> TimeoutPresentation {
    let (text, seconds, suffix) = if ms == 0 {
        (Some("No limit".into()), None, "")
    } else if ms < 1000 {
        (Some(format!("{ms} ms")), None, "")
    } else if ms.is_multiple_of(60_000) {
        (Some(format!("{} min", ms / 60_000)), None, "")
    } else {
        (None, Some(milliseconds_as_seconds(ms)), " s")
    };
    TimeoutPresentation {
        text,
        seconds,
        suffix: suffix.into(),
    }
}
#[must_use]
pub fn auth_title(kind: &str) -> String {
    match kind {
        "basic" => "Basic",
        "bearer" => "Bearer",
        "apiKey" => "API key",
        "oauth2" => "OAuth 2",
        _ => "No auth",
    }
    .into()
}
#[must_use]
pub fn active_count(fields: &[DomainField]) -> u64 {
    fields
        .iter()
        .filter(|f| f.enabled && !f.name.trim().is_empty())
        .count() as u64
}
#[must_use]
pub fn sanitized(text: &str) -> String {
    use unicode_segmentation::UnicodeSegmentation as _;
    let text: String = text
        .graphemes(true)
        .map(|c| {
            if c.len() == 1
                && c.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            {
                c
            } else {
                "-"
            }
        })
        .collect();
    if text.is_empty() {
        "value".into()
    } else {
        text
    }
}

/// Uses the same floating-point representation as native timeout formatting.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "Timeouts are rendered as floating-point seconds, matching the existing native formatter."
)]
pub fn milliseconds_as_seconds(ms: u64) -> f64 {
    ms as f64 / 1000.0
}
#[must_use]
pub fn import_headline(names: &[String], requests: i64, groups: i64) -> String {
    fn counted(n: i64, noun: &str) -> String {
        format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
    }
    let mut text = format!("Imported {}", counted(requests, "request"));
    if groups > 0 {
        let _ = write!(text, " in {}", counted(groups, "folder"));
    }
    match names {
        [] => text.push('.'),
        [name] => {
            let _ = write!(text, " into “{name}”.");
        }
        _ => {
            let _ = write!(text, " into {} collections.", names.len());
        }
    }
    text
}
#[must_use]
pub fn import_environment_line(names: &[String], localized_list: &str) -> Option<String> {
    match names {
        [] => None,
        [name] => Some(format!(
            "Created the environment “{name}” for its variables."
        )),
        _ => Some(format!(
            "Created {} environments: {localized_list}.",
            names.len()
        )),
    }
}
#[must_use]
pub fn git_change_kind(staged: &str, unstaged: &str, conflicted: bool) -> String {
    let d = [staged, unstaged];
    if conflicted || d.contains(&"unmerged") {
        return "Conflict".into();
    }
    for (deltas, label) in [
        (&["added", "untracked"][..], "New"),
        (&["deleted"][..], "Deleted"),
        (&["renamed"][..], "Renamed"),
        (&["copied"][..], "Copied"),
        (&["type_changed"][..], "Type Changed"),
    ] {
        if d.iter().any(|d| deltas.contains(d)) {
            return label.into();
        }
    }
    "Modified".into()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushAvailability {
    Push,
    Publish { branch: String },
    Unavailable { reason: String },
}
#[must_use]
pub fn push_availability(
    branch: Option<String>,
    upstream: Option<&str>,
    ahead: u64,
) -> PushAvailability {
    let Some(branch) = branch else {
        return PushAvailability::Unavailable {
            reason: "Check out a branch to push. HEAD is detached.".into(),
        };
    };
    if upstream.is_none() {
        return PushAvailability::Publish { branch };
    }
    if ahead == 0 {
        return PushAvailability::Unavailable {
            reason: "No local commits to push.".into(),
        };
    }
    PushAvailability::Push
}
#[must_use]
pub fn separate_url_query(mut draft: DomainRequestDraft) -> DomainRequestDraft {
    let explicit = std::mem::take(&mut draft.query);
    let url = draft.url.clone();
    let mut result = edit_url(draft, &url);
    result.query.extend(explicit);
    result
}
#[must_use]
pub fn legacy_environment(
    id: String,
    name: String,
    values: std::collections::BTreeMap<String, DomainValueSource>,
) -> DomainEnvironment {
    DomainEnvironment {
        id,
        name,
        variables: values
            .into_iter()
            .zip(0_i64..)
            .map(|((key, value), order)| DomainVariable {
                id: format!("variable-{order}"),
                key,
                value,
                enabled: true,
                order,
            })
            .collect(),
    }
}
#[must_use]
pub fn switch_auth(auth: DomainAuthentication, kind: &str) -> DomainAuthentication {
    if auth_kind(&auth) == kind {
        auth
    } else {
        new_auth(kind)
    }
}
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "The run record stores the application standard-hasher map."
)]
pub fn run_input(
    draft: DomainRequestDraft,
    variables: HashMap<String, DomainValueSource>,
    workspace_proxy: Option<DomainProxy>,
    app_proxy: Option<DomainProxy>,
) -> super::models::DomainRunInput {
    super::models::DomainRunInput {
        method: draft.method,
        url: draft.url,
        query: draft.query,
        headers: draft.headers,
        authentication: draft.authentication,
        body: draft.body,
        variables,
        app_proxy,
        workspace_proxy,
        request_proxy: draft.proxy,
        total_timeout_ms: draft.transport.total_timeout_ms,
        read_timeout_ms: draft.transport.read_timeout_ms,
        max_response_bytes: None,
        validate_tls: draft.transport.validate_tls,
        follow_redirects: draft.transport.follow_redirects,
        maximum_redirects: u8::try_from(draft.transport.maximum_redirects.clamp(0, 255))
            .unwrap_or_default(),
        client_certificate_reference: draft.transport.client_certificate_reference,
        custom_ca_path: draft.transport.custom_ca_path,
    }
}
#[must_use]
pub const fn clamp_redirects(value: i64) -> i64 {
    if value < 0 {
        0
    } else if value > 10 {
        10
    } else {
        value
    }
}
/// The complete result of one URL edit, independent of request bodies and credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlQueryEdit {
    pub url: String,
    pub query: Vec<DomainField>,
    pub display_url: String,
}
#[must_use]
pub fn edit_url_query(url: String, query: Vec<DomainField>, text: &str) -> UrlQueryEdit {
    let draft = edit_url(
        DomainRequestDraft {
            url,
            query,
            ..DomainRequestDraft::default()
        },
        text,
    );
    UrlQueryEdit {
        display_url: display_url(&draft),
        url: draft.url,
        query: draft.query,
    }
}
#[must_use]
pub fn display_url_query(url: String, query: Vec<DomainField>) -> String {
    display_url(&DomainRequestDraft {
        url,
        query,
        ..DomainRequestDraft::default()
    })
}
#[must_use]
pub fn separate_url_query_values(url: String, query: Vec<DomainField>) -> UrlQueryEdit {
    let draft = separate_url_query(DomainRequestDraft {
        url,
        query,
        ..DomainRequestDraft::default()
    });
    UrlQueryEdit {
        display_url: display_url(&draft),
        url: draft.url,
        query: draft.query,
    }
}
#[must_use]
pub fn effective_transport(
    request: DomainTransport,
    workspace: DomainTransport,
    inherits: bool,
) -> DomainTransport {
    if inherits { workspace } else { request }
}
