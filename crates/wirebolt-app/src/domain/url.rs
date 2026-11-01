//! Editable URL/query synchronization, percent encoding and stable query row identity.
use super::{
    identity::unique_id,
    models::{DomainField, DomainRequestDraft, DomainValueSource},
};
use percent_encoding::percent_decode_str;
// URLComponents query encoding: query punctuation is legal except item delimiters.
#[must_use]
pub fn encode_query(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~!$'()*+,/:;?@".contains(&b) {
            out.push(char::from(b));
        } else {
            use std::fmt::Write;
            write!(out, "%{b:02X}").expect("String write");
        }
    }
    out
}
#[must_use]
pub fn display_url(draft: &DomainRequestDraft) -> String {
    let query: Vec<_> = draft
        .query
        .iter()
        .filter(|f| f.enabled)
        .map(|f| {
            format!(
                "{}={}",
                encode_query(&f.name),
                encode_query(f.value.editable())
            )
        })
        .collect();
    if query.is_empty() {
        return draft.url.clone();
    }
    let (prefix, fragment) = draft
        .url
        .split_once('#')
        .map_or((draft.url.as_str(), String::new()), |(p, f)| {
            (p, format!("#{f}"))
        });
    format!(
        "{prefix}{}{}{fragment}",
        if prefix.contains('?') { '&' } else { '?' },
        query
            .join("&")
            .replace("%7B%7B", "{{")
            .replace("%7D%7D", "}}")
    )
}
#[must_use]
pub fn edit_url(mut draft: DomainRequestDraft, text: &str) -> DomainRequestDraft {
    let (prefix, fragment) = text
        .split_once('#')
        .map_or((text, String::new()), |(p, f)| (p, format!("#{f}")));
    let Some((base, query)) = prefix.split_once('?') else {
        draft.url = text.into();
        draft.query.retain(|f| !f.enabled);
        return draft;
    };
    let mut previous: Vec<_> = draft.query.iter().filter(|f| f.enabled).cloned().collect();
    let disabled: Vec<_> = draft.query.into_iter().filter(|f| !f.enabled).collect();
    draft.query = if query.is_empty() {
        vec![]
    } else {
        query
            .split('&')
            .map(|item| {
                let (n, v) = item.split_once('=').unwrap_or((item, ""));
                let name = percent_decode_str(n).decode_utf8_lossy().into_owned();
                let value = percent_decode_str(v).decode_utf8_lossy().into_owned();
                let mut field = previous.iter().position(|f| f.name == name).map_or_else(
                    || DomainField {
                        id: unique_id(),
                        name: name.clone(),
                        value: DomainValueSource::literal(&value),
                        enabled: true,
                        sensitive: false,
                    },
                    |i| previous.remove(i),
                );
                if field.value.editable() != value {
                    field.value = DomainValueSource::literal(value);
                }
                field
            })
            .collect()
    };
    draft.query.extend(disabled);
    draft.url = format!("{base}{fragment}");
    draft
}
#[must_use]
pub fn separate_url_query(mut draft: DomainRequestDraft) -> DomainRequestDraft {
    let explicit = std::mem::take(&mut draft.query);
    let url = draft.url.clone();
    let mut result = edit_url(draft, &url);
    result.query.extend(explicit);
    result
}
/// The complete result of one URL edit, independent of request bodies and credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlQueryEdit {
    pub url: String,
    pub query: Vec<DomainField>,
    pub display_url: String,
}
#[must_use]
pub fn edit_url_query(url: String, query: Vec<DomainField>, text: &str) -> UrlQueryEdit {
    let draft = edit_url(
        DomainRequestDraft {
            url,
            query,
            ..DomainRequestDraft::default()
        },
        text,
    );
    UrlQueryEdit {
        display_url: display_url(&draft),
        url: draft.url,
        query: draft.query,
    }
}
#[must_use]
pub fn display_url_query(url: String, query: Vec<DomainField>) -> String {
    display_url(&DomainRequestDraft {
        url,
        query,
        ..DomainRequestDraft::default()
    })
}
#[must_use]
pub fn separate_url_query_values(url: String, query: Vec<DomainField>) -> UrlQueryEdit {
    let draft = separate_url_query(DomainRequestDraft {
        url,
        query,
        ..DomainRequestDraft::default()
    });
    UrlQueryEdit {
        display_url: display_url(&draft),
        url: draft.url,
        query: draft.query,
    }
}
