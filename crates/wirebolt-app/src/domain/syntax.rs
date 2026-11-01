//! Batched syntax ranges and hanging-indent columns for the native code editor.
use crate::response::ResponseTextRange;
use regex::Regex;
use std::{borrow::Cow, sync::LazyLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxLanguage {
    Json,
    Xml,
    Html,
    Http,
    Plain,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxTokenKind {
    Keyword,
    Number,
    String,
    Key,
    Url,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxToken {
    pub range: ResponseTextRange,
    pub kind: SyntaxTokenKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxIndent {
    pub range: ResponseTextRange,
    pub columns: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxChunk {
    pub range: ResponseTextRange,
    pub tokens: Vec<SyntaxToken>,
    pub indents: Vec<SyntaxIndent>,
}
struct Rule {
    expression: Regex,
    kind: SyntaxTokenKind,
    capture: usize,
}
fn rule(pattern: &str, kind: SyntaxTokenKind, capture: usize) -> Rule {
    Rule {
        expression: Regex::new(pattern).expect("syntax pattern"),
        kind,
        capture,
    }
}
static JSON: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(r"\b(true|false)\b", SyntaxTokenKind::Keyword, 0),
        rule(r"\bnull\b", SyntaxTokenKind::Keyword, 0),
        rule(r"-?\b\d+(?:\.\d+)?\b", SyntaxTokenKind::Number, 0),
        rule(
            r#"\"(?:\\[^\n\r\u{b}\u{c}\u{85}\u{2028}\u{2029}]|[^\"\\])*\""#,
            SyntaxTokenKind::String,
            0,
        ),
        rule(
            r#"(\"(?:\\[^\n\r\u{b}\u{c}\u{85}\u{2028}\u{2029}]|[^\"\\])*\")\s*:"#,
            SyntaxTokenKind::Key,
            1,
        ),
        rule(r#"https?://[^\"\s]+"#, SyntaxTokenKind::Url, 0),
    ]
});
static XML: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(r"</?[A-Za-z][^>]*>", SyntaxTokenKind::Key, 0),
        rule(r#"\"[^\"]*\""#, SyntaxTokenKind::String, 0),
    ]
});
static HTTP: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            r"(?m)^[A-Z]+\s+\S+\s+HTTP/\d(?:\.\d)?$",
            SyntaxTokenKind::Keyword,
            0,
        ),
        rule(r"(?m)^([A-Za-z0-9-]+):", SyntaxTokenKind::Key, 1),
        rule(r"•+", SyntaxTokenKind::String, 0),
    ]
});

/// Returns ordered attribute passes within bounded, whole-line chunks. Later
/// tokens override earlier foreground colors, matching the native editor.
#[must_use]
pub fn highlight(source: &str, language: SyntaxLanguage) -> Vec<SyntaxChunk> {
    let lines = lines(source);
    let chunked = matches!(
        language,
        SyntaxLanguage::Json | SyntaxLanguage::Plain | SyntaxLanguage::Http
    ) && source.len() > 16 * 1024;
    let mut result = Vec::new();
    let mut start_byte = 0;
    let mut start_unit = 0;
    let mut end_unit = 0;
    for (end_byte, length) in &lines {
        end_unit += length;
        if !chunked || (end_unit - start_unit < 16 * 1024 && *end_byte < source.len()) {
            continue;
        }
        result.push(chunk(
            &source[start_byte..*end_byte],
            start_unit,
            end_unit - start_unit,
            language,
        ));
        start_byte = *end_byte;
        start_unit = end_unit;
    }
    if !chunked || result.is_empty() {
        return vec![chunk(
            source,
            0,
            source.encode_utf16().count() as u64,
            language,
        )];
    }
    result
}

// NSString line boundaries: CRLF is one terminator; Unicode line/paragraph
// separators and NEL also terminate a native line.
fn lines(source: &str) -> Vec<(usize, u64)> {
    let mut result = Vec::new();
    let mut units = 0;
    let mut chars = source.char_indices().peekable();
    while let Some((byte, character)) = chars.next() {
        units += character.len_utf16() as u64;
        let mut end = byte + character.len_utf8();
        if character == '\r' && chars.peek().is_some_and(|(_, c)| *c == '\n') {
            chars.next();
            units += 1;
            end += 1;
        }
        if matches!(character, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            result.push((end, units));
            units = 0;
        }
    }
    if units > 0 {
        result.push((source.len(), units));
    }
    result
}
fn chunk(source: &str, location: u64, length: u64, language: SyntaxLanguage) -> SyntaxChunk {
    let rules: &[Rule] = match language {
        SyntaxLanguage::Json => &JSON,
        SyntaxLanguage::Xml | SyntaxLanguage::Html => &XML,
        SyntaxLanguage::Http => &HTTP,
        SyntaxLanguage::Plain => &[],
    };
    // ICU's multiline anchors recognize all native newline characters. Rust's
    // anchors recognize LF; normalize only this matching view, preserving every
    // UTF-16 offset (including both units of CRLF).
    let matching_source = if language == SyntaxLanguage::Http
        && source.contains(['\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}'])
    {
        Cow::Owned(source.replace(
            ['\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}'],
            "\n",
        ))
    } else {
        Cow::Borrowed(source)
    };
    let matching_source = matching_source.as_ref();
    let mut tokens = Vec::new();
    for rule in rules {
        let mut byte = 0;
        let mut unit = 0;
        for captures in rule.expression.captures_iter(matching_source) {
            let hit = captures.get(rule.capture).expect("syntax capture");
            unit += matching_source[byte..hit.start()].encode_utf16().count() as u64;
            let length = hit.as_str().encode_utf16().count() as u64;
            tokens.push(SyntaxToken {
                range: ResponseTextRange {
                    location: unit,
                    length,
                },
                kind: rule.kind,
            });
            byte = hit.end();
            unit += length;
        }
    }
    let mut indents = Vec::new();
    let mut byte = 0;
    let mut unit = 0;
    for (end, length) in lines(source) {
        let mut columns = 0;
        for character in source[byte..end].chars() {
            match character {
                ' ' => columns += 1,
                '\t' => columns += 4 - columns % 4,
                _ => break,
            }
        }
        if columns > 0 {
            indents.push(SyntaxIndent {
                range: ResponseTextRange {
                    location: unit,
                    length,
                },
                columns,
            });
        }
        byte = end;
        unit += length;
    }
    SyntaxChunk {
        range: ResponseTextRange { location, length },
        tokens,
        indents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_precedence_urls_and_utf16_ranges() {
        let source =
            "{\"🚀\": true, \"url\": \"https://example.com\", \"n\": -12.5, \"s\": \"false\"}";
        let batch = highlight(source, SyntaxLanguage::Json);
        let tokens = &batch[0].tokens;
        assert_eq!(
            tokens
                .iter()
                .filter(|t| t.kind == SyntaxTokenKind::Key)
                .count(),
            4
        );
        let key = tokens
            .iter()
            .find(|t| t.kind == SyntaxTokenKind::Key)
            .unwrap();
        assert_eq!(
            key.range,
            ResponseTextRange {
                location: 1,
                length: 4
            }
        );
        assert_eq!(tokens.last().unwrap().kind, SyntaxTokenKind::Url);
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == SyntaxTokenKind::Number && t.range.length == 5)
        );
    }
    #[test]
    fn native_line_endings_and_tab_stops() {
        let batch = highlight(
            "🚀\r\n \tx\r  y\u{2028}\t z\u{2029}end",
            SyntaxLanguage::Plain,
        );
        assert_eq!(
            batch[0]
                .indents
                .iter()
                .map(|i| (i.range.location, i.columns))
                .collect::<Vec<_>>(),
            vec![(4, 4), (8, 2), (12, 5)]
        );
    }
    #[test]
    fn chunking_is_bounded_and_keeps_all_text() {
        let source = "  {\"🚀\": true}\n".repeat(4000);
        let chunks = highlight(&source, SyntaxLanguage::Json);
        assert!(chunks.len() > 1);
        assert_eq!(
            chunks.iter().map(|c| c.range.length).sum::<u64>(),
            source.encode_utf16().count() as u64
        );
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].range.end(), pair[1].range.location);
        }
        assert_eq!(highlight(&source, SyntaxLanguage::Xml).len(), 1);
    }
    #[test]
    fn markup_and_http_passes() {
        let xml = highlight("<a href=\"x\">hi</a>", SyntaxLanguage::Xml);
        assert_eq!(
            xml[0].tokens.iter().map(|t| t.kind).collect::<Vec<_>>(),
            vec![
                SyntaxTokenKind::Key,
                SyntaxTokenKind::Key,
                SyntaxTokenKind::String
            ]
        );
        let http = highlight(
            "GET / HTTP/1.1\nHost: example.com\nSecret: •••",
            SyntaxLanguage::Http,
        );
        assert_eq!(http[0].tokens.len(), 4);
    }
}
