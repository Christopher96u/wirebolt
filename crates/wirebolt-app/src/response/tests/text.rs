use super::*;
#[test]
fn edits_match_fresh_index() {
    for initial in ["", "abc", "one\ntwo\n", "café 東京 🚀\r\nsecond\n", "\n\n"] {
        let mut source = initial.to_owned();
        let mut index = LineIndex::new(initial);
        for replacement in ["\n", "👨‍👩‍👧‍👦", "x\ny\n", "", "\r\n", "東京"] {
            for position in [0, index.length] {
                let range = ResponseTextRange {
                    location: position.min(index.length),
                    length: 0,
                };
                source = super::super::text::replace_utf16(&source, range, replacement);
                index.replace(range, replacement).unwrap();
                assert_eq!(index, LineIndex::new(&source));
            }
        }
        index
            .replace(
                ResponseTextRange {
                    location: 0,
                    length: index.length,
                },
                "",
            )
            .unwrap();
        assert_eq!(index.starts, [0]);
        assert_eq!(index.length, 0);
    }
}
#[test]
fn removes_and_joins_lines_without_duplicate_offsets() {
    let mut index = LineIndex::new("one\ntwo\nthree\n");
    index
        .replace(
            ResponseTextRange {
                location: 3,
                length: 5,
            },
            "!",
        )
        .unwrap();
    assert_eq!(index.starts, [0, 10]);
    assert_eq!(index.line(9), 0);
    assert_eq!(index.line(10), 1);
    index
        .replace(
            ResponseTextRange {
                location: 10,
                length: 0,
            },
            "x\ny",
        )
        .unwrap();
    assert_eq!(index.starts, [0, 10, 12]);
}
#[test]
fn repeated_middle_edits_use_utf16_coordinates() {
    let mut source = "東京 🚀\n".repeat(1000);
    let mut index = LineIndex::new(&source);
    for step in 0_usize..80 {
        let line = (step * 17) % index.starts.len();
        let start = index.starts[line];
        let end = index.starts.get(line + 1).copied().unwrap_or(index.length);
        let range = ResponseTextRange {
            location: start,
            length: end - start,
        };
        let replacement = if step.is_multiple_of(3) {
            "joined"
        } else {
            "café\n🚀\n"
        };
        source = super::super::text::replace_utf16(&source, range, replacement);
        index.replace(range, replacement).unwrap();
        assert_eq!(index.starts, LineIndex::new(&source).starts);
    }
}
fn query(text: &str) -> SearchQuery {
    SearchQuery {
        text: text.into(),
        ..SearchQuery::default()
    }
}
#[test]
fn literal_case_word_and_regex_options_preserve_utf16_ranges() {
    let text = "🚀 café CAFÉ cafeteria\nitem-12 item-34";
    let q = query("café");
    let insensitive = q.matches(text, None, &cancellation()).unwrap();
    assert_eq!(insensitive.len(), 2);
    assert_eq!(insensitive[0].location, 3);
    assert_eq!(
        SearchQuery {
            match_case: true,
            ..q.clone()
        }
        .matches(text, None, &cancellation())
        .unwrap()
        .len(),
        1
    );
    assert!(
        SearchQuery {
            whole_word: true,
            ..query("cafe")
        }
        .matches(text, None, &cancellation())
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        SearchQuery {
            regular_expression: true,
            ..query(r"item-\d+")
        }
        .matches(text, None, &cancellation())
        .unwrap()
        .len(),
        2
    );
    assert!(
        SearchQuery {
            regular_expression: true,
            ..query("[")
        }
        .matches(text, None, &cancellation())
        .is_err()
    );
    let scope = ResponseTextRange {
        location: 8,
        length: 4,
    };
    assert_eq!(
        q.matches(text, Some(scope), &cancellation()).unwrap(),
        [scope]
    );
}
#[test]
fn capped_search_and_zero_length_patterns_terminate() {
    assert_eq!(
        query("x")
            .matches(&"x".repeat(30_000), None, &cancellation())
            .unwrap()
            .len(),
        20_000
    );
    assert_eq!(
        SearchQuery {
            regular_expression: true,
            ..query("x{0}")
        }
        .matches("x", None, &cancellation())
        .unwrap()
        .len(),
        2
    );
    assert!(
        SearchQuery::default()
            .matches("x", None, &cancellation())
            .unwrap()
            .is_empty()
    );
}
#[test]
fn space_runs_use_utf16_offsets() {
    for text in ["", "{\"a\":1}", "a\tb"] {
        assert!(space_runs(text).is_empty());
    }
    assert_eq!(
        space_runs("  \"café\":  \"東京 🙂\" "),
        [(0, 2), (9, 2), (14, 1), (18, 1)]
            .map(|(location, length)| ResponseTextRange { location, length })
    );
}
#[test]
fn invalid_edits_and_cancelled_search_are_rejected() {
    let mut index = LineIndex::new("a");
    assert!(
        index
            .replace(
                ResponseTextRange {
                    location: 2,
                    length: 0
                },
                ""
            )
            .is_err()
    );
    let cancel = cancellation();
    cancel.cancel();
    assert_eq!(
        query("a").matches("a", None, &cancel),
        Err(ResponseError::Cancelled)
    );
}

#[test]
fn line_index_delta_reconstructs_the_native_snapshot() {
    let mut index = LineIndex::new("one\ntwo\n東京 🚀\n");
    let mut mirror = index.starts.clone();
    let change = index
        .replace(
            ResponseTextRange {
                location: 3,
                length: 5,
            },
            "x\ny\n",
        )
        .unwrap();
    let first = super::super::size(change.first);
    let removed = super::super::size(change.removed);
    mirror.splice(first..first + removed, change.inserted.iter().copied());
    for offset in &mut mirror[first + change.inserted.len()..] {
        *offset = offset.checked_add_signed(change.delta).unwrap();
    }
    assert_eq!(mirror, index.starts);
    assert_eq!(change.length, index.length);
}
