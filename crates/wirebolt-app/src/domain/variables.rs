//! Template offsets use UTF-16, the coordinate system of native text controls.
use super::models::{DomainEnvironment, DomainValueSource};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateReference {
    pub name: String,
    pub location: u64,
    pub length: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenTemplateReference {
    pub prefix: String,
    pub location: u64,
    pub length: u64,
    pub is_closed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub name: String,
    pub value: DomainValueSource,
    pub environment_name: String,
    pub display_value: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateSegment {
    Text { value: String },
    Variable { name: String },
}

#[must_use]
pub fn references(text: &str) -> Vec<TemplateReference> {
    let mut result = vec![];
    let mut offset = 0;
    while let Some(open) = text[offset..].find("{{").map(|i| i + offset) {
        let Some(close) = text[open + 2..].find("}}").map(|i| i + open + 2) else {
            break;
        };
        let name = text[open + 2..close].trim();
        if !name.is_empty() {
            result.push(TemplateReference {
                name: name.into(),
                location: text[..open].encode_utf16().count() as u64,
                length: text[open..close + 2].encode_utf16().count() as u64,
            });
        }
        offset = close + 2;
    }
    result
}
#[must_use]
pub fn open_reference(text: &str, caret: u64) -> Option<OpenTemplateReference> {
    let chars: Vec<u16> = text.encode_utf16().collect();
    let caret = usize::try_from(caret).ok()?;
    if caret < 2 || caret > chars.len() {
        return None;
    }
    let begin = caret.saturating_sub(130);
    let open = (begin..caret - 1)
        .rev()
        .find(|&i| chars[i..i + 2] == [123, 123])?;
    let mut start = open + 2;
    while start < caret && chars[start] == 123 {
        start += 1;
    }
    let prefix = String::from_utf16(&chars[start..caret]).ok()?;
    if prefix.contains(['{', '}']) || prefix.chars().any(char::is_whitespace) {
        return None;
    }
    Some(OpenTemplateReference {
        prefix,
        location: start as u64,
        length: (caret - start) as u64,
        is_closed: chars.get(caret..caret + 2) == Some(&[125, 125]),
    })
}
#[must_use]
pub fn segments(text: &str) -> Vec<TemplateSegment> {
    let chars: Vec<u16> = text.encode_utf16().collect();
    let mut offset = 0;
    let mut result = vec![];
    for r in references(text) {
        let start = usize::try_from(r.location).unwrap_or(usize::MAX);
        if start > offset {
            result.push(TemplateSegment::Text {
                value: String::from_utf16_lossy(&chars[offset..start]),
            });
        }
        result.push(TemplateSegment::Variable { name: r.name });
        offset = start + usize::try_from(r.length).unwrap_or(usize::MAX);
    }
    if offset < chars.len() {
        result.push(TemplateSegment::Text {
            value: String::from_utf16_lossy(&chars[offset..]),
        });
    }
    result
}
#[must_use]
pub fn enabled_values(environment: &DomainEnvironment) -> BTreeMap<String, DomainValueSource> {
    let mut rows: Vec<_> = environment
        .variables
        .iter()
        .filter(|v| v.enabled && !v.key.is_empty())
        .collect();
    rows.sort_by(|a, b| (&a.order, &a.key).cmp(&(&b.order, &b.key)));
    rows.into_iter()
        .map(|v| (v.key.clone(), v.value.clone()))
        .collect()
}
#[must_use]
pub fn catalog(environments: &[DomainEnvironment], selected: Option<&str>) -> Vec<CatalogEntry> {
    let mut entries = BTreeMap::new();
    for environment in [
        environments.iter().find(|e| e.id == "global"),
        environments
            .iter()
            .find(|e| Some(e.id.as_str()) == selected && e.id != "global"),
    ]
    .into_iter()
    .flatten()
    {
        for (name, value) in enabled_values(environment) {
            entries.insert(
                name.clone(),
                CatalogEntry {
                    name,
                    display_value: match &value {
                        DomainValueSource::Literal { value } => value.clone(),
                        DomainValueSource::Secret { .. } => "•••• (secret)".into(),
                    },
                    value,
                    environment_name: environment.name.clone(),
                },
            );
        }
    }
    let mut entries: Vec<_> = entries.into_values().collect();
    entries.sort_by(|a, b| super::sidebar::natural_compare(&a.name, &b.name));
    entries
}
#[must_use]
pub fn completions(entries: &[CatalogEntry], prefix: &str) -> Vec<CatalogEntry> {
    let prefix = prefix.to_lowercase();
    let mut result: Vec<_> = entries
        .iter()
        .filter(|e| e.name.to_lowercase().starts_with(&prefix))
        .cloned()
        .collect();
    result.extend(
        entries
            .iter()
            .filter(|e| {
                let name = e.name.to_lowercase();
                !name.starts_with(&prefix) && name.contains(&prefix)
            })
            .cloned(),
    );
    result
}
#[must_use]
pub fn display_value(entry: &CatalogEntry) -> &str {
    match &entry.value {
        DomainValueSource::Literal { value } => value,
        DomainValueSource::Secret { .. } => "•••• (secret)",
    }
}
#[must_use]
pub fn summary(entries: &[CatalogEntry], text: &str) -> Option<String> {
    let mut seen = HashSet::new();
    let lines: Vec<_> = references(text)
        .into_iter()
        .filter(|r| seen.insert(r.name.clone()))
        .map(|r| {
            entries.iter().find(|e| e.name == r.name).map_or_else(
                || format!("{}: not defined in the active environments", r.name),
                |e| format!("{} = {} ({})", e.name, display_value(e), e.environment_name),
            )
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}
#[must_use]
pub fn delimiter_ranges(text: &str, location: u64, length: u64, name: &str) -> Vec<u64> {
    let chars: Vec<u16> = text.encode_utf16().collect();
    let start = usize::try_from(location).unwrap_or(usize::MAX);
    let length = usize::try_from(length).unwrap_or(usize::MAX);
    if length < 4 || start + length > chars.len() {
        return vec![location, 0, location, 0];
    }
    let inner = String::from_utf16_lossy(&chars[start + 2..start + length - 2]);
    let name_start = start
        + 2
        + inner
            .find(name)
            .map_or(0, |i| inner[..i].encode_utf16().count());
    let name_end = name_start + name.encode_utf16().count();
    vec![
        location,
        (name_start - start) as u64,
        name_end as u64,
        (start + length - name_end) as u64,
    ]
}
/// Locale-sensitive comparison is supplied by the native platform in batches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameMatch {
    pub has_prefix: bool,
    pub contains: bool,
}
pub trait VariableComparison: Send + Sync {
    fn sorted_names(&self, names: Vec<String>) -> Vec<String>;
    fn name_matches(&self, names: Vec<String>, prefix: String) -> Vec<NameMatch>;
}
pub struct CatalogIndex {
    pub entries: Vec<CatalogEntry>,
    comparison: std::sync::Arc<dyn VariableComparison>,
}
impl std::fmt::Debug for CatalogIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogIndex")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}
impl CatalogIndex {
    #[must_use]
    pub fn new(
        environments: &[DomainEnvironment],
        selected: Option<&str>,
        comparison: std::sync::Arc<dyn VariableComparison>,
    ) -> Self {
        let entries = catalog(environments, selected);
        let names = comparison.sorted_names(entries.iter().map(|e| e.name.clone()).collect());
        let ranks: BTreeMap<_, _> = names
            .into_iter()
            .enumerate()
            .map(|(i, name)| (name, i))
            .collect();
        let mut entries = entries;
        entries.sort_by_key(|e| ranks.get(&e.name).copied().unwrap_or(usize::MAX));
        Self {
            entries,
            comparison,
        }
    }
    #[must_use]
    pub fn completions(&self, prefix: &str) -> Vec<u64> {
        if prefix.is_empty() {
            return (0..self.entries.len() as u64).collect();
        }
        let flags = self.comparison.name_matches(
            self.entries.iter().map(|e| e.name.clone()).collect(),
            prefix.into(),
        );
        let mut result = vec![];
        let mut rest = vec![];
        for (i, m) in flags.into_iter().take(self.entries.len()).enumerate() {
            if m.has_prefix {
                result.push(i as u64);
            } else if m.contains {
                rest.push(i as u64);
            }
        }
        result.extend(rest);
        result
    }
    #[must_use]
    pub fn summary(&self, text: &str) -> Option<String> {
        summary(&self.entries, text)
    }
}
