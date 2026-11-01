use super::*;
fn projection(source: &str, collapsed: Vec<u64>, syntax: CodeSyntax) -> CodeDocument {
    CodeDocument::new(source.into(), collapsed, syntax, &cancellation()).unwrap()
}
#[test]
fn incremental_edits_match_full_folding() {
    for replacement in [" ", "café", "東京🚀", "123", ""] {
        let source = "{\n \"message\": \"hello\",\n \"items\": [\n  1, 2\n ]\n}";
        let initial = projection(source, vec![], CodeSyntax::Json);
        let range = ResponseTextRange {
            location: 15,
            length: 5,
        };
        let changed = super::super::text::replace_utf16(source, range, replacement);
        let updated = initial.updating_edit(range, replacement, "hello").unwrap();
        assert_eq!(
            updated.folds,
            projection(&changed, vec![], CodeSyntax::Json).folds
        );
        assert_eq!(updated.text.as_str(), changed);
    }
}
#[test]
fn structural_edits_require_reparsing() {
    for replacement in ["\n", "\r", "\\", "\"", "{", "}", "[", "]"] {
        let initial = projection("{\n 1\n}", vec![], CodeSyntax::Json);
        assert!(
            initial
                .updating_edit(
                    ResponseTextRange {
                        location: 2,
                        length: 0
                    },
                    replacement,
                    ""
                )
                .is_none()
        );
    }
}
#[test]
fn ignores_brackets_inside_escaped_strings_and_keeps_original_line_numbers() {
    let source = "{\n  \"quoted\": \"[\\\"{\",\n  \"items\": [\n    0, 1, true, \"東京🚀\"\n  ]\n}";
    let p = projection(source, vec![2], CodeSyntax::Json);
    assert_eq!(p.folds.iter().map(|f| f.line).collect::<Vec<_>>(), [0, 2]);
    assert_eq!(
        p.text.as_str(),
        "{\n  \"quoted\": \"[\\\"{\",\n  \"items\": […]\n}"
    );
    assert_eq!(
        p.replacing(ResponseTextRange::default(), " "),
        format!(" {source}")
    );
    let location = super::super::count(p.text[..p.text.find('…').unwrap()].encode_utf16().count());
    assert_eq!(
        p.replacing(
            ResponseTextRange {
                location,
                length: 1
            },
            "2"
        ),
        "{\n  \"quoted\": \"[\\\"{\",\n  \"items\": [2]\n}"
    );
}
#[test]
fn nested_folds_round_trip_unicode_and_cross_fold_replacement() {
    let source = "{\n \"🚀\": [\n  0,\n  1\n ]\n}";
    let p = projection(source, vec![0, 1], CodeSyntax::Json);
    assert_eq!(p.text.as_str(), "{…}");
    assert_eq!(
        p.replacing(
            ResponseTextRange {
                location: 0,
                length: 3
            },
            "[]"
        ),
        "[]"
    );
    assert_eq!(
        projection(source, vec![], CodeSyntax::Json).text.as_str(),
        source
    );
    let p = projection(source, vec![1], CodeSyntax::Json);
    let source_len = super::super::count(source.encode_utf16().count());
    let display_len = super::super::count(p.text.encode_utf16().count());
    assert_eq!(p.source_offset(display_len, false), source_len);
    assert_eq!(p.display_offset(source_len), display_len);
}
#[test]
fn markup_preserves_quoted_attributes_comments_cdata_and_unicode() {
    let source = "<root attr=\">\">\n  <ns:item>\n    東京🚀\n  </ns:item>\n  <!-- <fake>\n  </fake> -->\n  <![CDATA[<fake>\n  </fake>]]>\n</root>";
    let p = projection(source, vec![1], CodeSyntax::Xml);
    assert_eq!(
        p.folds.iter().map(|f| f.line).collect::<Vec<_>>(),
        [0, 1, 4, 6]
    );
    assert!(p.text.contains("<ns:item>…</ns:item>"));
    assert_eq!(
        p.replacing(ResponseTextRange::default(), " "),
        format!(" {source}")
    );
    assert_eq!(
        projection(source, vec![0], CodeSyntax::Xml).text.as_str(),
        "<root attr=\">\">…</root>"
    );
}
#[test]
fn html_void_elements_and_raw_text_do_not_create_false_folds() {
    let source = "<DIV>\n<br><img src=\"a>b\"><input>\n<SCRIPT>\nif (a < b) { value = '<fake>'; }\n</script>\n<p>\nbody\n</p>\n</div>";
    let p = projection(source, vec![2], CodeSyntax::Html);
    assert_eq!(
        p.folds.iter().map(|f| f.line).collect::<Vec<_>>(),
        [0, 2, 5]
    );
    assert!(p.text.contains("<SCRIPT>…</script>"));
    assert!(
        projection("<root>\n<unclosed>", vec![], CodeSyntax::Xml)
            .folds
            .is_empty()
    );
    assert!(
        projection("{\ntext\n}", vec![], CodeSyntax::None)
            .folds
            .is_empty()
    );
}

#[test]
fn incremental_projection_edits_keep_old_snapshots_and_repair_partial_surrogates() {
    let initial = projection("🚀 text", vec![], CodeSyntax::None);
    let changed = initial
        .updating_edit(
            ResponseTextRange {
                location: 1,
                length: 0,
            },
            "x",
            "",
        )
        .unwrap();
    assert_eq!(initial.source.as_str(), "🚀 text");
    assert_eq!(initial.text.as_str(), "🚀 text");
    assert_eq!(changed.source.as_str(), "�x� text");
    assert_eq!(changed.text.as_str(), "�x� text");
    assert_eq!(changed.source_offset(u64::MAX, false), 8);
}
