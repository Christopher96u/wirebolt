//! Bruno importer for collection folders (`bruno.json`, `collection.bru`,
//! `folder.bru`, request `.bru` files and `environments/*.bru`) and for the
//! JSON document written by Bruno's "Export collection".
//!
//! A folder is first converted into the same JSON shape as an export, so both
//! inputs share one mapping.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::{
    ImportError, ImportedEnvironment, ImportedGroup, ImportedRequest, ParsedImport,
    SecretCollector,
    bru::{self, BruContent, BruFile, BruPair},
    support::{
        Diagnostics, Row, array, empty_collection, environment, field, flag, header,
        inherit_headers, oauth_references, order, overlay, parsed, scalar, split_query,
        substitute_path_parameters, text,
    },
};
use crate::{
    ApiKeyPlacement, MultipartPart, MultipartPartKind, Oauth2Configuration, Oauth2Grant,
    RequestAuthentication, RequestBody, RequestHeader, ValueSource,
};

const DEFAULT_NAME: &str = "Bruno Collection";
const REDIRECT_URI: &str = "wirebolt://oauth/callback";
const MAX_FILES: usize = 10_000;

// MARK: - Folder bundle

pub(super) fn folder_source(root: &str, files: &[(String, String)]) -> String {
    json!({
        "root": root,
        "files": files
            .iter()
            .map(|(path, contents)| json!({ "path": path, "contents": contents }))
            .collect::<Vec<_>>(),
    })
    .to_string()
}

pub(super) fn parse_folder(source: &str, name: Option<&str>) -> Result<ParsedImport, ImportError> {
    let bundle: Value = serde_json::from_str(source)
        .map_err(|_| ImportError::new("The Bruno collection folder couldn't be read."))?;
    let root_directory = text(&bundle, "root");
    let files = array(&bundle, "files")
        .iter()
        .take(MAX_FILES)
        .filter_map(|file| {
            let path = text(file, "path").trim_start_matches("./");
            (!path.is_empty() && !is_ignored(path))
                .then(|| (path.to_owned(), text(file, "contents")))
        })
        .collect::<Vec<_>>();
    let mut diagnostics = Diagnostics::default();
    let config = files
        .iter()
        .find(|(path, _)| path == "bruno.json")
        .and_then(|(_, contents)| serde_json::from_str::<Value>(contents).ok());
    if !files.iter().any(|(path, _)| is_bru(path)) {
        return Err(ImportError::new(
            if files.iter().any(|(path, _)| has_extension(path, "yml")) {
                "Bruno YAML collections aren't supported yet. Use Export Collection in Bruno and import the JSON file."
            } else {
                "The folder isn't a Bruno collection."
            },
        ));
    }
    let collection_name = config
        .as_ref()
        .and_then(|config| config.get("name").and_then(Value::as_str))
        .filter(|name| !name.trim().is_empty())
        .or(name)
        .unwrap_or(DEFAULT_NAME);

    let parsed = files
        .iter()
        .filter(|(path, _)| is_bru(path))
        .filter_map(|(path, contents)| {
            let file = bru::parse(contents);
            if file.is_err() {
                diagnostics.note(
                    "Some .bru files could not be read and were skipped",
                    path.clone(),
                );
            }
            file.ok().map(|file| (path.as_str(), file))
        })
        .collect::<Vec<_>>();

    let mut document = json!({
        "name": collection_name,
        "items": folder_items("", &parsed),
        "environments": parsed
            .iter()
            .filter_map(|(path, file)| {
                let stem = path.strip_prefix("environments/")?.strip_suffix(".bru")?;
                (!stem.contains('/')).then(|| environment_from_bru(stem, file))
            })
            .collect::<Vec<_>>(),
    });
    if let Some((_, file)) = parsed.iter().find(|(path, _)| *path == "collection.bru") {
        document["root"] = root_from_bru(file);
    }
    Ok(convert(&document, non_empty(root_directory), diagnostics))
}

fn has_extension(path: &str, extension: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
}

fn is_bru(path: &str) -> bool {
    has_extension(path, "bru")
}

fn is_ignored(path: &str) -> bool {
    path.split('/').any(|component| {
        component.starts_with('.') || component == "node_modules" || component.is_empty()
    })
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

fn directory_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".bru").unwrap_or(name)
}

/// Builds export-shaped items for one directory: sub folders, then requests.
fn folder_items(directory: &str, files: &[(&str, BruFile)]) -> Vec<Value> {
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    let subdirectories = files
        .iter()
        .filter_map(|(path, _)| {
            let rest = path.strip_prefix(&prefix)?;
            let (child, _) = rest.split_once('/')?;
            (!(directory.is_empty() && child == "environments")).then_some(child)
        })
        .collect::<BTreeSet<_>>();
    let mut items = subdirectories
        .into_iter()
        .map(|child| {
            let path = format!("{prefix}{child}");
            let folder = files
                .iter()
                .find(|(file, _)| *file == format!("{path}/folder.bru"))
                .map(|(_, file)| file);
            let mut item = json!({
                "type": "folder",
                "name": folder
                    .and_then(|file| file.value("meta", "name"))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| child.to_owned()),
                "items": folder_items(&path, files),
            });
            if let Some(folder) = folder {
                item["seq"] = number(folder.value("meta", "seq"));
                item["root"] = root_from_bru(folder);
            }
            item
        })
        .collect::<Vec<_>>();
    items.extend(
        files
            .iter()
            .filter(|(path, _)| {
                directory_of(path) == directory
                    && !path.ends_with("/folder.bru")
                    && *path != "folder.bru"
                    && *path != "collection.bru"
            })
            .map(|(path, file)| request_from_bru(file_stem(path), file)),
    );
    items
}

fn number(value: Option<String>) -> Value {
    value
        .and_then(|value| value.trim().parse::<f64>().ok())
        .and_then(serde_json::Number::from_f64)
        .map_or(Value::Null, Value::Number)
}

fn pairs_json(pairs: &[BruPair]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|pair| json!({ "name": pair.name, "value": pair.value, "enabled": pair.enabled }))
            .collect(),
    )
}

const METHOD_BLOCKS: [&str; 10] = [
    "get", "post", "put", "delete", "patch", "options", "head", "connect", "trace", "http",
];

fn request_from_bru(stem: &str, file: &BruFile) -> Value {
    let kind = file
        .value("meta", "type")
        .unwrap_or_else(|| "http".to_owned());
    let block = if kind == "ws" || kind == "grpc" {
        kind.as_str()
    } else {
        METHOD_BLOCKS
            .iter()
            .copied()
            .find(|name| file.block(name).is_some())
            .unwrap_or("get")
    };
    let method = if block == "http" {
        file.value(block, "method").unwrap_or_default()
    } else {
        block.to_owned()
    };
    let mut params = Vec::new();
    for (name, kind) in [
        ("params:query", "query"),
        ("query", "query"),
        ("params:path", "path"),
    ] {
        params.extend(file.pairs(name).into_iter().map(|pair| {
            json!({ "name": pair.name, "value": pair.value, "enabled": pair.enabled, "type": kind })
        }));
    }
    let legacy_json = file.text("body").map(str::to_owned);
    let body_mode = file
        .value(block, "body")
        .or_else(|| legacy_json.as_ref().map(|_| "json".to_owned()))
        .unwrap_or_else(|| "none".to_owned());
    json!({
        "type": kind,
        "name": file
            .value("meta", "name")
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| stem.to_owned()),
        "seq": number(file.value("meta", "seq")),
        "request": {
            "url": file.value(block, "url").unwrap_or_default(),
            "method": method.to_ascii_uppercase(),
            "headers": pairs_json(&file.pairs("headers")),
            "params": params,
            "body": {
                "mode": body_mode,
                "json": file.text("body:json").map(str::to_owned).or(legacy_json),
                "text": file.text("body:text"),
                "xml": file.text("body:xml"),
                "sparql": file.text("body:sparql"),
                "graphql": {
                    "query": file.text("body:graphql"),
                    "variables": file.text("body:graphql:vars"),
                },
                "formUrlEncoded": pairs_json(&file.pairs("body:form-urlencoded")),
                "multipartForm": file
                    .pairs("body:multipart-form")
                    .iter()
                    .map(multipart_from_bru)
                    .collect::<Vec<_>>(),
                "file": file.pairs("body:file").iter().map(file_from_bru).collect::<Vec<_>>(),
                "ws": if file.block("body:ws").is_some() { json!([{}]) } else { json!([]) },
            },
            "auth": auth_from_bru(file, file.value(block, "auth").as_deref().unwrap_or("none")),
            "script": { "req": file.text("script:pre-request"), "res": file.text("script:post-response") },
            "vars": { "req": pairs_json(&file.pairs("vars:pre-request")), "res": pairs_json(&file.pairs("vars:post-response")) },
            "assertions": pairs_json(&file.pairs("assert")),
            "tests": file.text("tests"),
            "docs": file.text("docs"),
        },
    })
}

/// Splits Bruno's `value @contentType(type)` suffix.
fn content_type_suffix(value: &str) -> (&str, Option<&str>) {
    value
        .rfind("@contentType(")
        .filter(|_| value.trim_end().ends_with(')'))
        .map_or((value, None), |index| {
            let content_type = value[index + "@contentType(".len()..]
                .trim_end()
                .trim_end_matches(')');
            (value[..index].trim_end(), Some(content_type))
        })
}

fn file_reference(value: &str) -> Option<&str> {
    value.strip_prefix("@file(")?.strip_suffix(')')
}

fn multipart_from_bru(pair: &BruPair) -> Value {
    let (value, content_type) = content_type_suffix(&pair.value);
    match file_reference(value) {
        Some(paths) => json!({
            "type": "file",
            "name": pair.name,
            "value": paths.split('|').filter(|path| !path.is_empty()).collect::<Vec<_>>(),
            "contentType": content_type,
            "enabled": pair.enabled,
        }),
        None => json!({
            "type": "text",
            "name": pair.name,
            "value": value,
            "contentType": content_type,
            "enabled": pair.enabled,
        }),
    }
}

fn file_from_bru(pair: &BruPair) -> Value {
    let (value, content_type) = content_type_suffix(&pair.value);
    json!({
        "filePath": file_reference(value).unwrap_or(value),
        "contentType": content_type,
        "selected": pair.enabled,
    })
}

fn auth_from_bru(file: &BruFile, mode: &str) -> Value {
    let values = |block: &str| {
        Value::Object(
            file.pairs(block)
                .into_iter()
                .map(|pair| (pair.name, Value::String(pair.value)))
                .collect::<Map<_, _>>(),
        )
    };
    let oauth = values("auth:oauth2");
    json!({
        "mode": mode,
        "basic": values("auth:basic"),
        "bearer": values("auth:bearer"),
        "apikey": values("auth:apikey"),
        "oauth2": {
            "grantType": oauth.get("grant_type"),
            "authorizationUrl": oauth.get("authorization_url"),
            "accessTokenUrl": oauth.get("access_token_url"),
            "clientId": oauth.get("client_id"),
            "clientSecret": oauth.get("client_secret"),
            "scope": oauth.get("scope"),
            "callbackUrl": oauth.get("callback_url"),
        },
    })
}

/// `collection.bru` and `folder.bru` share this layout.
fn root_from_bru(file: &BruFile) -> Value {
    json!({
        "request": {
            "headers": pairs_json(&file.pairs("headers")),
            "auth": auth_from_bru(file, file.value("auth", "mode").as_deref().unwrap_or_default()),
            "script": { "req": file.text("script:pre-request"), "res": file.text("script:post-response") },
            "vars": { "req": pairs_json(&file.pairs("vars:pre-request")), "res": pairs_json(&file.pairs("vars:post-response")) },
            "tests": file.text("tests"),
        },
        "docs": file.text("docs"),
    })
}

fn environment_from_bru(stem: &str, file: &BruFile) -> Value {
    let mut variables = file
        .pairs("vars")
        .into_iter()
        .map(|pair| json!({ "name": pair.name, "value": pair.value, "enabled": pair.enabled, "secret": false }))
        .collect::<Vec<_>>();
    if let Some(BruContent::List(secrets)) = file.block("vars:secret") {
        variables.extend(secrets.iter().map(|pair| {
            json!({ "name": pair.name, "value": "", "enabled": pair.enabled, "secret": true })
        }));
    }
    json!({ "name": stem, "variables": variables })
}

// MARK: - Export JSON

pub(super) fn parse_export(source: &str) -> Result<ParsedImport, ImportError> {
    let root: Value = serde_json::from_str(source)
        .map_err(|_| ImportError::new("The Bruno export isn't valid JSON."))?;
    let is_collection = root.get("version").is_some_and(Value::is_string)
        && root.get("name").is_some_and(Value::is_string)
        && root.get("items").is_some_and(Value::is_array);
    if !is_collection {
        return Err(ImportError::new(
            "The document isn't a Bruno collection export.",
        ));
    }
    Ok(convert(&root, None, Diagnostics::default()))
}

// MARK: - Mapping

struct Converter<'a> {
    base_directory: Option<&'a str>,
    requests: Vec<ImportedRequest>,
    groups: Vec<ImportedGroup>,
    diagnostics: Diagnostics,
    secrets: SecretCollector,
}

#[derive(Clone, Default)]
struct Inherited {
    authentication: RequestAuthentication,
    headers: Vec<RequestHeader>,
}

fn convert(root: &Value, base_directory: Option<&str>, diagnostics: Diagnostics) -> ParsedImport {
    let name = non_empty(text(root, "name"))
        .unwrap_or(DEFAULT_NAME)
        .to_owned();
    let mut converter = Converter {
        base_directory,
        requests: Vec::new(),
        groups: Vec::new(),
        diagnostics,
        secrets: SecretCollector::default(),
    };
    let collection_root = root.get("root").unwrap_or(&Value::Null);
    let inherited = converter.inheritance(collection_root, &name, &Inherited::default());
    converter.walk(array(root, "items"), None, &inherited);
    let collection_variables = collection_variables(collection_root.pointer("/request/vars/req"));
    let environments = converter.environments(&name, root, collection_variables);
    let mut collection = empty_collection();
    collection.name = name;
    collection.groups = converter.groups;
    collection.requests = converter.requests;
    collection.warnings = converter.diagnostics.into_warnings();
    parsed(vec![collection], environments, converter.secrets)
}

fn sequence(item: &Value) -> f64 {
    item.get("seq").and_then(Value::as_f64).unwrap_or(f64::MAX)
}

fn is_folder(item: &Value) -> bool {
    text(item, "type") == "folder"
}

impl Converter<'_> {
    fn walk(&mut self, items: &[Value], parent: Option<&str>, inherited: &Inherited) {
        let mut items = items.iter().collect::<Vec<_>>();
        items.sort_by(|left, right| {
            is_folder(right)
                .cmp(&is_folder(left))
                .then(sequence(left).total_cmp(&sequence(right)))
                .then_with(|| text(left, "name").cmp(text(right, "name")))
        });
        for (index, item) in items.into_iter().enumerate() {
            let name = non_empty(text(item, "name"))
                .unwrap_or("Untitled")
                .to_owned();
            match text(item, "type") {
                "folder" => {
                    let source_id = format!("bruno-group-{}", self.groups.len());
                    self.groups.push(ImportedGroup {
                        source_id: source_id.clone(),
                        name: name.clone(),
                        parent_source_id: parent.map(str::to_owned),
                        order: order(index),
                    });
                    let root = item.get("root").unwrap_or(&Value::Null);
                    let inherited = self.inheritance(root, &name, inherited);
                    if root
                        .pointer("/request/vars/req")
                        .and_then(Value::as_array)
                        .is_some_and(|vars| !vars.is_empty())
                    {
                        self.diagnostics
                            .note("Folder variables were not imported", name.clone());
                    }
                    self.walk(array(item, "items"), Some(&source_id), &inherited);
                }
                "http" | "http-request" | "graphql" | "graphql-request" | "ws" | "ws-request" => {
                    let request = self.request(item, &name, parent, index, inherited);
                    self.requests.push(request);
                }
                "grpc" | "grpc-request" => self
                    .diagnostics
                    .note("gRPC requests aren't supported", name),
                _ => {}
            }
        }
    }

    /// Applies a collection or folder root's headers and authentication.
    fn inheritance(&mut self, root: &Value, name: &str, parent: &Inherited) -> Inherited {
        let request = root.get("request").unwrap_or(&Value::Null);
        let mut headers = self.headers(request, name);
        inherit_headers(&mut headers, &parent.headers);
        let authentication = match request.pointer("/auth/mode").and_then(Value::as_str) {
            None | Some("" | "inherit") => parent.authentication.clone(),
            Some(_) => self.authentication(&request["auth"], name),
        };
        self.scripts(request, name);
        if non_empty(text(root, "docs")).is_some() {
            self.diagnostics
                .note("Collection and folder documentation was not imported", name);
        }
        Inherited {
            authentication,
            headers,
        }
    }

    fn scripts(&mut self, request: &Value, name: &str) {
        let has_script = ["/script/req", "/script/res", "/tests"]
            .iter()
            .any(|pointer| {
                request
                    .pointer(pointer)
                    .and_then(Value::as_str)
                    .is_some_and(|script| !script.trim().is_empty())
            });
        if has_script || !array(request, "assertions").is_empty() {
            self.diagnostics.note(
                "Scripts, tests and assertions aren't run by Wirebolt and were not imported",
                name,
            );
        }
        if request
            .pointer("/vars/res")
            .and_then(Value::as_array)
            .is_some_and(|vars| !vars.is_empty())
        {
            self.diagnostics
                .note("Post-response variables were not imported", name);
        }
    }

    fn request(
        &mut self,
        item: &Value,
        name: &str,
        parent: Option<&str>,
        index: usize,
        inherited: &Inherited,
    ) -> ImportedRequest {
        let request = item.get("request").unwrap_or(&Value::Null);
        let web_socket = text(item, "type").starts_with("ws");
        let params = array(request, "params");
        let path_parameters = params
            .iter()
            .filter(|param| text(param, "type") == "path")
            .map(|param| {
                (
                    text(param, "name").to_owned(),
                    self.value(param, "value", name),
                )
            })
            .collect::<Vec<_>>();
        let query = params
            .iter()
            .filter(|param| text(param, "type") != "path")
            .map(|param| {
                field(
                    text(param, "name"),
                    self.value(param, "value", name),
                    enabled(param),
                )
            })
            .collect::<Vec<_>>();
        let mut url = self.value(request, "url", name);
        if !query.is_empty() {
            url = split_query(&url).0.to_owned();
        }
        let url = substitute_path_parameters(&url, &path_parameters);
        let mut headers = self.headers(request, name);
        inherit_headers(&mut headers, &inherited.headers);
        let authentication = if text(&request["auth"], "mode") == "inherit" {
            inherited.authentication.clone()
        } else {
            self.authentication(&request["auth"], name)
        };
        self.scripts(request, name);
        if request
            .pointer("/vars/req")
            .and_then(Value::as_array)
            .is_some_and(|vars| !vars.is_empty())
        {
            self.diagnostics
                .note("Request variables were not imported", name);
        }
        if web_socket && request.pointer("/body/ws/0").is_some() {
            self.diagnostics
                .note("Saved WebSocket messages were not imported", name);
        }
        ImportedRequest {
            source_id: format!("bruno-request-{}", self.requests.len()),
            name: name.to_owned(),
            group_source_id: parent.map(str::to_owned),
            order: order(index),
            method: if web_socket {
                "GET".to_owned()
            } else {
                non_empty(text(request, "method"))
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
                self.body(request.get("body").unwrap_or(&Value::Null), name)
            },
            web_socket,
            note: text(request, "docs").trim().to_owned(),
        }
    }

    /// Reads a text value and notes Bruno-only dynamic variables.
    fn value(&mut self, object: &Value, key: &str, subject: &str) -> String {
        let value = object.get(key).and_then(scalar).unwrap_or_default();
        if value.contains("{{process.env.") || value.contains("{{$") {
            self.diagnostics.note(
                "Bruno dynamic variables such as {{$randomInt}} and {{process.env.…}} were kept as text and must be defined as environment variables",
                subject,
            );
        }
        value
    }

    fn headers(&mut self, request: &Value, subject: &str) -> Vec<RequestHeader> {
        array(request, "headers")
            .iter()
            .filter(|row| !text(row, "name").is_empty())
            .map(|row| {
                header(
                    text(row, "name"),
                    self.value(row, "value", subject),
                    enabled(row),
                )
            })
            .collect()
    }

    fn body(&mut self, body: &Value, subject: &str) -> RequestBody {
        let content = |key: &str| text(body, key).to_owned();
        match text(body, "mode") {
            "json" => RequestBody::Json {
                value: content("json"),
            },
            "text" => RequestBody::Text {
                content_type: None,
                value: content("text"),
            },
            "xml" => RequestBody::Xml {
                value: content("xml"),
            },
            "sparql" => RequestBody::Text {
                content_type: Some("application/sparql-query".to_owned()),
                value: content("sparql"),
            },
            "graphql" => RequestBody::Json {
                value: graphql_json(body.get("graphql").unwrap_or(&Value::Null)),
            },
            "formUrlEncoded" => RequestBody::FormUrlEncoded {
                fields: array(body, "formUrlEncoded")
                    .iter()
                    .map(|row| {
                        field(
                            text(row, "name"),
                            self.value(row, "value", subject),
                            enabled(row),
                        )
                    })
                    .collect(),
            },
            "multipartForm" => RequestBody::Multipart {
                parts: self.multipart(array(body, "multipartForm"), subject),
            },
            "file" => array(body, "file")
                .iter()
                .find(|file| flag(file, "selected"))
                .or_else(|| array(body, "file").first())
                .map_or(RequestBody::Empty, |file| RequestBody::File {
                    path: self.path(text(file, "filePath"), subject),
                    content_type: non_empty(text(file, "contentType")).map(str::to_owned),
                }),
            "" | "none" => RequestBody::Empty,
            other => {
                self.diagnostics.note(
                    format!("Body type “{other}” isn't supported and was not imported"),
                    subject,
                );
                RequestBody::Empty
            }
        }
    }

    fn multipart(&mut self, rows: &[Value], subject: &str) -> Vec<MultipartPart> {
        let mut parts = Vec::new();
        for row in rows {
            let name = text(row, "name").to_owned();
            let content_type = non_empty(text(row, "contentType")).map(str::to_owned);
            if text(row, "type") == "file" {
                let paths = match row.get("value") {
                    Some(Value::Array(paths)) => {
                        paths.iter().filter_map(Value::as_str).collect::<Vec<_>>()
                    }
                    Some(Value::String(path)) => vec![path.as_str()],
                    _ => Vec::new(),
                };
                for path in paths {
                    let path = self.path(path, subject);
                    parts.push(MultipartPart {
                        id: format!("part-{}", parts.len()),
                        name: name.clone(),
                        kind: MultipartPartKind::File,
                        value: ValueSource::literal(""),
                        file_name: path.rsplit('/').next().map(str::to_owned),
                        file_path: Some(path),
                        content_type: content_type.clone(),
                        enabled: enabled(row),
                    });
                }
            } else {
                parts.push(MultipartPart {
                    id: format!("part-{}", parts.len()),
                    name,
                    kind: MultipartPartKind::Text,
                    value: ValueSource::literal(self.value(row, "value", subject)),
                    file_name: None,
                    file_path: None,
                    content_type,
                    enabled: enabled(row),
                });
            }
        }
        parts
    }

    /// Bruno stores upload paths relative to the collection folder.
    fn path(&mut self, path: &str, subject: &str) -> String {
        if path.starts_with('/') || path.is_empty() {
            return path.to_owned();
        }
        if let Some(base) = self.base_directory {
            return format!("{}/{path}", base.trim_end_matches('/'));
        }
        self.diagnostics.note(
            "Upload file paths are relative to the Bruno collection folder; choose the files again",
            subject,
        );
        path.to_owned()
    }

    fn authentication(&mut self, auth: &Value, subject: &str) -> RequestAuthentication {
        let mode = text(auth, "mode");
        let mut value = |section: &str, key: &str| {
            ValueSource::literal(
                auth.get(section)
                    .map(|section| self.value(section, key, subject))
                    .unwrap_or_default(),
            )
        };
        match mode {
            "" | "none" | "inherit" => RequestAuthentication::None,
            "basic" => RequestAuthentication::Basic {
                username: value("basic", "username"),
                password: value("basic", "password"),
            },
            "bearer" => RequestAuthentication::Bearer {
                token: value("bearer", "token"),
            },
            "apikey" => {
                let key = auth
                    .pointer("/apikey/key")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let placement = if text(&auth["apikey"], "placement") == "queryparams" {
                    ApiKeyPlacement::Query
                } else {
                    ApiKeyPlacement::Header
                };
                RequestAuthentication::ApiKey {
                    placement,
                    name: key,
                    value: value("apikey", "value"),
                }
            }
            "oauth2" => self.oauth2(&auth["oauth2"], subject),
            other => {
                let label = match other {
                    "awsv4" => "AWS Signature v4",
                    "digest" => "Digest",
                    "ntlm" => "NTLM",
                    "oauth1" => "OAuth 1.0",
                    "wsse" => "WSSE",
                    "akamai-edgegrid" => "Akamai EdgeGrid",
                    _ => "This",
                };
                self.diagnostics.note(
                    format!("{label} authentication isn't supported; the request was imported without it"),
                    subject,
                );
                RequestAuthentication::None
            }
        }
    }

    fn oauth2(&mut self, oauth: &Value, subject: &str) -> RequestAuthentication {
        let grant = match text(oauth, "grantType") {
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
        let client_secret = text(oauth, "clientSecret");
        if client_secret.contains("{{") {
            self.diagnostics.note(
                "OAuth 2.0 client secrets that use variables must be entered again in the Auth tab",
                subject,
            );
        }
        let Some((client_secret_reference, access_token_reference)) =
            oauth_references(client_secret, &mut self.secrets)
        else {
            return RequestAuthentication::None;
        };
        RequestAuthentication::Oauth2 {
            configuration: Oauth2Configuration {
                grant,
                authorization_url: text(oauth, "authorizationUrl").to_owned(),
                token_url: text(oauth, "accessTokenUrl").to_owned(),
                client_id: text(oauth, "clientId").to_owned(),
                client_secret_reference,
                scopes: text(oauth, "scope").to_owned(),
                audience: String::new(),
                redirect_uri: non_empty(text(oauth, "callbackUrl"))
                    .unwrap_or(REDIRECT_URI)
                    .to_owned(),
                access_token_reference,
            },
        }
    }

    /// Collection variables are the base layer; each environment overrides them.
    fn environments(
        &mut self,
        collection_name: &str,
        root: &Value,
        base: Vec<Row>,
    ) -> Vec<ImportedEnvironment> {
        let environments = array(root, "environments");
        if environments.is_empty() {
            return if base.is_empty() {
                Vec::new()
            } else {
                vec![environment(collection_name, "", base, &mut self.secrets)]
            };
        }
        environments
            .iter()
            .map(|source| {
                let rows = array(source, "variables")
                    .iter()
                    .filter(|row| !text(row, "name").is_empty())
                    .map(|row| {
                        let secret = flag(row, "secret");
                        let value = row.get("value").and_then(scalar).unwrap_or_default();
                        if secret && value.is_empty() {
                            self.diagnostics.note(
                                "Bruno doesn't export secret values; set them in Configure Environments",
                                text(row, "name"),
                            );
                        }
                        Row {
                            key: text(row, "name").to_owned(),
                            value,
                            enabled: enabled(row),
                            secret,
                        }
                    })
                    .collect();
                let mut merged = base.clone();
                overlay(&mut merged, rows);
                let name = non_empty(text(source, "name")).unwrap_or("Environment");
                environment(collection_name, name, merged, &mut self.secrets)
            })
            .collect()
    }
}

fn collection_variables(rows: Option<&Value>) -> Vec<Row> {
    rows.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| !text(row, "name").is_empty())
        .map(|row| {
            Row::literal(
                text(row, "name"),
                row.get("value").and_then(scalar).unwrap_or_default(),
                enabled(row),
            )
        })
        .collect()
}

fn enabled(row: &Value) -> bool {
    row.get("enabled").and_then(Value::as_bool).unwrap_or(true)
}

/// Bruno keeps the query and its variables as separate texts.
fn graphql_json(graphql: &Value) -> String {
    let query = text(graphql, "query");
    let variables = text(graphql, "variables").trim();
    let variables = if variables.is_empty() {
        Value::Null
    } else {
        serde_json::from_str::<Value>(variables)
            .unwrap_or_else(|_| Value::String(variables.to_owned()))
    };
    let mut document = json!({ "query": query });
    if !variables.is_null() {
        document["variables"] = variables;
    }
    serde_json::to_string_pretty(&document).unwrap_or_default()
}
