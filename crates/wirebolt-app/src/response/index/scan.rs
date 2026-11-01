use super::{
    Checkpoint, File, Paragraph, Read, ResponseCancellation, ResponseResult, TextIndex, TextRow,
    WrapLayout, count, shape_paragraphs, size,
};
use std::io::{Seek, SeekFrom};
impl TextIndex {
    pub(super) fn scan(
        &self,
        checkpoint: Option<&Checkpoint>,
        collect: bool,
        cancellation: &ResponseCancellation,
        consume: impl FnMut(TextRow) -> bool,
    ) -> ResponseResult<()> {
        self.scan_limited(checkpoint, collect, u64::MAX, cancellation, consume)
    }
    pub(super) fn scan_limited(
        &self,
        checkpoint: Option<&Checkpoint>,
        collect: bool,
        row_hint: u64,
        cancellation: &ResponseCancellation,
        consume: impl FnMut(TextRow) -> bool,
    ) -> ResponseResult<()> {
        if let Some(layout) = &self.options.wrapping {
            self.scan_wrapped(layout, checkpoint, collect, row_hint, cancellation, consume)
        } else {
            self.scan_plain(checkpoint, collect, cancellation, consume)
        }
    }
    fn scan_plain(
        &self,
        checkpoint: Option<&Checkpoint>,
        collect: bool,
        cancellation: &ResponseCancellation,
        mut consume: impl FnMut(TextRow) -> bool,
    ) -> ResponseResult<()> {
        let mut reader = File::open(&self.options.path)?;
        let mut offset = checkpoint.map_or(0, |c| c.offset);
        reader.seek(SeekFrom::Start(offset))?;
        let mut state = PlainRow {
            row: TextRow {
                spaces: vec![],
                number: checkpoint.map_or(0, |c| c.row),
                line: checkpoint.map_or(0, |c| c.line),
                continuation: checkpoint.is_some_and(|c| c.continuation),
                byte_offset: offset,
                text: String::new(),
                columns: 0,
                indent: 0.0,
            },
            bytes: Vec::new(),
        };
        let mut word_break: Option<(u64, u64)> = None;
        let mut buffer = vec![0; 256 * 1024];
        loop {
            cancellation.check()?;
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                state.emit(collect, &mut consume);
                return Ok(());
            }
            for &byte in &buffer[..read] {
                if byte == 10 {
                    if !state.emit(collect, &mut consume) {
                        return Ok(());
                    }
                    word_break = None;
                    state.row.line += 1;
                    state.row.continuation = false;
                    state.row.byte_offset = offset + 1;
                } else {
                    if byte & 0xc0 != 0x80 {
                        if state.row.columns >= self.options.columns {
                            let split =
                                word_break.filter(|(split, _)| *split > state.row.byte_offset);
                            if !state.soft_wrap(split, offset, collect, &mut consume) {
                                return Ok(());
                            }
                            word_break = None;
                            state.row.continuation = true;
                        }
                        state.row.columns += if byte == 9 {
                            4 - state.row.columns % 4
                        } else {
                            1
                        };
                    }
                    if collect {
                        state.bytes.push(byte);
                    }
                    if byte == 32 || byte == 9 {
                        word_break = Some((offset + 1, state.row.columns));
                    }
                }
                offset += 1;
            }
        }
    }
    fn scan_wrapped(
        &self,
        layout: &WrapLayout,
        checkpoint: Option<&Checkpoint>,
        collect: bool,
        row_hint: u64,
        cancellation: &ResponseCancellation,
        mut consume: impl FnMut(TextRow) -> bool,
    ) -> ResponseResult<()> {
        let mut reader = File::open(&self.options.path)?;
        let mut offset = checkpoint.map_or(0, |c| c.offset);
        reader.seek(SeekFrom::Start(offset))?;
        let mut number = checkpoint.map_or(0, |c| c.row);
        let mut line = checkpoint.map_or(0, |c| c.line);
        let mut continuation = checkpoint.is_some_and(|c| c.continuation);
        let mut indentation = checkpoint.map_or(0.0, |c| c.indent);
        let mut pending = Vec::new();
        let mut finished = false;
        // Shape only a bounded batch near a requested viewport. Full scans
        // retain large I/O batches; short reads avoid measuring the next 1,000 rows.
        let mut buffer = vec![0; if row_hint <= 512 { 8 * 1024 } else { 64 * 1024 }];
        loop {
            cancellation.check()?;
            if !finished {
                let read = reader.read(&mut buffer)?;
                finished = read == 0;
                pending.extend_from_slice(&buffer[..read]);
            }
            let mut spans = Vec::new();
            let mut cursor = 0;
            while cursor < pending.len() || finished {
                let newline = pending[cursor..]
                    .iter()
                    .position(|&b| b == 10)
                    .map(|p| cursor + p);
                let end = newline.unwrap_or(pending.len());
                spans.push((cursor, end, newline.is_some()));
                if newline.is_none() {
                    break;
                }
                cursor = end + 1;
            }
            let mut paragraphs: Vec<_> = spans
                .iter()
                .map(|&(start, end, _)| Paragraph::new(&pending[start..end], layout))
                .collect();
            shape_paragraphs(&mut paragraphs, layout, self.shaper.as_deref());
            let mut consumed = 0;
            for ((begin, end, newline), paragraph) in spans.into_iter().zip(paragraphs) {
                let complete = newline || finished;
                if !continuation {
                    indentation = paragraph.indentation;
                }
                let safe_end = if complete {
                    paragraph.units.len()
                } else {
                    paragraph.units.len().saturating_sub(512)
                };
                let mut start = 0;
                loop {
                    let indent = if continuation { indentation } else { 0.0 };
                    let range = paragraph.next(start, indent, layout);
                    if !complete && size(range.end()) >= safe_end {
                        break;
                    }
                    let mut row = TextRow {
                        spaces: vec![],
                        number,
                        line,
                        continuation,
                        byte_offset: offset + count(begin + paragraph.offsets[start]),
                        text: if collect {
                            String::from_utf16_lossy(&paragraph.units[start..size(range.end())])
                        } else {
                            String::new()
                        },
                        columns: paragraph.columns(range, layout.advance),
                        indent,
                    };
                    if collect {
                        row.spaces = crate::response::space_runs(&row.text);
                    }
                    if !consume(row) {
                        return Ok(());
                    }
                    number += 1;
                    continuation = true;
                    start = size(range.end());
                    if start >= paragraph.units.len() {
                        break;
                    }
                }
                if complete {
                    if !newline {
                        return Ok(());
                    }
                    consumed = end + 1;
                    line += 1;
                    continuation = false;
                    indentation = 0.0;
                } else {
                    consumed = begin + paragraph.offsets[start];
                    break;
                }
            }
            if consumed > 0 {
                pending.drain(..consumed);
                offset += count(consumed);
            }
        }
    }
}
struct PlainRow {
    row: TextRow,
    bytes: Vec<u8>,
}
impl PlainRow {
    fn emit(&mut self, collect: bool, consume: &mut impl FnMut(TextRow) -> bool) -> bool {
        let text = if collect {
            String::from_utf8_lossy(&self.bytes).replace('\r', "")
        } else {
            String::new()
        };
        let more = consume(TextRow {
            spaces: if collect {
                crate::response::space_runs(&text)
            } else {
                vec![]
            },
            text,
            ..self.row.clone()
        });
        self.row.number += 1;
        self.bytes.clear();
        self.row.columns = 0;
        more
    }
    fn soft_wrap(
        &mut self,
        split: Option<(u64, u64)>,
        offset: u64,
        collect: bool,
        consume: &mut impl FnMut(TextRow) -> bool,
    ) -> bool {
        if let Some((split, column)) = split {
            let suffix_count = size(offset - split);
            let suffix = if collect {
                self.bytes.split_off(self.bytes.len() - suffix_count)
            } else {
                vec![]
            };
            let remaining = self.row.columns - column;
            self.row.columns = column;
            if !self.emit(collect, consume) {
                return false;
            }
            self.bytes = suffix;
            self.row.columns = remaining;
            self.row.byte_offset = split;
        } else {
            if !self.emit(collect, consume) {
                return false;
            }
            self.row.byte_offset = offset;
        }
        true
    }
}
