mod support;
use support::*;
use wirebolt_app::domain::{models::DomainValueSource, variables::*};
#[test]
fn references_trim_names_ignore_empty_and_unclosed() {
    let text = "https://{{host}}/{{ version }}/{{}}/x?{{open";
    let refs = references(text);
    assert_eq!(
        refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["host", "version"]
    );
    assert_eq!(
        slice(text, refs[1].location, refs[1].length),
        "{{ version }}"
    );
    assert!(references("plain").is_empty());
}
fn slice(text: &str, start: u64, len: u64) -> String {
    let units: Vec<_> = text.encode_utf16().collect();
    let start = usize::try_from(start).unwrap();
    let len = usize::try_from(len).unwrap();
    String::from_utf16(&units[start..start + len]).unwrap()
}
#[test]
fn utf16_offsets_survive_unicode_and_surrogate_pairs() {
    let text = "café 🚀 {{host}}";
    let r = &references(text)[0];
    assert_eq!(slice(text, r.location, r.length), "{{host}}");
}
#[test]
fn unfinished_reference_completion_obeys_caret_boundaries() {
    let open = open_reference("https://{{ho", 12).unwrap();
    assert_eq!(open.prefix, "ho");
    assert_eq!((open.location, open.length), (10, 2));
    assert!(!open.is_closed);
    let closed = open_reference("{{}}/path", 2).unwrap();
    assert!(closed.is_closed);
    assert!(closed.prefix.is_empty());
    for (text, caret) in [("{{host}}/x", 10), ("{{a b", 5), ("{", 1)] {
        assert!(open_reference(text, caret).is_none());
    }
}
#[test]
fn selected_environment_overrides_globals_and_ignores_disabled() {
    let env = environments();
    let c = catalog(&env, Some("staging"));
    assert_eq!(
        c.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["host", "token", "version"]
    );
    assert_eq!(c[0].display_value, "staging.example.com");
    assert_eq!(c[0].environment_name, "Staging");
    assert_eq!(c[2].environment_name, "Global");
    let globals = catalog(&env, None);
    assert_eq!(globals.len(), 2);
    assert_eq!(globals[0].display_value, "global.example.com");
}
#[test]
fn secret_references_never_expose_names_as_values() {
    let c = catalog(&environments(), Some("staging"));
    assert_eq!(c[1].display_value, "•••• (secret)");
    let text = summary(&c, "Bearer {{token}}").unwrap();
    assert_eq!(text, "token = •••• (secret) (Staging)");
    assert!(!text.contains("staging.token"));
}
#[test]
fn summaries_deduplicate_references_and_flag_undefined_names() {
    let c = catalog(&environments(), Some("production"));
    assert_eq!(
        summary(&c, "{{host}}/{{host}}/{{missing}}").as_deref(),
        Some(
            "host = api.example.com (Production)\nmissing: not defined in the active environments"
        )
    );
    assert_eq!(summary(&c, "no variables"), None);
}
#[test]
fn completions_rank_prefixes_before_substrings() {
    let c = catalog(&environments(), Some("staging"));
    let names = |prefix| {
        completions(&c, prefix)
            .into_iter()
            .map(|e| e.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(""), ["host", "token", "version"]);
    assert_eq!(names("o"), ["host", "token", "version"]);
    assert_eq!(names("T"), ["token", "host"]);
}
#[test]
fn duplicate_environment_rows_follow_order_and_key() {
    let mut e = environment(
        "e",
        "E",
        &[
            ("a", DomainValueSource::literal("first"), true),
            ("a", DomainValueSource::literal("last"), true),
        ],
    );
    assert_eq!(enabled_values(&e)["a"].editable(), "last");
    e.variables[1].enabled = false;
    assert_eq!(enabled_values(&e)["a"].editable(), "first");
}
#[test]
fn chip_delimiters_hide_only_braces_and_padding() {
    let text = "{{base_url}}/orders?limit={{ page_size }}&q=é";
    let refs = references(text);
    let ranges: Vec<_> = refs
        .iter()
        .map(|r| delimiter_ranges(text, r.location, r.length, &r.name))
        .collect();
    assert_eq!(
        ranges
            .iter()
            .map(|r| slice(text, r[0], r[1]))
            .collect::<Vec<_>>(),
        ["{{", "{{ "]
    );
    assert_eq!(
        ranges
            .iter()
            .map(|r| slice(text, r[2], r[3]))
            .collect::<Vec<_>>(),
        ["}}", " }}"]
    );
    for (r, range) in refs.iter().zip(ranges) {
        assert_eq!(
            slice(text, range[0] + range[1], range[2] - range[0] - range[1]),
            r.name
        );
    }
}
#[test]
fn display_segments_keep_plain_text_and_remove_variable_markup() {
    assert_eq!(
        segments("https://{{ host }}/{{id}}"),
        vec![
            TemplateSegment::Text {
                value: "https://".into()
            },
            TemplateSegment::Variable {
                name: "host".into()
            },
            TemplateSegment::Text { value: "/".into() },
            TemplateSegment::Variable { name: "id".into() }
        ]
    );
    assert_eq!(
        segments("plain"),
        vec![TemplateSegment::Text {
            value: "plain".into()
        }]
    );
    assert!(segments("").is_empty());
}
struct NativeComparison;
impl VariableComparison for NativeComparison {
    fn sorted_names(&self, mut names: Vec<String>) -> Vec<String> {
        names.reverse();
        names
    }
    fn name_matches(&self, names: Vec<String>, prefix: String) -> Vec<NameMatch> {
        names
            .into_iter()
            .map(|name| NameMatch {
                has_prefix: name.to_lowercase().starts_with(&prefix.to_lowercase()),
                contains: name.to_lowercase().contains(&prefix.to_lowercase()),
            })
            .collect()
    }
}
#[test]
fn native_comparison_is_batched_and_rust_keeps_ranking_in_snapshot_order() {
    let index = CatalogIndex::new(
        &environments(),
        Some("staging"),
        std::sync::Arc::new(NativeComparison),
    );
    assert_eq!(
        index
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["version", "token", "host"]
    );
    let names: Vec<_> = index
        .completions("T")
        .iter()
        .map(|&i| index.entries[usize::try_from(i).unwrap()].name.as_str())
        .collect();
    assert_eq!(names, ["token", "host"]);
    assert_eq!(
        index.summary("Bearer {{token}}").as_deref(),
        Some("token = •••• (secret) (Staging)")
    );
}
