use super::*;
fn tree(text: &str) -> JsonTreeNode {
    json_tree(text.as_bytes(), 200_000, &cancellation()).unwrap()
}
#[test]
fn scalar_types() {
    let children = tree(r#"{"zero":0,"one":1,"yes":true,"no":false}"#)
        .children
        .unwrap();
    assert_eq!(children[0].kind, JsonNodeKind::Number);
    assert_eq!(children[0].value, "0");
    assert_eq!(children[1].value, "1");
    assert_eq!(children[2].kind, JsonNodeKind::Boolean);
    assert_eq!(children[3].value, "false");
}
#[test]
fn preserves_key_order() {
    let keys = ["zeta", "alpha", "middle", "b", "a", "alpha"];
    let json = format!(
        "{{{}}}",
        keys.iter()
            .enumerate()
            .map(|(i, k)| format!("\"{k}\":{i}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let root = tree(&json);
    assert_eq!(root.value, "Object(6 items)");
    assert_eq!(
        root.children
            .unwrap()
            .iter()
            .map(|n| n.key.as_str())
            .collect::<Vec<_>>(),
        keys
    );
}
#[test]
fn scalar_spelling() {
    let children = tree(r#"[9007199254740993, 1e+999, -0.50, "café 🚀 \"q\"", null]"#)
        .children
        .unwrap();
    assert_eq!(
        children
            .iter()
            .map(|n| n.value.as_str())
            .collect::<Vec<_>>(),
        [
            "9007199254740993",
            "1e+999",
            "-0.50",
            "café 🚀 \"q\"",
            "null"
        ]
    );
    assert_eq!(
        children.iter().map(|n| n.key.as_str()).collect::<Vec<_>>(),
        ["0", "1", "2", "3", "4"]
    );
    assert_eq!(children[2].id, "root/2");
}
#[test]
fn invalid_tree() {
    for json in [
        "",
        "{",
        "[1,]",
        r#"{"a":}"#,
        "[01]",
        "tru",
        "[1] 2",
        "[\"a\nb\"]",
    ] {
        assert!(
            json_tree(json.as_bytes(), 200_000, &cancellation()).is_err(),
            "{json}"
        );
    }
}
#[test]
fn node_limit() {
    let json = format!(
        "{{\"items\":[{}],\"after\":true}}",
        (0..100)
            .map(|i| format!("{{\"id\":{i}}}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let root = json_tree(json.as_bytes(), 10, &cancellation()).unwrap();
    let children = root.children.unwrap();
    let items = &children[0];
    assert_eq!(items.value, "Array(100 items)");
    let last = items.children.as_ref().unwrap().last().unwrap();
    assert_eq!(last.kind, JsonNodeKind::Truncated);
    assert!(last.value.ends_with("more items not shown"));
    assert_eq!(children.last().unwrap().kind, JsonNodeKind::Truncated);
}
#[test]
fn preserves_tokens() {
    let raw = r#"{"z":9007199254740993,"z":1e+999,"nested":[true,false,null,{},[]],"s":"a\"b\\c\n\u6771京🚀"}"#;
    let (_root, mut store) = body(raw.as_bytes());
    let doc = store
        .formatted_json(false, &cancellation())
        .unwrap()
        .unwrap();
    let expected = "{\n  \"z\": 9007199254740993,\n  \"z\": 1e+999,\n  \"nested\": [\n    true,\n    false,\n    null,\n    {},\n    []\n  ],\n  \"s\": \"a\\\"b\\\\c\\n\\u6771京🚀\"\n}";
    assert_eq!(doc.preview, expected);
    assert_eq!(std::fs::read_to_string(doc.path()).unwrap(), expected);
    assert_eq!(
        store.viewport(0, super::super::count(raw.len())).unwrap(),
        raw.as_bytes()
    );
    assert!(Arc::ptr_eq(
        &doc,
        &store
            .formatted_json(false, &cancellation())
            .unwrap()
            .unwrap()
    ));
}
#[test]
fn invalid_json_falls_back_without_changing_body() {
    for raw in [
        "",
        "{",
        "[1,]",
        r#"{"a":}"#,
        r#"{"a":1,}"#,
        "[01]",
        "[1.]",
        "[1e]",
        "[-]",
        "[+1]",
        "true false",
        r#"["\x"]"#,
        r#"["\uZZZZ"]"#,
        "[\"a\nb\"]",
        "[NaN]",
    ] {
        let (_root, mut store) = body(raw.as_bytes());
        assert!(
            store
                .formatted_json(false, &cancellation())
                .unwrap()
                .is_none(),
            "{raw}"
        );
        assert_eq!(
            store.viewport(0, super::super::count(raw.len())).unwrap(),
            raw.as_bytes()
        );
    }
}
#[test]
fn scalars() {
    for raw in ["null", "-12.50e-9", "true", r#""hello""#, "{}", "[]"] {
        let (_root, mut store) = body(format!(" \n{raw}\r\t").as_bytes());
        assert_eq!(
            store
                .formatted_json(false, &cancellation())
                .unwrap()
                .unwrap()
                .preview,
            raw
        );
    }
}
#[test]
fn chunk_boundaries() {
    let value = format!("{}🚀東京{}", "a".repeat(65_525), "b".repeat(70_000));
    let raw = format!("{{\"value\":\"{value}\",\"n\":-0.002E+04}}");
    let (_root, mut store) = body(raw.as_bytes());
    assert_eq!(
        store
            .formatted_json(false, &cancellation())
            .unwrap()
            .unwrap()
            .preview,
        format!("{{\n  \"value\": \"{value}\",\n  \"n\": -0.002E+04\n}}")
    );
}
#[test]
fn large_response() {
    let raw = format!(
        "[{}]",
        vec![r#"{"id":9007199254740993,"ok":true}"#; 35_000].join(",")
    );
    let (_root, mut store) = body(raw.as_bytes());
    let document = store
        .formatted_json(false, &cancellation())
        .unwrap()
        .unwrap();
    assert!(document.byte_count > 1024 * 1024);
    assert!(document.preview.len() <= 32 * 1024);
    let pretty = std::fs::read_to_string(document.path()).unwrap();
    assert!(pretty.ends_with("\n  }\n]"));
    assert_eq!(pretty.matches("9007199254740993").count(), 35_000);
}
#[test]
fn invalid_encoding_and_depth() {
    for bytes in [
        vec![34, 0xc0, 0xaf, 34],
        vec![34, 0xed, 0xa0, 0x80, 34],
        format!("{}0{}", "[".repeat(258), "]".repeat(258)).into_bytes(),
    ] {
        let (_root, mut store) = body(&bytes);
        assert!(
            store
                .formatted_json(false, &cancellation())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store.viewport(0, super::super::count(bytes.len())).unwrap(),
            bytes
        );
    }
}
#[test]
fn decodes_unicode_escapes() {
    let raw = r#"{"name":"caf\u00e9","rocket":"\ud83d\ude80","quote":"\u0022\u005c","control":"\u0000\u202e","lone":"\ud800x","mixed":"\ud83d\n"}"#;
    let (_root, mut store) = body(raw.as_bytes());
    let document = store
        .formatted_json(true, &cancellation())
        .unwrap()
        .unwrap();
    assert!(document.contains_unicode_escapes);
    assert_eq!(
        document.preview,
        "{\n  \"name\": \"café\",\n  \"rocket\": \"🚀\",\n  \"quote\": \"\\u0022\\u005c\",\n  \"control\": \"\\u0000\\u202e\",\n  \"lone\": \"\\ud800x\",\n  \"mixed\": \"\\ud83d\\n\"\n}"
    );
    assert!(
        store
            .formatted_json(false, &cancellation())
            .unwrap()
            .unwrap()
            .preview
            .contains(r#""caf\u00e9""#)
    );
    assert_eq!(
        store.viewport(0, super::super::count(raw.len())).unwrap(),
        raw.as_bytes()
    );
}
#[test]
fn shares_presentation_without_escapes() {
    let (_root, mut store) = body(r#"{"a":"café"}"#.as_bytes());
    let decoded = store
        .formatted_json(true, &cancellation())
        .unwrap()
        .unwrap();
    assert!(!decoded.contains_unicode_escapes);
    assert!(Arc::ptr_eq(
        &decoded,
        &store
            .formatted_json(false, &cancellation())
            .unwrap()
            .unwrap()
    ));
}
#[test]
fn formats_only_complete_bodies() {
    let root = tempfile::tempdir().unwrap();
    let mut store = BodyStore::new("stream", root.path()).unwrap();
    store.append(br#"{"a":"caf"#).unwrap();
    assert!(
        store
            .formatted_json(true, &cancellation())
            .unwrap()
            .is_none()
    );
    store.append(br#"\u00e9"}"#).unwrap();
    store.finish().unwrap();
    assert_eq!(
        store
            .formatted_json(true, &cancellation())
            .unwrap()
            .unwrap()
            .preview,
        "{\n  \"a\": \"café\"\n}"
    );
    assert_eq!(
        store
            .formatted_json(false, &cancellation())
            .unwrap()
            .unwrap()
            .preview,
        "{\n  \"a\": \"caf\\u00e9\"\n}"
    );
}

#[test]
fn streams_long_numeric_tokens_and_file_trees() {
    let raw = "9".repeat(200_000);
    let (_root, mut store) = body(raw.as_bytes());
    let document = store
        .formatted_json(false, &cancellation())
        .unwrap()
        .unwrap();
    assert_eq!(document.preview, raw);
    assert_eq!(document.byte_count, 200_000);
    let (_tree_root, tree_store) = body(b"\xef\xbb\xbf{\"z\":1,\"a\":2,\"z\":3}");
    let root = json_tree_file(&tree_store.path(), 200_000, &cancellation()).unwrap();
    assert_eq!(
        root.children
            .unwrap()
            .iter()
            .map(|n| n.key.as_str())
            .collect::<Vec<_>>(),
        ["z", "a", "z"]
    );
}
#[test]
fn unmatched_surrogate_does_not_consume_a_following_valid_pair() {
    let raw = r#"["\ud800\ud83d\ude80"]"#;
    assert_eq!(tree(raw).children.unwrap()[0].value, "�🚀");
    let (_root, mut store) = body(raw.as_bytes());
    assert_eq!(
        store
            .formatted_json(true, &cancellation())
            .unwrap()
            .unwrap()
            .preview,
        "[\n  \"\\ud800\\ud83d\\ude80\"\n]"
    );
}
