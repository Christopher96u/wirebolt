use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

use super::{
    ImportError, ImportedCollection, ImportedEnvironment, ImportedGroup, ImportedRequest,
    ImportedRequestSettings, ImportedWorkspace,
};
use crate::{
    MultipartPart, MultipartPartKind, RequestAuthentication, RequestBody, RequestHeader,
    RequestValueField, ValueSource,
    export_engine::{NativeEnvironment, NativeRequest},
};

// Exported paths describe hierarchy only. Import never opens these paths; it
// only checks whether referenced upload files exist so it can warn.
pub(super) fn parse(root: &Value) -> Result<ImportedCollection, ImportError> {
    parse_named(root, None)
}

/// Imports the document as one collection. A single exported collection keeps
/// its own name instead of becoming a folder inside a file-named collection.
pub(super) fn parse_named(
    root: &Value,
    file_name: Option<&str>,
) -> Result<ImportedCollection, ImportError> {
    let export = Export::new(root)?;
    let roots = &export.collection_roots;
    let collection = match roots.as_slice() {
        [collection] if export.loose().next().is_none() => export.collection(
            name(collection),
            Some(collection),
            export.members(collection),
            &mut BTreeMap::new(),
        )?,
        _ => export.collection(
            export.fallback_name(file_name),
            None,
            export.nodes.iter().enumerate(),
            &mut BTreeMap::new(),
        )?,
    };
    if collection.requests.is_empty() && export.folders.is_empty() {
        return Err(ImportError::new("legacy collection has no requests"));
    }
    Ok(collection)
}

/// Imports every exported collection separately, plus requests outside any
/// collection (named after the file) and the exported environments.
pub(super) fn parse_workspace(
    root: &Value,
    file_name: Option<&str>,
) -> Result<ImportedWorkspace, ImportError> {
    let export = Export::new(root)?;
    let roots = &export.collection_roots;
    let mut request_settings = BTreeMap::new();
    let mut collections = Vec::new();
    if export.loose().next().is_some() || roots.is_empty() {
        collections.push(export.collection(
            export.fallback_name(file_name),
            None,
            export.loose(),
            &mut request_settings,
        )?);
    }
    for &collection in roots {
        collections.push(export.collection(
            name(collection),
            Some(collection),
            export.members(collection),
            &mut request_settings,
        )?);
    }
    if export.folders.is_empty() && collections.iter().all(|c| c.requests.is_empty()) {
        return Err(ImportError::new("legacy collection has no requests"));
    }
    let environments = match root.pointer("/wirebolt/environments") {
        Some(value) => serde_json::from_value::<Vec<NativeEnvironment>>(value.clone())
            .map_err(|_| ImportError::new("invalid Wirebolt environment metadata"))?
            .into_iter()
            .map(|environment| ImportedEnvironment {
                name: environment.name,
                global: environment.global,
                variables: environment.variables,
            })
            .collect(),
        None => Vec::new(),
    };
    Ok(ImportedWorkspace {
        collections,
        environments,
        request_settings,
    })
}

fn name(node: &Value) -> String {
    node["name"].as_str().unwrap_or("Untitled").to_owned()
}

fn path(node: &Value) -> &str {
    node["path"].as_str().unwrap_or_default()
}

/// Top-level folders that stand for whole collections: the ones Wirebolt
/// marks, or else a lone top-level folder, as in older collection exports.
fn collection_roots<'a>(nodes: &'a [Value], folders: &[&'a Value]) -> Vec<&'a Value> {
    let is_top_level = |node: &Value| !folders.iter().any(|folder| is_inside(node, folder));
    let marked: Vec<_> = folders
        .iter()
        .copied()
        .filter(|folder| folder.pointer("/wirebolt/collection") == Some(&Value::Bool(true)))
        .filter(|folder| is_top_level(folder))
        .collect();
    if !marked.is_empty() {
        return marked;
    }
    let mut top_level = nodes.iter().filter(|node| is_top_level(node));
    match (top_level.next(), top_level.next()) {
        (Some(folder), None) if folder["kind"] == "folder" => vec![folder],
        _ => Vec::new(),
    }
}

fn is_inside(node: &Value, folder: &Value) -> bool {
    let folder = path(folder);
    !folder.is_empty()
        && path(node)
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}

struct Export<'a> {
    root: &'a Value,
    nodes: &'a [Value],
    folders: Vec<&'a Value>,
    /// Top-level folders that stand for whole collections.
    collection_roots: Vec<&'a Value>,
    /// Position of each node among its folder's children.
    sibling_positions: HashMap<&'a str, usize>,
}

impl<'a> Export<'a> {
    fn new(root: &'a Value) -> Result<Self, ImportError> {
        if root.get("version").and_then(Value::as_u64) != Some(1) {
            return Err(ImportError::new("unsupported legacy collection version"));
        }
        let nodes = root
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| ImportError::new("legacy collection has no nodes"))?;
        let folders: Vec<_> = nodes
            .iter()
            .filter(|node| node["kind"] == "folder")
            .collect();
        let mut sibling_positions = HashMap::new();
        for folder in &folders {
            let children = folder["folderMetadata"]["childIds"].as_array();
            for (position, child) in children.into_iter().flatten().enumerate() {
                if let Some(child) = child.as_str() {
                    sibling_positions.entry(child).or_insert(position);
                }
            }
        }
        let collection_roots = collection_roots(nodes, &folders);
        Ok(Self {
            root,
            nodes,
            folders,
            collection_roots,
            sibling_positions,
        })
    }

    fn fallback_name(&self, file_name: Option<&str>) -> String {
        file_name
            .or_else(|| self.root["workspaceName"].as_str())
            .unwrap_or("Imported Collection")
            .to_owned()
    }

    fn members(&self, collection: &'a Value) -> impl Iterator<Item = (usize, &'a Value)> {
        self.nodes
            .iter()
            .enumerate()
            .filter(move |(_, node)| is_inside(node, collection))
    }

    /// Nodes outside every collection root.
    fn loose(&self) -> impl Iterator<Item = (usize, &'a Value)> {
        self.nodes.iter().enumerate().filter(|(_, node)| {
            !self
                .collection_roots
                .iter()
                .any(|root| std::ptr::eq(*root, *node) || is_inside(node, root))
        })
    }

    /// Nearest enclosing folder below the collection root.
    fn parent(&self, node: &Value, collection: Option<&Value>) -> Option<String> {
        self.folders
            .iter()
            .filter(|folder| is_inside(node, folder))
            .max_by_key(|folder| path(folder).len())
            .filter(|folder| !collection.is_some_and(|root| std::ptr::eq(root, **folder)))
            .and_then(|folder| folder["uuid"].as_str())
            .map(str::to_owned)
    }

    fn collection(
        &self,
        name: String,
        root: Option<&Value>,
        members: impl Iterator<Item = (usize, &'a Value)>,
        request_settings: &mut BTreeMap<String, ImportedRequestSettings>,
    ) -> Result<ImportedCollection, ImportError> {
        let mut result = ImportedCollection {
            name,
            groups: Vec::new(),
            requests: Vec::new(),
            warnings: Vec::new(),
        };
        for (index, node) in members {
            let source_id = node["uuid"]
                .as_str()
                .ok_or_else(|| ImportError::new("legacy node has no identifier"))?
                .to_owned();
            let sibling_index = self
                .sibling_positions
                .get(source_id.as_str())
                .copied()
                .unwrap_or(index);
            let order = i64::try_from(sibling_index).unwrap_or(i64::MAX);
            let parent_source_id = self.parent(node, root);
            match node["kind"].as_str() {
                Some("folder") => result.groups.push(ImportedGroup {
                    source_id,
                    name: self::name(node),
                    parent_source_id,
                    order,
                }),
                Some("request" | "websocketRequest") => {
                    let (request, settings) = request(node, source_id, parent_source_id, order)?;
                    warn_about_missing_uploads(&request, &mut result.warnings);
                    if let Some(settings) = settings {
                        request_settings.insert(request.source_id.clone(), settings);
                    }
                    result.requests.push(request);
                }
                _ => return Err(ImportError::new("unsupported legacy node type")),
            }
        }
        Ok(result)
    }
}

fn warn_about_missing_uploads(request: &ImportedRequest, warnings: &mut Vec<String>) {
    let paths: Vec<&str> = match &request.body {
        RequestBody::File { path, .. } => vec![path.as_str()],
        RequestBody::Multipart { parts } => parts
            .iter()
            .filter(|part| part.kind == MultipartPartKind::File)
            .filter_map(|part| part.file_path.as_deref())
            .collect(),
        _ => Vec::new(),
    };
    for path in paths {
        if !path.is_empty() && !Path::new(path).exists() {
            warnings.push(format!(
                "“{}” uploads {path}, which does not exist on this Mac. Choose the file again before sending.",
                request.name
            ));
        }
    }
}

fn request(
    node: &Value,
    source_id: String,
    group_source_id: Option<String>,
    order: i64,
) -> Result<(ImportedRequest, Option<ImportedRequestSettings>), ImportError> {
    let request = &node["flow"]["request"];
    let url = request["url"]
        .as_str()
        .ok_or_else(|| ImportError::new("legacy request has no URL"))?;
    let method = request["method"]
        .as_object()
        .and_then(|method| method.iter().next())
        .map_or_else(
            || "GET".to_owned(),
            |(name, value)| {
                if name == "custom" {
                    value["_0"].as_str().unwrap_or("GET").to_owned()
                } else {
                    name.to_ascii_uppercase()
                }
            },
        );
    let method = if node["kind"] == "websocketRequest" {
        "GET".to_owned()
    } else {
        method
    };
    let native = node
        .get("wirebolt")
        .map(|native| serde_json::from_value::<NativeRequest>(native.clone()))
        .transpose()
        .map_err(|_| ImportError::new("invalid Wirebolt request metadata"))?;
    let (authentication, definition) = match native {
        Some(native) => (native.authentication, native.request),
        None => (authentication(&request["auth"])?, None),
    };
    let mut imported = ImportedRequest {
        source_id,
        name: name(node),
        group_source_id,
        order,
        method,
        url: url.to_owned(),
        headers: Vec::new(),
        query: Vec::new(),
        authentication,
        body: RequestBody::Empty,
        web_socket: node["kind"] == "websocketRequest",
        note: node["note"].as_str().unwrap_or_default().to_owned(),
    };
    // Wirebolt's own definition is authoritative; the portable fields are a
    // lossy mirror of it for other readers.
    if let Some(definition) = definition {
        imported.headers = definition.headers;
        imported.query = definition.query;
        imported.body = definition.body;
        let settings = ImportedRequestSettings {
            proxy_override: definition.proxy_override,
            transport: definition.transport,
            inherits_workspace_transport: definition.inherits_workspace_transport,
        };
        return Ok((imported, Some(settings)));
    }
    imported.query = rows(request.get("queries"));
    imported.url = without_repeated_query(url, &imported.query);
    imported.headers = rows(request.get("headers"))
        .into_iter()
        .filter(|field| {
            matches!(imported.authentication, RequestAuthentication::None)
                || !field.name.eq_ignore_ascii_case("authorization")
        })
        .map(|field| RequestHeader {
            id: field.id,
            name: field.name,
            value: field.value,
            enabled: field.enabled,
            sensitive: field.sensitive,
        })
        .collect();
    imported.body = body(&request["body"]["contentType"])?;
    Ok((imported, None))
}

/// Legacy documents repeat enabled query rows in the URL. Drops only the URL
/// pairs that a row already describes, so other query text is never lost.
fn without_repeated_query(url: &str, rows: &[RequestValueField]) -> String {
    let Some((base, rest)) = url.split_once('?') else {
        return url.to_owned();
    };
    let (query, fragment) = match rest.split_once('#') {
        Some((query, fragment)) => (query, Some(fragment)),
        None => (rest, None),
    };
    let mut unmatched: Vec<_> = rows.iter().collect();
    let kept: Vec<_> = query
        .split('&')
        .filter(|segment| !segment.is_empty())
        .filter(|segment| {
            let Some((name, value)) = url::form_urlencoded::parse(segment.as_bytes()).next() else {
                return true;
            };
            let repeated = unmatched.iter().position(|row| {
                row.name == name && matches!(&row.value, ValueSource::Literal(v) if *v == value)
            });
            repeated.map(|index| unmatched.remove(index)).is_none()
        })
        .collect();
    let mut result = base.to_owned();
    if !kept.is_empty() {
        result.push('?');
        result.push_str(&kept.join("&"));
    }
    if let Some(fragment) = fragment {
        result.push('#');
        result.push_str(fragment);
    }
    result
}

fn authentication(value: &Value) -> Result<RequestAuthentication, ImportError> {
    Ok(match value["mode"].as_str() {
        None | Some("none") => RequestAuthentication::None,
        Some("basic") => RequestAuthentication::Basic {
            username: ValueSource::literal(value["basic"]["username"].as_str().unwrap_or_default()),
            password: ValueSource::literal(value["basic"]["password"].as_str().unwrap_or_default()),
        },
        Some("bearer") => RequestAuthentication::Bearer {
            token: ValueSource::literal(value["bearer"]["token"].as_str().unwrap_or_default()),
        },
        _ => return Err(ImportError::new("unsupported legacy authentication type")),
    })
}

fn rows(value: Option<&Value>) -> Vec<RequestValueField> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| {
            !(row["key"].as_str().unwrap_or_default().is_empty()
                && row["value"].as_str().unwrap_or_default().is_empty())
        })
        .enumerate()
        .map(|(index, row)| RequestValueField {
            id: format!("legacy-field-{index}"),
            name: row["key"].as_str().unwrap_or_default().to_owned(),
            value: ValueSource::literal(row["value"].as_str().unwrap_or_default()),
            enabled: row["isEnabled"].as_bool().unwrap_or(true),
            sensitive: false,
        })
        .collect()
}

fn body(value: &Value) -> Result<RequestBody, ImportError> {
    let Some((kind, payload)) = value.as_object().and_then(|object| object.iter().next()) else {
        return Ok(RequestBody::Empty);
    };
    let text = || payload["_0"].as_str().unwrap_or_default().to_owned();
    Ok(match kind.as_str() {
        "none" => RequestBody::Empty,
        "fileURL" => RequestBody::File {
            path: text(),
            content_type: None,
        },
        "json" => RequestBody::Json { value: text() },
        "formURLEncoded" => RequestBody::FormUrlEncoded {
            fields: rows(payload.get("_0")),
        },
        "xml" => RequestBody::Xml { value: text() },
        "html" => RequestBody::Html { value: text() },
        "text" | "raw" | "rawText" => RequestBody::Text {
            content_type: None,
            value: text(),
        },
        "base64" => RequestBody::Text {
            content_type: Some("application/octet-stream".into()),
            value: text(),
        },
        "hex" => RequestBody::Text {
            content_type: Some("application/octet-stream; encoding=hex".into()),
            value: text(),
        },
        "multipartFormData" => {
            let mut parts = Vec::new();
            for part in payload["_0"]["parts"].as_array().into_iter().flatten() {
                let bytes = STANDARD
                    .decode(part["value"].as_str().unwrap_or_default())
                    .map_err(|_| ImportError::new("invalid legacy multipart encoding"))?;
                let (kind, value) = match String::from_utf8(bytes) {
                    Ok(value) => (MultipartPartKind::Text, value),
                    Err(error) => (
                        MultipartPartKind::Binary,
                        STANDARD.encode(error.into_bytes()),
                    ),
                };
                parts.push(MultipartPart {
                    id: part["id"].as_str().unwrap_or_default().to_owned(),
                    name: part["part"].as_str().unwrap_or_default().to_owned(),
                    kind,
                    value: ValueSource::literal(value),
                    file_path: None,
                    file_name: part["fileName"].as_str().map(str::to_owned),
                    content_type: part["contentType"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                    enabled: part["isEnabled"].as_bool().unwrap_or(true),
                });
            }
            RequestBody::Multipart { parts }
        }
        _ => return Err(ImportError::new("unsupported legacy body type")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn websocket_kind_and_exported_note_survive_the_saved_request_conversion() {
        let fixture = json!({"version":1,"nodes":[
            {"uuid":"root","path":"/export/root","kind":"folder","name":"API"},
            {"uuid":"socket","path":"/export/root/socket.request","kind":"websocketRequest","name":"Echo","note":"Parity note: café 東京 🚀\nSecond line","flow":{"request":{
                "url":"{{socketURL}}", "info":{"start":1,"end":2}, "body":{"contentType":{"text":{"_0":"hello"}}}
            }}}
        ]});
        let imported = parse(&fixture).unwrap().requests.remove(0);
        let request = imported.into_request(crate::DocumentId::new("socket").unwrap(), None);
        assert!(request.web_socket);
        assert_eq!(request.note, "Parity note: café 東京 🚀\nSecond line");
        assert_eq!(request.url, "{{socketURL}}");
    }

    #[test]
    fn native_file_and_hex_exports_preserve_the_source_without_opening_files() {
        assert_eq!(
            body(&json!({"fileURL":{"_0":"/missing/export/upload.bin"}})).unwrap(),
            RequestBody::File {
                path: "/missing/export/upload.bin".into(),
                content_type: None
            }
        );
        assert_eq!(
            body(&json!({"hex":{"_0":"00 01 FF FE 43 61 66 C3 A9"}})).unwrap(),
            RequestBody::Text {
                content_type: Some("application/octet-stream; encoding=hex".into()),
                value: "00 01 FF FE 43 61 66 C3 A9".into()
            }
        );
    }

    #[test]
    fn imports_exported_nodes_without_query_duplication_or_filesystem_access() {
        let fixture = json!({"version":1,"workspaceName":"Fixture","nodes":[
            {"uuid":"root","path":"/not-a-real-folder/root","kind":"folder","name":"API"},
            {"uuid":"nested","path":"/not-a-real-folder/root/nested","kind":"folder","name":"Nested"},
            {"uuid":"request","path":"/not-a-real-folder/root/nested/request.request","kind":"request","name":"Echo","flow":{"request":{
                "url":"http://127.0.0.1/echo?q=1", "method":{"patch":{}},
                "queries":[{"key":"q","value":"1","isEnabled":true},{"key":"hidden","value":"2","isEnabled":false}],
                "headers":[{"key":"Accept","value":"application/json","isEnabled":false}],
                "body":{"contentType":{"json":{"_0":"{\"n\":1}"}}}
            }}}
        ]});
        let imported = parse(&fixture).unwrap();
        assert_eq!(imported.name, "API");
        assert_eq!(imported.groups.len(), 1);
        assert_eq!(imported.groups[0].parent_source_id, None);
        let request = &imported.requests[0];
        assert_eq!(request.group_source_id.as_deref(), Some("nested"));
        assert_eq!(request.method, "PATCH");
        assert_eq!(request.url, "http://127.0.0.1/echo");
        assert_eq!(request.query.len(), 2);
        assert!(!request.query[1].enabled);
        assert!(!request.headers[0].enabled);
        assert_eq!(
            request.body,
            RequestBody::Json {
                value: "{\"n\":1}".into()
            }
        );
    }

    #[test]
    fn url_query_text_is_kept_unless_a_row_repeats_it() {
        let rows = [
            RequestValueField::enabled("q", ValueSource::literal("a&b")),
            RequestValueField::enabled("page", ValueSource::literal("2")),
        ];
        assert_eq!(
            without_repeated_query(
                "https://api.example.test/items?fixed=1&q=a%26b&page=3#top",
                &rows
            ),
            "https://api.example.test/items?fixed=1&page=3#top"
        );
        assert_eq!(
            without_repeated_query("{{baseUrl}}/items?q=a%26b&page=2", &rows),
            "{{baseUrl}}/items"
        );
        assert_eq!(
            without_repeated_query("{{baseUrl}}/items?view=compact", &[]),
            "{{baseUrl}}/items?view=compact"
        );
    }

    #[test]
    fn urlencoded_export_preserves_unicode_disabled_rows_and_omits_the_ghost_row() {
        let imported = body(&json!({"formURLEncoded":{"_0":[
            {"key":"name","value":"café 東京","isEnabled":true},
            {"key":"disabled","value":"keep","isEnabled":false},
            {"key":"","value":"","isEnabled":true}
        ]}}))
        .unwrap();
        let RequestBody::FormUrlEncoded { fields } = imported else {
            panic!("expected form")
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].value, ValueSource::literal("café 東京"));
        assert!(!fields[1].enabled);
    }

    #[test]
    fn multipart_export_decodes_embedded_text_and_preserves_metadata() {
        let body = body(&json!({"multipartFormData":{"_0":{"parts":[{"id":"part","part":"metadata","value":"eyJuIjoxfQ==","contentType":"application/json","fileName":"data.json"}]}}})).unwrap();
        let RequestBody::Multipart { parts } = body else {
            panic!("expected multipart")
        };
        assert_eq!(parts[0].value, ValueSource::literal("{\"n\":1}"));
        assert_eq!(parts[0].file_name.as_deref(), Some("data.json"));
        assert_eq!(parts[0].content_type.as_deref(), Some("application/json"));
    }

    #[test]
    fn rejects_unknown_versions_and_body_types_before_importing_anything() {
        assert!(parse(&json!({"version":2,"nodes":[]})).is_err());
        assert!(body(&json!({"futureBody":{"_0":"must not disappear"}})).is_err());
    }
}
