//! Request-body transitions and formatting; native collation is a platform service.
use super::{DomainBody, DomainMultipartPart, DomainValueSource, request::unique_id};
use crate::response::{JsonNodeKind, JsonTreeNode, ResponseCancellation, json_tree};
use std::collections::HashMap;
mod number;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainBodyKind {
    Empty,
    Text,
    Json,
    Xml,
    Html,
    Raw,
    Form,
    Multipart,
    File,
}

#[must_use]
pub const fn kind(body: &DomainBody) -> DomainBodyKind {
    composer_kind(match body {
        DomainBody::Empty => DomainBodyKind::Empty,
        DomainBody::Text { .. } => DomainBodyKind::Text,
        DomainBody::Raw { .. } => DomainBodyKind::Raw,
        DomainBody::Json { .. } => DomainBodyKind::Json,
        DomainBody::Xml { .. } => DomainBodyKind::Xml,
        DomainBody::Html { .. } => DomainBodyKind::Html,
        DomainBody::FormUrlEncoded { .. } => DomainBodyKind::Form,
        DomainBody::Multipart { .. } => DomainBodyKind::Multipart,
        DomainBody::File { .. } => DomainBodyKind::File,
    })
}
#[must_use]
pub const fn composer_kind(kind: DomainBodyKind) -> DomainBodyKind {
    match kind {
        DomainBodyKind::Text => DomainBodyKind::Raw,
        other => other,
    }
}

#[must_use]
pub fn switching(body: DomainBody, kind: DomainBodyKind) -> DomainBody {
    let previous = match body {
        DomainBody::Text { value, .. }
        | DomainBody::Json { value }
        | DomainBody::Xml { value }
        | DomainBody::Html { value }
        | DomainBody::Raw { value, .. } => value,
        _ => String::new(),
    };
    match kind {
        DomainBodyKind::Empty => DomainBody::Empty,
        DomainBodyKind::Text => DomainBody::Text {
            content_type: None,
            value: previous,
        },
        DomainBodyKind::Json => DomainBody::Json {
            value: if previous.is_empty() {
                "{\n  \n}".into()
            } else {
                previous
            },
        },
        DomainBodyKind::Xml => DomainBody::Xml { value: previous },
        DomainBodyKind::Html => DomainBody::Html { value: previous },
        DomainBodyKind::Raw => DomainBody::Raw {
            content_type: None,
            value: previous,
        },
        DomainBodyKind::Form => DomainBody::FormUrlEncoded { fields: vec![] },
        DomainBodyKind::Multipart => DomainBody::Multipart {
            parts: if previous.is_empty() {
                vec![]
            } else {
                vec![DomainMultipartPart {
                    id: unique_id(),
                    name: "name".into(),
                    value: DomainValueSource::literal(previous),
                    enabled: true,
                    ..DomainMultipartPart::default()
                }]
            },
        },
        DomainBodyKind::File => DomainBody::File {
            path: String::new(),
            content_type: None,
        },
    }
}

#[must_use]
pub const fn can_format(body: &DomainBody) -> bool {
    kind_can_format(kind(body))
}

#[must_use]
pub const fn kind_can_format(kind: DomainBodyKind) -> bool {
    matches!(
        kind,
        DomainBodyKind::Json | DomainBodyKind::Xml | DomainBodyKind::Html
    )
}

/// The native adapter supplies Foundation's numeric, case-insensitive, forced
/// key ordering in one batch. Parsing, traversal, and serialization stay in Rust.
pub trait BodyKeyOrdering: Send + Sync {
    fn sorted_keys(&self, keys: Vec<String>) -> Vec<String>;
}

#[must_use]
pub fn formatted(body: DomainBody, ordering: &dyn BodyKeyOrdering) -> DomainBody {
    match body {
        DomainBody::Json { value } => DomainBody::Json {
            value: format_json(&value, ordering).unwrap_or(value),
        },
        DomainBody::Xml { value } => DomainBody::Xml {
            value: value.replace("><", ">\n<"),
        },
        DomainBody::Html { value } => DomainBody::Html {
            value: value.replace("><", ">\n<"),
        },
        other => other,
    }
}

fn format_json(source: &str, ordering: &dyn BodyKeyOrdering) -> Option<String> {
    let root = json_tree(
        &without_trailing_commas(source),
        u64::MAX,
        &ResponseCancellation::default(),
    )
    .ok()?;
    if !matches!(root.kind, JsonNodeKind::Object | JsonNodeKind::Array) {
        return None;
    }
    let mut keys = Vec::new();
    collect_keys(&root, &mut keys);
    keys.sort();
    keys.dedup();
    let ranks: HashMap<_, _> = ordering
        .sorted_keys(keys)
        .into_iter()
        .enumerate()
        .map(|(i, key)| (key, i))
        .collect();
    let mut output = String::new();
    write_json(&root, 0, &ranks, &mut output)?;
    Some(output)
}
fn collect_keys(node: &JsonTreeNode, keys: &mut Vec<String>) {
    for child in node.children.iter().flatten() {
        if node.kind == JsonNodeKind::Object {
            keys.push(child.key.clone());
        }
        collect_keys(child, keys);
    }
}
fn newline(output: &mut String, depth: usize) {
    output.push('\n');
    for _ in 0..depth {
        output.push_str("  ");
    }
}
fn quote(value: &str, output: &mut String) {
    // JSONSerialization escapes solidus and emits non-ASCII characters literally.
    output.push_str(
        &serde_json::to_string(value)
            .expect("JSON string")
            .replace('/', "\\/"),
    );
}
fn write_json(
    node: &JsonTreeNode,
    depth: usize,
    ranks: &HashMap<String, usize>,
    output: &mut String,
) -> Option<()> {
    match node.kind {
        JsonNodeKind::Object | JsonNodeKind::Array => {
            let object = node.kind == JsonNodeKind::Object;
            output.push(if object { '{' } else { '[' });
            let mut children: Vec<_> = node.children.as_ref()?.iter().collect();
            if object {
                // NSDictionary keeps the last occurrence of a duplicate JSON key.
                let mut unique = HashMap::new();
                for child in children {
                    unique.insert(&child.key, child);
                }
                children = unique.into_values().collect();
                children.sort_by_key(|child| ranks.get(&child.key).copied().unwrap_or(usize::MAX));
            }
            output.push('\n');
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                    output.push('\n');
                }
                for _ in 0..=depth {
                    output.push_str("  ");
                }
                if object {
                    quote(&child.key, output);
                    output.push_str(" : ");
                }
                write_json(child, depth + 1, ranks, output)?;
            }
            newline(output, depth);
            output.push(if object { '}' } else { ']' });
        }
        JsonNodeKind::String => quote(&node.value, output),
        JsonNodeKind::Number => output.push_str(&number::format(&node.value)?),
        JsonNodeKind::Boolean | JsonNodeKind::Null => output.push_str(&node.value),
        JsonNodeKind::Truncated => return None,
    }
    Some(())
}

// Foundation permits a trailing comma in objects and arrays. Keep that legacy
// input tolerance local to request formatting; response JSON stays strict.
fn without_trailing_commas(source: &str) -> Vec<u8> {
    let bytes = source.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut previous = None;
    for (index, &byte) in bytes.iter().enumerate() {
        if quoted {
            output.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
                previous = Some(byte);
            }
            continue;
        }
        if byte == b'"' {
            quoted = true;
        }
        if byte == b',' && previous.is_some_and(|p| !b"[{,:".contains(&p)) {
            let next = bytes[index + 1..].iter().find(|b| !b" \t\r\n".contains(b));
            if next.is_some_and(|b| b"]}".contains(b)) {
                continue;
            }
        }
        output.push(byte);
        if !b" \t\r\n".contains(&byte) {
            previous = Some(byte);
        }
    }
    output
}
