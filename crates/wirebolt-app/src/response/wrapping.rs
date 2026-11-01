use super::{ResponseTextRange, count, size};
use std::{cell::Cell, fmt};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq)]
pub struct WrapLayout {
    pub font_name: String,
    pub font_size: f64,
    pub width: f64,
    pub advance: f64,
}
#[derive(Clone, Debug)]
pub struct ShapingRequest {
    pub text: String,
    pub boundaries: Vec<u64>,
}
/// Native glyph clusters can span multiple Unicode graphemes (for example an
/// Arabic ligature). Subrange widths preserve shaping when a row cuts a cluster.
#[derive(Clone, Debug, Default)]
pub struct ShapingMetrics {
    pub advances: Vec<f64>,
    pub groups: Vec<ShapingGroup>,
}
#[derive(Clone, Debug)]
pub struct ShapingGroup {
    pub first: u64,
    pub count: u64,
    pub widths: Vec<f64>,
}
/// The platform measures whole batches. Indexing and line-breaking policy never
/// call out for a single row or glyph.
pub trait TextShaper: Send + Sync {
    fn measure(&self, requests: Vec<ShapingRequest>, layout: WrapLayout) -> Vec<ShapingMetrics>;
}
impl fmt::Debug for dyn TextShaper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TextShaper")
    }
}
#[derive(Clone, Debug)]
pub(super) struct Paragraph {
    pub text: String,
    pub units: Vec<u16>,
    pub offsets: Vec<usize>,
    pub boundaries: Vec<usize>,
    pub widths: Vec<f64>,
    pub groups: Vec<ShapingGroup>,
    pub indentation: f64,
    pub ascii: bool,
    window_length: usize,
    shaping_window: Cell<Option<(usize, usize)>>,
}
impl Paragraph {
    pub fn new(bytes: &[u8], layout: &WrapLayout) -> Self {
        // `from_utf8_lossy` consumes malformed subsequences just like Swift's
        // repairing decoder; retain the original byte boundary for every unit.
        let mut units = Vec::new();
        let mut offsets = vec![0];
        let mut cursor = 0;
        while cursor < bytes.len() {
            let remaining = &bytes[cursor..];
            let (valid, invalid) = match std::str::from_utf8(remaining) {
                Ok(text) => (text.len(), 0),
                Err(error) => (
                    error.valid_up_to(),
                    error
                        .error_len()
                        .unwrap_or(remaining.len() - error.valid_up_to()),
                ),
            };
            if valid > 0 {
                let text = std::str::from_utf8(&remaining[..valid]).expect("validated prefix");
                for ch in text.chars() {
                    let start = cursor;
                    cursor += ch.len_utf8();
                    let mut encoded = [0; 2];
                    let encoded = ch.encode_utf16(&mut encoded);
                    if encoded.len() == 2 {
                        units.push(encoded[0]);
                        offsets.push(start);
                    }
                    units.push(*encoded.last().expect("scalar"));
                    offsets.push(cursor);
                }
            }
            if invalid > 0 {
                cursor += invalid;
                units.push(0xfffd);
                offsets.push(cursor);
            }
        }
        if units.last() == Some(&13) {
            units.pop();
            offsets.pop();
        }
        let text = String::from_utf16_lossy(&units);
        let mut boundaries = vec![0];
        let mut position = 0;
        for cluster in text.graphemes(true) {
            position += cluster.encode_utf16().count();
            boundaries.push(position);
        }
        let mut columns = 0;
        for &unit in &units {
            match unit {
                32 => columns += 1,
                9 => columns += 4 - columns % 4,
                _ => break,
            }
        }
        let indentation =
            (f64::from(columns) * layout.advance).min((layout.width - layout.advance).max(0.0));
        let ascii = units.iter().all(|u| (32..=126).contains(u));
        Self {
            text,
            units,
            offsets,
            boundaries,
            widths: vec![],
            groups: vec![],
            indentation,
            ascii,
            window_length: finite_size((layout.width / layout.advance.max(1.0) * 8.0).min(32768.0))
                .max(2048),
            shaping_window: Cell::new(None),
        }
    }
    pub fn request(&self) -> ShapingRequest {
        ShapingRequest {
            text: self.text.clone(),
            boundaries: self.boundaries.iter().map(|&p| count(p)).collect(),
        }
    }
    pub fn next(&self, start: usize, indent: f64, layout: &WrapLayout) -> ResponseTextRange {
        let available = (layout.width - indent).max(1.0);
        let fit = if self.ascii {
            finite_size((available / layout.advance.max(1.0)).floor())
                .max(1)
                .min(self.units.len() - start)
        } else {
            let window_end = self.prepare_window(start, 0);
            let first = self
                .boundaries
                .partition_point(|&b| b <= start)
                .saturating_sub(1);
            let mut width = 0.0;
            let mut end = start;
            for index in first..self.boundaries.len().saturating_sub(1) {
                let cluster_start = self.boundaries[index];
                let advance = if self.units.get(cluster_start) == Some(&9) {
                    let tab = layout.advance * 4.0;
                    tab - (width % tab)
                } else {
                    self.widths.get(index).copied().unwrap_or(layout.advance)
                };
                let fitted_width = self.adjusted_width(width + advance, first, index + 1);
                if fitted_width > available && end > start {
                    break;
                }
                width += advance;
                end = self.boundaries[index + 1];
                if fitted_width > available || end >= window_end {
                    break;
                }
            }
            end - start
        };
        let mut length = preferred_break(&self.units, &self.boundaries, start, fit);
        if self.units[start..start + length]
            .iter()
            .any(|u| *u != 32 && *u != 9)
        {
            while self
                .units
                .get(start + length)
                .is_some_and(|u| *u == 32 || *u == 9)
            {
                length += 1;
            }
        }
        ResponseTextRange {
            location: count(start),
            length: count(length),
        }
    }
    pub fn columns(&self, range: ResponseTextRange, advance: f64) -> u64 {
        if self.ascii {
            return range.length;
        }
        let start = size(range.location);
        let end = size(range.end());
        self.prepare_window(start, size(range.length));
        let mut width = 0.0;
        for (index, boundary) in self
            .boundaries
            .iter()
            .enumerate()
            .take(self.boundaries.len().saturating_sub(1))
            .skip(self.boundaries.partition_point(|&b| b < start))
        {
            if *boundary < start {
                continue;
            }
            if *boundary >= end {
                break;
            }
            width += if self.units[*boundary] == 9 {
                let tab = advance * 4.0;
                tab - (width % tab)
            } else {
                self.widths.get(index).copied().unwrap_or(advance)
            };
        }
        count(finite_size(
            (self.adjusted_width(
                width,
                self.boundaries.partition_point(|&b| b < start),
                self.boundaries.partition_point(|&b| b < end),
            ) / advance.max(1.0))
            .ceil(),
        ))
    }
    fn prepare_window(&self, start: usize, length: usize) -> usize {
        let current = self.shaping_window.get();
        let needed = start
            .saturating_add((self.window_length / 2).max(length))
            .min(self.units.len());
        if current.is_none_or(|(lower, upper)| start < lower || upper < needed) {
            let end = start
                .saturating_add(self.window_length.max(length))
                .min(self.units.len());
            let boundary = self.boundaries.partition_point(|&boundary| boundary < end);
            let end = self
                .boundaries
                .get(boundary)
                .copied()
                .unwrap_or(self.units.len());
            self.shaping_window.set(Some((start, end)));
            end
        } else {
            current.map_or(self.units.len(), |(_, end)| end)
        }
    }
    fn adjusted_width(&self, mut width: f64, start: usize, end: usize) -> f64 {
        let first = self
            .groups
            .partition_point(|group| size(group.first + group.count) <= start);
        for group in &self.groups[first..] {
            let group_start = size(group.first);
            let group_end = size(group.first + group.count);
            if group_start >= end {
                break;
            }
            if start <= group_start && end >= group_end {
                continue;
            }
            let begin = start.max(group_start);
            let finish = end.min(group_end);
            let stride = size(group.count) + 1;
            if let Some(&measured) = group
                .widths
                .get((begin - group_start) * stride + finish - group_start)
            {
                width += measured - self.widths[begin..finish].iter().sum::<f64>();
            }
        }
        width
    }
}
// Font measurements are finite and bounded by the paragraph, so saturating float
// conversions here cannot index beyond the source.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn finite_size(value: f64) -> usize {
    value.max(0.0) as usize
}
const BREAK_AFTER: &str = " \t})]?|/&.,;¢°′″‰℃、。｡､￠，．：；？！％・･ゝゞヽヾーァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゕゖㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ々〻ｧｨｩｪｫｬｭｮｯｰ”〉》」』】〕）］｝｣";
const BREAK_BEFORE: &str = "([{‘“〈《「『【〔（［｛｢£¥＄￡￥+＋";
fn preferred_break(units: &[u16], boundaries: &[usize], start: usize, fitting: usize) -> usize {
    if start >= units.len() || fitting == 0 {
        return 0;
    }
    if start.saturating_add(fitting) >= units.len() {
        return fitting;
    }
    let first_content = units[start..]
        .iter()
        .position(|u| *u != 32 && *u != 9)
        .map_or(units.len(), |i| start + i);
    for end in (start + 1..=start + fitting).rev() {
        if end > first_content
            && (char::from_u32(u32::from(units[end - 1])).is_some_and(|c| BREAK_AFTER.contains(c))
                || char::from_u32(u32::from(units[end])).is_some_and(|c| BREAK_BEFORE.contains(c)))
            && boundaries.binary_search(&end).is_ok()
        {
            return end - start;
        }
    }
    fitting
}
#[derive(Clone, Debug)]
pub struct WrapBreakIndex {
    units: Vec<u16>,
    boundaries: Vec<usize>,
}
impl WrapBreakIndex {
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut boundaries = vec![0];
        let mut position = 0;
        for cluster in text.graphemes(true) {
            position += cluster.encode_utf16().count();
            boundaries.push(position);
        }
        Self {
            units: text.encode_utf16().collect(),
            boundaries,
        }
    }
    #[must_use]
    pub fn length(&self, start: u64, fitting: u64) -> u64 {
        count(preferred_break(
            &self.units,
            &self.boundaries,
            size(start),
            size(fitting),
        ))
    }
}
pub(super) fn shape_paragraphs(
    paragraphs: &mut [Paragraph],
    layout: &WrapLayout,
    shaper: Option<&dyn TextShaper>,
) {
    let requests = paragraphs
        .iter()
        .filter(|p| !p.ascii && !p.units.is_empty())
        .map(Paragraph::request)
        .collect::<Vec<_>>();
    if requests.is_empty() {
        return;
    }
    let measurements = shaper
        .map(|shaper| shaper.measure(requests, layout.clone()))
        .unwrap_or_default();
    let mut widths = measurements.into_iter();
    for paragraph in paragraphs
        .iter_mut()
        .filter(|p| !p.ascii && !p.units.is_empty())
    {
        let measured = widths.next().unwrap_or_default();
        paragraph.widths = measured.advances;
        paragraph.groups = measured.groups;
    }
}

#[derive(Clone, Debug)]
pub struct WrapBreakSnapshot {
    pub previous: Vec<u64>,
    pub next_content: Vec<u64>,
    pub length: u64,
}
impl WrapBreakIndex {
    #[must_use]
    pub fn snapshot(&self) -> WrapBreakSnapshot {
        let mut previous = vec![0; self.units.len() + 1];
        let mut last = 0;
        for (end, value) in previous
            .iter_mut()
            .enumerate()
            .skip(1)
            .take(self.units.len().saturating_sub(1))
        {
            if (char::from_u32(u32::from(self.units[end - 1]))
                .is_some_and(|c| BREAK_AFTER.contains(c))
                || char::from_u32(u32::from(self.units[end]))
                    .is_some_and(|c| BREAK_BEFORE.contains(c)))
                && self.boundaries.binary_search(&end).is_ok()
            {
                last = count(end);
            }
            *value = last;
        }
        let mut next_content = vec![count(self.units.len()); self.units.len() + 1];
        let mut next = count(self.units.len());
        for (index, unit) in self.units.iter().enumerate().rev() {
            if *unit != 32 && *unit != 9 {
                next = count(index);
            }
            next_content[index] = next;
        }
        WrapBreakSnapshot {
            previous,
            next_content,
            length: count(self.units.len()),
        }
    }
}
