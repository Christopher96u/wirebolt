use super::{ResponseCancellation, ResponseError, ResponseResult, count};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonTreeNode {
    pub id: String,
    pub key: String,
    pub kind: JsonNodeKind,
    pub value: String,
    pub children: Option<Vec<JsonTreeNode>>,
}
#[derive(Debug)]
pub struct JsonDocument {
    file: NamedTempFile,
    pub byte_count: u64,
    pub preview: String,
    pub contains_unicode_escapes: bool,
}
impl JsonDocument {
    /// # Errors
    /// Rejects invalid JSON, invalid UTF-8, excessive nesting/output, cancellation, and I/O errors.
    pub fn new(
        source: &Path,
        decode: bool,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Self> {
        let input = File::open(source)?;
        let limit = input.metadata()?.len().saturating_mul(16).max(1024 * 1024);
        let mut file = tempfile::Builder::new()
            .prefix("wirebolt-json-")
            .suffix(".body")
            .tempfile()?;
        let output = Presentation {
            writer: BufWriter::with_capacity(64 * 1024, file.as_file_mut()),
            written: 0,
            limit,
            preview: Vec::new(),
        };
        let mut parser = Parser {
            input: BufReader::with_capacity(64 * 1024, input),
            output: Some(output),
            decode,
            saw_escape: false,
            budget: 0,
            max_depth: 256,
            cancellation,
            steps: 0,
        };
        parser.document()?;
        let contains_unicode_escapes = parser.saw_escape;
        let mut output = parser.output.take().ok_or(ResponseError::InvalidJson)?;
        output.writer.flush()?;
        let byte_count = output.written;
        let preview_limit = if byte_count <= 1024 * 1024 {
            1024 * 1024
        } else {
            32 * 1024
        };
        let preview =
            String::from_utf8_lossy(&output.preview[..output.preview.len().min(preview_limit)])
                .into_owned();
        drop(output);
        drop(parser);
        Ok(Self {
            file,
            byte_count,
            preview,
            contains_unicode_escapes,
        })
    }
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.file.path().to_owned()
    }
}
/// # Errors
/// Rejects malformed JSON, excessive nesting, and cancellation.
pub fn json_tree(
    data: &[u8],
    node_limit: u64,
    cancellation: &ResponseCancellation,
) -> ResponseResult<JsonTreeNode> {
    json_tree_reader(BufReader::new(data), node_limit, cancellation)
}
/// # Errors
/// Returns I/O, JSON syntax, depth-limit and cancellation errors.
pub fn json_tree_file(
    path: &Path,
    node_limit: u64,
    cancellation: &ResponseCancellation,
) -> ResponseResult<JsonTreeNode> {
    json_tree_reader(
        BufReader::with_capacity(64 * 1024, File::open(path)?),
        node_limit,
        cancellation,
    )
}
fn json_tree_reader(
    mut input: impl BufRead,
    node_limit: u64,
    cancellation: &ResponseCancellation,
) -> ResponseResult<JsonTreeNode> {
    if input.fill_buf()?.starts_with(&[0xef, 0xbb, 0xbf]) {
        input.consume(3);
    }
    let mut parser: Parser<'_, _, std::io::Sink> = Parser {
        input,
        output: None,
        decode: false,
        saw_escape: false,
        budget: node_limit.max(1),
        max_depth: 512,
        cancellation,
        steps: 0,
    };
    parser.document()?.ok_or(ResponseError::InvalidJson)
}

struct Presentation<W> {
    writer: W,
    written: u64,
    limit: u64,
    preview: Vec<u8>,
}
struct Parser<'a, R, W> {
    input: R,
    output: Option<Presentation<W>>,
    decode: bool,
    saw_escape: bool,
    budget: u64,
    max_depth: usize,
    cancellation: &'a ResponseCancellation,
    steps: usize,
}
impl<R: BufRead, W: Write> Parser<'_, R, W> {
    fn peek(&mut self) -> ResponseResult<Option<u8>> {
        Ok(self.input.fill_buf()?.first().copied())
    }
    fn take(&mut self) -> ResponseResult<u8> {
        self.steps += 1;
        if self.steps.is_multiple_of(4096) {
            self.cancellation.check()?;
        }
        let byte = self.peek()?.ok_or(ResponseError::InvalidJson)?;
        self.input.consume(1);
        Ok(byte)
    }
    fn emit(&mut self, bytes: &[u8]) -> ResponseResult<()> {
        if let Some(output) = &mut self.output {
            if output.written.saturating_add(count(bytes.len())) > output.limit {
                return Err(ResponseError::PresentationLimit);
            }
            output.writer.write_all(bytes)?;
            output.written += count(bytes.len());
            output.preview.extend_from_slice(
                &bytes[..bytes
                    .len()
                    .min((1024 * 1024_usize).saturating_sub(output.preview.len()))],
            );
        }
        Ok(())
    }
    fn whitespace(&mut self) -> ResponseResult<()> {
        while self.peek()?.is_some_and(|b| [9, 10, 13, 32].contains(&b)) {
            self.take()?;
        }
        Ok(())
    }
    fn expect(&mut self, byte: u8) -> ResponseResult<()> {
        if self.take()? != byte {
            return Err(ResponseError::InvalidJson);
        }
        self.emit(&[byte])
    }
    fn newline(&mut self, depth: usize) -> ResponseResult<()> {
        self.emit(b"\n")?;
        for _ in 0..depth {
            self.emit(b"  ")?;
        }
        Ok(())
    }
    fn document(&mut self) -> ResponseResult<Option<JsonTreeNode>> {
        self.cancellation.check()?;
        let root = self.value("Root".into(), "root".into(), 0)?;
        self.whitespace()?;
        if self.peek()?.is_some() {
            return Err(ResponseError::InvalidJson);
        }
        Ok(root)
    }
    fn value(
        &mut self,
        key: String,
        path: String,
        depth: usize,
    ) -> ResponseResult<Option<JsonTreeNode>> {
        if depth > self.max_depth {
            return Err(ResponseError::PresentationLimit);
        }
        let materialize = self.budget > 0;
        self.budget = self.budget.saturating_sub(1);
        self.whitespace()?;
        let byte = self.peek()?.ok_or(ResponseError::InvalidJson)?;
        let (kind, value, children) = match byte {
            b'{' | b'[' => {
                let object = byte == b'{';
                let closing = if object { b'}' } else { b']' };
                self.expect(byte)?;
                self.whitespace()?;
                let mut children = Vec::new();
                let mut omitted = 0;
                let mut total = 0;
                if self.peek()? == Some(closing) {
                    self.expect(closing)?;
                } else {
                    loop {
                        self.newline(depth + 1)?;
                        self.whitespace()?;
                        let child_key = if object {
                            let key = self.string(materialize && self.budget > 0)?;
                            self.whitespace()?;
                            self.expect(b':')?;
                            self.emit(b" ")?;
                            key
                        } else if self.budget > 0 {
                            total.to_string()
                        } else {
                            String::new()
                        };
                        let child_path = if self.budget > 0 {
                            format!("{path}/{}", child_key.replace('~', "~0").replace('/', "~1"))
                        } else {
                            String::new()
                        };
                        if let Some(child) = self.value(child_key, child_path, depth + 1)? {
                            children.push(child);
                        } else {
                            omitted += 1;
                        }
                        total += 1;
                        self.whitespace()?;
                        if self.peek()? == Some(closing) {
                            self.newline(depth)?;
                            self.expect(closing)?;
                            break;
                        }
                        self.expect(b',')?;
                    }
                }
                let kind = if object {
                    JsonNodeKind::Object
                } else {
                    JsonNodeKind::Array
                };
                if materialize && omitted > 0 {
                    children.push(JsonTreeNode {
                        id: format!("{path}/…"),
                        key: "…".into(),
                        kind: JsonNodeKind::Truncated,
                        value: format!("{omitted} more items not shown"),
                        children: None,
                    });
                }
                (
                    kind,
                    if materialize {
                        format!("{}({total} items)", kind.name())
                    } else {
                        String::new()
                    },
                    Some(children),
                )
            }
            b'"' => (JsonNodeKind::String, self.string(materialize)?, None),
            b't' | b'f' | b'n' => self.literal_value(byte, materialize)?,
            _ => (JsonNodeKind::Number, self.number(materialize)?, None),
        };
        Ok(materialize.then_some(JsonTreeNode {
            id: path,
            key,
            kind,
            value,
            children,
        }))
    }
    fn literal_value(
        &mut self,
        byte: u8,
        materialize: bool,
    ) -> ResponseResult<(JsonNodeKind, String, Option<Vec<JsonTreeNode>>)> {
        Ok({
            let literal: &[u8] = match byte {
                b't' => b"true",
                b'f' => b"false",
                _ => b"null",
            };
            for &byte in literal {
                self.expect(byte)?;
            }
            (
                if byte == b'n' {
                    JsonNodeKind::Null
                } else {
                    JsonNodeKind::Boolean
                },
                if materialize {
                    String::from_utf8_lossy(literal).into_owned()
                } else {
                    String::new()
                },
                None,
            )
        })
    }
    fn number(&mut self, decode: bool) -> ResponseResult<String> {
        let mut number = Vec::new();
        if self.peek()? == Some(b'-') {
            number.push(self.take()?);
        }
        if self.peek()? == Some(b'0') {
            number.push(self.take()?);
        } else {
            self.digits(&mut number, decode)?;
        }
        if self.peek()? == Some(b'.') {
            number.push(self.take()?);
            self.digits(&mut number, decode)?;
        }
        if self.peek()?.is_some_and(|b| b == b'e' || b == b'E') {
            number.push(self.take()?);
            if self.peek()?.is_some_and(|b| b == b'+' || b == b'-') {
                number.push(self.take()?);
            }
            self.digits(&mut number, decode)?;
        }
        self.emit(&number)?;
        Ok(if decode {
            String::from_utf8_lossy(&number).into_owned()
        } else {
            String::new()
        })
    }
    fn digits(&mut self, output: &mut Vec<u8>, decode: bool) -> ResponseResult<()> {
        if !self.peek()?.is_some_and(|b| b.is_ascii_digit()) {
            return Err(ResponseError::InvalidJson);
        }
        while self.peek()?.is_some_and(|b| b.is_ascii_digit()) {
            output.push(self.take()?);
            // A numeric token can itself span the response. Formatting and
            // validation keep only a fixed output block; a tree scalar retains
            // its spelling only when the node budget materialises it.
            if !decode && output.len() >= 64 * 1024 {
                self.emit(output)?;
                output.clear();
            }
        }
        Ok(())
    }
    fn hex4(&mut self) -> ResponseResult<(u32, [u8; 4])> {
        let mut digits = [0; 4];
        let mut unit = 0;
        for digit in &mut digits {
            *digit = self.take()?;
            unit = (unit << 4)
                | char::from(*digit)
                    .to_digit(16)
                    .ok_or(ResponseError::InvalidJson)?;
        }
        Ok((unit, digits))
    }
    fn emit_escape(&mut self, digits: [u8; 4]) -> ResponseResult<()> {
        self.emit(b"\\u")?;
        self.emit(&digits)
    }
    fn emit_unicode(&mut self, unit: u32, digits: [u8; 4]) -> ResponseResult<()> {
        if self.decode && displayable(unit) {
            let mut buf = [0; 4];
            self.emit(
                char::from_u32(unit)
                    .ok_or(ResponseError::InvalidJson)?
                    .encode_utf8(&mut buf)
                    .as_bytes(),
            )
        } else {
            self.emit_escape(digits)
        }
    }
    fn string(&mut self, decode: bool) -> ResponseResult<String> {
        self.expect(b'"')?;
        let mut decoded = Vec::new();
        loop {
            let byte = self.take()?;
            if byte < 32 {
                return Err(ResponseError::InvalidJson);
            }
            if byte == b'"' {
                self.emit(&[byte])?;
                return Ok(String::from_utf8_lossy(&decoded).into_owned());
            }
            if byte != b'\\' {
                if self.output.is_some() && byte >= 128 {
                    let length = match byte {
                        194..=223 => 2,
                        224..=239 => 3,
                        240..=244 => 4,
                        _ => return Err(ResponseError::InvalidJson),
                    };
                    let mut bytes = [0; 4];
                    bytes[0] = byte;
                    for next in &mut bytes[1..length] {
                        *next = self.take()?;
                    }
                    std::str::from_utf8(&bytes[..length])
                        .map_err(|_| ResponseError::InvalidJson)?;
                    self.emit(&bytes[..length])?;
                    if decode {
                        decoded.extend_from_slice(&bytes[..length]);
                    }
                } else {
                    self.emit(&[byte])?;
                    if decode {
                        decoded.push(byte);
                    }
                }
                continue;
            }
            let escape = self.take()?;
            if escape != b'u' {
                self.simple_escape(escape, decode, &mut decoded)?;
                continue;
            }
            self.saw_escape = true;
            let (unit, digits) = self.hex4()?;
            self.unicode_escape(unit, digits, decode, &mut decoded)?;
        }
    }
    fn unicode_escape(
        &mut self,
        mut unit: u32,
        mut digits: [u8; 4],
        decode: bool,
        decoded: &mut Vec<u8>,
    ) -> ResponseResult<()> {
        loop {
            if !(0xd800..0xdc00).contains(&unit) || self.peek()? != Some(b'\\') {
                self.emit_unicode(unit, digits)?;
                if decode {
                    append_scalar(decoded, unit);
                }
                return Ok(());
            }
            self.take()?;
            let next = self.take()?;
            if next != b'u' {
                self.emit_escape(digits)?;
                if decode {
                    append_scalar(decoded, 0xfffd);
                }
                return self.simple_escape(next, decode, decoded);
            }
            let (low, low_digits) = self.hex4()?;
            if (0xdc00..0xe000).contains(&low) {
                let scalar = 0x10000 + ((unit - 0xd800) << 10) + low - 0xdc00;
                if self.decode {
                    self.emit_unicode(scalar, digits)?;
                } else {
                    self.emit_escape(digits)?;
                    self.emit_escape(low_digits)?;
                }
                if decode {
                    append_scalar(decoded, scalar);
                }
                return Ok(());
            }
            self.emit_escape(digits)?;
            if decode {
                append_scalar(decoded, 0xfffd);
            }
            if self.output.is_none() {
                unit = low;
                digits = low_digits;
                continue;
            }
            self.emit_unicode(low, low_digits)?;
            if decode {
                append_scalar(decoded, low);
            }
            return Ok(());
        }
    }
    fn simple_escape(
        &mut self,
        byte: u8,
        decode: bool,
        decoded: &mut Vec<u8>,
    ) -> ResponseResult<()> {
        let scalar = match byte {
            b'"' | b'/' | b'\\' => byte,
            b'b' => 8,
            b'f' => 12,
            b'n' => 10,
            b'r' => 13,
            b't' => 9,
            _ => return Err(ResponseError::InvalidJson),
        };
        self.emit(&[b'\\', byte])?;
        if decode {
            decoded.push(scalar);
        }
        Ok(())
    }
}
fn append_scalar(output: &mut Vec<u8>, scalar: u32) {
    let mut buf = [0; 4];
    output.extend_from_slice(
        char::from_u32(scalar)
            .unwrap_or('�')
            .encode_utf8(&mut buf)
            .as_bytes(),
    );
}
fn displayable(unit: u32) -> bool {
    !matches!(unit, 0..0x20 | 0x22 | 0x5c | 0x7f..=0x9f | 0xad | 0x200b..=0x200f | 0x2028..=0x202e | 0x2060..=0x206f | 0xd800..=0xdfff | 0xfeff | 0xfff9..=0xfffb)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonNodeKind {
    Object,
    Array,
    String,
    Number,
    Boolean,
    Null,
    Truncated,
}
impl JsonNodeKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Object => "Object",
            Self::Array => "Array",
            Self::String => "String",
            Self::Number => "Number",
            Self::Boolean => "Boolean",
            Self::Null => "Null",
            Self::Truncated => "Truncated",
        }
    }
}
