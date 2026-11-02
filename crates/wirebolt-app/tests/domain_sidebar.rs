mod support;
use std::collections::HashSet;
use support::*;
use wirebolt_app::domain::{models::*, sidebar::*};
fn visible(
    index: &SidebarIndex,
    query: &str,
    collapsed: &[&str],
    expanded: &[&str],
) -> Vec<SidebarRow> {
    index
        .visible(
            query,
            &collapsed.iter().map(|s| (*s).into()).collect(),
            &expanded.iter().map(|s| (*s).into()).collect(),
        )
        .into_iter()
        .map(|i| index.rows[usize::try_from(i).unwrap()].clone())
        .collect()
}
fn ids(rows: &[SidebarRow]) -> Vec<&str> {
    rows.iter().map(|r| r.id.as_str()).collect()
}
#[test]
fn rows_carry_paths_without_parsing_during_rendering() {
    let index = SidebarIndex::new(&[collection()]);
    assert_eq!(
        index
            .rows
            .iter()
            .find(|r| r.id == "request:c/inside")
            .unwrap()
            .request_path,
        "/needle"
    );
    assert!(index.rows[0].request_path.is_empty());
    let mut c = collection();
    c.id = "workspace-root".into();
    c.requests = vec![location("empty", "Empty", "", None)];
    assert!(
        SidebarIndex::new(&[c])
            .rows
            .iter()
            .find(|r| r.request_index.is_some())
            .unwrap()
            .request_path
            .is_empty()
    );
}
#[test]
fn expansion_and_filtering_preserve_ancestors_and_identity() {
    let index = SidebarIndex::new(&[collection()]);
    assert_eq!(
        ids(&visible(&index, "", &[], &[])),
        ["collection:c", "group:c:folder", "request:c/root"]
    );
    let expanded = visible(&index, "", &[], &["c:folder", "c:nested"]);
    assert_eq!(
        expanded.iter().map(|r| r.depth).collect::<Vec<_>>(),
        [0, 1, 2, 3, 1]
    );
    for q in ["cafe", "東京", "needle", "GET", "Collection", "Folder"] {
        let matches = visible(&index, q, &["c"], &[]);
        assert!(ids(&matches).contains(&"request:c/inside"));
        assert_eq!(matches[0].id, "collection:c");
        assert!(
            matches
                .iter()
                .all(|r| expanded.iter().any(|e| e.id == r.id))
        );
    }
    assert!(visible(&index, "missing", &[], &[]).is_empty());
    assert_eq!(visible(&index, "", &["c"], &[]).len(), 1);
}
#[test]
fn filtering_matches_exact_normalized_search() {
    let names = [
        "Café 東京",
        "Cafe\u{301} decomposed",
        "Kelvin \u{212a}",
        "Greek\u{037e}question",
        "a\u{20dd} circled",
        "👨‍👩‍👧 family",
        "🇺🇸 flag",
        "Plain GET",
        "ÅNGSTRÖM",
        "naïve",
    ];
    let c = DomainCollection {
        id: "c".into(),
        name: "§".into(),
        requests: names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                location(
                    &format!("r{i}"),
                    n,
                    &format!("https://example.invalid/{i}"),
                    None,
                )
            })
            .collect(),
        ..DomainCollection::default()
    };
    let index = SidebarIndex::new(&[c]);
    for query in [
        "cafe",
        "café",
        "e",
        "k",
        "K",
        ";",
        "a",
        "👨",
        "🇺",
        "get",
        "angstrom",
        "naive",
        "ï",
        "東京",
        "example.invalid/3",
        "missing",
        " cafe ",
        "\u{212a}",
        "\u{037e}",
    ] {
        let norm = normalize(query.trim());
        let expected: Vec<_> = index
            .rows
            .iter()
            .filter(|r| r.request_index.is_some() && matches(&r.search, &norm))
            .map(|r| r.id.clone())
            .collect();
        let actual: Vec<_> = visible(&index, query, &[], &[])
            .into_iter()
            .filter(|r| r.request_index.is_some())
            .map(|r| r.id)
            .collect();
        assert_eq!(actual, expected, "query {query}");
    }
}
#[test]
fn ascii_normalization_preserves_every_ascii_byte_except_uppercase() {
    let every: String = (0_u8..128).map(char::from).collect();
    for text in [
        &every,
        "Request 42 GET https://Example.INVALID/Path?Q=1",
        "",
        "MiXeD\0Case",
    ] {
        assert_eq!(normalize(text), text.to_ascii_lowercase());
    }
}
#[test]
fn root_collection_has_no_header_and_tied_order_is_deterministic() {
    let mut c = collection();
    c.id = "workspace-root".into();
    let index = SidebarIndex::new(&[c]);
    assert_eq!(index.rows[0].depth, 0);
    assert!(
        index
            .rows
            .iter()
            .all(|r| r.group_index.is_some() || r.request_index.is_some())
    );
    assert_eq!(
        index.rows.len(),
        index
            .rows
            .iter()
            .map(|r| &r.id)
            .collect::<HashSet<_>>()
            .len()
    );
}
#[test]
fn rows_know_parent_title_and_move_identity() {
    let index = SidebarIndex::new(&[collection()]);
    assert_eq!(
        index
            .rows
            .iter()
            .map(|r| r.parent_id.as_deref())
            .collect::<Vec<_>>(),
        [
            None,
            Some("collection:c"),
            Some("group:c:folder"),
            Some("group:c:nested"),
            Some("collection:c")
        ]
    );
    assert_eq!(
        index
            .rows
            .iter()
            .map(|r| r.title.as_str())
            .collect::<Vec<_>>(),
        ["Collection", "Folder", "Nested", "Café 東京", "Root"]
    );
    assert_eq!(
        index
            .rows
            .iter()
            .map(|r| r.move_identifier.as_deref())
            .collect::<Vec<_>>(),
        [
            None,
            Some("group|c|folder"),
            Some("group|c|nested"),
            Some("request|c|inside"),
            Some("request|c|root")
        ]
    );
}
#[test]
fn arrow_keys_visit_rows_and_follow_outline_conventions() {
    let index = SidebarIndex::new(&[collection()]);
    let closed = visible(&index, "", &[], &[]);
    assert_eq!(step(&closed, None, 1).as_deref(), Some("collection:c"));
    assert_eq!(step(&closed, None, -1).as_deref(), Some("request:c/root"));
    assert_eq!(
        step(&closed, Some("collection:c"), 1).as_deref(),
        Some("group:c:folder")
    );
    assert_eq!(
        step(&closed, Some("request:c/root"), 1).as_deref(),
        Some("request:c/root")
    );
    assert_eq!(
        arrow(&closed, Some("group:c:folder"), true, false),
        SidebarOutcome::Expand {
            id: "group:c:folder".into()
        }
    );
    let rows = visible(&index, "", &[], &["c:folder", "c:nested"]);
    assert_eq!(
        arrow(&rows, Some("group:c:folder"), true, true),
        SidebarOutcome::Select {
            id: "group:c:nested".into()
        }
    );
    assert_eq!(
        step(&rows, Some("group:c:nested"), 1).as_deref(),
        Some("request:c/inside")
    );
    assert_eq!(
        arrow(&rows, Some("request:c/inside"), true, false),
        SidebarOutcome::None
    );
    assert_eq!(
        arrow(&rows, Some("request:c/inside"), false, false),
        SidebarOutcome::Select {
            id: "group:c:nested".into()
        }
    );
    assert_eq!(
        arrow(&rows, Some("group:c:nested"), false, true),
        SidebarOutcome::Collapse {
            id: "group:c:nested".into()
        }
    );
    assert_eq!(
        arrow(&rows, Some("group:c:nested"), false, false),
        SidebarOutcome::Select {
            id: "group:c:folder".into()
        }
    );
    assert_eq!(
        arrow(&rows, Some("collection:c"), false, true),
        SidebarOutcome::Collapse {
            id: "collection:c".into()
        }
    );
}
#[test]
fn type_select_cycles_titles_and_folds_diacritics() {
    let mut c = collection();
    let mut second = location("second", "Reports", "", None);
    second.order = 1;
    c.requests.push(second);
    let index = SidebarIndex::new(&[c]);
    let rows = visible(&index, "", &[], &["c:folder", "c:nested"]);
    for (prefix, current, expected) in [
        ("r", None, Some("request:c/root")),
        ("r", Some("request:c/root"), Some("request:c/second")),
        ("r", Some("request:c/second"), Some("request:c/root")),
        ("rep", Some("request:c/root"), Some("request:c/second")),
        ("cafe", None, Some("request:c/inside")),
        ("zzz", None, None),
    ] {
        assert_eq!(type_select(&rows, current, prefix).as_deref(), expected);
    }
}
#[test]
fn subtrees_and_outlines_keep_parents_before_children() {
    let c = collection();
    for root in [None, Some("folder")] {
        let ids: Vec<_> = subtree(&c, root)
            .iter()
            .map(|&i| c.groups[usize::try_from(i).unwrap()].id.as_str())
            .collect();
        assert_eq!(ids, ["folder", "nested"]);
    }
    assert_eq!(folder_outline(&c), vec![vec![1, 0], vec![0, 1]]);
    assert_eq!(ordered_children(&c, None), ["group:folder", "request:root"]);
}
#[test]
fn siblings_skip_descendants_and_stay_in_their_collection() {
    let mut c = collection();
    let mut last = location("last", "Last", "", None);
    last.order = 1;
    c.requests.push(last);
    let mut other = collection();
    other.id = "d".into();
    other.order = 1;
    other.groups = vec![];
    let mut r = location("other", "Other", "", None);
    r.collection_id = "d".into();
    other.requests = vec![r];
    let index = SidebarIndex::new(&[c, other]);
    let sibling = |id, delta| {
        index
            .sibling(id, delta)
            .map(|i| index.rows[usize::try_from(i).unwrap()].id.as_str())
    };
    assert_eq!(sibling("request:c/root", -1), Some("group:c:folder"));
    assert_eq!(sibling("group:c:folder", 1), Some("request:c/root"));
    assert_eq!(sibling("request:c/last", 1), None);
    assert_eq!(sibling("group:c:nested", -1), None);
    assert_eq!(sibling("request:d/other", -1), None);
}

#[test]
fn displayed_request_paths_preserve_templates_escapes_and_authorities() {
    for (url, expected) in [
        ("{{base_url}}/products?limit=20", "/products"),
        (
            "{{ base_url }}/products/{{id}}?limit=20#details",
            "/products/{{id}}",
        ),
        ("{{base_url}}", "/"),
        ("{{base_url}}?limit=20#details", "/"),
        ("https://api.example.com/v1/users/42", "/v1/users/42"),
        ("https://api.example.com?redirect=/elsewhere#details", "/"),
        ("https://api.example.com/v1/#details?limit=20", "/v1/"),
        ("http://localhost:8080/health?full=true", "/health"),
        ("https://[2001:db8::1]:8443/v1/users#details", "/v1/users"),
        ("http://[::1]:8080", "/"),
        ("ws://127.0.0.1:18990/events", "/events"),
        ("wss://[::1]:18990/events?token=secret", "/events"),
        ("https://{{host}}:{{port}}/users/{{id}}", "/users/{{id}}"),
        ("https://example.com/a%2Fb/%23tag?q=%2F", "/a%2Fb/%23tag"),
        ("//example.com:8080/products", "/products"),
        ("/products?limit=20#details", "/products"),
        ("  https://example.com/products  \n", "/products"),
        ("", ""),
        (" \n ", ""),
        ("not a URL", ""),
        ("{{unfinished", ""),
        ("://invalid", ""),
    ] {
        assert_eq!(request_path(url), expected, "{url}");
    }
}

#[test]
fn filtering_does_not_match_partial_emoji_clusters() {
    assert!(!matches("👨‍👩‍👧 family", "👨"));
    assert!(!matches("🇺🇸 flag", "🇺"));
    assert!(matches("👨‍👩‍👧 family", "family"));
}
#[test]
fn canonical_equivalent_sibling_names_sort_by_identity() {
    let mut c = collection();
    c.groups = vec![];
    c.requests = vec![
        location("z", "é", "", None),
        location("a", "e\u{301}", "", None),
    ];
    let index = SidebarIndex::new(&[c.clone()]);
    assert_eq!(
        ids(&index.rows),
        ["collection:c", "request:c/a", "request:c/z"]
    );
    assert_eq!(ordered_children(&c, None), ["request:a", "request:z"]);
}

#[test]
fn folder_drop_policy_preserves_request_and_descendant_rules() {
    let c = DomainSidebarCollection::from(&collection());
    assert!(can_drop_inside(&c, "nested", "request|other|request"));
    assert!(!can_drop_inside(&c, "nested", "group|c|folder"));
    assert!(!can_drop_inside(&c, "folder", "group|c|folder"));
    assert!(can_drop_inside(&c, "folder", "group|c|nested"));
    assert!(!can_drop_inside(&c, "folder", "group|other|nested"));
    for malformed in ["", "group|c", "request|c|r|extra", "collection|c|folder"] {
        assert!(!can_drop_inside(&c, "folder", malformed));
    }
}
#[test]
fn move_menu_orders_collections_and_limits_folders_to_their_tree() {
    let mut first = DomainSidebarCollection::from(&collection());
    first.id = "z".into();
    first.name = "Cafe\u{301}".into();
    first.order = 1;
    let mut tied = first.clone();
    tied.id = "a".into();
    tied.name = "Café".into();
    let mut root = first.clone();
    root.id = "workspace-root".into();
    root.order = 0;
    let collections = [first, tied, root];
    assert_eq!(
        move_collections(&collections, "request|z|request"),
        [2, 1, 0]
    );
    assert_eq!(move_collections(&collections, "group|z|folder"), [0]);
    assert!(move_collections(&collections, "group|missing|folder").is_empty());
}
#[test]
fn environment_picker_excludes_globals_and_stably_orders_canonical_names() {
    let names = [
        ("global", "Global"),
        ("z", "Zulu"),
        ("a", "Café"),
        ("b", "Cafe\u{301}"),
        ("c", "Alpha"),
    ];
    let environments: Vec<_> = names
        .into_iter()
        .map(|(id, name)| DomainEnvironment {
            id: id.into(),
            name: name.into(),
            variables: vec![],
        })
        .collect();
    assert_eq!(selectable_environments(&environments), [4, 2, 3, 1]);
}
