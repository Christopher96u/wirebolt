use super::{ResponseCancellation, ResponseResult, count};
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
    pub block_separator: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownRun {
    pub text: String,
    pub style: MarkdownStyle,
}
/// Semantic runs supplied by the platform's Markdown implementation. Block and
/// item identities are opaque and only compared for adjacent-run transitions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownInputRun {
    pub text: String,
    pub style: MarkdownStyle,
    pub block: Option<u64>,
    pub item: Option<MarkdownListItem>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkdownListItem {
    pub id: u64,
    pub number: u64,
    pub ordered: bool,
}
pub trait MarkdownParser: Send + Sync {
    /// # Errors
    /// Returns a platform parsing error or cancellation.
    fn parse(
        &self,
        source: &str,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Vec<MarkdownInputRun>>;
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
        parser: &dyn MarkdownParser,
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
        let document = Arc::new(assemble_markdown(
            parser.parse(source, cancellation)?,
            self.parses,
            cancellation,
        )?);
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
fn assemble_markdown(
    input: Vec<MarkdownInputRun>,
    id: u64,
    cancellation: &ResponseCancellation,
) -> ResponseResult<MarkdownDocument> {
    let input_count = input.len();
    let mut runs: Vec<MarkdownRun> = Vec::with_capacity(input_count);
    let mut previous_block = None;
    let mut previous_item = None;
    for mut run in input {
        cancellation.check()?;
        if run.block != previous_block && !runs.is_empty() {
            runs.push(MarkdownRun {
                text: "\n".into(),
                style: MarkdownStyle {
                    block_separator: true,
                    ..MarkdownStyle::default()
                },
            });
        }
        // Only these link schemes were rendered by the original native preview.
        run.style.link = run.style.link.filter(|link| {
            link.split_once(':').is_some_and(|(scheme, _)| {
                ["http", "https", "mailto"]
                    .iter()
                    .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
            })
        });
        if let Some(item) = run.item
            && Some(item.id) != previous_item
        {
            runs.push(MarkdownRun {
                text: if item.ordered {
                    format!("{}.  ", item.number)
                } else {
                    "•  ".into()
                },
                style: run.style.clone(),
            });
        }
        previous_block = run.block;
        previous_item = run.item.map(|item| item.id);
        runs.push(MarkdownRun {
            text: run.text,
            style: run.style,
        });
    }
    let units: usize = runs.iter().map(|r| r.text.encode_utf16().count()).sum();
    // Preserve the native cache's accounting: prefixes/separators add text, not
    // semantic runs. Foundation can supply empty runs with presentation intents.
    let cost = count(units) * 2 + count(input_count) * 256;
    Ok(MarkdownDocument { id, runs, cost })
}
