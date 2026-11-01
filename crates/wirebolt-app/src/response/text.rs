use super::{ResponseCancellation, ResponseError, ResponseResult, ResponseTextRange, count, size};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    pub starts: Vec<u64>,
    pub length: u64,
}
/// A native mirror applies this one edit event instead of lifting every line
/// start again. Rust alone computes the splice, inserted offsets and suffix shift.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndexChange {
    pub first: u64,
    pub removed: u64,
    pub inserted: Vec<u64>,
    pub delta: i64,
    pub length: u64,
}
impl LineIndex {
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        let mut length = 0;
        for unit in text.encode_utf16() {
            length += 1;
            if unit == 10 {
                starts.push(length);
            }
        }
        Self { starts, length }
    }
    /// # Errors
    /// Rejects edits outside the indexed UTF-16 range.
    pub fn replace(
        &mut self,
        range: ResponseTextRange,
        text: &str,
    ) -> ResponseResult<LineIndexChange> {
        if range.end() > self.length {
            return Err(ResponseError::Io {
                reason: "Invalid text range".into(),
            });
        }
        let first = self
            .starts
            .partition_point(|&start| start <= range.location);
        let last = self.starts.partition_point(|&start| start <= range.end());
        let units: Vec<_> = text.encode_utf16().collect();
        let inserted: Vec<_> = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| **unit == 10)
            .map(|(offset, _)| range.location + count(offset) + 1)
            .collect();
        let next = first + inserted.len();
        self.starts.splice(first..last, inserted.iter().copied());
        for start in &mut self.starts[next..] {
            *start = *start - range.length + count(units.len());
        }
        self.length = self.length - range.length + count(units.len());
        Ok(LineIndexChange {
            first: count(first),
            removed: count(last - first),
            inserted,
            delta: i64::try_from(units.len()).unwrap_or(i64::MAX)
                - i64::try_from(range.length).unwrap_or(i64::MAX),
            length: self.length,
        })
    }
    #[must_use]
    pub fn line(&self, offset: u64) -> u64 {
        count(
            self.starts
                .partition_point(|&start| start <= offset)
                .saturating_sub(1),
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regular_expression: bool,
}
impl SearchQuery {
    pub const MAXIMUM_MATCHES: usize = 20_000;
    /// # Errors
    /// Returns invalid regex, execution-limit, or cancellation errors.
    pub fn matches(
        &self,
        source: &str,
        scope: Option<ResponseTextRange>,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Vec<ResponseTextRange>> {
        self.matches_with_engine(source, scope, cancellation, &super::PortableRegexEngine)
    }
    /// # Errors
    /// Returns the injected engine's pattern, execution-limit, or cancellation errors.
    pub fn matches_with_engine(
        &self,
        source: &str,
        scope: Option<ResponseTextRange>,
        cancellation: &ResponseCancellation,
        engine: &dyn super::RegexEngine,
    ) -> ResponseResult<Vec<ResponseTextRange>> {
        engine.matches(self, source, scope, cancellation)
    }
}
#[cfg(test)]
pub(super) fn utf16_byte(text: &str, offset: u64) -> usize {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units >= offset {
            return byte;
        }
        units += count(character.len_utf16());
    }
    text.len()
}
pub(super) fn replace_utf16(text: &str, range: ResponseTextRange, replacement: &str) -> String {
    let mut units: Vec<_> = text.encode_utf16().collect();
    let start = size(range.location).min(units.len());
    let end = size(range.end()).min(units.len()).max(start);
    units.splice(start..end, replacement.encode_utf16());
    String::from_utf16_lossy(&units)
}
#[must_use]
pub fn space_runs(text: &str) -> Vec<ResponseTextRange> {
    let mut result = Vec::new();
    for (offset, unit) in text.encode_utf16().enumerate() {
        if unit == 32 {
            if let Some(last) = result
                .last_mut()
                .filter(|last: &&mut ResponseTextRange| last.end() == count(offset))
            {
                last.length += 1;
            } else {
                result.push(ResponseTextRange {
                    location: count(offset),
                    length: 1,
                });
            }
        }
    }
    result
}
#[must_use]
pub fn uses_text_index(bytes: u64, preview: &str) -> bool {
    if bytes > 64 * 1024 {
        return true;
    }
    let mut length = 0;
    preview.encode_utf16().any(|unit| {
        length = if unit == 10 { 0 } else { length + 1 };
        length > 2048
    })
}

pub(super) fn utf16_byte_exact(text: &str, offset: u64) -> Option<usize> {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += count(character.len_utf16());
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}
