//! Request fields, authentication and credential ownership, environment migration and run inputs.
use super::{
    identity::unique_id,
    models::{
        ApiKeyPlacement, DomainAuthentication, DomainAuthenticationKind, DomainBody,
        DomainEnvironment, DomainField, DomainOAuthConfiguration, DomainProxy, DomainRequestDraft,
        DomainTransport, DomainValueSource, DomainVariable, DomainWorkspace, Oauth2Grant,
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

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
/// Chooses a writable per-request name, copying legacy shared credentials on edit.
#[must_use]
pub fn editable_credential_reference(source: &DomainValueSource, role: &str) -> String {
    match source {
        DomainValueSource::Secret { secret } if !secret.is_empty() && !legacy_reference(secret) => {
            secret.clone()
        }
        _ => credential_reference(role, "auth"),
    }
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
pub fn auth_kind(auth: &DomainAuthentication) -> DomainAuthenticationKind {
    match auth {
        DomainAuthentication::None => DomainAuthenticationKind::None,
        DomainAuthentication::Basic { .. } => DomainAuthenticationKind::Basic,
        DomainAuthentication::Bearer { .. } => DomainAuthenticationKind::Bearer,
        DomainAuthentication::ApiKey { .. } => DomainAuthenticationKind::ApiKey,
        DomainAuthentication::Oauth2 { .. } => DomainAuthenticationKind::Oauth2,
    }
}
#[must_use]
pub fn new_auth(kind: DomainAuthenticationKind) -> DomainAuthentication {
    match kind {
        DomainAuthenticationKind::Basic => DomainAuthentication::Basic {
            username: DomainValueSource::literal(""),
            password: DomainValueSource::Secret {
                secret: credential_reference("password", "auth"),
            },
        },
        DomainAuthenticationKind::Bearer => DomainAuthentication::Bearer {
            token: DomainValueSource::Secret {
                secret: credential_reference("token", "auth"),
            },
        },
        DomainAuthenticationKind::ApiKey => DomainAuthentication::ApiKey {
            placement: ApiKeyPlacement::Header,
            name: "X-API-Key".into(),
            value: DomainValueSource::Secret {
                secret: credential_reference("api-key", "auth"),
            },
        },
        DomainAuthenticationKind::Oauth2 => DomainAuthentication::Oauth2 {
            configuration: DomainOAuthConfiguration {
                grant: Oauth2Grant::AuthorizationCodePkce,
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
        DomainAuthenticationKind::None => DomainAuthentication::None,
    }
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
pub fn auth_title(kind: DomainAuthenticationKind) -> String {
    match kind {
        DomainAuthenticationKind::Basic => "Basic",
        DomainAuthenticationKind::Bearer => "Bearer",
        DomainAuthenticationKind::ApiKey => "API key",
        DomainAuthenticationKind::Oauth2 => "OAuth 2",
        DomainAuthenticationKind::None => "No auth",
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
pub fn switch_auth(
    auth: DomainAuthentication,
    kind: DomainAuthenticationKind,
) -> DomainAuthentication {
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
pub fn effective_transport(
    request: DomainTransport,
    workspace: DomainTransport,
    inherits: bool,
) -> DomainTransport {
    if inherits { workspace } else { request }
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
