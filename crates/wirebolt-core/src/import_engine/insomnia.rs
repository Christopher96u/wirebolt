//! Insomnia importer: v4 exports (`__export_format: 4`, JSON or YAML, a flat
//! `resources` list linked by `parentId`) and v5 YAML collections
//! (`type: collection.insomnia.rest/5.0`, a nested `collection` tree).
//!
//! Both are normalised into one tree of folders and requests so they share the
//! mapping of bodies, authentication, template tags and environments.

use std::collections::HashMap;

use serde_json::Value;

use super::{
    ImportError, ImportedCollection, ImportedEnvironment, ImportedGroup, ImportedRequest,
    ImportedWorkspace, slug,
    support::{
        Diagnostics, Row, array, empty_collection, environment, field, flag, header,
        inherit_headers, order, overlay, scalar, substitute_path_parameters, text,
    },
    yaml,
};
use crate::{
    ApiKeyPlacement, MultipartPart, MultipartPartKind, Oauth2Configuration, Oauth2Grant,
    RequestAuthentication, RequestBody, RequestHeader, SecretName, ValueSource,
};

const DEFAULT_NAME: &str = "Insomnia Collection";
const REDIRECT_URI: &str = "wirebolt://oauth/callback";

/// One Insomnia workspace: its name, request tree and environments.
struct Source<'a> {
    name: String,
    roots: Vec<Node<'a>>,
    environments: Vec<(String, &'a Value, Vec<&'a Value>)>,
}

pub(super) fn parse(
    source: &str,
    file_name: Option<&str>,
) -> Result<ImportedWorkspace, ImportError> {
    let root = match serde_json::from_str::<Value>(source) {
        Ok(root) => root,
        Err(_) => yaml::parse(source)
            .map_err(|_| ImportError::new("Insomnia export is not valid JSON or YAML"))?,
    };
    let kind = text(&root, "type");
    let mut diagnostics = Diagnostics::default();
    let sources = if root.get("__export_format").and_then(Value::as_i64) == Some(4) {
        v4(&root, file_name, &mut diagnostics)?
    } else if kind.starts_with("collection.insomnia.rest/5")
        || kind.starts_with("spec.insomnia.rest/5")
    {
        vec![v5(&root, file_name, &mut diagnostics)]
    } else if root.get("__export_format").is_some() {
        return Err(ImportError::new(
            "this Insomnia export format is too old; export it again from Insomnia as v4 or v5",
        ));
    } else if kind.contains("insomnia") {
        return Err(ImportError::new(
            "only Insomnia collections and design documents can be imported",
        ));
    } else {
        return Err(ImportError::new("document is not an Insomnia export"));
    };
    let mut workspace = ImportedWorkspace {
        collections: Vec::new(),
        environments: Vec::new(),
        request_settings: std::collections::BTreeMap::new(),
    };
    for source in sources {
        let mut importer = Importer {
            collection: empty_collection(),
            diagnostics: std::mem::take(&mut diagnostics),
        };
        importer.walk(&source.roots, None, &Inherited::default());
        for (name, base, subs) in &source.environments {
            workspace.environments.extend(build_environments(
                name,
                base,
                subs,
                &mut importer.diagnostics,
            ));
        }
        diagnostics = importer.diagnostics;
        if !importer.collection.requests.is_empty() {
            importer.collection.name = source.name;
            workspace.collections.push(importer.collection);
        }
    }
    let Some(first) = workspace.collections.first_mut() else {
        return Err(ImportError::new("Insomnia export has no requests"));
    };
    first.warnings = diagnostics.into_warnings();
    Ok(workspace)
}

struct Importer {
    collection: ImportedCollection,
    diagnostics: Diagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Folder,
    Request,
    WebSocket,
    Unsupported(&'static str),
}

/// One folder or request, with its children sorted in Insomnia's order.
struct Node<'a> {
    kind: Kind,
    id: String,
    value: &'a Value,
    children: Vec<Node<'a>>,
}

#[derive(Clone, Default)]
struct Inherited {
    authentication: Option<RequestAuthentication>,
    headers: Vec<RequestHeader>,
}

fn v4<'a>(
    root: &'a Value,
    file_name: Option<&str>,
    diagnostics: &mut Diagnostics,
) -> Result<Vec<Source<'a>>, ImportError> {
    let resources = root
        .get("resources")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::new("Insomnia export has no resources"))?;
    let mut children = HashMap::<&str, Vec<&Value>>::new();
    for resource in resources {
        children
            .entry(text(resource, "parentId"))
            .or_default()
            .push(resource);
    }
    note_unsupported_resources(resources, diagnostics);
    let workspaces = resources
        .iter()
        .filter(|resource| text(resource, "_type") == "workspace")
        .collect::<Vec<_>>();
    if workspaces.is_empty() {
        // Some tools export requests without their workspace resource.
        return Ok(vec![Source {
            name: file_name.unwrap_or(DEFAULT_NAME).to_owned(),
            roots: v4_nodes("", &children),
            environments: Vec::new(),
        }]);
    }
    let single = workspaces.len() == 1;
    Ok(workspaces
        .into_iter()
        .map(|workspace| {
            let id = text(workspace, "_id");
            let own_name = non_empty(text(workspace, "name"));
            let name = if single {
                own_name.or(file_name)
            } else {
                own_name
            }
            .unwrap_or(DEFAULT_NAME)
            .to_owned();
            let environments = v4_environment_resources(id, &children)
                .into_iter()
                .map(|base| {
                    let subs = v4_environment_resources(text(base, "_id"), &children);
                    (name.clone(), base, subs)
                })
                .collect();
            Source {
                roots: v4_nodes(id, &children),
                environments,
                name,
            }
        })
        .collect())
}

fn v4_environment_resources<'a>(
    parent: &str,
    children: &HashMap<&str, Vec<&'a Value>>,
) -> Vec<&'a Value> {
    let mut list = children
        .get(parent)
        .into_iter()
        .flatten()
        .copied()
        .filter(|resource| text(resource, "_type") == "environment")
        .collect::<Vec<_>>();
    list.sort_by(|left, right| sort_key(left).total_cmp(&sort_key(right)));
    list
}

fn v4_nodes<'a>(parent: &str, children: &HashMap<&str, Vec<&'a Value>>) -> Vec<Node<'a>> {
    let mut nodes = children
        .get(parent)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let kind = match text(value, "_type") {
                "request_group" => Kind::Folder,
                "request" => Kind::Request,
                "websocket_request" => Kind::WebSocket,
                "grpc_request" => Kind::Unsupported("gRPC requests aren’t supported"),
                "socketio_request" | "socket_io_request" => {
                    Kind::Unsupported("Socket.IO requests aren’t supported")
                }
                _ => return None,
            };
            let id = text(value, "_id");
            Some(Node {
                kind,
                id: id.to_owned(),
                value,
                children: if kind == Kind::Folder {
                    v4_nodes(id, children)
                } else {
                    Vec::new()
                },
            })
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| sort_key(left.value).total_cmp(&sort_key(right.value)));
    nodes
}

fn note_unsupported_resources(resources: &[Value], diagnostics: &mut Diagnostics) {
    for resource in resources {
        let name = text(resource, "name");
        match text(resource, "_type") {
            "websocket_payload" => {
                diagnostics.note("Saved WebSocket messages were not imported", String::new());
            }
            "unit_test_suite" | "unit_test" => {
                diagnostics.note("Insomnia unit tests were not imported", name);
            }
            "api_spec" if !text(resource, "contents").is_empty() => {
                diagnostics.note("API design documents were not imported", name);
            }
            "cookie_jar" if !array(resource, "cookies").is_empty() => {
                diagnostics.note("Cookies were not imported", String::new());
            }
            "client_certificate" => diagnostics.note(
                "Client certificates were not imported; configure them in Workspace Settings",
                String::new(),
            ),
            "mock_server" | "mock_route" => {
                diagnostics.note("Mock servers were not imported", name);
            }
            "proto_file" | "proto_directory" => {
                diagnostics.note("Protocol buffer files were not imported", name);
            }
            _ => {}
        }
    }
}

fn v5<'a>(root: &'a Value, file_name: Option<&str>, diagnostics: &mut Diagnostics) -> Source<'a> {
    let name = non_empty(text(root, "name"))
        .or(file_name)
        .unwrap_or(DEFAULT_NAME)
        .to_owned();
    if root
        .pointer("/cookieJar/cookies")
        .and_then(Value::as_array)
        .is_some_and(|cookies| !cookies.is_empty())
    {
        diagnostics.note("Cookies were not imported", String::new());
    }
    if !array(root, "testSuites").is_empty() {
        diagnostics.note("Insomnia unit tests were not imported", String::new());
    }
    if !array(root, "certificates").is_empty() {
        diagnostics.note(
            "Client certificates were not imported; configure them in Workspace Settings",
            String::new(),
        );
    }
    if root
        .pointer("/spec/contents")
        .is_some_and(|spec| !spec.is_null())
    {
        diagnostics.note("API design documents were not imported", name.clone());
    }
    let environments = root
        .get("environments")
        .map(|base| {
            let subs = array(base, "subEnvironments").iter().collect::<Vec<_>>();
            vec![(name.clone(), base, subs)]
        })
        .unwrap_or_default();
    Source {
        roots: v5_nodes(array(root, "collection")),
        environments,
        name,
    }
}

fn v5_nodes(items: &[Value]) -> Vec<Node<'_>> {
    let mut nodes = items
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let id = value
                .pointer("/meta/id")
                .and_then(Value::as_str)
                .map_or_else(|| format!("item-{index}"), str::to_owned);
            let has_url = value.get("url").is_some();
            let kind = if value.get("method").is_some() {
                Kind::Request
            } else if value.get("reflectionApi").is_some() {
                Kind::Unsupported("gRPC requests aren’t supported")
            } else if id.starts_with("socketio-req") {
                Kind::Unsupported("Socket.IO requests aren’t supported")
            } else if id.starts_with("ws-req") || (has_url && text(value, "url").starts_with("ws"))
            {
                Kind::WebSocket
            } else if has_url {
                Kind::Unsupported("Unrecognised requests were skipped")
            } else {
                Kind::Folder
            };
            Node {
                kind,
                children: if kind == Kind::Folder {
                    v5_nodes(array(value, "children"))
                } else {
                    Vec::new()
                },
                id,
                value,
            }
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| sort_key(left.value).total_cmp(&sort_key(right.value)));
    nodes
}

fn sort_key(value: &Value) -> f64 {
    value
        .get("metaSortKey")
        .or_else(|| value.pointer("/meta/sortKey"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

fn description(value: &Value) -> String {
    value
        .get("description")
        .or_else(|| value.pointer("/meta/description"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn has_scripts(value: &Value) -> bool {
    [
        "preRequestScript",
        "afterResponseScript",
        "/scripts/preRequest",
        "/scripts/afterResponse",
    ]
    .iter()
    .any(|key| {
        let script = if key.starts_with('/') {
            value.pointer(key)
        } else {
            value.get(*key)
        };
        script
            .and_then(Value::as_str)
            .is_some_and(|script| !script.trim().is_empty())
    })
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

impl Importer {
    fn walk(&mut self, nodes: &[Node<'_>], parent: Option<&str>, inherited: &Inherited) {
        for (index, node) in nodes.iter().enumerate() {
            let name = non_empty(text(node.value, "name")).unwrap_or("Untitled");
            match node.kind {
                Kind::Folder => {
                    let source_id = format!("insomnia-{}", node.id);
                    self.collection.groups.push(ImportedGroup {
                        source_id: source_id.clone(),
                        name: name.to_owned(),
                        parent_source_id: parent.map(str::to_owned),
                        order: order(index),
                    });
                    let inherited = self.folder_inheritance(node.value, name, inherited);
                    self.walk(&node.children, Some(&source_id), &inherited);
                }
                Kind::Request | Kind::WebSocket => {
                    let request = self.request(node, name, parent, index, inherited);
                    self.collection.requests.push(request);
                }
                Kind::Unsupported(reason) => self.diagnostics.note(reason, name),
            }
        }
    }

    fn folder_inheritance(&mut self, folder: &Value, name: &str, parent: &Inherited) -> Inherited {
        let mut inherited = parent.clone();
        if let Some(authentication) = self.authentication(folder, name, &format!("folder-{name}")) {
            inherited.authentication = Some(authentication);
        }
        let mut headers = self.headers(folder, name);
        inherit_headers(&mut headers, &parent.headers);
        inherited.headers = headers;
        if folder
            .get("environment")
            .and_then(Value::as_object)
            .is_some_and(|environment| !environment.is_empty())
        {
            self.diagnostics
                .note("Folder environment variables were not imported", name);
        }
        if has_scripts(folder) {
            self.diagnostics
                .note("Scripts aren’t run by Wirebolt and were not imported", name);
        }
        if !description(folder).trim().is_empty() {
            self.diagnostics
                .note("Folder descriptions were not imported", name);
        }
        inherited
    }

    fn request(
        &mut self,
        node: &Node<'_>,
        name: &str,
        parent: Option<&str>,
        index: usize,
        inherited: &Inherited,
    ) -> ImportedRequest {
        let value = node.value;
        let path_parameters = array(value, "pathParameters")
            .iter()
            .map(|parameter| {
                (
                    text(parameter, "name").to_owned(),
                    self.template(text(parameter, "value"), name),
                )
            })
            .collect::<Vec<_>>();
        let url = self.template(text(value, "url"), name);
        let url = substitute_path_parameters(&url, &path_parameters);
        let mut headers = self.headers(value, name);
        inherit_headers(&mut headers, &inherited.headers);
        let query = array(value, "parameters")
            .iter()
            .filter(|parameter| {
                !(text(parameter, "name").is_empty() && text(parameter, "value").is_empty())
            })
            .map(|parameter| {
                field(
                    &self.template(text(parameter, "name"), name),
                    self.template(text(parameter, "value"), name),
                    !flag(parameter, "disabled"),
                )
            })
            .collect();
        let authentication = self
            .authentication(value, name, &node.id)
            .or_else(|| inherited.authentication.clone())
            .unwrap_or_default();
        if has_scripts(value) {
            self.diagnostics
                .note("Scripts aren’t run by Wirebolt and were not imported", name);
        }
        let web_socket = node.kind == Kind::WebSocket;
        ImportedRequest {
            source_id: format!("insomnia-{}", node.id),
            name: name.to_owned(),
            group_source_id: parent.map(str::to_owned),
            order: order(index),
            method: if web_socket {
                "GET".to_owned()
            } else {
                non_empty(text(value, "method"))
                    .unwrap_or("GET")
                    .to_ascii_uppercase()
            },
            url,
            headers,
            query,
            authentication,
            body: if web_socket {
                RequestBody::Empty
            } else {
                self.body(value.get("body"), name)
            },
            web_socket,
            note: description(value),
        }
    }

    fn headers(&mut self, value: &Value, subject: &str) -> Vec<RequestHeader> {
        array(value, "headers")
            .iter()
            .filter(|row| !(text(row, "name").is_empty() && text(row, "value").is_empty()))
            .map(|row| {
                header(
                    &self.template(text(row, "name"), subject),
                    self.template(text(row, "value"), subject),
                    !flag(row, "disabled"),
                )
            })
            .collect()
    }

    fn body(&mut self, body: Option<&Value>, subject: &str) -> RequestBody {
        let Some(body) = body.filter(|body| body.is_object()) else {
            return RequestBody::Empty;
        };
        let mime = text(body, "mimeType");
        let essence = mime
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let content = self.template(text(body, "text"), subject);
        let file_name = text(body, "fileName");
        match essence.as_str() {
            "application/x-www-form-urlencoded" => RequestBody::FormUrlEncoded {
                fields: array(body, "params")
                    .iter()
                    .map(|param| {
                        field(
                            &self.template(text(param, "name"), subject),
                            self.template(text(param, "value"), subject),
                            !flag(param, "disabled"),
                        )
                    })
                    .collect(),
            },
            "multipart/form-data" => RequestBody::Multipart {
                parts: array(body, "params")
                    .iter()
                    .enumerate()
                    .map(|(index, param)| self.multipart_part(index, param, subject))
                    .collect(),
            },
            "application/graphql" => RequestBody::Json {
                value: graphql_json(&content),
            },
            "application/xml" | "text/xml" => RequestBody::Xml { value: content },
            "text/html" => RequestBody::Html { value: content },
            _ if essence == "application/json" || essence.ends_with("+json") => {
                RequestBody::Json { value: content }
            }
            _ if !file_name.is_empty() => RequestBody::File {
                path: file_name.to_owned(),
                content_type: non_empty(mime)
                    .filter(|_| essence != "application/octet-stream")
                    .map(str::to_owned),
            },
            "" if content.is_empty() => RequestBody::Empty,
            _ => RequestBody::Text {
                content_type: non_empty(mime).map(str::to_owned),
                value: content,
            },
        }
    }

    fn multipart_part(&mut self, index: usize, param: &Value, subject: &str) -> MultipartPart {
        let is_file = text(param, "type") == "file";
        let path = text(param, "fileName");
        MultipartPart {
            id: format!("part-{index}"),
            name: self.template(text(param, "name"), subject),
            kind: if is_file {
                MultipartPartKind::File
            } else {
                MultipartPartKind::Text
            },
            value: ValueSource::literal(if is_file {
                String::new()
            } else {
                self.template(text(param, "value"), subject)
            }),
            file_path: (is_file && !path.is_empty()).then(|| path.to_owned()),
            file_name: (is_file && !path.is_empty())
                .then(|| path.rsplit('/').next().unwrap_or(path).to_owned()),
            content_type: None,
            enabled: !flag(param, "disabled"),
        }
    }

    /// `None` means the item inherits authentication from its parent folder.
    fn authentication(
        &mut self,
        value: &Value,
        subject: &str,
        source_id: &str,
    ) -> Option<RequestAuthentication> {
        let auth = value.get("authentication")?;
        let kind = auth.get("type").and_then(Value::as_str)?;
        if flag(auth, "disabled") {
            return None;
        }
        let mut literal = |key: &str| ValueSource::literal(self.template(text(auth, key), subject));
        Some(match kind {
            "none" => RequestAuthentication::None,
            "basic" => RequestAuthentication::Basic {
                username: literal("username"),
                password: literal("password"),
            },
            "bearer" => {
                let prefix = text(auth, "prefix").trim();
                if !prefix.is_empty() && !prefix.eq_ignore_ascii_case("bearer") {
                    self.diagnostics.note(
                        "Custom bearer token prefixes were replaced with “Bearer”",
                        subject,
                    );
                }
                RequestAuthentication::Bearer {
                    token: ValueSource::literal(self.template(text(auth, "token"), subject)),
                }
            }
            "apikey" => {
                let placement = match text(auth, "addTo") {
                    "queryParams" => ApiKeyPlacement::Query,
                    "cookie" => {
                        self.diagnostics.note(
                            "API keys sent as cookies aren’t supported; add a Cookie header instead",
                            subject,
                        );
                        return Some(RequestAuthentication::None);
                    }
                    _ => ApiKeyPlacement::Header,
                };
                RequestAuthentication::ApiKey {
                    placement,
                    name: self.template(text(auth, "key"), subject),
                    value: ValueSource::literal(self.template(text(auth, "value"), subject)),
                }
            }
            "oauth2" => self.oauth2(auth, subject, source_id),
            other => {
                let label = match other {
                    "digest" => "Digest",
                    "ntlm" => "NTLM",
                    "iam" => "AWS IAM",
                    "hawk" => "Hawk",
                    "oauth1" => "OAuth 1.0",
                    "asap" => "Atlassian ASAP",
                    "netrc" => "Netrc",
                    _ => "This",
                };
                self.diagnostics.note(
                    format!("{label} authentication isn’t supported; the request was imported without it"),
                    subject,
                );
                RequestAuthentication::None
            }
        })
    }

    fn oauth2(&mut self, auth: &Value, subject: &str, source_id: &str) -> RequestAuthentication {
        let grant = match text(auth, "grantType") {
            "authorization_code" => Oauth2Grant::AuthorizationCodePkce,
            "client_credentials" => Oauth2Grant::ClientCredentials,
            _ => {
                self.diagnostics.note(
                    "Only authorization code and client credentials OAuth 2.0 grants are supported; other grants were imported without authentication",
                    subject,
                );
                return RequestAuthentication::None;
            }
        };
        if !text(auth, "clientSecret").is_empty() {
            self.diagnostics.note(
                "OAuth 2.0 client secrets aren’t imported; enter them in the Auth tab",
                subject,
            );
        }
        let Some((client_secret_reference, access_token_reference)) = oauth_references(source_id)
        else {
            return RequestAuthentication::None;
        };
        let redirect = text(auth, "redirectUrl");
        RequestAuthentication::Oauth2 {
            configuration: Oauth2Configuration {
                grant,
                authorization_url: self.template(text(auth, "authorizationUrl"), subject),
                token_url: self.template(text(auth, "accessTokenUrl"), subject),
                client_id: self.template(text(auth, "clientId"), subject),
                client_secret_reference,
                scopes: self.template(text(auth, "scope"), subject),
                audience: self.template(text(auth, "audience"), subject),
                redirect_uri: non_empty(redirect).unwrap_or(REDIRECT_URI).to_owned(),
                access_token_reference,
            },
        }
    }

    fn template(&mut self, value: &str, subject: &str) -> String {
        let (converted, unsupported) = convert_template(value);
        if unsupported {
            self.diagnostics.note(
                "Insomnia template tags such as response references, functions or filters were kept as text and must be replaced",
                subject,
            );
        }
        converted
    }
}

/// Keychain reference names unique to one imported request; `source` is a
/// stable description of the request (its ID or collection path).
pub(super) fn oauth_references(source: &str) -> Option<(SecretName, SecretName)> {
    // FNV-1a keeps long or similar names distinct after slugging.
    let hash = source
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    let key = format!("{}-{:08x}", slug(source), hash >> 32);
    Some((
        SecretName::new(format!("oauth.{key}.client-secret")).ok()?,
        SecretName::new(format!("oauth.{key}.access-token")).ok()?,
    ))
}

/// Insomnia GraphQL bodies store `{"query": …, "variables": …}` as text.
fn graphql_json(text: &str) -> String {
    if serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok() {
        text.to_owned()
    } else {
        serde_json::json!({ "query": text }).to_string()
    }
}

/// Rewrites `{{ _.name }}` and `{{ name }}` to Wirebolt's `{{name}}`. Other
/// Nunjucks expressions and `{% tag %}` functions are kept verbatim; the
/// returned flag reports that at least one was found.
pub(super) fn convert_template(value: &str) -> (String, bool) {
    if !value.contains("{{") && !value.contains("{%") {
        return (value.to_owned(), false);
    }
    let mut output = String::with_capacity(value.len());
    let mut unsupported = false;
    let mut rest = value;
    while let Some(start) = rest.find(['{']) {
        output.push_str(&rest[..start]);
        let candidate = &rest[start..];
        if let Some(inner) = candidate.strip_prefix("{{")
            && let Some(end) = inner.find("}}")
        {
            if let Some(name) = variable_name(inner[..end].trim()) {
                output.push_str("{{");
                output.push_str(name);
                output.push_str("}}");
            } else {
                unsupported = true;
                output.push_str(&candidate[..end + 4]);
            }
            rest = &inner[end + 2..];
        } else if let Some(inner) = candidate.strip_prefix("{%")
            && let Some(end) = inner.find("%}")
        {
            unsupported = true;
            output.push_str(&candidate[..end + 4]);
            rest = &inner[end + 2..];
        } else {
            output.push('{');
            rest = &candidate[1..];
        }
    }
    output.push_str(rest);
    (output, unsupported)
}

fn variable_name(expression: &str) -> Option<&str> {
    let name = if let Some(path) = expression.strip_prefix("_.") {
        path
    } else if let Some(key) = expression
        .strip_prefix("_['")
        .and_then(|key| key.strip_suffix("']"))
        .or_else(|| {
            expression
                .strip_prefix("_[\"")
                .and_then(|key| key.strip_suffix("\"]"))
        })
    {
        key
    } else {
        expression
    };
    let valid = !name.is_empty()
        && !name.starts_with("vault.")
        && name.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '-' | '.' | '$')
        });
    valid.then_some(name)
}

/// Insomnia layers a sub environment over its base environment. Wirebolt has
/// no layering, so each sub environment becomes one environment containing the
/// merged rows; a base without sub environments becomes one environment.
fn build_environments(
    collection_name: &str,
    base: &Value,
    subs: &[&Value],
    diagnostics: &mut Diagnostics,
) -> Vec<ImportedEnvironment> {
    let base_rows = variables(base, diagnostics);
    if subs.is_empty() {
        if base_rows.is_empty() {
            return Vec::new();
        }
        return vec![environment(collection_name, "", base_rows, diagnostics)];
    }
    subs.iter()
        .enumerate()
        .map(|(index, sub)| {
            let mut rows = base_rows.clone();
            overlay(&mut rows, variables(sub, diagnostics));
            let name = non_empty(text(sub, "name"))
                .map_or_else(|| format!("Environment {}", index + 1), str::to_owned);
            environment(collection_name, &name, rows, diagnostics)
        })
        .collect()
}

fn variables(environment: &Value, diagnostics: &mut Diagnostics) -> Vec<Row> {
    let subject = text(environment, "name");
    let mut rows = Vec::new();
    let mut unsupported = false;
    let key_value_rows = array(environment, "kvPairData");
    if text(environment, "environmentType") == "kv" && !key_value_rows.is_empty() {
        for row in key_value_rows {
            let key = text(row, "name");
            if key.is_empty() {
                continue;
            }
            let secret = text(row, "type") == "secret";
            if secret {
                diagnostics.note(
                    "Insomnia vault secrets aren’t exported; set their values in Configure Environments",
                    key,
                );
            }
            let (value, tags) = convert_template(text(row, "value"));
            unsupported |= tags;
            rows.push(Row {
                key: key.to_owned(),
                value: if secret { String::new() } else { value },
                enabled: row.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                secret,
            });
        }
    } else if let Some(data) = environment.get("data").and_then(Value::as_object) {
        let order = environment
            .pointer("/dataPropertyOrder/&")
            .and_then(Value::as_array)
            .map(|keys| keys.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut keys = order
            .iter()
            .copied()
            .filter(|key| data.contains_key(*key))
            .collect::<Vec<_>>();
        keys.extend(
            data.keys()
                .map(String::as_str)
                .filter(|key| !order.contains(key)),
        );
        for key in keys {
            if key.starts_with("__insomnia_vault") {
                diagnostics.note(
                    "Insomnia vault secrets aren’t exported; set their values in Configure Environments",
                    String::new(),
                );
                continue;
            }
            flatten(key, &data[key], &mut rows, &mut unsupported);
        }
    }
    if unsupported {
        diagnostics.note(
            "Insomnia template tags such as response references, functions or filters were kept as text and must be replaced",
            subject,
        );
    }
    rows
}

/// Nested environment objects become dotted names (`{{ _.api.host }}` → `{{api.host}}`).
fn flatten(key: &str, value: &Value, rows: &mut Vec<Row>, unsupported: &mut bool) {
    match value {
        Value::Object(object) => {
            for (child, value) in object {
                flatten(&format!("{key}.{child}"), value, rows, unsupported);
            }
        }
        Value::Array(_) => rows.push(Row::literal(key, value.to_string(), true)),
        _ => {
            let (text, tags) = convert_template(&scalar(value).unwrap_or_default());
            *unsupported |= tags;
            rows.push(Row::literal(key, text, true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::convert_template;

    #[test]
    fn converts_variable_references_and_keeps_unsupported_tags() {
        assert_eq!(
            convert_template("{{ _.base_url }}/v1/{{ _['item-id'] }}?q={{token}}"),
            ("{{base_url}}/v1/{{item-id}}?q={{token}}".to_owned(), false)
        );
        assert_eq!(
            convert_template("{{ _.api.host }}"),
            ("{{api.host}}".to_owned(), false)
        );
        let (converted, unsupported) = convert_template(
            "Bearer {% response 'body', 'req_1', 'b64::JC50b2tlbg==::46b', 'never', 60 %} {{ _.name | upper }}",
        );
        assert!(unsupported);
        assert_eq!(
            converted,
            "Bearer {% response 'body', 'req_1', 'b64::JC50b2tlbg==::46b', 'never', 60 %} {{ _.name | upper }}"
        );
        assert_eq!(
            convert_template("{\"a\": {}}"),
            ("{\"a\": {}}".to_owned(), false)
        );
    }
}
