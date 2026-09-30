//! Helpers shared by the Insomnia and Bruno importers.

use serde_json::Value;

use super::{ImportedCollection, ImportedEnvironment, ImportedWorkspace, slug};
use crate::{EnvironmentVariable, RequestHeader, RequestValueField, SecretName, ValueSource};

/// One environment row before it becomes an [`EnvironmentVariable`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Row {
    pub key: String,
    pub value: String,
    pub enabled: bool,
    /// A Keychain-backed variable. Its material is never imported: the row
    /// becomes a secret reference the user fills in on this Mac.
    pub secret: bool,
}

impl Row {
    pub fn literal(key: impl Into<String>, value: impl Into<String>, enabled: bool) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            enabled,
            secret: false,
        }
    }
}

/// Replaces rows with the same key and appends new ones, keeping first-seen order.
pub(super) fn overlay(base: &mut Vec<Row>, rows: Vec<Row>) {
    for row in rows {
        match base.iter_mut().find(|existing| existing.key == row.key) {
            Some(existing) => *existing = row,
            None => base.push(row),
        }
    }
}

/// Builds an environment; secret rows become references named
/// `<collection>.<environment>.<key>` so a re-import finds the same item.
pub(super) fn environment(
    collection: &str,
    name: &str,
    rows: Vec<Row>,
    diagnostics: &mut Diagnostics,
) -> ImportedEnvironment {
    let variables = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let value = if row.secret {
                let key = row
                    .key
                    .chars()
                    .map(|character| {
                        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                            character
                        } else {
                            '-'
                        }
                    })
                    .collect::<String>();
                let reference = format!("{}.{}.{key}", slug(collection), slug(name));
                if !row.value.is_empty() {
                    diagnostics.note(
                        "Secret values aren’t imported; set them in Configure Environments",
                        row.key.clone(),
                    );
                }
                SecretName::new(reference.chars().take(128).collect::<String>())
                    .map_or_else(|_| ValueSource::literal(""), ValueSource::secret)
            } else {
                ValueSource::literal(row.value)
            };
            EnvironmentVariable {
                id: format!("variable-{index}"),
                key: row.key,
                value,
                enabled: row.enabled,
                order: order(index),
            }
        })
        .collect();
    ImportedEnvironment {
        name: if name.is_empty() {
            collection.to_owned()
        } else {
            format!("{collection} – {name}")
        },
        global: false,
        variables,
    }
}

pub(super) fn workspace(
    collection: ImportedCollection,
    environments: Vec<ImportedEnvironment>,
) -> ImportedWorkspace {
    let mut workspace = ImportedWorkspace::from(collection);
    workspace.environments = environments;
    workspace
}

pub(super) const fn empty_collection() -> ImportedCollection {
    ImportedCollection {
        name: String::new(),
        groups: Vec::new(),
        requests: Vec::new(),
        warnings: Vec::new(),
    }
}

/// Groups repeated import diagnostics into one line per kind, so a large
/// collection produces a short, readable list.
#[derive(Debug, Default)]
pub(super) struct Diagnostics {
    kinds: Vec<(String, Vec<String>)>,
}

impl Diagnostics {
    pub fn note(&mut self, message: impl Into<String>, subject: impl Into<String>) {
        let message = message.into();
        let subject = subject.into();
        if let Some((_, subjects)) = self.kinds.iter_mut().find(|(kind, _)| *kind == message) {
            if !subjects.contains(&subject) {
                subjects.push(subject);
            }
        } else {
            self.kinds.push((message, vec![subject]));
        }
    }

    pub fn into_warnings(self) -> Vec<String> {
        const LISTED: usize = 5;
        self.kinds
            .into_iter()
            .map(|(message, subjects)| {
                let subjects = subjects
                    .into_iter()
                    .filter(|subject| !subject.is_empty())
                    .collect::<Vec<_>>();
                if subjects.is_empty() {
                    return message;
                }
                let mut listed = subjects
                    .iter()
                    .take(LISTED)
                    .map(|subject| format!("“{subject}”"))
                    .collect::<Vec<_>>()
                    .join(", ");
                if subjects.len() > LISTED {
                    listed = format!("{listed} and {} more", subjects.len() - LISTED);
                }
                format!("{message}: {listed}")
            })
            .collect()
    }
}

pub(super) fn order(index: usize) -> i64 {
    i64::try_from(index).unwrap_or(i64::MAX)
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

pub(super) fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub(super) fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Converts a scalar to text; numbers and booleans keep their literal form.
pub(super) fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

pub(super) fn header(name: &str, value: String, enabled: bool) -> RequestHeader {
    let mut header = RequestHeader::enabled(name, ValueSource::literal(value));
    header.enabled = enabled;
    header
}

pub(super) fn field(name: &str, value: String, enabled: bool) -> RequestValueField {
    let mut field = RequestValueField::enabled(name, ValueSource::literal(value));
    field.enabled = enabled;
    field
}

/// Splits `url` into the part before `?` and its query text.
pub(super) fn split_query(url: &str) -> (&str, Option<&str>) {
    let (without_fragment, _) = url.split_once('#').unwrap_or((url, ""));
    match without_fragment.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    }
}

/// Replaces `:name` path segments that have a value. Segments without a value
/// stay as written so the user can still see and fill them in.
pub(super) fn substitute_path_parameters(url: &str, parameters: &[(String, String)]) -> String {
    if parameters.iter().all(|(_, value)| value.is_empty()) {
        return url.to_owned();
    }
    let (path, rest) = match url.find(['?', '#']) {
        Some(index) => url.split_at(index),
        None => (url, ""),
    };
    let mut output = String::with_capacity(url.len());
    let mut remainder = path;
    while let Some(index) = remainder.find("/:") {
        output.push_str(&remainder[..=index]);
        let after = &remainder[index + 2..];
        let end = after
            .find(|character: char| {
                !(character.is_ascii_alphanumeric() || character == '_' || character == '-')
            })
            .unwrap_or(after.len());
        let name = &after[..end];
        if let Some((_, value)) = parameters
            .iter()
            .find(|(candidate, value)| candidate == name && !value.is_empty())
        {
            output.push_str(value);
        } else {
            output.push(':');
            output.push_str(name);
        }
        remainder = &after[end..];
    }
    output.push_str(remainder);
    output.push_str(rest);
    output
}

/// Adds inherited folder or collection headers the request does not override.
pub(super) fn inherit_headers(headers: &mut Vec<RequestHeader>, inherited: &[RequestHeader]) {
    let mut merged = inherited
        .iter()
        .filter(|parent| {
            !headers
                .iter()
                .any(|own| own.name.eq_ignore_ascii_case(&parent.name))
        })
        .map(|parent| {
            let mut copy = RequestHeader::enabled(parent.name.clone(), parent.value.clone());
            copy.enabled = parent.enabled;
            copy
        })
        .collect::<Vec<_>>();
    merged.append(headers);
    *headers = merged;
}

#[cfg(test)]
mod tests {
    use super::{Diagnostics, split_query, substitute_path_parameters};

    #[test]
    fn substitutes_only_path_parameters_with_values() {
        let parameters = vec![
            ("id".to_owned(), "{{userId}}".to_owned()),
            ("tab".to_owned(), String::new()),
        ];
        assert_eq!(
            substitute_path_parameters("https://h:8443/users/:id/:tab?x=:id", &parameters),
            "https://h:8443/users/{{userId}}/:tab?x=:id"
        );
    }

    #[test]
    fn splits_query_without_fragment() {
        assert_eq!(
            split_query("https://h/p?a=1#top"),
            ("https://h/p", Some("a=1"))
        );
        assert_eq!(split_query("https://h/p"), ("https://h/p", None));
    }

    #[test]
    fn groups_repeated_warnings() {
        let mut diagnostics = Diagnostics::default();
        for index in 0..7 {
            diagnostics.note("Scripts were not imported", format!("R{index}"));
        }
        diagnostics.note("Cookies were not imported", "");
        assert_eq!(
            diagnostics.into_warnings(),
            vec![
                "Scripts were not imported: “R0”, “R1”, “R2”, “R3”, “R4” and 2 more".to_owned(),
                "Cookies were not imported".to_owned(),
            ]
        );
    }
}
