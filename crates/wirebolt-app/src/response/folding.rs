use super::text::{replace_utf16, utf16_byte_exact};
use super::{ResponseCancellation, ResponseResult, ResponseTextRange, count, size};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeSyntax {
    Json,
    Xml,
    Html,
    None,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    pub line: u64,
    pub range: ResponseTextRange,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldReplacement {
    pub source: ResponseTextRange,
    pub display: ResponseTextRange,
}
#[derive(Clone, Debug)]
pub struct CodeDocument {
    pub source: Arc<String>,
    pub text: Arc<String>,
    pub folds: Vec<Fold>,
    pub collapsed: Vec<u64>,
    replacements: Vec<FoldReplacement>,
    source_length: u64,
}
impl CodeDocument {
    #[must_use]
    pub fn unfolded(source: String, folds: Vec<Fold>) -> Self {
        let source_length = count(source.encode_utf16().count());
        let source = Arc::new(source);
        Self {
            source_length,
            text: Arc::clone(&source),
            source,
            folds,
            collapsed: vec![],
            replacements: vec![],
        }
    }

    /// # Errors
    /// Returns cancellation if the parser is cancelled.
    pub fn new(
        source: String,
        collapsed: Vec<u64>,
        syntax: CodeSyntax,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Self> {
        let folds = match syntax {
            CodeSyntax::Json => json_folds(&source, cancellation)?,
            CodeSyntax::Xml | CodeSyntax::Html => {
                markup_folds(&source, syntax == CodeSyntax::Html, cancellation)?
            }
            CodeSyntax::None => vec![],
        };
        if collapsed.is_empty() {
            return Ok(Self::unfolded(source, folds));
        }
        let units: Vec<_> = source.encode_utf16().collect();
        let mut output = Vec::new();
        let mut cursor = 0;
        let mut replacements = Vec::new();
        for fold in &folds {
            if collapsed.contains(&fold.line) && size(fold.range.location) >= cursor {
                output.extend_from_slice(&units[cursor..size(fold.range.location)]);
                replacements.push(FoldReplacement {
                    source: fold.range,
                    display: ResponseTextRange {
                        location: count(output.len()),
                        length: 1,
                    },
                });
                output.push(0x2026);
                cursor = size(fold.range.end());
            }
        }
        output.extend_from_slice(&units[cursor..]);
        Ok(Self {
            source_length: count(source.encode_utf16().count()),
            source: Arc::new(source),
            text: Arc::new(String::from_utf16_lossy(&output)),
            folds,
            collapsed,
            replacements,
        })
    }
    /// Incremental native editor ingress sends only the edit, never its entire
    /// multi-megabyte snapshot. Shared source/text storage preserves value semantics.
    #[must_use]
    pub fn updating_edit(
        &self,
        range: ResponseTextRange,
        replacement: &str,
        removed: &str,
    ) -> Option<Self> {
        let folds = self.updated_folds(range, replacement, removed)?;
        if range.end() > self.source_length {
            return None;
        }
        let mut source = self.source.as_ref().clone();
        if let (Some(start), Some(end)) = (
            utf16_byte_exact(&source, range.location),
            utf16_byte_exact(&source, range.end()),
        ) {
            source.replace_range(start..end, replacement);
        } else {
            source = replace_utf16(&source, range, replacement);
        }
        let source = Arc::new(source);
        Some(Self {
            source_length: self.source_length - range.length
                + count(replacement.encode_utf16().count()),
            text: Arc::clone(&source),
            source,
            folds,
            collapsed: vec![],
            replacements: vec![],
        })
    }
    fn updated_folds(
        &self,
        range: ResponseTextRange,
        replacement: &str,
        removed: &str,
    ) -> Option<Vec<Fold>> {
        if !self.collapsed.is_empty()
            || replacement
                .encode_utf16()
                .chain(removed.encode_utf16())
                .any(|u| [10, 13, 34, 92, 123, 125, 91, 93].contains(&u))
        {
            return None;
        }
        let inserted = count(replacement.encode_utf16().count());
        let adjust = |value: u64| value.saturating_add(inserted).saturating_sub(range.length);
        let folds = self
            .folds
            .iter()
            .map(|fold| {
                let start = if fold.range.location > range.location {
                    adjust(fold.range.location)
                } else {
                    fold.range.location
                };
                let end = if fold.range.end() >= range.end() {
                    adjust(fold.range.end())
                } else {
                    fold.range.end()
                };
                Fold {
                    line: fold.line,
                    range: ResponseTextRange {
                        location: start,
                        length: end.saturating_sub(start),
                    },
                }
            })
            .collect();
        Some(folds)
    }
    #[must_use]
    pub fn source_offset(&self, display: u64, after_placeholder: bool) -> u64 {
        let mut delta = 0;
        for r in &self.replacements {
            if display < r.display.location {
                break;
            }
            if display < r.display.end() {
                return if after_placeholder {
                    r.source.end()
                } else {
                    r.source.location
                };
            }
            delta += r.source.length.saturating_sub(r.display.length);
        }
        (display + delta).min(self.source_length)
    }
    #[must_use]
    pub fn display_offset(&self, source: u64) -> u64 {
        let mut delta = 0;
        for r in &self.replacements {
            if source < r.source.location {
                break;
            }
            if source < r.source.end() {
                return r.display.location;
            }
            delta += r.source.length.saturating_sub(r.display.length);
        }
        source.saturating_sub(delta)
    }
    #[must_use]
    pub fn replacing(&self, range: ResponseTextRange, replacement: &str) -> String {
        let start = self.source_offset(range.location, false);
        let end = self.source_offset(range.end(), range.length > 0);
        replace_utf16(
            &self.source,
            ResponseTextRange {
                location: start,
                length: end.saturating_sub(start),
            },
            replacement,
        )
    }
    #[must_use]
    pub fn source_offsets(&self, offsets: Vec<u64>) -> Vec<u64> {
        offsets
            .into_iter()
            .map(|offset| self.source_offset(offset, false))
            .collect()
    }
}
fn json_folds(text: &str, cancellation: &ResponseCancellation) -> ResponseResult<Vec<Fold>> {
    let mut stack = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut line = 0;
    let mut folds = Vec::new();
    for (offset, unit) in text.encode_utf16().enumerate() {
        if offset.is_multiple_of(4096) {
            cancellation.check()?;
        }
        if unit == 10 {
            line += 1;
        }
        if quoted {
            if escaped {
                escaped = false;
            } else if unit == 92 {
                escaped = true;
            } else if unit == 34 {
                quoted = false;
            }
            continue;
        }
        if unit == 34 {
            quoted = true;
            continue;
        }
        if unit == 123 || unit == 91 {
            stack.push((unit, offset, line));
        } else if let Some(&(open, start, start_line)) = stack.last()
            && ((open == 123 && unit == 125) || (open == 91 && unit == 93))
        {
            stack.pop();
            if line > start_line {
                folds.push(Fold {
                    line: start_line,
                    range: ResponseTextRange {
                        location: count(start + 1),
                        length: count(offset - start - 1),
                    },
                });
            }
        }
    }
    folds.sort_by_key(|f| f.range.location);
    Ok(folds)
}
fn markup_folds(
    text: &str,
    html: bool,
    cancellation: &ResponseCancellation,
) -> ResponseResult<Vec<Fold>> {
    let units: Vec<_> = text.encode_utf16().collect();
    let mut stack: Vec<(String, usize, u64)> = Vec::new();
    let mut folds = Vec::new();
    let mut offset = 0;
    let mut line = 0;
    let has = |index: usize, token: &str| {
        units
            .get(index..index + token.len())
            .is_some_and(|u| u.iter().copied().eq(token.encode_utf16()))
    };
    let mut append = |start: usize, end: usize, line: u64| {
        if end > start && units[start..end].contains(&10) {
            folds.push(Fold {
                line,
                range: ResponseTextRange {
                    location: count(start),
                    length: count(end - start),
                },
            });
        }
    };
    while offset < units.len() {
        if offset.is_multiple_of(4096) {
            cancellation.check()?;
        }
        if units[offset] != 60 {
            if units[offset] == 10 {
                line += 1;
            }
            offset += 1;
            continue;
        }
        let start = offset;
        let start_line = line;
        if has(start, "<!--") || has(start, "<![CDATA[") {
            let comment = has(start, "<!--");
            let opener = if comment { 4 } else { 9 };
            let closer = if comment { "-->" } else { "]]>" };
            offset += opener;
            while offset < units.len() && !has(offset, closer) {
                if units[offset] == 10 {
                    line += 1;
                }
                offset += 1;
            }
            if offset < units.len() {
                append(start + opener, offset, start_line);
                offset += 3;
            }
            continue;
        }
        let end = tag_end(&units, start);
        if end == units.len() {
            break;
        }
        line += count(units[start..=end].iter().filter(|&&u| u == 10).count());
        offset = end + 1;
        let closing = units[start + 1] == 47;
        let name_start = start + if closing { 2 } else { 1 };
        let mut name_end = name_start;
        while name_end < end && ![9, 10, 13, 32, 47].contains(&units[name_end]) {
            name_end += 1;
        }
        if name_end <= name_start || [33, 63].contains(&units[name_start]) {
            continue;
        }
        let name = String::from_utf16_lossy(&units[name_start..name_end]);
        let name = if html { name.to_lowercase() } else { name };
        if closing {
            if let Some(index) = stack.iter().rposition(|(n, _, _)| *n == name) {
                let (_, opened_end, opened_line) = stack[index];
                append(opened_end, start, opened_line);
                stack.truncate(index);
            }
        } else if units[end - 1] != 47
            && !(html
                && [
                    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
                    "param", "source", "track", "wbr",
                ]
                .contains(&name.as_str()))
        {
            stack.push((name.clone(), end + 1, start_line));
            if html && ["script", "style", "textarea", "title"].contains(&name.as_str()) {
                let remaining = String::from_utf16_lossy(&units[offset..]);
                if let Some(byte) = remaining
                    .as_bytes()
                    .windows(name.len() + 2)
                    .position(|window| window.eq_ignore_ascii_case(format!("</{name}").as_bytes()))
                {
                    let skipped = remaining[..byte].encode_utf16().collect::<Vec<_>>();
                    line += count(skipped.iter().filter(|&&u| u == 10).count());
                    offset += skipped.len();
                }
            }
        }
    }
    folds.sort_by_key(|f| f.range.location);
    Ok(folds)
}

fn tag_end(units: &[u16], start: usize) -> usize {
    let mut end = start + 1;
    let mut quote = None;
    while end < units.len() {
        let unit = units[end];
        if let Some(current) = quote {
            if current == unit {
                quote = None;
            }
        } else if unit == 34 || unit == 39 {
            quote = Some(unit);
        } else if unit == 62 {
            break;
        }
        end += 1;
    }
    end
}
