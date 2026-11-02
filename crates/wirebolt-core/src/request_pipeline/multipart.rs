//! Multipart preparation retains file paths, never complete large uploads.
use super::*;
use crate::{
    MultipartPartKind,
    request::multipart::{MultipartBody, MultipartSegment},
};

pub(super) fn prepare_multipart<R: SecretResolver + ?Sized>(
    parts: &[crate::MultipartPart],
    resolver: &mut TemplateResolver<'_, R>,
    headers: &mut HeaderMap,
) -> PreparedBodySource {
    let boundary = "wirebolt-boundary-7MA4YWxkTrZu0gW";
    // The encoder owns the boundary. Imported headers may describe a different body.
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("multipart/form-data; boundary=wirebolt-boundary-7MA4YWxkTrZu0gW"),
    );
    let mut segments = Vec::new();
    for (index, part) in parts.iter().enumerate().filter(|(_, part)| part.enabled) {
        let Some(header) = part_header(part, index, boundary, resolver) else {
            continue;
        };
        segments.push(MultipartSegment::Bytes(header));
        append_part_content(part, index, resolver, &mut segments);
        segments.push(MultipartSegment::Bytes(b"\r\n".to_vec()));
    }
    segments.push(MultipartSegment::Bytes(
        format!("--{boundary}--\r\n").into_bytes(),
    ));
    let Some(body) = MultipartBody::new(segments) else {
        resolver.issue(
            FieldPath::Fixed("body"),
            RequestIssueKind::InvalidBody,
            None,
        );
        return PreparedBodySource::Bytes(Vec::new());
    };
    let byte_count = body.byte_count();
    if let Ok(body) = body.into_prepared() {
        headers.insert(CONTENT_LENGTH, HeaderValue::from(byte_count));
        body
    } else {
        resolver.issue(
            FieldPath::Fixed("body"),
            RequestIssueKind::FileUnavailable,
            None,
        );
        PreparedBodySource::Bytes(Vec::new())
    }
}

fn part_header<R: SecretResolver + ?Sized>(
    part: &crate::MultipartPart,
    index: usize,
    boundary: &str,
    resolver: &mut TemplateResolver<'_, R>,
) -> Option<Vec<u8>> {
    let name =
        resolver.resolve_template(&part.name, FieldPath::indexed("body.parts", index, "name"))?;
    let filename = part.file_name.as_deref().or_else(|| {
        (part.kind == MultipartPartKind::File).then(|| {
            part.file_path
                .as_deref()
                .and_then(|path| std::path::Path::new(path).file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("file")
        })
    });
    if name.len() + filename.map_or(0, str::len) > MAX_MULTIPART_HEADER_BYTES {
        resolver.issue(
            FieldPath::indexed("body.parts", index, "name"),
            RequestIssueKind::InvalidBody,
            None,
        );
        return None;
    }
    let escaped_name = name.replace(['"', '\r', '\n'], "_");
    let mut body = format!("--{boundary}\r\n").into_bytes();
    let disposition = filename.map_or_else(String::new, |name| {
        format!("; filename=\"{}\"", name.replace(['\"', '\r', '\n'], "_"))
    });
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{escaped_name}\"{disposition}\r\n")
            .as_bytes(),
    );
    let content_type = part
        .content_type
        .as_deref()
        .filter(|value| !value.is_empty())
        .or_else(|| (part.kind == MultipartPartKind::File).then_some("application/octet-stream"));
    if let Some(content_type) = content_type {
        if content_type.len() > MAX_MULTIPART_HEADER_BYTES
            || HeaderValue::from_str(content_type).is_err()
        {
            resolver.issue(
                FieldPath::indexed("body.parts", index, "content_type"),
                RequestIssueKind::InvalidHeaderValue,
                None,
            );
            return None;
        }
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
    }
    body.extend_from_slice(b"\r\n");
    if body.len() > MAX_MULTIPART_HEADER_BYTES {
        resolver.issue(
            FieldPath::indexed("body.parts", index, "name"),
            RequestIssueKind::InvalidBody,
            None,
        );
        return None;
    }
    Some(body)
}

fn append_part_content<R: SecretResolver + ?Sized>(
    part: &crate::MultipartPart,
    index: usize,
    resolver: &mut TemplateResolver<'_, R>,
    segments: &mut Vec<MultipartSegment>,
) {
    match part.kind {
        MultipartPartKind::Binary => {
            if let Some(value) = resolver.resolve_source(
                &part.value,
                FieldPath::indexed("body.parts", index, "value"),
            ) {
                if let Ok(bytes) = STANDARD.decode(value.as_bytes()) {
                    segments.push(MultipartSegment::Bytes(bytes));
                } else {
                    resolver.issue(
                        FieldPath::indexed("body.parts", index, "value"),
                        RequestIssueKind::InvalidBody,
                        None,
                    );
                }
            }
        }
        MultipartPartKind::Text => {
            if let Some(value) = resolver.resolve_source(
                &part.value,
                FieldPath::indexed("body.parts", index, "value"),
            ) {
                segments.push(MultipartSegment::Bytes(value.into_owned().into_bytes()));
            }
        }
        MultipartPartKind::File => {
            let file = part.file_path.as_deref().and_then(|path| {
                let metadata = fs::metadata(path).ok()?;
                metadata.is_file().then(|| MultipartSegment::File {
                    path: path.into(),
                    byte_count: metadata.len(),
                })
            });
            if let Some(file) = file {
                segments.push(file);
            } else {
                resolver.issue(
                    FieldPath::indexed("body.parts", index, "file_path"),
                    RequestIssueKind::FileUnavailable,
                    None,
                );
            }
        }
    }
}

const MAX_MULTIPART_HEADER_BYTES: usize = 64 * 1024;
