use super::*;
fn text(doc: &MarkdownDocument) -> String {
    doc.runs.iter().map(|r| r.text.as_str()).collect()
}
struct PlainParser;
impl MarkdownParser for PlainParser {
    fn parse(
        &self,
        source: &str,
        _: &ResponseCancellation,
    ) -> ResponseResult<Vec<MarkdownInputRun>> {
        Ok(if source.is_empty() {
            vec![]
        } else {
            vec![input(source, 1)]
        })
    }
}
fn input(text: &str, block: u64) -> MarkdownInputRun {
    MarkdownInputRun {
        text: text.into(),
        style: MarkdownStyle::default(),
        block: Some(block),
        item: None,
    }
}
struct FixtureParser(Vec<MarkdownInputRun>);
impl MarkdownParser for FixtureParser {
    fn parse(&self, _: &str, _: &ResponseCancellation) -> ResponseResult<Vec<MarkdownInputRun>> {
        Ok(self.0.clone())
    }
}
#[test]
fn platform_intents_keep_block_item_and_inline_transitions() {
    let mut title = input("Title", 1);
    title.style.heading = 1;
    let mut bold = input("Bold", 3);
    bold.style.bold = true;
    bold.style.depth = 1;
    bold.item = Some(MarkdownListItem {
        id: 2,
        number: 3,
        ordered: true,
    });
    let mut continuation = bold.clone();
    continuation.text = " item".into();
    continuation.style.bold = false;
    let mut nested = input("nested 🚀", 6);
    nested.style.depth = 2;
    nested.item = Some(MarkdownListItem {
        id: 5,
        number: 1,
        ordered: false,
    });
    let mut quote = input("quoted code\n", 7);
    quote.style.quote = true;
    quote.style.code = true;
    let parser = FixtureParser(vec![title, bold, continuation, nested, quote]);
    let mut cache = MarkdownCache::new(4096);
    let doc = cache.render("fixture", &cancellation(), &parser).unwrap();
    assert_eq!(
        text(&doc),
        "Title\n3.  Bold item\n•  nested 🚀\nquoted code\n"
    );
    assert!(doc.runs[1].style.block_separator);
    assert!(doc.runs.iter().any(|r| r.text == "Bold" && r.style.bold));
    assert_eq!(
        doc.cost,
        text(&doc).encode_utf16().count() as u64 * 2 + 5 * 256
    );
}
#[test]
fn only_safe_native_links_are_rendered() {
    for (link, allowed) in [
        ("HTTPS://example.com", true),
        ("mailto:a@b.com", true),
        ("http://x", true),
        ("../page", false),
        ("file:///tmp/a", false),
        ("javascript:alert", false),
    ] {
        let mut run = input("link", 1);
        run.style.link = Some(link.into());
        let doc = MarkdownCache::new(1024)
            .render(link, &cancellation(), &FixtureParser(vec![run]))
            .unwrap();
        assert_eq!(doc.runs[0].style.link.is_some(), allowed);
    }
}
#[test]
fn cache_reuse_invalidation_and_budget() {
    let mut cache = MarkdownCache::new(1024);
    let first = cache.render("one", &cancellation(), &PlainParser).unwrap();
    let repeated = cache.render("one", &cancellation(), &PlainParser).unwrap();
    assert!(Arc::ptr_eq(&first, &repeated));
    assert_eq!(cache.stats().parse_count, 1);
    let edited = cache.render("two", &cancellation(), &PlainParser).unwrap();
    assert_eq!(text(&edited), "two");
    assert_eq!(text(&first), "one");
    for index in 0..20 {
        cache
            .render(&format!("Note {index}"), &cancellation(), &PlainParser)
            .unwrap();
    }
    assert!(cache.stats().retained_bytes <= 1024);
    cache
        .render(&"large ".repeat(1000), &cancellation(), &PlainParser)
        .unwrap();
    assert!(cache.stats().retained_bytes <= 1024);
}
#[test]
fn cancellation_does_not_parse_or_publish() {
    let mut cache = MarkdownCache::new(1024);
    let cancel = cancellation();
    cancel.cancel();
    assert!(matches!(
        cache.render("Cancel me", &cancel, &PlainParser),
        Err(ResponseError::Cancelled)
    ));
    assert_eq!(cache.stats().parse_count, 0);
}
#[test]
fn cancelled_platform_parser_does_not_publish() {
    struct CancellingParser;
    impl MarkdownParser for CancellingParser {
        fn parse(
            &self,
            _: &str,
            cancellation: &ResponseCancellation,
        ) -> ResponseResult<Vec<MarkdownInputRun>> {
            cancellation.cancel();
            Ok(vec![input("hidden", 1)])
        }
    }
    let mut cache = MarkdownCache::new(1024);
    assert!(matches!(
        cache.render("source", &cancellation(), &CancellingParser),
        Err(ResponseError::Cancelled)
    ));
    assert!(cache.stats().retained_ids.is_empty());
}
#[test]
fn empty_platform_document() {
    let doc = MarkdownCache::new(1024)
        .render("", &cancellation(), &PlainParser)
        .unwrap();
    assert_eq!(text(&doc), "");
    assert_eq!(doc.cost, 0);
}
