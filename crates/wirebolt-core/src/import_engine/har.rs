//! HTTP Archive (HAR 1.2), as saved by browsers and proxies.

use serde_json::Value;

use super::{
    ImportError, ImportedCollection, ImportedRequest, authorization_header_authentication,
    is_connection_header, listed_warning, method_and_path_name, percent_decoded,
};
use crate::{
    MultipartPart, MultipartPartKind, RequestBody, RequestHeader, RequestValueField, ValueSource,
};

pub(super) fn parse(
    source: &str,
    file_name: Option<&str>,
) -> Result<ImportedCollection, ImportError> {
    let root: Value = serde_json::from_str(source)
        .map_err(|_| ImportError::new("The HAR file isn't valid JSON."))?;
    let entries = root
        .pointer("/log/entries")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::new("The document isn't a HAR file: it has no log entries."))?;
    let mut collection = ImportedCollection {
        name: file_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Imported HAR")
            .to_owned(),
        ..ImportedCollection::default()
    };
    let mut skipped = 0_usize;
    let mut missing_files = Vec::new();
    for entry in entries {
        let Some(request) = entry.get("request") else {
            skipped += 1;
            continue;
        };
        let Some(url) = request
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| is_request_url(url))
        else {
            skipped += 1;
            continue;
        };
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .filter(|method| !method.is_empty())
            .unwrap_or("GET")
            .to_ascii_uppercase();
        let web_socket = url
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ws:"))
            || url
                .get(..4)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("wss:"));
        let name = method_and_path_name(&method, url);
        let body = body(request.get("postData"), &name, &mut missing_files);
        let multipart = matches!(body, RequestBody::Multipart { .. });
        let mut headers = request
            .get("headers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|header| {
                let name = header.get("name")?.as_str()?;
                // The multipart encoder writes its own boundary.
                let stale_boundary = multipart && name.eq_ignore_ascii_case("content-type");
                // The WebSocket client performs its own handshake.
                let handshake = web_socket
                    && name
                        .get(..14)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("sec-websocket-"));
                (!is_connection_header(name) && !stale_boundary && !handshake).then(|| {
                    RequestHeader::enabled(
                        name,
                        ValueSource::literal(
                            header.get("value").and_then(Value::as_str).unwrap_or(""),
                        ),
                    )
                })
            })
            .collect();
        let authentication = authorization_header_authentication(&mut headers);
        let index = collection.requests.len();
        collection.requests.push(ImportedRequest {
            source_id: format!("har-{index}"),
            name,
            order: i64::try_from(index).unwrap_or(i64::MAX),
            web_socket,
            method,
            url: url.to_owned(),
            headers,
            authentication,
            body,
            ..ImportedRequest::default()
        });
    }
    if collection.requests.is_empty() {
        return Err(ImportError::new(
            "The HAR file has no HTTP requests to import.",
        ));
    }
    if skipped > 0 {
        collection.warnings.push(format!(
            "Skipped {skipped} {} that weren't HTTP or WebSocket requests.",
            if skipped == 1 { "entry" } else { "entries" }
        ));
    }
    if !missing_files.is_empty() {
        collection.warnings.push(listed_warning(
            "HAR files don't include uploaded files; choose them again before sending",
            &missing_files,
        ));
    }
    Ok(collection)
}

fn is_request_url(url: &str) -> bool {
    ["http://", "https://", "ws://", "wss://"]
        .iter()
        .any(|scheme| {
            url.len() > scheme.len()
                && url
                    .get(..scheme.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
}

fn body(post_data: Option<&Value>, name: &str, missing_files: &mut Vec<String>) -> RequestBody {
    let Some(post_data) = post_data else {
        return RequestBody::Empty;
    };
    let mime = post_data
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let lower_mime = mime.to_ascii_lowercase();
    let text = post_data.get("text").and_then(Value::as_str).unwrap_or("");
    let params = post_data
        .get("params")
        .and_then(Value::as_array)
        .filter(|params| !params.is_empty());
    if lower_mime.starts_with("multipart/form-data")
        && let Some(params) = params
    {
        return multipart(params, name, missing_files);
    }
    if lower_mime.starts_with("application/x-www-form-urlencoded") {
        let fields: Vec<_> = if text.is_empty() {
            params
                .into_iter()
                .flatten()
                .filter_map(|param| {
                    let key = param.get("name")?.as_str()?;
                    let value = param.get("value").and_then(Value::as_str).unwrap_or("");
                    Some(RequestValueField::enabled(
                        form_decoded(key),
                        ValueSource::literal(form_decoded(value)),
                    ))
                })
                .collect()
        } else {
            url::form_urlencoded::parse(text.as_bytes())
                .map(|(key, value)| {
                    RequestValueField::enabled(
                        key.into_owned(),
                        ValueSource::literal(value.into_owned()),
                    )
                })
                .collect()
        };
        if !fields.is_empty() {
            return RequestBody::FormUrlEncoded { fields };
        }
    }
    if text.is_empty() {
        return RequestBody::Empty;
    }
    let value = text.to_owned();
    if lower_mime.contains("json") {
        RequestBody::Json { value }
    } else if lower_mime.contains("xml") {
        RequestBody::Xml { value }
    } else if lower_mime.starts_with("text/html") {
        RequestBody::Html { value }
    } else {
        RequestBody::Text {
            content_type: Some(mime).filter(|mime| !mime.is_empty()),
            value,
        }
    }
}

fn multipart(params: &[Value], name: &str, missing_files: &mut Vec<String>) -> RequestBody {
    let parts = params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            let part_name = param.get("name")?.as_str()?.to_owned();
            let file_name = param
                .get("fileName")
                .and_then(Value::as_str)
                .filter(|file_name| !file_name.is_empty())
                .map(str::to_owned);
            if file_name.is_some() {
                missing_files.push(format!("{name} ({part_name})"));
            }
            Some(MultipartPart {
                id: format!("part-{index}"),
                name: part_name,
                kind: if file_name.is_some() {
                    MultipartPartKind::File
                } else {
                    MultipartPartKind::Text
                },
                value: ValueSource::literal(if file_name.is_some() {
                    ""
                } else {
                    param.get("value").and_then(Value::as_str).unwrap_or("")
                }),
                file_path: None,
                file_name,
                content_type: param
                    .get("contentType")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                enabled: true,
            })
        })
        .collect();
    RequestBody::Multipart { parts }
}

/// Decodes `application/x-www-form-urlencoded` text, where `+` is a space.
fn form_decoded(value: &str) -> String {
    percent_decoded(&value.replace('+', " "))
}
