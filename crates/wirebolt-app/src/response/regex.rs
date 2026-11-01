//! Portable search engine and the platform-engine seam used by response searches.
use super::{
    ResponseCancellation, ResponseError, ResponseResult, ResponseTextRange, SearchQuery, count,
};
use regex::RegexBuilder;

/// A complete search runs inside Rust, including cancellation and result limiting.
/// Platform bridges can preserve their native regular-expression dialect here.
pub trait RegexEngine: Send + Sync {
    /// # Errors
    /// Returns pattern, execution-limit, or cancellation errors.
    fn matches(
        &self,
        query: &SearchQuery,
        source: &str,
        scope: Option<ResponseTextRange>,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Vec<ResponseTextRange>>;
}

/// Rust's bounded, portable regex dialect. Platform-specific dialects are supplied
/// explicitly by the bridge; unsupported expressions return `InvalidPattern`.
#[derive(Debug)]
pub struct PortableRegexEngine;
impl RegexEngine for PortableRegexEngine {
    fn matches(
        &self,
        query: &SearchQuery,
        source: &str,
        scope: Option<ResponseTextRange>,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Vec<ResponseTextRange>> {
        cancellation.check()?;
        if query.text.is_empty() {
            return Ok(vec![]);
        }
        let pattern = if query.regular_expression {
            query.text.clone()
        } else {
            regex::escape(&query.text)
        };
        let expression = RegexBuilder::new(&pattern)
            .multi_line(true)
            .case_insensitive(!query.match_case)
            .build()
            .map_err(|e| ResponseError::InvalidPattern {
                reason: e.to_string(),
            })?;
        let length = count(source.encode_utf16().count());
        let region = scope.unwrap_or(ResponseTextRange {
            location: 0,
            length,
        });
        let start = region.location.min(length);
        let end = region.end().min(length).max(start);
        let start_byte = super::text::utf16_byte_exact(source, start);
        let end_byte = super::text::utf16_byte_exact(source, end);
        let (Some(start_byte), Some(end_byte)) = (start_byte, end_byte) else {
            return Ok(vec![]);
        };
        let text = &source[start_byte..end_byte];
        let mut result = Vec::new();
        let mut byte_offset = 0;
        let mut utf16_offset = start;
        for hit in expression.find_iter(text) {
            cancellation.check()?;
            if query.whole_word && !whole_word(text, hit.start(), hit.end()) {
                continue;
            }
            utf16_offset += count(text[byte_offset..hit.start()].encode_utf16().count());
            let length = count(hit.as_str().encode_utf16().count());
            result.push(ResponseTextRange {
                location: utf16_offset,
                length,
            });
            byte_offset = hit.end();
            utf16_offset += length;
            if result.len() == SearchQuery::MAXIMUM_MATCHES {
                break;
            }
        }
        cancellation.check()?;
        Ok(result)
    }
}
fn whole_word(source: &str, start: usize, end: usize) -> bool {
    use std::sync::LazyLock;
    static WORD: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"[\p{L}\p{N}_]").expect("word boundary pattern"));
    let is_word = |c: char| WORD.is_match(c.encode_utf8(&mut [0; 4]));
    !source[..start].chars().next_back().is_some_and(is_word)
        && !source[end..].chars().next().is_some_and(is_word)
}
