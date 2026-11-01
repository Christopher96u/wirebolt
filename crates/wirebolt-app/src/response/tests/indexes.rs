use super::*;
use unicode_segmentation::UnicodeSegmentation;
fn index(
    store: &BodyStore,
    columns: u64,
    prefix: &str,
    wrapping: Option<WrapLayout>,
    limit: u64,
) -> TextIndex {
    TextIndex::new(
        options(&store.path(), columns, prefix, wrapping),
        limit,
        Some(shaper()),
        &cancellation(),
    )
    .unwrap()
}
fn rows(index: &TextIndex, start: u64, count: u64) -> Vec<TextRow> {
    index.rows(start, count, &cancellation()).unwrap()
}
#[test]
fn bounded_index_shows_same_first_rows_as_complete_index() {
    let (_root, store) = body("  café 東京 👨‍👩‍👧‍👦 /some/long/path\n".repeat(1000).as_bytes());
    let partial = index(&store, 10, "", Some(layout(90.0)), 256);
    let full = index(&store, 10, "", Some(layout(90.0)), u64::MAX);
    assert!(!partial.is_complete);
    assert!(full.is_complete);
    assert_eq!(partial.row_count, 256);
    assert_eq!(rows(&partial, 0, 256), rows(&full, 0, 256));
}
#[test]
fn byte_anchor_survives_wrapping_changes_across_checkpoints() {
    let text = (0..1000)
        .map(|i| format!("{i} café 東京 🚀 /some/long/path"))
        .collect::<Vec<_>>()
        .join("\n");
    let (_root, store) = body(text.as_bytes());
    let wide = index(&store, 100, "", Some(layout(900.0)), u64::MAX);
    let narrow = index(&store, 10, "", Some(layout(90.0)), u64::MAX);
    for original in [0, 127, 500, 999] {
        let row = rows(&wide, original, 1).remove(0);
        let mapped = narrow
            .row_at_byte(row.byte_offset, &cancellation())
            .unwrap();
        let restored = rows(&narrow, mapped, 1).remove(0);
        assert_eq!(restored.line, row.line);
        assert_eq!(restored.byte_offset, row.byte_offset);
        assert_eq!(
            wide.row_at_byte(restored.byte_offset, &cancellation())
                .unwrap(),
            original
        );
    }
}
#[test]
fn resize_anchor_keeps_raw_headers_above_body() {
    let (_root, store) = body(b"body\nsecond");
    let prefix = "HTTP/1.1 200 OK\nContent-Type: application/json\n\n";
    let wide = index(&store, 100, prefix, Some(layout(900.0)), u64::MAX);
    let narrow = index(&store, 10, prefix, Some(layout(90.0)), u64::MAX);
    for row in rows(&wide, 0, wide.row_count) {
        let mapped = narrow.preserving(&row, &cancellation()).unwrap();
        assert_eq!(rows(&narrow, mapped, 1)[0].line, row.line);
    }
}
#[test]
fn pixel_wrapping_preserves_unicode_and_checkpoint_indentation() {
    let text = format!("  {}", "path/to?q=cafe\u{301}, 東京 👨‍👩‍👧‍👦 🚀; ".repeat(4000));
    let (_root, store) = body(text.as_bytes());
    let index = index(&store, 20, "", Some(layout(160.0)), u64::MAX);
    let all = rows(&index, 0, index.row_count);
    assert_eq!(
        all.iter().map(|r| r.text.as_str()).collect::<String>(),
        text
    );
    assert!(all[0].indent.abs() < f64::EPSILON);
    assert!(all[1..].iter().all(|r| r.continuation && r.indent > 0.0));
    let boundaries: std::collections::HashSet<_> = text
        .grapheme_indices(true)
        .map(|(i, _)| super::super::count(i))
        .chain([super::super::count(text.len())])
        .collect();
    assert!(all.iter().all(|r| boundaries.contains(&r.byte_offset)));
    for start in (127..index.row_count).step_by(128) {
        assert_eq!(
            rows(&index, start, 3),
            all[super::super::size(start)..all.len().min(super::super::size(start) + 3)]
        );
    }
    assert_eq!(std::fs::read(store.path()).unwrap(), text.as_bytes());
}
#[test]
fn pixel_wrapping_keeps_raw_prefix_and_search_positions() {
    let text = "  /json?limit=10&search=wirebolt\r\n\n";
    let (_root, store) = body(text.as_bytes());
    let prefix = "HTTP/1.1 200 OK\nX-Long-Header: abcdefghijklmnop\n\n";
    let index = index(&store, 20, prefix, Some(layout(160.0)), u64::MAX);
    let all = rows(&index, 0, index.row_count);
    let mut combined = String::new();
    for row in &all {
        if row.number > 0 && !row.continuation {
            combined.push('\n');
        }
        combined.push_str(&row.text);
    }
    assert_eq!(combined, format!("{prefix}{}", text.replace('\r', "")));
    let hit = index
        .search(
            &SearchQuery {
                text: "search=wirebolt".into(),
                ..SearchQuery::default()
            },
            None,
            &cancellation(),
        )
        .unwrap()
        .remove(0);
    let start = rows(&index, hit.start.row, 1).remove(0);
    let suffix = super::super::text::utf16_byte(&start.text, hit.start.column);
    assert!(start.text[suffix..].starts_with("search="));
    assert_eq!(
        index
            .first_match("search=wirebolt", &cancellation())
            .unwrap(),
        Some(hit.start.row)
    );
    assert!(all[all.len() - 2..].iter().all(|r| r.text.is_empty()));
}
#[test]
fn pixel_wrapping_decodes_malformed_utf8_and_narrow_clusters() {
    let mut bytes = vec![0xf0, 0x9f, 0x41, 0xed, 0xa0, 0x80, 0x80];
    bytes.extend_from_slice("👨‍👩‍👧‍👦e\u{301}".as_bytes());
    let (_root, store) = body(&bytes);
    let idx = index(&store, 1, "", Some(layout(1.0)), u64::MAX);
    let all = rows(&idx, 0, idx.row_count);
    assert_eq!(
        all.iter().map(|r| r.text.as_str()).collect::<String>(),
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(
        all[all.len() - 2..]
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["👨‍👩‍👧‍👦", "e\u{301}"]
    );
    std::fs::write(store.path(), []).unwrap();
    assert_eq!(
        index(&store, 1, "", Some(layout(160.0)), u64::MAX).row_count,
        1
    );
}
#[test]
fn raw_headers_precede_every_body_row_without_copying_file() {
    let text = (0..400)
        .map(|i| format!("body-{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (_root, store) = body(text.as_bytes());
    let idx = index(
        &store,
        80,
        "HTTP/1.1 200 OK\nContent-Type: text/plain\n\n",
        None,
        u64::MAX,
    );
    assert_eq!(idx.row_count, 403);
    assert_eq!(idx.line_count, 403);
    assert_eq!(
        rows(&idx, 0, 4)
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["HTTP/1.1 200 OK", "Content-Type: text/plain", "", "body-0"]
    );
    assert_eq!(
        rows(&idx, 401, 2)
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["body-398", "body-399"]
    );
    assert_eq!(
        idx.first_match("Content-Type", &cancellation()).unwrap(),
        Some(1)
    );
    assert_eq!(
        idx.first_match("body-399", &cancellation()).unwrap(),
        Some(402)
    );
    assert_eq!(std::fs::read_to_string(store.path()).unwrap(), text);
}
#[test]
fn unwrapped_long_unicode_line_is_not_truncated_or_split() {
    let text = "café 東京 🚀".repeat(10_000);
    let (_root, store) = body(text.as_bytes());
    let idx = index(&store, u64::MAX / 4, "", None, u64::MAX);
    assert_eq!(idx.row_count, 1);
    assert_eq!(rows(&idx, 0, 1)[0].text, text);
}
#[test]
fn indexes_across_checkpoints_and_utf8_chunk_boundaries() {
    let lines = (0..10_000)
        .map(|i| format!("{i}: café 東京 🚀 {}", "x".repeat(30)))
        .collect::<Vec<_>>();
    let (_root, store) = body(lines.join("\n").as_bytes());
    let idx = index(&store, 18, "", None, u64::MAX);
    assert_eq!(idx.line_count, 10_000);
    assert!(idx.row_count > idx.line_count);
    let first = rows(&idx, 0, 10)
        .into_iter()
        .filter(|r| r.line == 0)
        .map(|r| r.text)
        .collect::<String>();
    assert_eq!(first, lines[0]);
    let last: Vec<_> = rows(&idx, idx.row_count - 10, 10)
        .into_iter()
        .filter(|r| r.line == 9999)
        .collect();
    assert_eq!(
        last.iter().map(|r| r.text.as_str()).collect::<String>(),
        lines[9999]
    );
    assert!(!last[0].continuation);
    assert_eq!(
        idx.first_match("9999: café 東京", &cancellation()).unwrap(),
        Some(last[0].number)
    );
    assert_eq!(idx.first_match("absent", &cancellation()).unwrap(), None);
}
#[test]
fn empty_and_trailing_newline_have_editable_final_line() {
    let (_root, store) = body(b"");
    assert_eq!(index(&store, 80, "", None, u64::MAX).row_count, 1);
    std::fs::write(store.path(), b"1234\n").unwrap();
    let idx = index(&store, 4, "", None, u64::MAX);
    assert_eq!(idx.row_count, 2);
    assert_eq!(
        rows(&idx, 0, 2)
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["1234", ""]
    );
}
#[test]
fn indexed_search_crosses_soft_wraps_newlines_and_unicode() {
    let (_root, store) = body("🚀 hello café world\r\nSECOND line\n".as_bytes());
    let idx = index(&store, 8, "HTTP/1.1 200\n\n", None, u64::MAX);
    let matches = idx
        .search(
            &SearchQuery {
                text: r"café world\r?\nSECOND".into(),
                regular_expression: true,
                ..SearchQuery::default()
            },
            None,
            &cancellation(),
        )
        .unwrap();
    assert_eq!(matches.len(), 1);
    let hit = &matches[0];
    assert!(hit.start.row < hit.end.row);
    let first = rows(&idx, hit.start.row, 1).remove(0);
    assert!(
        first.text[super::super::text::utf16_byte(&first.text, hit.start.column)..]
            .starts_with("café")
    );
    let last = rows(&idx, hit.end.row, 1).remove(0);
    assert_eq!(
        &last.text[..super::super::text::utf16_byte(&last.text, hit.end.column)],
        "SECOND"
    );
    assert_eq!(
        idx.search(
            &SearchQuery {
                text: "200".into(),
                ..SearchQuery::default()
            },
            None,
            &cancellation()
        )
        .unwrap()[0]
            .start
            .row,
        1
    );
    assert!(
        idx.search(
            &SearchQuery {
                text: "absent".into(),
                ..SearchQuery::default()
            },
            None,
            &cancellation()
        )
        .unwrap()
        .is_empty()
    );
}
#[test]
fn selection_is_applied_before_match_limit() {
    let (_root, store) = body(format!("{}🚀 x x\n", "x\n".repeat(25_000)).as_bytes());
    let idx = index(&store, 80, "", None, u64::MAX);
    let start = SearchPosition {
        row: 25_000,
        column: 3,
    };
    let end = SearchPosition {
        row: 25_000,
        column: 6,
    };
    let matches = idx
        .search(
            &SearchQuery {
                text: "x".into(),
                ..SearchQuery::default()
            },
            Some((start.clone(), end.clone())),
            &cancellation(),
        )
        .unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].start, start);
    assert_eq!(matches[1].end, end);
}
#[test]
fn first_viewport_upgrades_to_full_index_and_invalidates_with_file() {
    let (_root, store) = body("row\n".repeat(1000).as_bytes());
    let cache = TextIndexCache::new(8);
    let opt = options(&store.path(), 80, "", None);
    let first = cache
        .index(opt.clone(), true, None, &cancellation())
        .unwrap();
    assert!(!first.is_complete);
    assert_eq!(first.row_count, 256);
    assert_eq!(rows(&first, 250, 100).len(), 6);
    let full = cache
        .index(opt.clone(), false, None, &cancellation())
        .unwrap();
    assert!(full.is_complete);
    assert_eq!(full.row_count, 1001);
    assert!(
        cache
            .index(opt.clone(), true, None, &cancellation())
            .unwrap()
            .is_complete
    );
    std::fs::write(store.path(), b"replacement").unwrap();
    assert_eq!(rows(&first, 0, 1)[0].text, "replacement");
    let changed = cache.index(opt, true, None, &cancellation()).unwrap();
    assert_eq!(rows(&changed, 0, 1)[0].text, "replacement");
}
#[test]
fn cache_separates_files_widths_prefixes_and_file_revisions() {
    let (_root, store) = body("abcdef\n東京🚀".as_bytes());
    let cache = TextIndexCache::new(2);
    let opt = options(&store.path(), 3, "", None);
    let narrow = cache
        .index(opt.clone(), false, None, &cancellation())
        .unwrap();
    let wide_opt = IndexOptions { columns: 30, ..opt };
    let wide = cache
        .index(wide_opt.clone(), false, None, &cancellation())
        .unwrap();
    assert!(narrow.row_count > wide.row_count);
    let prefixed = cache
        .index(
            IndexOptions {
                prefix: "HTTP/1.1 200 OK\n".into(),
                ..wide_opt.clone()
            },
            false,
            None,
            &cancellation(),
        )
        .unwrap();
    assert_eq!(rows(&prefixed, 0, 1)[0].text, "HTTP/1.1 200 OK");
    let cached = cache
        .index(wide_opt.clone(), false, None, &cancellation())
        .unwrap();
    assert_eq!(rows(&cached, 0, 5), rows(&wide, 0, 5));
    std::fs::write(store.path(), b"replacement\nwith\nmore\nlines").unwrap();
    let changed = cache.index(wide_opt, false, None, &cancellation()).unwrap();
    assert_eq!(changed.row_count, 4);
    assert_eq!(rows(&changed, 0, 1)[0].text, "replacement");
}
#[test]
fn cancelled_read_does_not_poison_next_request() {
    let (_root, store) = body(b"hello\nworld");
    let cache = TextIndexCache::new(8);
    let opt = options(&store.path(), 80, "", None);
    let cancel = cancellation();
    cancel.cancel();
    assert!(matches!(
        cache.index(opt.clone(), false, None, &cancel),
        Err(ResponseError::Cancelled)
    ));
    let index = cache.index(opt, false, None, &cancellation()).unwrap();
    assert_eq!(
        rows(&index, 0, 2)
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["hello", "world"]
    );
}
#[test]
fn wrapped_layouts_are_keyed_by_columns() {
    let (_root, store) = body(
        "{\"message\":\"café 東京 🚀 wraps across several visual rows\"}\n"
            .repeat(400)
            .as_bytes(),
    );
    let cache = TextIndexCache::new(8);
    let advance = 7.2;
    let wrap = layout(40.25 * advance);
    let opt = options(&store.path(), 40, "", Some(wrap.clone()));
    let full = cache
        .index(opt.clone(), false, Some(shaper()), &cancellation())
        .unwrap();
    assert!(full.is_complete && full.row_count > 400);
    assert!(
        cache
            .index(opt.clone(), true, Some(shaper()), &cancellation())
            .unwrap()
            .is_complete
    );
    let other = cache
        .index(
            IndexOptions {
                columns: 41,
                wrapping: Some(layout(41.25 * advance)),
                ..opt
            },
            true,
            Some(shaper()),
            &cancellation(),
        )
        .unwrap();
    assert!(!other.is_complete);
    assert!(rows(&full, 0, 8).iter().all(|r| r.columns <= 40));
}
#[test]
fn long_lines_use_indexed_presentation_below_byte_threshold() {
    assert!(uses_text_index(6000, &"東京".repeat(1500)));
    assert!(uses_text_index(65_537, "small preview"));
    assert!(!uses_text_index(6000, &"small\n".repeat(1000)));
}
#[test]
fn wrapping_measurement_is_batched() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct CountingShaper(AtomicUsize);
    impl TextShaper for CountingShaper {
        fn measure(
            &self,
            requests: Vec<ShapingRequest>,
            layout: WrapLayout,
        ) -> Vec<ShapingMetrics> {
            self.0.fetch_add(1, Ordering::Relaxed);
            TestShaper.measure(requests, layout)
        }
    }
    let (_root, store) = body("東京🚀\n".repeat(1000).as_bytes());
    let adapter = Arc::new(CountingShaper(AtomicUsize::new(0)));
    let idx = TextIndex::new(
        options(&store.path(), 2, "", Some(layout(15.0))),
        u64::MAX,
        Some(adapter.clone()),
        &cancellation(),
    )
    .unwrap();
    assert!(idx.row_count > 1000);
    assert!(adapter.0.load(Ordering::Relaxed) < 10);
}

#[test]
fn cancelling_a_shared_scan_owner_does_not_cancel_another_waiter() {
    use std::sync::{
        Barrier,
        atomic::{AtomicBool, Ordering},
    };
    #[derive(Debug)]
    struct GatedShaper {
        entered: Barrier,
        release: Barrier,
        first: AtomicBool,
    }
    impl TextShaper for GatedShaper {
        fn measure(
            &self,
            requests: Vec<ShapingRequest>,
            layout: WrapLayout,
        ) -> Vec<ShapingMetrics> {
            if self.first.swap(false, Ordering::Relaxed) {
                self.entered.wait();
                self.release.wait();
            }
            TestShaper.measure(requests, layout)
        }
    }
    let (_root, store) = body("東京🚀\n".repeat(1000).as_bytes());
    let cache = Arc::new(TextIndexCache::new(8));
    let adapter = Arc::new(GatedShaper {
        entered: Barrier::new(2),
        release: Barrier::new(2),
        first: AtomicBool::new(true),
    });
    let opt = options(&store.path(), 20, "", Some(layout(160.0)));
    let owner_cancellation = Arc::new(cancellation());
    let owner = {
        let cache = cache.clone();
        let adapter = adapter.clone();
        let opt = opt.clone();
        let cancel = owner_cancellation.clone();
        std::thread::spawn(move || cache.index(opt, false, Some(adapter), &cancel))
    };
    adapter.entered.wait();
    let waiter = {
        let cache = cache.clone();
        let adapter = adapter.clone();
        std::thread::spawn(move || cache.index(opt, false, Some(adapter), &cancellation()))
    };
    std::thread::sleep(std::time::Duration::from_millis(5));
    owner_cancellation.cancel();
    adapter.release.wait();
    assert!(matches!(
        owner.join().unwrap(),
        Err(ResponseError::Cancelled)
    ));
    assert!(waiter.join().unwrap().unwrap().is_complete);
}

#[test]
fn viewport_batches_space_spans_in_utf16_coordinates() {
    let (_root, store) = body("  café 東京 🚀 \nsecond row".as_bytes());
    let index = index(&store, 80, "Header: value\n", None, u64::MAX);
    let rows = rows(&index, 0, 3);
    for row in rows {
        assert_eq!(row.spaces, space_runs(&row.text));
    }
}

#[test]
fn glyph_subrange_metrics_preserve_columns_when_a_row_splits_a_ligature() {
    let layout = layout(1.0);
    let mut paragraph = crate::response::wrapping::Paragraph::new("لا ".as_bytes(), &layout);
    paragraph.widths = vec![layout.advance, 0.0, layout.advance];
    paragraph.groups = vec![ShapingGroup {
        first: 0,
        count: 2,
        widths: vec![
            0.0,
            layout.advance,
            layout.advance,
            0.0,
            0.0,
            layout.advance,
            0.0,
            0.0,
            0.0,
        ],
    }];
    assert_eq!(
        paragraph.columns(
            ResponseTextRange {
                location: 0,
                length: 2
            },
            layout.advance
        ),
        1
    );
    assert_eq!(
        paragraph.columns(
            ResponseTextRange {
                location: 1,
                length: 2
            },
            layout.advance
        ),
        2
    );
    assert_eq!(
        paragraph.next(0, 0.0, &layout),
        ResponseTextRange {
            location: 0,
            length: 1
        }
    );
    assert_eq!(
        paragraph.next(1, 0.0, &layout),
        ResponseTextRange {
            location: 1,
            length: 2
        }
    );
}

#[test]
fn zero_width_text_keeps_bounded_native_shaping_windows() {
    let layout = layout(90.0);
    let mut paragraph = crate::response::wrapping::Paragraph::new(
        &("x".to_owned() + &"\u{200b}".repeat(10_000) + "y").into_bytes(),
        &layout,
    );
    paragraph.widths = vec![0.0; paragraph.boundaries.len() - 1];
    paragraph.widths[0] = layout.advance;
    *paragraph.widths.last_mut().unwrap() = layout.advance;
    let first = paragraph.next(0, 0.0, &layout);
    assert_eq!(first.length, 2048);
    assert_eq!(paragraph.columns(first, layout.advance), 1);
    assert_eq!(paragraph.next(2048, 0.0, &layout).length, 2048);
}

#[test]
fn search_maps_last_byte_final_newline_and_eof_exactly() {
    for (text, query, regex, start, end) in [
        ("abc!", "!", false, (0, 3), (0, 4)),
        ("abc!\n", "!\n", false, (0, 3), (1, 0)),
        ("abc!\n", r"\z", true, (1, 0), (1, 0)),
        ("abc!", r"\z", true, (0, 4), (0, 4)),
    ] {
        let (_root, store) = body(text.as_bytes());
        for wrapping in [None, Some(layout(900.0))] {
            let idx = index(&store, 80, "", wrapping, u64::MAX);
            let matches = idx
                .search(
                    &SearchQuery {
                        text: query.into(),
                        regular_expression: regex,
                        ..SearchQuery::default()
                    },
                    None,
                    &cancellation(),
                )
                .unwrap();
            assert_eq!(
                matches,
                vec![IndexedMatch {
                    start: SearchPosition {
                        row: start.0,
                        column: start.1
                    },
                    end: SearchPosition {
                        row: end.0,
                        column: end.1
                    },
                }]
            );
        }
    }
}

#[test]
fn search_maps_malformed_utf8_surrogates_and_combining_marks() {
    let mut bytes = vec![0xff];
    bytes.extend_from_slice("🚀e\u{301}!".as_bytes());
    let (_root, store) = body(&bytes);
    for wrapping in [None, Some(layout(900.0))] {
        let idx = index(&store, 80, "", wrapping, u64::MAX);
        for (query, column, end) in [("�", 0, 1), ("🚀", 1, 3), ("e\u{301}", 3, 5), ("!", 5, 6)]
        {
            let matches = idx
                .search(
                    &SearchQuery {
                        text: query.into(),
                        ..SearchQuery::default()
                    },
                    None,
                    &cancellation(),
                )
                .unwrap();
            assert_eq!(matches[0].start, SearchPosition { row: 0, column });
            assert_eq!(
                matches[0].end,
                SearchPosition {
                    row: 0,
                    column: end
                }
            );
        }
    }
}

#[test]
fn capped_search_maps_the_twenty_thousandth_match_exactly() {
    let (_root, store) = body("needle xxxxxxxxx\n".repeat(30_000).as_bytes());
    let idx = index(&store, 100, "Header\n", None, u64::MAX);
    let hits = idx
        .search(
            &SearchQuery {
                text: "needle".into(),
                ..SearchQuery::default()
            },
            None,
            &cancellation(),
        )
        .unwrap();
    assert_eq!(hits.len(), 20_000);
    assert_eq!(
        hits[19_999],
        IndexedMatch {
            start: SearchPosition {
                row: 20_000,
                column: 0
            },
            end: SearchPosition {
                row: 20_000,
                column: 6
            },
        }
    );
}

#[test]
fn prefix_only_search_does_not_scan_or_shape_the_body_again() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct CountingShaper(AtomicUsize);
    impl TextShaper for CountingShaper {
        fn measure(
            &self,
            requests: Vec<ShapingRequest>,
            layout: WrapLayout,
        ) -> Vec<ShapingMetrics> {
            self.0.fetch_add(requests.len(), Ordering::Relaxed);
            TestShaper.measure(requests, layout)
        }
    }
    let (_root, store) = body("unrelated body\n".repeat(100_000).as_bytes());
    let adapter = Arc::new(CountingShaper(AtomicUsize::new(0)));
    let idx = TextIndex::new(
        options(&store.path(), 80, "Header: needle\n", Some(layout(900.0))),
        u64::MAX,
        Some(adapter.clone()),
        &cancellation(),
    )
    .unwrap();
    adapter.0.store(0, Ordering::Relaxed);
    let hits = idx
        .search(
            &SearchQuery {
                text: "needle".into(),
                ..SearchQuery::default()
            },
            None,
            &cancellation(),
        )
        .unwrap();
    assert_eq!(hits[0].start, SearchPosition { row: 0, column: 8 });
    assert_eq!(hits[0].end, SearchPosition { row: 0, column: 14 });
    assert_eq!(adapter.0.load(Ordering::Relaxed), 0);
}

#[test]
fn capped_mapping_stops_within_one_shaping_batch_of_the_last_endpoint() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct CountingShaper(AtomicUsize);
    impl TextShaper for CountingShaper {
        fn measure(
            &self,
            requests: Vec<ShapingRequest>,
            layout: WrapLayout,
        ) -> Vec<ShapingMetrics> {
            self.0.fetch_add(requests.len(), Ordering::Relaxed);
            TestShaper.measure(requests, layout)
        }
    }
    let (_root, store) = body("needle\n".repeat(100_000).as_bytes());
    let adapter = Arc::new(CountingShaper(AtomicUsize::new(0)));
    let idx = TextIndex::new(
        options(&store.path(), 80, "", Some(layout(900.0))),
        u64::MAX,
        Some(adapter.clone()),
        &cancellation(),
    )
    .unwrap();
    adapter.0.store(0, Ordering::Relaxed);
    let hits = idx
        .search(
            &SearchQuery {
                text: "needle".into(),
                ..SearchQuery::default()
            },
            None,
            &cancellation(),
        )
        .unwrap();
    assert_eq!(hits.len(), 20_000);
    assert_eq!(
        hits[19_999].end,
        SearchPosition {
            row: 19_999,
            column: 6
        }
    );
    assert!(adapter.0.load(Ordering::Relaxed) <= 20_001 + 65_536 / 7);
}

#[test]
fn cancellation_between_matching_and_mapping_does_not_publish_partial_hits() {
    struct CancellingEngine;
    impl RegexEngine for CancellingEngine {
        fn matches(
            &self,
            _query: &SearchQuery,
            _source: &str,
            _scope: Option<ResponseTextRange>,
            cancellation: &ResponseCancellation,
        ) -> ResponseResult<Vec<ResponseTextRange>> {
            cancellation.cancel();
            Ok(vec![ResponseTextRange {
                location: 0,
                length: 1,
            }])
        }
    }
    let (_root, store) = body(b"body");
    for prefix in ["", "Header\n"] {
        let idx = index(&store, 80, prefix, None, u64::MAX);
        assert!(matches!(
            idx.search_with_engine(
                &SearchQuery {
                    text: "x".into(),
                    ..SearchQuery::default()
                },
                None,
                &cancellation(),
                &CancellingEngine
            ),
            Err(ResponseError::Cancelled)
        ));
    }
}
