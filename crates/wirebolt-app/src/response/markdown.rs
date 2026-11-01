use super::{ResponseCancellation, ResponseResult, count};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use sha2::{Digest, Sha256};
use std::{collections::VecDeque, sync::Arc};
// These independent presentation traits can coexist; they are not exclusive states.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkdownStyle {
    pub heading: u32,
    pub code: bool,
    pub quote: bool,
    pub depth: u32,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub link: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownRun {
    pub text: String,
    pub style: MarkdownStyle,
}
#[derive(Clone, Debug)]
pub struct MarkdownDocument {
    pub id: u64,
    pub runs: Vec<MarkdownRun>,
    pub cost: u64,
}
#[derive(Clone, Debug)]
pub struct MarkdownCacheStats {
    pub retained_bytes: u64,
    pub parse_count: u64,
    pub retained_ids: Vec<u64>,
}
#[derive(Debug)]
pub struct MarkdownCache {
    limit: u64,
    entries: VecDeque<([u8; 32], Arc<MarkdownDocument>)>,
    bytes: u64,
    parses: u64,
}
impl MarkdownCache {
    #[must_use]
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            entries: VecDeque::new(),
            bytes: 0,
            parses: 0,
        }
    }
    #[must_use]
    pub fn stats(&self) -> MarkdownCacheStats {
        MarkdownCacheStats {
            retained_bytes: self.bytes,
            parse_count: self.parses,
            retained_ids: self.entries.iter().map(|(_, d)| d.id).collect(),
        }
    }
    /// # Errors
    /// Returns cancellation before parsing or publishing a document.
    pub fn render(
        &mut self,
        source: &str,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Arc<MarkdownDocument>> {
        cancellation.check()?;
        let key: [u8; 32] = Sha256::digest(source.as_bytes()).into();
        if let Some(position) = self.entries.iter().position(|(k, _)| *k == key)
            && let Some(entry) = self.entries.remove(position)
        {
            let result = Arc::clone(&entry.1);
            self.entries.push_back(entry);
            return Ok(result);
        }
        self.parses += 1;
        let document = Arc::new(parse_markdown(source, self.parses, cancellation)?);
        cancellation.check()?;
        if document.cost <= self.limit {
            while self.bytes + document.cost > self.limit || self.entries.len() >= 8 {
                let Some((_, old)) = self.entries.pop_front() else {
                    break;
                };
                self.bytes -= old.cost;
            }
            self.bytes += document.cost;
            self.entries.push_back((key, Arc::clone(&document)));
        }
        Ok(document)
    }
}
fn parse_markdown(
    source: &str,
    id: u64,
    cancellation: &ResponseCancellation,
) -> ResponseResult<MarkdownDocument> {
    let mut runs: Vec<MarkdownRun> = Vec::new();
    let mut style = MarkdownStyle::default();
    let mut styles = Vec::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut item: Option<(bool, u64)> = None;
    let mut new_block = false;
    let mut image_depth: usize = 0;
    for event in Parser::new_ext(
        source,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES,
    ) {
        cancellation.check()?;
        let inline_code = matches!(event, Event::Code(_));
        match event {
            Event::Start(tag) => {
                styles.push(style.clone());
                match tag {
                    Tag::Heading { level, .. } => {
                        style.heading = level as u32;
                        new_block = true;
                    }
                    Tag::Paragraph | Tag::TableCell | Tag::HtmlBlock => new_block = true,
                    Tag::CodeBlock(_) => {
                        style.code = true;
                        new_block = true;
                    }
                    Tag::BlockQuote(_) => {
                        style.quote = true;
                        new_block = true;
                    }
                    Tag::List(start) => {
                        lists.push(start);
                        style.depth += 1;
                    }
                    Tag::Item => {
                        item = Some((
                            lists.last().is_some_and(Option::is_some),
                            lists.last().copied().flatten().unwrap_or(1),
                        ));
                        new_block = true;
                    }
                    Tag::Strong => style.bold = true,
                    Tag::Emphasis => style.italic = true,
                    Tag::Strikethrough => style.strikethrough = true,
                    Tag::Link { dest_url, .. } if image_depth == 0 => {
                        style.link = safe_markdown_link(&dest_url);
                    }
                    Tag::Image { .. } => image_depth += 1,
                    _ => (),
                }
            }
            Event::End(end) => {
                if end == TagEnd::Item {
                    if let Some(Some(number)) = lists.last_mut() {
                        *number += 1;
                    }
                    item = None;
                }
                if end == TagEnd::List(false) || end == TagEnd::List(true) {
                    lists.pop();
                }
                if end == TagEnd::Image {
                    image_depth = image_depth.saturating_sub(1);
                }
                style = styles.pop().unwrap_or_default();
            }
            Event::Text(text) | Event::Code(text) | Event::InlineHtml(text) => {
                append_block_text(
                    &text,
                    &style,
                    &mut runs,
                    &mut new_block,
                    &mut item,
                    inline_code,
                );
            }
            Event::Html(text) => {
                append_block_text(&text, &style, &mut runs, &mut new_block, &mut item, false);
                if !text.ends_with('\n') {
                    append_run("\n", &style, &mut runs);
                }
            }
            Event::SoftBreak => append_run(" ", &style, &mut runs),
            Event::HardBreak => append_run("\n", &style, &mut runs),
            Event::Rule => {
                if !runs.is_empty() {
                    append_run("\n", &MarkdownStyle::default(), &mut runs);
                }
                append_run("⸻", &style, &mut runs);
                new_block = true;
            }
            _ => (),
        }
    }
    let units: usize = runs.iter().map(|r| r.text.encode_utf16().count()).sum();
    let cost = count(units) * 2 + count(runs.len()) * 256;
    Ok(MarkdownDocument { id, runs, cost })
}

fn append_run(text: &str, style: &MarkdownStyle, runs: &mut Vec<MarkdownRun>) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut().filter(|r| r.style == *style) {
        last.text.push_str(text);
    } else {
        runs.push(MarkdownRun {
            text: text.into(),
            style: style.clone(),
        });
    }
}

fn safe_markdown_link(url: &str) -> Option<String> {
    url.split(':')
        .next()
        .filter(|scheme| ["http", "https", "mailto"].contains(&scheme.to_lowercase().as_str()))
        .map(|_| url.to_owned())
}

fn append_block_text(
    text: &str,
    style: &MarkdownStyle,
    runs: &mut Vec<MarkdownRun>,
    new_block: &mut bool,
    item: &mut Option<(bool, u64)>,
    inline_code: bool,
) {
    if *new_block {
        if !runs.is_empty() {
            runs.push(MarkdownRun {
                text: "\n".into(),
                style: MarkdownStyle::default(),
            });
        }
        *new_block = false;
    }
    if let Some((ordered, number)) = item.take() {
        append_run(
            &if ordered {
                format!("{number}.  ")
            } else {
                "•  ".into()
            },
            style,
            runs,
        );
    }
    let mut run_style = style.clone();
    run_style.code |= inline_code;
    append_run(text, &run_style, runs);
}
