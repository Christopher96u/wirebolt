use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

use super::{ImportError, ImportedCollection, ImportedGroup, ImportedRequest};
use crate::{
    MultipartPart, MultipartPartKind, RequestAuthentication, RequestBody, RequestHeader,
    RequestValueField, ValueSource,
};

// Exported paths describe hierarchy only. Import never opens these paths.
pub(super) fn parse(root: &Value) -> Result<ImportedCollection, ImportError> {
    parse_named(root, None)
}

pub(super) fn parse_named(
    root: &Value,
    file_name: Option<&str>,
) -> Result<ImportedCollection, ImportError> {
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
    let collection = folders.iter().copied().find(|node| {
        if file_name.is_some() {
            return false;
        }
        let path = node["path"].as_str().unwrap_or_default();
        !folders.iter().any(|parent| {
            parent["path"]
                .as_str()
                .is_some_and(|parent| path.starts_with(&format!("{parent}/")))
        })
    });
    let collection_id = collection.and_then(|node| node["uuid"].as_str());
    let parent = |node: &Value| -> Option<String> {
        let path = node["path"].as_str()?;
        folders
            .iter()
            .filter(|folder| {
                folder["path"]
                    .as_str()
                    .is_some_and(|folder| path.starts_with(&format!("{folder}/")))
            })
            .max_by_key(|folder| folder["path"].as_str().map_or(0, str::len))
            .and_then(|folder| folder["uuid"].as_str())
            .filter(|id| Some(*id) != collection_id)
            .map(str::to_owned)
    };
    let mut result = ImportedCollection {
        name: file_name
            .or_else(|| collection.and_then(|node| node["name"].as_str()))
            .or_else(|| root["workspaceName"].as_str())
            .unwrap_or("Imported Collection")
            .to_owned(),
        groups: Vec::new(),
        requests: Vec::new(),
        warnings: Vec::new(),
    };
    for (index, node) in nodes.iter().enumerate() {
        let source_id = node["uuid"]
            .as_str()
            .ok_or_else(|| ImportError::new("legacy node has no identifier"))?
            .to_owned();
        let name = node["name"].as_str().unwrap_or("Untitled").to_owned();
        let sibling_index = folders
            .iter()
            .find_map(|folder| {
                folder["folderMetadata"]["childIds"]
                    .as_array()?
                    .iter()
                    .position(|id| id.as_str() == Some(source_id.as_str()))
            })
            .unwrap_or(index);
        let order = i64::try_from(sibling_index).unwrap_or(i64::MAX);
        match node["kind"].as_str() {
            Some("folder") if Some(source_id.as_str()) != collection_id => {
                result.groups.push(ImportedGroup {
                    source_id,
                    name,
                    parent_source_id: parent(node),
                    order,
                });
            }
            Some("request" | "websocketRequest") => {
                result
                    .requests
                    .push(request(node, source_id, name, parent(node), order)?);
            }
            Some("folder") => {}
            _ => return Err(ImportError::new("unsupported legacy node type")),
        }
    }
    if result.requests.is_empty() && folders.is_empty() {
        return Err(ImportError::new("legacy collection has no requests"));
    }
    Ok(result)
}

fn request(
    node: &Value,
    source_id: String,
    name: String,
    group_source_id: Option<String>,
    order: i64,
) -> Result<ImportedRequest, ImportError> {
    let request = &node["flow"]["request"];
    let mut url = request["url"]
        .as_str()
        .ok_or_else(|| ImportError::new("legacy request has no URL"))?
        .to_owned();
    let query = rows(request.get("queries"));
    if !query.is_empty()
        && let Ok(mut parsed) = url::Url::parse(&url)
    {
        parsed.set_query(None);
        url = parsed.into();
    }
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
    let authentication = if let Some(native) = node.pointer("/wirebolt/authentication") {
        serde_json::from_value(native.clone())
            .map_err(|_| ImportError::new("invalid Wirebolt authentication metadata"))?
    } else {
        authentication(&request["auth"])?
    };
    let headers = rows(request.get("headers"))
        .into_iter()
        .filter(|field| {
            matches!(authentication, RequestAuthentication::None)
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
    Ok(ImportedRequest {
        source_id,
        name,
        group_source_id,
        order,
        method,
        url,
        headers,
        query,
        authentication,
        body: body(&request["body"]["contentType"])?,
        web_socket: node["kind"] == "websocketRequest",
        note: node["note"].as_str().unwrap_or_default().to_owned(),
    })
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
