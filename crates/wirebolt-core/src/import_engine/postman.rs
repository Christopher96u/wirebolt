//! Postman Collection v2.0 and v2.1.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{
    ImportError, ImportedCollection, ImportedEnvironment, ImportedGroup, ImportedRequest,
    ImportedRequestSettings, ImportedWorkspace, ParsedImport, SecretCollector,
    environment_variable, listed_warning, percent_decoded, slug,
};
use crate::{
    ApiKeyPlacement, MultipartPart, MultipartPartKind, Oauth2Configuration, Oauth2Grant,
    RequestAuthentication, RequestBody, RequestHeader, RequestValueField, SecretName,
    TransportSettings, ValueSource,
};

/// The redirect URI Wirebolt's OAuth 2.0 authorization flow listens on.
const OAUTH_REDIRECT_URI: &str = "wirebolt://oauth/callback";

pub(super) fn parse(source: &str) -> Result<ParsedImport, ImportError> {
    let root: Value = serde_json::from_str(source)
        .map_err(|_| ImportError::new("The Postman collection isn't valid JSON."))?;
    let Some(items) = root.get("item").and_then(Value::as_array) else {
        if root.get("requests").is_some() && root.get("order").is_some() {
            return Err(ImportError::new(
                "Postman Collection v1 isn't supported. Export the collection from Postman as Collection v2.1 and import it again.",
            ));
        }
        return Err(ImportError::new(
            "The document isn't a Postman collection: it has no items.",
        ));
    };
    let name = root
        .pointer("/info/name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Imported Postman")
        .to_owned();
    let mut importer = Importer::new(name.clone());
    if description(root.pointer("/info/description")).is_some() {
        importer.findings.descriptions.push(name.clone());
    }
    importer.variables(root.get("variable"));
    importer.scripts(root.get("event"), &name);
    let transport = behavior_transport(root.get("protocolProfileBehavior"), None);
    let auth = effective_auth(root.get("auth"), None);
    importer.items(items, None, auth, transport.as_ref(), &[]);
    if importer.collection.requests.is_empty() {
        return Err(ImportError::new("The Postman collection has no requests."));
    }
    Ok(importer.finish())
}

/// Things that could not be imported exactly, reported once per kind.
#[derive(Default)]
struct Findings {
    scripts: Vec<String>,
    examples: Vec<String>,
    descriptions: Vec<String>,
    unsupported_auth: BTreeMap<String, Vec<String>>,
    pkce: Vec<String>,
    oauth_secret_variables: Vec<String>,
    disabled_bodies: Vec<String>,
    unsupported_bodies: Vec<String>,
    missing_files: Vec<String>,
    empty_path_variables: Vec<String>,
    overridden_variables: Vec<String>,
    skipped: Vec<String>,
}

/// A collection variable before it becomes an environment row.
struct Variable {
    key: String,
    value: String,
    enabled: bool,
    secret: bool,
}

struct Importer {
    collection: ImportedCollection,
    variables: Vec<Variable>,
    request_settings: BTreeMap<String, ImportedRequestSettings>,
    secrets: SecretCollector,
    findings: Findings,
}

impl Importer {
    fn new(name: String) -> Self {
        Self {
            collection: ImportedCollection {
                name,
                ..ImportedCollection::default()
            },
            variables: Vec::new(),
            request_settings: BTreeMap::new(),
            secrets: SecretCollector::default(),
            findings: Findings::default(),
        }
    }

    fn items<'a>(
        &mut self,
        items: &'a [Value],
        parent: Option<&str>,
        auth: Option<&'a Value>,
        transport: Option<&TransportSettings>,
        path: &[String],
    ) {
        for (index, item) in items.iter().enumerate() {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("Untitled");
            let mut item_path = path.to_vec();
            item_path.push(name.to_owned());
            let label = item_path.join(" / ");
            let order = i64::try_from(index).unwrap_or(i64::MAX);
            self.scripts(item.get("event"), &label);
            if let Some(children) = item.get("item").and_then(Value::as_array) {
                let source_id = format!("group-{}-{}", self.collection.groups.len(), slug(name));
                self.collection.groups.push(ImportedGroup {
                    source_id: source_id.clone(),
                    name: name.to_owned(),
                    parent_source_id: parent.map(str::to_owned),
                    order,
                });
                if description(item.get("description")).is_some() {
                    self.findings.descriptions.push(label.clone());
                }
                self.variables(item.get("variable"));
                let folder_transport =
                    behavior_transport(item.get("protocolProfileBehavior"), transport);
                self.items(
                    children,
                    Some(&source_id),
                    effective_auth(item.get("auth"), auth),
                    folder_transport.as_ref().or(transport),
                    &item_path,
                );
                continue;
            }
            match item.get("request") {
                Some(request @ (Value::Object(_) | Value::String(_))) => {
                    let (request, transport) = self.request(item, request, &label, auth, transport);
                    let source_id =
                        format!("request-{}-{}", self.collection.requests.len(), slug(name));
                    if let Some(transport) = transport {
                        self.request_settings.insert(
                            source_id.clone(),
                            ImportedRequestSettings {
                                proxy_override: None,
                                transport,
                                inherits_workspace_transport: false,
                            },
                        );
                    }
                    self.collection.requests.push(ImportedRequest {
                        source_id,
                        name: name.to_owned(),
                        group_source_id: parent.map(str::to_owned),
                        order,
                        ..request
                    });
                }
                _ => self.findings.skipped.push(label),
            }
        }
    }

    fn request(
        &mut self,
        item: &Value,
        request: &Value,
        label: &str,
        inherited_auth: Option<&Value>,
        inherited_transport: Option<&TransportSettings>,
    ) -> (ImportedRequest, Option<TransportSettings>) {
        if let Some(count) = item
            .get("response")
            .and_then(Value::as_array)
            .map(Vec::len)
            .filter(|count| *count > 0)
        {
            self.findings.examples.push(format!("{label} ({count})"));
        }
        // Postman v2.0 allows a request to be just its URL.
        if let Value::String(url) = request {
            let (url, query) = split_query(url);
            return (
                ImportedRequest {
                    url,
                    query,
                    authentication: self.authentication(inherited_auth, label),
                    ..ImportedRequest::default()
                },
                inherited_transport.cloned(),
            );
        }
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .filter(|method| !method.trim().is_empty())
            .unwrap_or("GET")
            .to_ascii_uppercase();
        let (url, query) = self.url(request.get("url"), label);
        let headers = headers(request.get("header"));
        let body = self.body(request.get("body"), &headers, label);
        let authentication =
            self.authentication(effective_auth(request.get("auth"), inherited_auth), label);
        let transport =
            behavior_transport(item.get("protocolProfileBehavior"), inherited_transport)
                .or_else(|| inherited_transport.cloned());
        let request = ImportedRequest {
            method,
            url,
            headers,
            query,
            authentication,
            body,
            note: description(request.get("description"))
                .or_else(|| description(item.get("description")))
                .unwrap_or_default(),
            ..ImportedRequest::default()
        };
        (request, transport)
    }

    fn url(&mut self, value: Option<&Value>, label: &str) -> (String, Vec<RequestValueField>) {
        let object = match value {
            Some(Value::String(url)) => return split_query(url),
            Some(Value::Object(object)) => object,
            _ => return (String::new(), Vec::new()),
        };
        let raw = object
            .get("raw")
            .and_then(Value::as_str)
            .map_or_else(|| url_from_parts(object), str::to_owned);
        let (mut url, mut query) = split_query(&raw);
        if let Some(rows) = object.get("query").and_then(Value::as_array) {
            query = rows
                .iter()
                .filter_map(|row| {
                    let key = scalar(row.get("key")).unwrap_or_default();
                    let value = scalar(row.get("value")).unwrap_or_default();
                    if key.is_empty() && value.is_empty() {
                        return None;
                    }
                    let mut field = RequestValueField::enabled(
                        percent_decoded(&key),
                        ValueSource::literal(percent_decoded(&value)),
                    );
                    field.enabled = !disabled(row);
                    Some(field)
                })
                .collect();
        }
        for variable in object
            .get("variable")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(key) = scalar(variable.get("key")).filter(|key| !key.is_empty()) else {
                continue;
            };
            let value = scalar(variable.get("value")).unwrap_or_default();
            let replacement = if value.is_empty() {
                self.findings
                    .empty_path_variables
                    .push(format!("{label} (:{key})"));
                format!("{{{{{key}}}}}")
            } else {
                value
            };
            url = substitute_path_variable(&url, &key, &replacement);
        }
        (url, query)
    }

    fn body(
        &mut self,
        body: Option<&Value>,
        headers: &[RequestHeader],
        label: &str,
    ) -> RequestBody {
        let Some(body) = body.filter(|body| body.is_object()) else {
            return RequestBody::Empty;
        };
        let mode = body.get("mode").and_then(Value::as_str).unwrap_or("none");
        if disabled(body) && mode != "none" {
            self.findings.disabled_bodies.push(label.to_owned());
            return RequestBody::Empty;
        }
        match mode {
            "none" => RequestBody::Empty,
            "raw" => raw_body(body, headers),
            "urlencoded" => RequestBody::FormUrlEncoded {
                fields: body
                    .get("urlencoded")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|field| {
                        let key = scalar(field.get("key")).filter(|key| !key.is_empty())?;
                        let mut row = RequestValueField::enabled(
                            key,
                            ValueSource::literal(scalar(field.get("value")).unwrap_or_default()),
                        );
                        row.enabled = !disabled(field);
                        Some(row)
                    })
                    .collect(),
            },
            "formdata" => self.multipart(body, label),
            "file" => {
                if let Some(path) = body
                    .pointer("/file/src")
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty())
                {
                    RequestBody::File {
                        path: path.to_owned(),
                        content_type: None,
                    }
                } else {
                    self.findings.missing_files.push(label.to_owned());
                    RequestBody::Empty
                }
            }
            "graphql" => graphql_body(body.get("graphql")),
            other => {
                self.findings
                    .unsupported_bodies
                    .push(format!("{label} ({other})"));
                RequestBody::Empty
            }
        }
    }

    fn multipart(&mut self, body: &Value, label: &str) -> RequestBody {
        let mut parts = Vec::new();
        for (index, part) in body
            .get("formdata")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let Some(name) = scalar(part.get("key")).filter(|key| !key.is_empty()) else {
                continue;
            };
            let is_file = part.get("type").and_then(Value::as_str) == Some("file");
            let file_path = match part.get("src") {
                Some(Value::String(path)) if !path.is_empty() => Some(path.clone()),
                Some(Value::Array(paths)) => {
                    paths.first().and_then(Value::as_str).map(str::to_owned)
                }
                _ => None,
            };
            if is_file && file_path.is_none() {
                self.findings
                    .missing_files
                    .push(format!("{label} ({name})"));
            }
            parts.push(MultipartPart {
                id: format!("part-{index}"),
                name,
                kind: if is_file {
                    MultipartPartKind::File
                } else {
                    MultipartPartKind::Text
                },
                value: ValueSource::literal(if is_file {
                    String::new()
                } else {
                    scalar(part.get("value")).unwrap_or_default()
                }),
                file_name: part
                    .get("fileName")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                file_path: if is_file { file_path } else { None },
                content_type: part
                    .get("contentType")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                enabled: !disabled(part),
            });
        }
        RequestBody::Multipart { parts }
    }

    fn authentication(&mut self, auth: Option<&Value>, label: &str) -> RequestAuthentication {
        let Some(auth) = auth else {
            return RequestAuthentication::None;
        };
        let kind = auth.get("type").and_then(Value::as_str).unwrap_or("noauth");
        let parameter = |key: &str| auth_parameter(auth, kind, key).unwrap_or_default();
        match kind {
            "noauth" => RequestAuthentication::None,
            "basic" => RequestAuthentication::Basic {
                username: ValueSource::literal(parameter("username")),
                password: ValueSource::literal(parameter("password")),
            },
            "bearer" => RequestAuthentication::Bearer {
                token: ValueSource::literal(parameter("token")),
            },
            "apikey" => RequestAuthentication::ApiKey {
                placement: if parameter("in") == "query" {
                    ApiKeyPlacement::Query
                } else {
                    ApiKeyPlacement::Header
                },
                name: Some(parameter("key"))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "X-API-Key".to_owned()),
                value: ValueSource::literal(parameter("value")),
            },
            "oauth2" => self.oauth2(auth, label),
            other => {
                self.findings
                    .unsupported_auth
                    .entry(auth_display_name(other))
                    .or_default()
                    .push(label.to_owned());
                RequestAuthentication::None
            }
        }
    }

    fn oauth2(&mut self, auth: &Value, label: &str) -> RequestAuthentication {
        let parameter = |key: &str| auth_parameter(auth, "oauth2", key).unwrap_or_default();
        let grant_type = Some(parameter("grant_type"))
            .filter(|grant| !grant.is_empty())
            .unwrap_or_else(|| "authorization_code".to_owned());
        let grant = match grant_type.as_str() {
            "client_credentials" => Oauth2Grant::ClientCredentials,
            "authorization_code_with_pkce" => Oauth2Grant::AuthorizationCodePkce,
            "authorization_code" => {
                self.findings.pkce.push(label.to_owned());
                Oauth2Grant::AuthorizationCodePkce
            }
            other => {
                self.findings
                    .unsupported_auth
                    .entry(format!("OAuth 2.0 {} grant", other.replace('_', " ")))
                    .or_default()
                    .push(label.to_owned());
                return RequestAuthentication::None;
            }
        };
        // The bridge gives both references unique per-request Keychain names.
        let (Ok(placeholder_secret), Ok(access_token_reference)) = (
            SecretName::new("oauth2-client-secret"),
            SecretName::new("oauth2-access-token"),
        ) else {
            return RequestAuthentication::None;
        };
        let client_secret = parameter("clientSecret");
        let client_secret_reference = if client_secret.contains("{{") {
            self.findings.oauth_secret_variables.push(label.to_owned());
            None
        } else if client_secret.is_empty() {
            None
        } else {
            self.secrets.add("oauth2-client-secret", client_secret)
        }
        .unwrap_or(placeholder_secret);
        let configuration = Oauth2Configuration {
            grant,
            authorization_url: parameter("authUrl"),
            token_url: parameter("accessTokenUrl"),
            client_id: parameter("clientId"),
            client_secret_reference,
            scopes: parameter("scope"),
            audience: parameter("audience"),
            redirect_uri: OAUTH_REDIRECT_URI.to_owned(),
            access_token_reference,
        };
        RequestAuthentication::Oauth2 { configuration }
    }

    fn variables(&mut self, variables: Option<&Value>) {
        for variable in variables.and_then(Value::as_array).into_iter().flatten() {
            let Some(key) = scalar(variable.get("key"))
                .or_else(|| scalar(variable.get("id")))
                .filter(|key| !key.is_empty())
            else {
                continue;
            };
            let value = scalar(variable.get("value")).unwrap_or_default();
            if let Some(existing) = self.variables.iter().find(|existing| existing.key == key) {
                if existing.value != value {
                    self.findings.overridden_variables.push(key);
                }
                continue;
            }
            self.variables.push(Variable {
                key,
                value,
                enabled: !disabled(variable),
                secret: variable.get("type").and_then(Value::as_str) == Some("secret"),
            });
        }
    }

    fn scripts(&mut self, events: Option<&Value>, label: &str) {
        for event in events.and_then(Value::as_array).into_iter().flatten() {
            let has_code = match event.pointer("/script/exec") {
                Some(Value::Array(lines)) => lines
                    .iter()
                    .any(|line| line.as_str().is_some_and(|line| !line.trim().is_empty())),
                Some(Value::String(code)) => !code.trim().is_empty(),
                _ => false,
            };
            if !has_code {
                continue;
            }
            let kind = match event.get("listen").and_then(Value::as_str) {
                Some("prerequest") => "pre-request",
                Some("test") => "test",
                _ => "script",
            };
            self.findings.scripts.push(format!("{label} ({kind})"));
        }
    }

    fn finish(mut self) -> ParsedImport {
        let mut environments = Vec::new();
        if !self.variables.is_empty() {
            let secrets = &mut self.secrets;
            environments.push(ImportedEnvironment {
                name: self.collection.name.clone(),
                global: false,
                variables: std::mem::take(&mut self.variables)
                    .into_iter()
                    .enumerate()
                    .map(|(order, variable)| {
                        environment_variable(
                            order,
                            variable.key,
                            variable.value,
                            variable.enabled,
                            variable.secret,
                            secrets,
                        )
                    })
                    .collect(),
            });
        }
        let findings = self.findings;
        let warnings = &mut self.collection.warnings;
        let mut report = |message: &str, items: &[String]| {
            if !items.is_empty() {
                warnings.push(listed_warning(message, items));
            }
        };
        for (kind, requests) in &findings.unsupported_auth {
            report(
                &format!(
                    "{kind} authentication isn't supported, so these requests have no authentication"
                ),
                requests,
            );
        }
        report(
            "OAuth 2.0 authorization code now uses PKCE and the redirect URI wirebolt://oauth/callback; register it with the provider",
            &findings.pkce,
        );
        report(
            "OAuth 2.0 client secrets that reference variables weren't imported; enter them in the Auth tab",
            &findings.oauth_secret_variables,
        );
        report(
            "Unsupported body types were imported as empty bodies",
            &findings.unsupported_bodies,
        );
        report(
            "Disabled bodies weren't imported because they aren't sent",
            &findings.disabled_bodies,
        );
        report(
            "File uploads without a file path need a file before sending",
            &findings.missing_files,
        );
        report(
            "Path variables without a value became {{variable}} references",
            &findings.empty_path_variables,
        );
        report(
            "Folder variables that redefine a variable kept the first value",
            &findings.overridden_variables,
        );
        report(
            "Scripts aren't supported and weren't imported",
            &findings.scripts,
        );
        report(
            "Saved example responses weren't imported",
            &findings.examples,
        );
        report(
            "Collection and folder descriptions weren't imported; request descriptions are in Notes",
            &findings.descriptions,
        );
        report("Items without a request were skipped", &findings.skipped);
        ParsedImport {
            workspace: ImportedWorkspace {
                collections: vec![self.collection],
                environments,
                request_settings: self.request_settings,
            },
            secrets: self.secrets.secrets,
        }
    }
}

/// The auth that applies at this level: its own unless it inherits.
fn effective_auth<'a>(auth: Option<&'a Value>, inherited: Option<&'a Value>) -> Option<&'a Value> {
    match auth {
        Some(auth)
            if auth.is_object() && auth.get("type").and_then(Value::as_str) != Some("inherit") =>
        {
            Some(auth)
        }
        _ => inherited,
    }
}

/// Reads one auth parameter from v2.1 `[{key, value}]` or v2.0 `{key: value}`.
fn auth_parameter(auth: &Value, kind: &str, key: &str) -> Option<String> {
    match auth.get(kind)? {
        Value::Array(parameters) => parameters
            .iter()
            .find(|parameter| parameter.get("key").and_then(Value::as_str) == Some(key))
            .and_then(|parameter| scalar(parameter.get("value"))),
        Value::Object(parameters) => scalar(parameters.get(key)),
        _ => None,
    }
}

fn auth_display_name(kind: &str) -> String {
    match kind {
        "awsv4" => "AWS Signature".to_owned(),
        "digest" => "Digest".to_owned(),
        "hawk" => "Hawk".to_owned(),
        "ntlm" => "NTLM".to_owned(),
        "oauth1" => "OAuth 1.0".to_owned(),
        "edgegrid" => "Akamai EdgeGrid".to_owned(),
        "jwt" => "JWT Bearer".to_owned(),
        "asap" => "ASAP".to_owned(),
        other => other.to_owned(),
    }
}

/// Postman `protocolProfileBehavior` settings Wirebolt can honour per request.
fn behavior_transport(
    behavior: Option<&Value>,
    inherited: Option<&TransportSettings>,
) -> Option<TransportSettings> {
    let behavior = behavior?;
    let strict_ssl = behavior.get("strictSSL").and_then(Value::as_bool);
    let follow_redirects = behavior.get("followRedirects").and_then(Value::as_bool);
    let maximum_redirects = behavior
        .get("maxRedirects")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok());
    if strict_ssl.is_none() && follow_redirects.is_none() && maximum_redirects.is_none() {
        return None;
    }
    let mut settings = inherited.cloned().unwrap_or_default();
    if let Some(strict_ssl) = strict_ssl {
        settings.validate_tls = strict_ssl;
    }
    if let Some(follow_redirects) = follow_redirects {
        settings.follow_redirects = follow_redirects;
    }
    if let Some(maximum_redirects) = maximum_redirects {
        settings.maximum_redirects = maximum_redirects;
    }
    Some(settings)
}

fn headers(value: Option<&Value>) -> Vec<RequestHeader> {
    match value {
        Some(Value::Array(headers)) => headers
            .iter()
            .filter_map(|header| {
                let name = scalar(header.get("key")).filter(|name| !name.trim().is_empty())?;
                let mut row = RequestHeader::enabled(
                    name.trim(),
                    ValueSource::literal(scalar(header.get("value")).unwrap_or_default()),
                );
                row.enabled = !disabled(header);
                Some(row)
            })
            .collect(),
        // Postman v2.0 also allows a raw `Name: value` header block.
        Some(Value::String(block)) => block
            .lines()
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                (!name.trim().is_empty()).then(|| {
                    RequestHeader::enabled(name.trim(), ValueSource::literal(value.trim()))
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn raw_body(body: &Value, headers: &[RequestHeader]) -> RequestBody {
    let value = body
        .get("raw")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if value.is_empty() {
        return RequestBody::Empty;
    }
    let language = body
        .pointer("/options/raw/language")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    let content_type = headers
        .iter()
        .find(|header| header.enabled && header.name.eq_ignore_ascii_case("content-type"))
        .and_then(|header| match &header.value {
            ValueSource::Literal(value) => Some(value.to_ascii_lowercase()),
            ValueSource::Secret { .. } => None,
        });
    let kind = language.as_deref().or_else(|| {
        let content_type = content_type.as_deref()?;
        ["json", "xml", "html"]
            .into_iter()
            .find(|kind| content_type.contains(kind))
    });
    match kind {
        Some("json") => RequestBody::Json { value },
        Some("xml") => RequestBody::Xml { value },
        Some("html") => RequestBody::Html { value },
        Some("javascript") => RequestBody::Text {
            content_type: content_type
                .is_none()
                .then(|| "application/javascript".to_owned()),
            value,
        },
        _ if looks_like_json(&value) => RequestBody::Json { value },
        _ => RequestBody::Text {
            content_type: None,
            value,
        },
    }
}

fn looks_like_json(value: &str) -> bool {
    let trimmed = value.trim_start();
    (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<serde::de::IgnoredAny>(value).is_ok()
}

/// GraphQL bodies are sent as the standard `{query, variables}` JSON.
fn graphql_body(graphql: Option<&Value>) -> RequestBody {
    let query = graphql
        .and_then(|graphql| graphql.get("query"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let variables = match graphql.and_then(|graphql| graphql.get("variables")) {
        Some(Value::String(text)) if !text.trim().is_empty() => {
            // Keep `{{variable}}` placeholders intact even when they are not JSON yet.
            serde_json::from_str::<Value>(text)
                .ok()
                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                .unwrap_or_else(|| text.trim().to_owned())
        }
        Some(value @ Value::Object(_)) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_owned())
        }
        _ => "{}".to_owned(),
    };
    let query = serde_json::to_string(query).unwrap_or_else(|_| "\"\"".to_owned());
    let variables = variables.replace('\n', "\n  ");
    RequestBody::Json {
        value: format!("{{\n  \"query\": {query},\n  \"variables\": {variables}\n}}"),
    }
}

/// Splits `base?query` into the base URL and decoded query rows.
fn split_query(raw: &str) -> (String, Vec<RequestValueField>) {
    let Some((base, query)) = raw.split_once('?') else {
        return (raw.to_owned(), Vec::new());
    };
    let (query, fragment) = query
        .split_once('#')
        .map_or((query, None), |(query, fragment)| (query, Some(fragment)));
    let rows = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            RequestValueField::enabled(
                percent_decoded(name),
                ValueSource::literal(percent_decoded(value)),
            )
        })
        .collect();
    let base = fragment.map_or_else(|| base.to_owned(), |fragment| format!("{base}#{fragment}"));
    (base, rows)
}

fn url_from_parts(object: &serde_json::Map<String, Value>) -> String {
    let join = |value: Option<&Value>, separator: &str| match value {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| scalar(Some(part)).or_else(|| scalar(part.get("value"))))
            .collect::<Vec<_>>()
            .join(separator),
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    };
    let mut url = String::new();
    if let Some(protocol) = object.get("protocol").and_then(Value::as_str) {
        url.push_str(protocol);
        url.push_str("://");
    }
    url.push_str(&join(object.get("host"), "."));
    if let Some(port) = scalar(object.get("port")).filter(|port| !port.is_empty()) {
        url.push(':');
        url.push_str(&port);
    }
    let path = join(object.get("path"), "/");
    if !path.is_empty() {
        if !path.starts_with('/') {
            url.push('/');
        }
        url.push_str(&path);
    }
    url
}

/// Replaces `:key` path segments, leaving the host and query untouched.
fn substitute_path_variable(url: &str, key: &str, value: &str) -> String {
    let segment = format!(":{key}");
    let (prefix, path) = match url.find("://") {
        Some(scheme_end) => match url[scheme_end + 3..].find('/') {
            Some(path_start) => url.split_at(scheme_end + 3 + path_start),
            None => return url.to_owned(),
        },
        // `{{baseUrl}}/users/:id` has no scheme; its first segment is the host.
        None => url.find('/').map_or((url, ""), |index| url.split_at(index)),
    };
    let path = path
        .split('/')
        .map(|part| if part == segment { value } else { part })
        .collect::<Vec<_>>()
        .join("/");
    format!("{prefix}{path}")
}

fn description(value: Option<&Value>) -> Option<String> {
    let text = match value? {
        Value::String(text) => text.as_str(),
        Value::Object(object) => object.get("content").and_then(Value::as_str)?,
        _ => return None,
    };
    Some(text.trim().to_owned()).filter(|text| !text.is_empty())
}

fn disabled(value: &Value) -> bool {
    value
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Reads a string, number, or boolean as text.
fn scalar(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{split_query, substitute_path_variable};

    #[test]
    fn substitutes_only_whole_path_segments() {
        assert_eq!(
            substitute_path_variable("{{baseUrl}}/users/:id/items/:idx", "id", "42"),
            "{{baseUrl}}/users/42/items/:idx"
        );
        assert_eq!(
            substitute_path_variable("https://api.test:8443/:id", "id", "7"),
            "https://api.test:8443/7"
        );
    }

    #[test]
    fn splits_and_decodes_query_rows() {
        let (base, rows) = split_query("{{baseUrl}}/search?q=hello%20world&flag");
        assert_eq!(base, "{{baseUrl}}/search");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, crate::ValueSource::literal("hello world"));
        assert_eq!(rows[1].name, "flag");
    }
}
