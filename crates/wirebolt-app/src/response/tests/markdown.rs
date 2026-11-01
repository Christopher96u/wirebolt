use super::*;
fn text(doc: &MarkdownDocument) -> String {
    doc.runs.iter().map(|r| r.text.as_str()).collect()
}
#[test]
fn blocks_and_inline_formatting() {
    let mut cache = MarkdownCache::new(8 * 1024 * 1024);
    let doc=cache.render("# Title\n\n- **Bold** item\n- second\n\n> quote\n\n```swift\nlet x = 1\n```\n\n[link](https://example.com)",&cancellation()).unwrap();
    assert_eq!(
        text(&doc),
        "Title\n•  Bold item\n•  second\nquote\nlet x = 1\n\nlink"
    );
    assert_eq!(doc.runs[0].style.heading, 1);
    assert!(
        doc.runs
            .iter()
            .find(|r| r.text.contains("Bold"))
            .unwrap()
            .style
            .bold
    );
    assert!(
        doc.runs
            .iter()
            .find(|r| r.text.contains("let x"))
            .unwrap()
            .style
            .code
    );
    assert_eq!(
        doc.runs.last().unwrap().style.link.as_deref(),
        Some("https://example.com")
    );
}
#[test]
fn nested_lists_unicode_and_no_remote_resources() {
    let mut cache = MarkdownCache::new(8 * 1024 * 1024);
    let doc=cache.render("1. café 🚀\n2. 東京\n   - nested\n\n![alt text](https://example.invalid/tracking.png)\n\n[unsafe](file:///etc/passwd)",&cancellation()).unwrap();
    let text = text(&doc);
    for part in ["1.  café 🚀", "2.  東京", "•  nested", "alt text"] {
        assert!(text.contains(part), "{text}");
    }
    assert!(doc.runs.iter().all(|r| r.style.link.is_none()));
}
#[test]
fn cache_reuse_invalidation_and_budget() {
    let mut cache = MarkdownCache::new(1024);
    let first = cache.render("**one**", &cancellation()).unwrap();
    let repeated = cache.render("**one**", &cancellation()).unwrap();
    assert!(Arc::ptr_eq(&first, &repeated));
    assert_eq!(cache.stats().parse_count, 1);
    let edited = cache.render("**two**", &cancellation()).unwrap();
    assert_eq!(text(&edited), "two");
    assert_eq!(text(&first), "one");
    for index in 0..20 {
        cache
            .render(&format!("Note {index}"), &cancellation())
            .unwrap();
    }
    assert!(cache.stats().retained_bytes <= 1024);
    cache
        .render(&"large ".repeat(1000), &cancellation())
        .unwrap();
    assert!(cache.stats().retained_bytes <= 1024);
}
#[test]
fn cancelled_work_does_not_parse_or_publish() {
    let mut cache = MarkdownCache::new(1024);
    let cancel = cancellation();
    cancel.cancel();
    assert!(matches!(
        cache.render("# Cancel me", &cancel),
        Err(ResponseError::Cancelled)
    ));
    assert_eq!(cache.stats().parse_count, 0);
}
#[test]
fn empty_and_unfinished_markdown_remain_readable() {
    let mut cache = MarkdownCache::new(1024);
    assert!(text(&cache.render("", &cancellation()).unwrap()).is_empty());
    assert_eq!(
        text(&cache.render("**unfinished", &cancellation()).unwrap()),
        "**unfinished"
    );
    assert!(
        text(&cache.render("```\nunclosed code", &cancellation()).unwrap())
            .contains("unclosed code")
    );
}

#[test]
fn native_markdown_rules_html_tables_and_inline_code() {
    let mut cache = MarkdownCache::new(1024 * 1024);
    for (source, expected) in [
        ("---", "⸻"),
        ("before\n\n---\n\nafter", "before\n⸻\nafter"),
        ("<b>bold</b>", "<b>bold</b>"),
        ("<div>\nhello\n</div>", "<div>\nhello\n</div>\n"),
        ("| a | b |\n| - | - |\n| x | y |", "a\nb\nx\ny"),
    ] {
        assert_eq!(
            text(&cache.render(source, &cancellation()).unwrap()),
            expected
        );
    }
    let document = cache.render("one `code` two", &cancellation()).unwrap();
    assert!(
        document
            .runs
            .iter()
            .find(|r| r.text == "code")
            .unwrap()
            .style
            .code
    );
}
