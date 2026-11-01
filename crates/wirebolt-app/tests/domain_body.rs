use wirebolt_app::domain::{body::*, *};

struct LexicalOrdering;
impl BodyKeyOrdering for LexicalOrdering {
    fn sorted_keys(&self, mut keys: Vec<String>) -> Vec<String> {
        keys.sort();
        keys
    }
}
fn sources() -> Vec<(DomainBody, Option<&'static str>)> {
    vec![
        (DomainBody::Empty, None),
        (
            DomainBody::Text {
                content_type: Some("old/type".into()),
                value: "payload".into(),
            },
            Some("payload"),
        ),
        (
            DomainBody::Json {
                value: "payload".into(),
            },
            Some("payload"),
        ),
        (
            DomainBody::Xml {
                value: "payload".into(),
            },
            Some("payload"),
        ),
        (
            DomainBody::Html {
                value: "payload".into(),
            },
            Some("payload"),
        ),
        (
            DomainBody::Raw {
                content_type: Some("old/type".into()),
                value: "payload".into(),
            },
            Some("payload"),
        ),
        (
            DomainBody::FormUrlEncoded {
                fields: vec![DomainField::default()],
            },
            None,
        ),
        (
            DomainBody::Multipart {
                parts: vec![DomainMultipartPart {
                    name: "old".into(),
                    ..DomainMultipartPart::default()
                }],
            },
            None,
        ),
        (
            DomainBody::File {
                path: "/old/file".into(),
                content_type: Some("old/type".into()),
            },
            None,
        ),
        (
            DomainBody::Text {
                content_type: None,
                value: String::new(),
            },
            Some(""),
        ),
        (
            DomainBody::Json {
                value: String::new(),
            },
            Some(""),
        ),
        (
            DomainBody::Xml {
                value: String::new(),
            },
            Some(""),
        ),
        (
            DomainBody::Html {
                value: String::new(),
            },
            Some(""),
        ),
        (
            DomainBody::Raw {
                content_type: None,
                value: String::new(),
            },
            Some(""),
        ),
    ]
}
const KINDS: [DomainBodyKind; 9] = [
    DomainBodyKind::Empty,
    DomainBodyKind::Text,
    DomainBodyKind::Json,
    DomainBodyKind::Xml,
    DomainBodyKind::Html,
    DomainBodyKind::Raw,
    DomainBodyKind::Form,
    DomainBodyKind::Multipart,
    DomainBodyKind::File,
];

#[test]
fn every_body_transition_preserves_only_text_and_resets_metadata() {
    for (source, previous) in sources() {
        for target in KINDS {
            let value = previous.unwrap_or_default().to_owned();
            let actual = switching(source.clone(), target);
            let expected = match target {
                DomainBodyKind::Empty => DomainBody::Empty,
                DomainBodyKind::Text => DomainBody::Text {
                    content_type: None,
                    value,
                },
                DomainBodyKind::Json => DomainBody::Json {
                    value: if value.is_empty() {
                        "{\n  \n}".into()
                    } else {
                        value
                    },
                },
                DomainBodyKind::Xml => DomainBody::Xml { value },
                DomainBodyKind::Html => DomainBody::Html { value },
                DomainBodyKind::Raw => DomainBody::Raw {
                    content_type: None,
                    value,
                },
                DomainBodyKind::Form => DomainBody::FormUrlEncoded { fields: vec![] },
                DomainBodyKind::Multipart if value.is_empty() => {
                    DomainBody::Multipart { parts: vec![] }
                }
                DomainBodyKind::Multipart => {
                    let DomainBody::Multipart { parts } = &actual else {
                        panic!("expected multipart")
                    };
                    assert_eq!(parts.len(), 1);
                    assert!(!parts[0].id.is_empty());
                    assert_eq!(parts[0].name, "name");
                    assert_eq!(parts[0].kind, MultipartPartKind::Text);
                    assert_eq!(parts[0].value, DomainValueSource::literal(value));
                    assert!(parts[0].enabled);
                    assert_eq!(
                        (
                            &parts[0].file_path,
                            &parts[0].file_name,
                            &parts[0].content_type
                        ),
                        (&None, &None, &None)
                    );
                    let DomainBody::Multipart { parts: again } = switching(source.clone(), target)
                    else {
                        unreachable!()
                    };
                    assert_ne!(parts[0].id, again[0].id);
                    continue;
                }
                DomainBodyKind::File => DomainBody::File {
                    path: String::new(),
                    content_type: None,
                },
            };
            assert_eq!(actual, expected, "{source:?} -> {target:?}");
        }
    }
}
#[test]
fn composer_displays_legacy_text_as_raw_and_exposes_formatting_only_for_markup() {
    for ((body, _), expected) in sources().into_iter().take(9).zip([
        DomainBodyKind::Empty,
        DomainBodyKind::Raw,
        DomainBodyKind::Json,
        DomainBodyKind::Xml,
        DomainBodyKind::Html,
        DomainBodyKind::Raw,
        DomainBodyKind::Form,
        DomainBodyKind::Multipart,
        DomainBodyKind::File,
    ]) {
        assert_eq!(kind(&body), expected);
        assert_eq!(
            can_format(&body),
            matches!(
                expected,
                DomainBodyKind::Json | DomainBodyKind::Xml | DomainBodyKind::Html
            )
        );
    }
}
#[test]
fn formatting_preserves_unsupported_bodies_and_only_splits_adjacent_markup_tags() {
    for (body, _) in sources() {
        if !can_format(&body) {
            assert_eq!(formatted(body.clone(), &LexicalOrdering), body);
        }
    }
    let markup = "<a><b>text</b></a> <c>\n<d/>";
    let expected = "<a>\n<b>text</b>\n</a> <c>\n<d/>";
    assert_eq!(
        formatted(
            DomainBody::Xml {
                value: markup.into()
            },
            &LexicalOrdering
        ),
        DomainBody::Xml {
            value: expected.into()
        }
    );
    assert_eq!(
        formatted(
            DomainBody::Html {
                value: markup.into()
            },
            &LexicalOrdering
        ),
        DomainBody::Html {
            value: expected.into()
        }
    );
}
#[test]
fn json_formatting_matches_foundation_whitespace_strings_sorting_and_numbers() {
    for (source, expected) in [
        ("{}", "{\n\n}"),
        ("[]", "[\n\n]"),
        (
            r#"{"b":[1,{},[],2],"a":"https://a/b\n東京\t"}"#,
            "{\n  \"a\" : \"https:\\/\\/a\\/b\\n東京\\t\",\n  \"b\" : [\n    1,\n    {\n\n    },\n    [\n\n    ],\n    2\n  ]\n}",
        ),
        (
            "[true,null,1e-7,1.2300,9007199254740993]",
            "[\n  true,\n  null,\n  9.9999999999999995e-08,\n  1.23,\n  9007199254740993\n]",
        ),
        (
            "[-0,-0.0,1e-5,1e-4,1e15,1e16,1e17,1e18,1e19,1e20,1.2e-7,0.1]",
            "[\n  0,\n  -0,\n  1.0000000000000001e-05,\n  0.0001,\n  1000000000000000,\n  10000000000000000,\n  1e+17,\n  1e+18,\n  1e+19,\n  1e+20,\n  1.1999999999999999e-07,\n  0.10000000000000001\n]",
        ),
        (r#"{"a":1,"a":2}"#, "{\n  \"a\" : 2\n}"),
        (
            r#"["\u2028\u2029\u0001\u000b\u001f"]"#,
            "[\n  \"\u{2028}\u{2029}\\u0001\\u000b\\u001f\"\n]",
        ),
    ] {
        assert_eq!(
            formatted(
                DomainBody::Json {
                    value: source.into()
                },
                &LexicalOrdering
            ),
            DomainBody::Json {
                value: expected.into()
            },
            "{source}"
        );
    }
}
#[test]
fn json_fragments_and_invalid_documents_are_unchanged() {
    for source in [
        "",
        "garbage",
        "42",
        "null",
        "true",
        "\"text\"",
        "[,1]",
        "[1,,]",
        "{\"a\":,}",
        "{",
        "{} trailing",
        "[1e999]",
    ] {
        let body = DomainBody::Json {
            value: source.into(),
        };
        assert_eq!(formatted(body.clone(), &LexicalOrdering), body);
    }
}
#[test]
fn json_key_collation_is_batched_once_for_the_complete_document() {
    use std::sync::Mutex;
    struct NativeOrder(Mutex<Vec<Vec<String>>>);
    impl BodyKeyOrdering for NativeOrder {
        fn sorted_keys(&self, keys: Vec<String>) -> Vec<String> {
            self.0.lock().unwrap().push(keys);
            ["a", "A", "Ä", "a2", "a10"].map(str::to_owned).into()
        }
    }
    let ordering = NativeOrder(Mutex::new(vec![]));
    assert_eq!(formatted(DomainBody::Json { value: r#"{"a10":10,"A":{"Ä":1,"a":2},"a2":2,"a":1}"#.into() }, &ordering),
        DomainBody::Json { value: "{\n  \"a\" : 1,\n  \"A\" : {\n    \"a\" : 2,\n    \"Ä\" : 1\n  },\n  \"a2\" : 2,\n  \"a10\" : 10\n}".into() });
    assert_eq!(
        ordering.0.lock().unwrap().as_slice(),
        &[vec!["A", "a", "a10", "a2", "Ä"]]
    );
}

#[test]
fn json_preserves_foundation_decimal_precision_and_trailing_comma_tolerance() {
    for (source, expected) in [
        ("[1,]", "[\n  1\n]"),
        (r#"{"a":1,}"#, "{\n  \"a\" : 1\n}"),
        ("[1.23456789012345678]", "[\n  1.23456789012345678\n]"),
        ("[0.000000000000000001]", "[\n  0.000000000000000001\n]"),
        ("[0.10000000000000000]", "[\n  0.10000000000000001\n]"),
        ("[1.00000000000000000]", "[\n  1\n]"),
        (
            "[123456789012345678901234567890]",
            "[\n  123456789012345678901234567890\n]",
        ),
        ("[18446744073709551616]", "[\n  18446744073709551616\n]"),
        ("[1.234567890123456789e10]", "[\n  12345678901.23456789\n]"),
        (
            "[340282366920938463463374607431768211455]",
            "[\n  340282366920938463463374607431768211455\n]",
        ),
        (
            "[340282366920938463463374607431768211456]",
            "[\n  340282366920938463463374607431768211450\n]",
        ),
    ] {
        assert_eq!(
            formatted(
                DomainBody::Json {
                    value: source.into()
                },
                &LexicalOrdering
            ),
            DomainBody::Json {
                value: expected.into()
            },
            "{source}"
        );
    }
    for source in ["[1.23456789012345678e-129]", "[1.23456789012345678e160]"] {
        let body = DomainBody::Json {
            value: source.into(),
        };
        assert_eq!(formatted(body.clone(), &LexicalOrdering), body);
    }
}
