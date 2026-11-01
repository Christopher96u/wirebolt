mod cache;
mod scan;
use super::wrapping::{Paragraph, shape_paragraphs};
use super::{
    ResponseCancellation, ResponseResult, ResponseTextRange, SearchQuery, TextShaper, WrapLayout,
    count, size,
};
pub use cache::TextIndexCache;
use std::{
    fs::{self, File},
    io::Read,
    sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq)]
pub struct TextRow {
    pub number: u64,
    pub line: u64,
    pub continuation: bool,
    pub byte_offset: u64,
    pub text: String,
    pub columns: u64,
    pub indent: f64,
    pub spaces: Vec<ResponseTextRange>,
}
#[derive(Clone, Debug)]
struct Checkpoint {
    row: u64,
    line: u64,
    continuation: bool,
    offset: u64,
    indent: f64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchPosition {
    pub row: u64,
    pub column: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedMatch {
    pub start: SearchPosition,
    pub end: SearchPosition,
}
#[derive(Clone, Debug, PartialEq)]
pub struct IndexOptions {
    pub path: String,
    pub columns: u64,
    pub prefix: String,
    pub wrapping: Option<WrapLayout>,
}
#[derive(Debug)]
pub struct TextIndex {
    pub options: IndexOptions,
    pub row_count: u64,
    pub line_count: u64,
    pub is_complete: bool,
    pub maximum_columns: u64,
    checkpoints: Vec<Checkpoint>,
    prefix_rows: Vec<TextRow>,
    prefix_lines: u64,
    initial_rows: Vec<TextRow>,
    initial_revision: Option<fs::Metadata>,
    shaper: Option<Arc<dyn TextShaper>>,
}
impl TextIndex {
    /// # Errors
    /// Returns I/O or cancellation errors while scanning the response.
    pub fn new(
        mut options: IndexOptions,
        row_limit: u64,
        shaper: Option<Arc<dyn TextShaper>>,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Self> {
        options.columns = options.columns.max(1);
        let (prefix_rows, prefix_lines) = make_prefix_rows(&options, shaper.as_deref());
        let cache_first =
            row_limit <= 256 && (options.wrapping.is_some() || options.columns <= 4096);
        let initial_revision = if cache_first {
            Some(fs::metadata(&options.path)?)
        } else {
            None
        };
        let mut index = Self {
            options,
            row_count: count(prefix_rows.len()),
            line_count: prefix_lines + 1,
            is_complete: true,
            maximum_columns: prefix_rows.iter().map(|r| r.columns).max().unwrap_or(0),
            checkpoints: vec![],
            prefix_rows,
            prefix_lines,
            initial_rows: vec![],
            initial_revision,
            shaper,
        };
        let mut checkpoints = Vec::new();
        let mut rows = 0;
        let mut maximum = 0;
        let mut last_line = 0;
        let mut complete = true;
        let mut initial_rows = Vec::new();
        let mut retained = 0;
        let mut cache_open = cache_first;
        index.scan_limited(None, cache_first, row_limit, cancellation, |row| {
            if row.number.is_multiple_of(128) {
                checkpoints.push(Checkpoint {
                    row: row.number,
                    line: row.line,
                    continuation: row.continuation,
                    offset: row.byte_offset,
                    indent: row.indent,
                });
            }
            rows = row.number + 1;
            maximum = maximum.max(row.columns);
            last_line = row.line;
            if cache_open {
                let cost = row.text.len()
                    + row.spaces.len() * std::mem::size_of::<ResponseTextRange>()
                    + std::mem::size_of::<TextRow>();
                if retained + cost <= 128 * 1024 {
                    retained += cost;
                    initial_rows.push(row);
                } else {
                    cache_open = false;
                }
            }
            if rows >= row_limit.max(1) {
                complete = false;
                return false;
            }
            true
        })?;
        index.row_count += rows;
        index.line_count = last_line + prefix_lines + 1;
        index.is_complete = complete;
        index.maximum_columns = index.maximum_columns.max(maximum);
        index.checkpoints = checkpoints;
        index.initial_rows = initial_rows
            .into_iter()
            .map(|mut row| {
                row.number += count(index.prefix_rows.len());
                row.line += index.prefix_lines;
                row
            })
            .collect();
        Ok(index)
    }
    /// # Errors
    /// Returns I/O or cancellation errors reading a bounded viewport.
    pub fn rows(
        &self,
        start: u64,
        requested: u64,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Vec<TextRow>> {
        if requested == 0 || start >= self.row_count {
            return Ok(vec![]);
        }
        let requested = requested.min(self.row_count - start);
        let body_start = start.saturating_sub(count(self.prefix_rows.len()));
        let checkpoint = &self.checkpoints[size(body_start / 128).min(self.checkpoints.len() - 1)];
        let mut rows: Vec<_> = self
            .prefix_rows
            .iter()
            .skip(size(start))
            .take(size(requested))
            .cloned()
            .collect();
        if count(rows.len()) == requested {
            return Ok(rows);
        }
        let cached_start = size(body_start);
        let body_count = size(requested) - rows.len();
        if let Some(revision) = &self.initial_revision
            && cached_start.saturating_add(body_count) <= self.initial_rows.len()
        {
            let current = fs::metadata(&self.options.path)?;
            if current.len() == revision.len()
                && current.modified().ok() == revision.modified().ok()
            {
                rows.extend_from_slice(&self.initial_rows[cached_start..cached_start + body_count]);
                return Ok(rows);
            }
        }
        self.scan_limited(
            Some(checkpoint),
            true,
            requested.saturating_add(body_start - checkpoint.row),
            cancellation,
            |mut row| {
                if row.number >= body_start {
                    row.number += count(self.prefix_rows.len());
                    row.line += self.prefix_lines;
                    rows.push(row);
                }
                count(rows.len()) < requested
            },
        )?;
        Ok(rows)
    }
    /// # Errors
    /// Returns I/O or cancellation errors resolving the sparse byte anchor.
    pub fn row_at_byte(
        &self,
        offset: u64,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<u64> {
        let index = self
            .checkpoints
            .partition_point(|c| c.offset <= offset)
            .saturating_sub(1);
        let checkpoint = &self.checkpoints[index];
        let mut result = checkpoint.row;
        self.scan_limited(Some(checkpoint), false, 256, cancellation, |row| {
            if row.byte_offset > offset {
                return false;
            }
            result = row.number;
            true
        })?;
        Ok(result + count(self.prefix_rows.len()))
    }
    /// # Errors
    /// Returns I/O or cancellation errors resolving the anchor.
    pub fn preserving(
        &self,
        anchor: &TextRow,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<u64> {
        if anchor.line < self.prefix_lines {
            return Ok(self
                .prefix_rows
                .iter()
                .find(|r| r.line == anchor.line)
                .map_or(0, |r| r.number));
        }
        self.row_at_byte(anchor.byte_offset, cancellation)
    }
    /// # Errors
    /// Returns I/O, invalid regex, or cancellation errors.
    pub fn search(
        &self,
        query: &SearchQuery,
        selection: Option<(SearchPosition, SearchPosition)>,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Vec<IndexedMatch>> {
        self.search_with_engine(query, selection, cancellation, &super::PortableRegexEngine)
    }
    /// # Errors
    /// Returns I/O, pattern, execution-limit, or cancellation errors.
    pub fn search_with_engine(
        &self,
        query: &SearchQuery,
        selection: Option<(SearchPosition, SearchPosition)>,
        cancellation: &ResponseCancellation,
        engine: &dyn super::RegexEngine,
    ) -> ResponseResult<Vec<IndexedMatch>> {
        if query.text.is_empty() {
            return Ok(vec![]);
        }
        let data = fs::read(&self.options.path)?;
        cancellation.check()?;
        let source = self.options.prefix.clone() + &String::from_utf8_lossy(&data);
        let scope = if let Some((start, end)) = selection {
            let start = self.selection_offset(&start, &source, &data, cancellation)?;
            let end = self.selection_offset(&end, &source, &data, cancellation)?;
            Some(ResponseTextRange {
                location: start.min(end),
                length: start.abs_diff(end),
            })
        } else {
            None
        };
        let ranges = query.matches_with_engine(&source, scope, cancellation, engine)?;
        if ranges.is_empty() {
            return Ok(vec![]);
        }
        let mut events: Vec<_> = ranges
            .iter()
            .enumerate()
            .flat_map(|(i, r)| [(r.location, i, false), (r.end(), i, true)])
            .collect();
        events.sort_by_key(|e| e.0);
        let mut starts = vec![None; ranges.len()];
        let mut ends = starts.clone();
        let mut event_index = 0;
        let mut source_offset = 0;
        let mut consume = |number: u64, raw: &str, last: bool| {
            let length = count(raw.encode_utf16().count());
            let end = source_offset + length;
            while event_index < events.len()
                && (events[event_index].0 < end || last && events[event_index].0 == end)
            {
                let (offset, index, is_end) = events[event_index];
                let local = offset.saturating_sub(source_offset).min(length);
                let column = count(
                    raw.encode_utf16()
                        .take(size(local))
                        .filter(|u| *u != 13 && *u != 10)
                        .count(),
                );
                let position = SearchPosition {
                    row: number,
                    column,
                };
                if is_end {
                    ends[index] = Some(position);
                } else {
                    starts[index] = Some(position);
                }
                event_index += 1;
            }
            source_offset = end;
        };
        for (index, row) in self.prefix_rows.iter().enumerate() {
            let newline = self.prefix_rows.get(index + 1).map_or_else(
                || self.options.prefix.ends_with('\n'),
                |next| !next.continuation,
            );
            consume(
                row.number,
                &(row.text.clone() + if newline { "\n" } else { "" }),
                false,
            );
        }
        let mut previous: Option<TextRow> = None;
        // A single scan maps all capped search endpoints; there is no row-by-row bridge traffic.
        self.scan(None, false, cancellation, |row| {
            if let Some(previous) = &previous {
                consume(
                    previous.number + count(self.prefix_rows.len()),
                    &String::from_utf8_lossy(
                        &data[size(previous.byte_offset)..size(row.byte_offset)],
                    ),
                    false,
                );
            }
            previous = Some(row);
            true
        })?;
        if let Some(previous) = previous {
            consume(
                previous.number + count(self.prefix_rows.len()),
                &String::from_utf8_lossy(&data[size(previous.byte_offset)..]),
                true,
            );
        }
        Ok(starts
            .into_iter()
            .zip(ends)
            .filter_map(|(start, end)| {
                Some(IndexedMatch {
                    start: start?,
                    end: end?,
                })
            })
            .collect())
    }
    fn selection_offset(
        &self,
        position: &SearchPosition,
        source: &str,
        data: &[u8],
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<u64> {
        let Some(row) = self.rows(position.row, 1, cancellation)?.into_iter().next() else {
            return Ok(count(source.encode_utf16().count()));
        };
        let column = position.column.min(count(row.text.encode_utf16().count()));
        if position.row < count(self.prefix_rows.len()) {
            let mut offset = 0;
            for index in 0..size(position.row) {
                offset += count(self.prefix_rows[index].text.encode_utf16().count());
                if !self.prefix_rows[index + 1].continuation {
                    offset += 1;
                }
            }
            return Ok(offset + column);
        }
        Ok(count(self.options.prefix.encode_utf16().count())
            + count(
                String::from_utf8_lossy(&data[..size(row.byte_offset).min(data.len())])
                    .encode_utf16()
                    .count(),
            )
            + column)
    }
    /// # Errors
    /// Returns I/O or cancellation errors.
    pub fn first_match(
        &self,
        query: &str,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Option<u64>> {
        if query.is_empty() {
            return Ok(None);
        }
        if let Some(row) = self.prefix_rows.iter().find(|r| r.text.contains(query)) {
            return Ok(Some(row.number));
        }
        let needle = query.as_bytes();
        let mut reader = File::open(&self.options.path)?;
        let mut buffer = vec![0; 256 * 1024];
        let mut carry = Vec::new();
        let mut offset = 0;
        loop {
            cancellation.check()?;
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                return Ok(None);
            }
            let carried = carry.len();
            carry.extend_from_slice(&buffer[..read]);
            if let Some(found) = super::storage::find_bytes(&carry, needle) {
                return self
                    .row_at_byte(offset - count(carried) + count(found), cancellation)
                    .map(Some);
            }
            offset += count(read);
            let keep = needle.len().saturating_sub(1).min(carry.len());
            carry.drain(..carry.len() - keep);
        }
    }
}

fn make_prefix_rows(
    options: &IndexOptions,
    shaper: Option<&dyn TextShaper>,
) -> (Vec<TextRow>, u64) {
    let mut prefix_rows = Vec::new();
    let mut prefix_lines = 0;
    if !options.prefix.is_empty() {
        let lines: Vec<_> = options.prefix.split_terminator('\n').collect();
        prefix_lines = count(lines.len());
        if let Some(layout) = &options.wrapping {
            let mut paragraphs: Vec<_> = lines
                .iter()
                .map(|line| Paragraph::new(line.as_bytes(), layout))
                .collect();
            shape_paragraphs(&mut paragraphs, layout, shaper);
            for (line, p) in paragraphs.iter().enumerate() {
                let mut start = 0;
                loop {
                    let indent = if start == 0 { 0.0 } else { p.indentation };
                    let range = p.next(start, indent, layout);
                    prefix_rows.push(TextRow {
                        spaces: vec![],
                        number: count(prefix_rows.len()),
                        line: count(line),
                        continuation: start > 0,
                        byte_offset: 0,
                        text: String::from_utf16_lossy(&p.units[start..size(range.end())]),
                        columns: p.columns(range, layout.advance),
                        indent,
                    });
                    start = size(range.end());
                    if start >= p.units.len() {
                        break;
                    }
                }
            }
        } else {
            for (line, text) in lines.into_iter().enumerate() {
                let clusters: Vec<_> = text.graphemes(true).collect();
                let mut start: usize = 0;
                loop {
                    let end = start
                        .saturating_add(size(options.columns))
                        .min(clusters.len());
                    prefix_rows.push(TextRow {
                        spaces: vec![],
                        number: count(prefix_rows.len()),
                        line: count(line),
                        continuation: start > 0,
                        byte_offset: 0,
                        text: clusters[start..end].concat(),
                        columns: count(end - start),
                        indent: 0.0,
                    });
                    start = end;
                    if start >= clusters.len() {
                        break;
                    }
                }
            }
        }
    }
    for row in &mut prefix_rows {
        row.spaces = super::space_runs(&row.text);
    }
    (prefix_rows, prefix_lines)
}
