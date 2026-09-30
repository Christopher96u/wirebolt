use std::{error::Error, fmt};

use serde_json::Value;

mod legacy_v1;
mod workspace;

pub use workspace::{ImportedEnvironment, ImportedRequestSettings, ImportedWorkspace};

use crate::{
    MultipartPart, MultipartPartKind, Request, RequestAuthentication, RequestBody, RequestHeader,
    RequestValueField, ValueSource,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportFormat {
    Curl,
    Har,
    LegacyWorkspaceV1,
    PostmanV2,
}

impl ImportFormat {
    /// Parses the stable bridge token for one supported import format.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] for unknown format tokens.
    pub fn parse(value: &str) -> Result<Self, ImportError> {
        match value {
            "curl" => Ok(Self::Curl),
            "har" => Ok(Self::Har),
            "legacy_workspace_v1" => Ok(Self::LegacyWorkspaceV1),
            "postman_v2" => Ok(Self::PostmanV2),
            _ => Err(ImportError::new("unsupported import format")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImportedCollection {
    pub name: String,
    pub groups: Vec<ImportedGroup>,
    pub requests: Vec<ImportedRequest>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ImportedGroup {
    pub source_id: String,
    pub name: String,
    pub parent_source_id: Option<String>,
    pub order: i64,
}

#[derive(Clone, Debug)]
pub struct ImportedRequest {
    pub source_id: String,
    pub name: String,
    pub group_source_id: Option<String>,
    pub order: i64,
    pub method: String,
    pub url: String,
    pub headers: Vec<RequestHeader>,
    pub query: Vec<RequestValueField>,
    pub authentication: RequestAuthentication,
    pub body: RequestBody,
    pub web_socket: bool,
    pub note: String,
}

impl ImportedRequest {
    #[must_use]
    pub fn into_request(
        self,
        id: crate::DocumentId,
        group_id: Option<crate::DocumentId>,
    ) -> Request {
        let mut request = Request::new(id, self.name, self.method, self.url);
        request.group_id = group_id;
        request.order = self.order;
        request.headers = self.headers;
        request.query = self.query;
        request.authentication = self.authentication;
        request.body = self.body;
        request.web_socket = self.web_socket;
        request.note = self.note;
        request
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ImportError {
    reason: &'static str,
}

impl ImportError {
    const fn new(reason: &'static str) -> Self {
        Self { reason }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.reason)
    }
}

impl Error for ImportError {}

#[derive(Debug, Default)]
pub struct ImportEngine;

impl ImportEngine {
    /// Imports a file while retaining its top-level folders under the filename.
    ///
    /// # Errors
    /// Returns [`ImportError`] for invalid input without including source contents.
    pub fn parse_file(
        format: ImportFormat,
        source: &str,
        name: &str,
    ) -> Result<ImportedCollection, ImportError> {
        if format == ImportFormat::LegacyWorkspaceV1 {
            let root: Value = serde_json::from_str(source)
                .map_err(|_| ImportError::new("legacy workspace JSON is invalid"))?;
            if root.get("nodes").is_some() {
                return legacy_v1::parse_named(&root, Some(name));
            }
        }
        Self::parse(format, source)
    }

    /// Imports a file with everything it describes: a Wirebolt workspace export
    /// yields one collection per exported collection plus its environments;
    /// other formats yield the single collection of [`Self::parse_file`].
    ///
    /// # Errors
    /// Returns [`ImportError`] for invalid input without including source contents.
    pub fn parse_workspace_file(
        format: ImportFormat,
        source: &str,
        name: &str,
    ) -> Result<ImportedWorkspace, ImportError> {
        if format == ImportFormat::LegacyWorkspaceV1 {
            let root: Value = serde_json::from_str(source)
                .map_err(|_| ImportError::new("legacy workspace JSON is invalid"))?;
            if root.get("nodes").is_some() {
                return legacy_v1::parse_workspace(&root, Some(name));
            }
        }
        Self::parse_file(format, source, name).map(ImportedWorkspace::from)
    }

    /// Parses a source completely before returning any documents to storage.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] without including source contents.
    pub fn parse(format: ImportFormat, source: &str) -> Result<ImportedCollection, ImportError> {
        match format {
            ImportFormat::Curl => parse_curl(source),
            ImportFormat::Har => parse_har(source),
            ImportFormat::LegacyWorkspaceV1 => parse_legacy_workspace(source),
            ImportFormat::PostmanV2 => parse_postman(source),
        }
    }
}

fn parse_curl(source: &str) -> Result<ImportedCollection, ImportError> {
    let tokens = shell_tokens(source)?;
    if tokens.first().is_none_or(|token| token != "curl") {
        return Err(ImportError::new("cURL command must start with curl"));
    }
    let mut method = "GET".to_owned();
    let mut url = None;
    let mut headers = Vec::new();
    let mut body = None;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "-X" | "--request" => {
                index += 1;
                method = tokens
                    .get(index)
                    .ok_or_else(|| ImportError::new("missing cURL method"))?
                    .to_uppercase();
            }
            "-H" | "--header" => {
                index += 1;
                let header = tokens
                    .get(index)
                    .ok_or_else(|| ImportError::new("missing cURL header"))?;
                let (name, value) = header
                    .split_once(':')
                    .ok_or_else(|| ImportError::new("invalid cURL header"))?;
                headers.push(RequestHeader::enabled(
                    name.trim(),
                    ValueSource::literal(value.trim()),
                ));
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                index += 1;
                let value = tokens
                    .get(index)
                    .ok_or_else(|| ImportError::new("missing cURL body"))?
                    .clone();
                body = Some(
                    if serde_json::from_str::<serde::de::IgnoredAny>(&value).is_ok() {
                        RequestBody::Json { value }
                    } else {
                        RequestBody::Text {
                            content_type: None,
                            value,
                        }
                    },
                );
                if method == "GET" {
                    "POST".clone_into(&mut method);
                }
            }
            token if token.starts_with('-') => {}
            token => url = Some(token.to_owned()),
        }
        index += 1;
    }
    let url = url.ok_or_else(|| ImportError::new("cURL command has no URL"))?;
    Ok(ImportedCollection {
        name: "Imported cURL".to_owned(),
        groups: Vec::new(),
        requests: vec![ImportedRequest {
            source_id: "curl-request".to_owned(),
            name: "Imported Request".to_owned(),
            group_source_id: None,
            order: 0,
            method,
            url,
            headers,
            query: Vec::new(),
            authentication: RequestAuthentication::None,
            web_socket: false,
            note: String::new(),
            body: body.unwrap_or(RequestBody::Empty),
        }],
        warnings: Vec::new(),
    })
}

fn parse_har(source: &str) -> Result<ImportedCollection, ImportError> {
    let root: Value =
        serde_json::from_str(source).map_err(|_| ImportError::new("HAR JSON is invalid"))?;
    let entries = root
        .pointer("/log/entries")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::new("HAR has no entries"))?;
    let requests = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let request = entry.get("request")?;
            let url = request.get("url")?.as_str()?.to_owned();
            let method = request
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET")
                .to_owned();
            let headers = request
                .get("headers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(header_from_name_value)
                .collect();
            let body = request
                .pointer("/postData/text")
                .and_then(Value::as_str)
                .map_or(RequestBody::Empty, |value| {
                    let mime = request
                        .pointer("/postData/mimeType")
                        .and_then(Value::as_str);
                    if mime.is_some_and(|mime| mime.contains("json")) {
                        RequestBody::Json {
                            value: value.to_owned(),
                        }
                    } else {
                        RequestBody::Text {
                            content_type: mime.map(str::to_owned),
                            value: value.to_owned(),
                        }
                    }
                });
            Some(ImportedRequest {
                source_id: format!("har-{index}"),
                name: format!(
                    "{method} {}",
                    url.split('/').next_back().unwrap_or("Request")
                ),
                group_source_id: None,
                order: i64::try_from(index).unwrap_or(i64::MAX),
                method,
                url,
                headers,
                query: Vec::new(),
                authentication: RequestAuthentication::None,
                web_socket: false,
                note: String::new(),
                body,
            })
        })
        .collect::<Vec<_>>();
    if requests.is_empty() {
        return Err(ImportError::new("HAR has no importable requests"));
    }
    Ok(ImportedCollection {
        name: "Imported HAR".to_owned(),
        groups: Vec::new(),
        requests,
        warnings: Vec::new(),
    })
}

fn parse_postman(source: &str) -> Result<ImportedCollection, ImportError> {
    let root: Value =
        serde_json::from_str(source).map_err(|_| ImportError::new("Postman JSON is invalid"))?;
    let name = root
        .pointer("/info/name")
        .and_then(Value::as_str)
        .unwrap_or("Imported Postman")
        .to_owned();
    let items = root
        .get("item")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::new("Postman collection has no items"))?;
    let mut collection = ImportedCollection {
        name,
        groups: Vec::new(),
        requests: Vec::new(),
        warnings: Vec::new(),
    };
    parse_postman_items(items, None, &mut collection);
    if collection.requests.is_empty() {
        return Err(ImportError::new("Postman collection has no requests"));
    }
    Ok(collection)
}

fn parse_postman_items(items: &[Value], parent: Option<&str>, output: &mut ImportedCollection) {
    for (index, item) in items.iter().enumerate() {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Untitled");
        if let Some(children) = item.get("item").and_then(Value::as_array) {
            let source_id = format!("group-{}-{}", output.groups.len(), slug(name));
            output.groups.push(ImportedGroup {
                source_id: source_id.clone(),
                name: name.to_owned(),
                parent_source_id: parent.map(str::to_owned),
                order: i64::try_from(index).unwrap_or(i64::MAX),
            });
            parse_postman_items(children, Some(&source_id), output);
            continue;
        }
        let Some(request) = item.get("request") else {
            continue;
        };
        let Some(url) = postman_url(request.get("url")) else {
            output.warnings.push(format!("Skipped {name}: missing URL"));
            continue;
        };
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_owned();
        let headers = request
            .get("header")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|header| {
                !header
                    .get("disabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .filter_map(|header| {
                Some(RequestHeader::enabled(
                    header.get("key")?.as_str()?,
                    ValueSource::literal(header.get("value").and_then(Value::as_str).unwrap_or("")),
                ))
            })
            .collect();
        output.requests.push(ImportedRequest {
            source_id: format!("request-{}-{}", output.requests.len(), slug(name)),
            name: name.to_owned(),
            group_source_id: parent.map(str::to_owned),
            order: i64::try_from(index).unwrap_or(i64::MAX),
            method,
            url,
            headers,
            query: Vec::new(),
            authentication: RequestAuthentication::None,
            web_socket: false,
            note: String::new(),
            body: postman_body(request.get("body")),
        });
    }
}

fn postman_body(body: Option<&Value>) -> RequestBody {
    let Some(body) = body else {
        return RequestBody::Empty;
    };
    match body.get("mode").and_then(Value::as_str) {
        Some("raw") => {
            let value = body
                .get("raw")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            if serde_json::from_str::<serde::de::IgnoredAny>(&value).is_ok() {
                RequestBody::Json { value }
            } else {
                RequestBody::Text {
                    content_type: None,
                    value,
                }
            }
        }
        Some("urlencoded") => RequestBody::FormUrlEncoded {
            fields: body
                .get("urlencoded")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|field| {
                    Some(RequestValueField::enabled(
                        field.get("key")?.as_str()?,
                        ValueSource::literal(
                            field.get("value").and_then(Value::as_str).unwrap_or(""),
                        ),
                    ))
                })
                .collect(),
        },
        Some("formdata") => RequestBody::Multipart {
            parts: body
                .get("formdata")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(index, part)| {
                    let name = part.get("key")?.as_str()?.to_owned();
                    let is_file = part.get("type").and_then(Value::as_str) == Some("file");
                    Some(MultipartPart {
                        id: format!("part-{index}"),
                        name,
                        kind: if is_file {
                            MultipartPartKind::File
                        } else {
                            MultipartPartKind::Text
                        },
                        value: ValueSource::literal(
                            part.get("value").and_then(Value::as_str).unwrap_or(""),
                        ),
                        file_name: part
                            .get("fileName")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        file_path: part.get("src").and_then(Value::as_str).map(str::to_owned),
                        content_type: part
                            .get("contentType")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        enabled: !part
                            .get("disabled")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect(),
        },
        _ => RequestBody::Empty,
    }
}

fn parse_legacy_workspace(source: &str) -> Result<ImportedCollection, ImportError> {
    let root: Value = serde_json::from_str(source)
        .map_err(|_| ImportError::new("legacy workspace JSON is invalid"))?;
    if root.get("nodes").is_some() {
        return legacy_v1::parse(&root);
    }
    let name = root
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Imported Workspace")
        .to_owned();
    let requests = root
        .get("requests")
        .and_then(Value::as_array)
        .or_else(|| {
            root.pointer("/collection/requests")
                .and_then(Value::as_array)
        })
        .ok_or_else(|| ImportError::new("legacy workspace document has no requests"))?;
    let requests = requests
        .iter()
        .enumerate()
        .filter_map(|(index, request)| {
            let url = request.get("url")?.as_str()?.to_owned();
            let method = request
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET")
                .to_owned();
            Some(ImportedRequest {
                source_id: format!("legacy-workspace-{index}"),
                name: request
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled")
                    .to_owned(),
                group_source_id: None,
                order: i64::try_from(index).unwrap_or(i64::MAX),
                method,
                url,
                headers: request
                    .get("headers")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(header_from_name_value)
                    .collect(),
                body: RequestBody::Empty,
                query: Vec::new(),
                authentication: RequestAuthentication::None,
                web_socket: false,
                note: String::new(),
            })
        })
        .collect::<Vec<_>>();
    if requests.is_empty() {
        return Err(ImportError::new(
            "legacy workspace document has no importable requests",
        ));
    }
    Ok(ImportedCollection {
        name,
        groups: Vec::new(),
        requests,
        warnings: Vec::new(),
    })
}

fn header_from_name_value(value: &Value) -> Option<RequestHeader> {
    Some(RequestHeader::enabled(
        value.get("name").or_else(|| value.get("key"))?.as_str()?,
        ValueSource::literal(value.get("value").and_then(Value::as_str).unwrap_or("")),
    ))
}

fn postman_url(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Object(object) => object.get("raw").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    }
}

fn slug(value: &str) -> String {
    let value = value
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let value = value
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if value.is_empty() {
        "item".to_owned()
    } else {
        value.chars().take(40).collect()
    }
}

fn shell_tokens(source: &str) -> Result<Vec<String>, ImportError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in source.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                current.push(character);
            }
            continue;
        }
        if character.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if escaped || quote.is_some() {
        return Err(ImportError::new("cURL quoting is incomplete"));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::{ImportEngine, ImportFormat};
    use crate::{RequestBody, ValueSource};

    #[test]
    fn imports_quoted_curl_without_exposing_or_losing_values() {
        let collection = ImportEngine::parse(
            ImportFormat::Curl,
            r#"curl --request patch 'https://example.com/items?q=hello world' -H 'X-Token: secret ref' --data-raw '{"ok":true}'"#,
        )
        .expect("parse cURL");

        let request = &collection.requests[0];
        assert_eq!(request.method, "PATCH");
        assert_eq!(request.url, "https://example.com/items?q=hello world");
        assert_eq!(request.headers[0].value, ValueSource::literal("secret ref"));
        assert!(matches!(request.body, RequestBody::Json { .. }));
    }

    #[test]
    fn preserves_nested_postman_groups_custom_methods_and_multipart_files() {
        let source = r#"{
          "info": {"name": "Payments"},
          "item": [{"name": "Admin", "item": [{
            "name": "Upload", "request": {
              "method": "PURGE", "url": {"raw": "https://example.com/upload"},
              "body": {"mode": "formdata", "formdata": [
                {"key": "asset", "type": "file", "src": "/tmp/asset.bin"}
              ]}
            }
          }]}]
        }"#;
        let collection =
            ImportEngine::parse(ImportFormat::PostmanV2, source).expect("parse Postman");

        assert_eq!(collection.name, "Payments");
        assert_eq!(collection.groups.len(), 1);
        assert_eq!(collection.requests[0].method, "PURGE");
        assert_eq!(
            collection.requests[0].group_source_id.as_deref(),
            Some(collection.groups[0].source_id.as_str())
        );
        assert!(matches!(
            collection.requests[0].body,
            RequestBody::Multipart { .. }
        ));
    }

    #[test]
    fn rejects_invalid_sources_before_returning_any_documents() {
        assert!(ImportEngine::parse(ImportFormat::Curl, "curl 'unterminated").is_err());
        assert!(ImportEngine::parse(ImportFormat::Har, r#"{"log":{"entries":[]}}"#).is_err());
        assert!(ImportEngine::parse(ImportFormat::PostmanV2, "not json").is_err());
    }
}
