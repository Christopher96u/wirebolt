//! WebSocket document lifecycle and bounded message retention.
use crate::domain::{DomainBody, identity::unique_id};
use crate::response::{ResponseHeaderValue, reference_time};
use base64::{
    Engine, alphabet,
    engine::{GeneralPurpose, GeneralPurposeConfig},
};
use std::collections::VecDeque;
pub const MESSAGE_BYTES: usize = 16 * 1024 * 1024;
pub const MESSAGE_LIMIT: usize = 1000;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionSocketStatus {
    Disconnected,
    Connecting,
    Connected,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionSocketNotice {
    Connected,
    Disconnected,
    Failed,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SessionSocketMessage {
    pub id: String,
    pub timestamp: f64,
    pub data: Vec<u8>,
    pub binary: bool,
    pub outgoing: bool,
    pub notice: Option<SessionSocketNotice>,
    pub control: Option<String>,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SessionSocketSnapshot {
    pub generation: String,
    pub first_sequence: u64,
    pub next_sequence: u64,
    pub status: SessionSocketStatus,
    pub messages: Vec<SessionSocketMessage>,
    pub error: Option<String>,
    pub headers: Vec<ResponseHeaderValue>,
}
#[derive(Debug)]
pub struct SessionSocket {
    sequence: u64,
    generation: String,
    status: SessionSocketStatus,
    messages: VecDeque<SessionSocketMessage>,
    bytes: usize,
    error: Option<String>,
    headers: Vec<ResponseHeaderValue>,
}
impl Default for SessionSocket {
    fn default() -> Self {
        Self {
            sequence: 0,
            generation: unique_id(),
            status: SessionSocketStatus::Disconnected,
            messages: VecDeque::new(),
            bytes: 0,
            error: None,
            headers: vec![],
        }
    }
}
impl SessionSocket {
    #[must_use]
    pub fn snapshot(&self) -> SessionSocketSnapshot {
        self.snapshot_since(0)
    }
    #[must_use]
    pub fn snapshot_since(&self, since: u64) -> SessionSocketSnapshot {
        let first = self.sequence.saturating_sub(self.messages.len() as u64);
        SessionSocketSnapshot {
            first_sequence: first,
            next_sequence: self.sequence,
            generation: self.generation.clone(),
            status: self.status.clone(),
            messages: self
                .messages
                .iter()
                .skip(usize::try_from(since.saturating_sub(first)).unwrap_or(usize::MAX))
                .cloned()
                .collect(),
            error: self.error.clone(),
            headers: self.headers.clone(),
        }
    }
    pub fn begin(&mut self) -> Option<String> {
        if self.status != SessionSocketStatus::Disconnected {
            return None;
        }
        self.generation = unique_id();
        self.error = None;
        self.headers.clear();
        self.status = SessionSocketStatus::Connecting;
        Some(self.generation.clone())
    }
    #[must_use]
    pub fn accepts(&self, generation: &str) -> bool {
        self.generation == generation
    }
    pub fn connected(&mut self, generation: &str, headers: Vec<ResponseHeaderValue>, url: &str) {
        if !self.accepts(generation) {
            return;
        }
        self.headers = headers;
        self.status = SessionSocketStatus::Connected;
        self.notice(
            format!("Connected to {url}"),
            SessionSocketNotice::Connected,
        );
    }
    pub fn append(
        &mut self,
        generation: &str,
        data: Vec<u8>,
        binary: bool,
        outgoing: bool,
        control: Option<String>,
    ) {
        if self.accepts(generation) {
            self.push(data, binary, outgoing, None, control);
        }
    }
    fn notice(&mut self, text: String, notice: SessionSocketNotice) {
        self.push(text.into_bytes(), false, false, Some(notice), None);
    }
    fn push(
        &mut self,
        data: Vec<u8>,
        binary: bool,
        outgoing: bool,
        notice: Option<SessionSocketNotice>,
        control: Option<String>,
    ) {
        while !self.messages.is_empty()
            && (self.messages.len() >= MESSAGE_LIMIT || self.bytes + data.len() > MESSAGE_BYTES)
        {
            if let Some(message) = self.messages.pop_front() {
                self.bytes -= message.data.len();
            }
        }
        self.bytes += data.len();
        let text = String::from_utf8_lossy(&data).into_owned();
        self.sequence += 1;
        self.messages.push_back(SessionSocketMessage {
            id: unique_id(),
            timestamp: reference_time(),
            data,
            binary,
            outgoing,
            notice,
            control,
            text,
        });
    }
    pub fn finish(&mut self, generation: &str, error: Option<String>) {
        if !self.accepts(generation) {
            return;
        }
        if let Some(error) = error {
            self.error = Some(error);
        }
        self.notice(
            self.error.clone().unwrap_or_else(|| "Disconnected".into()),
            if self.error.is_some() {
                SessionSocketNotice::Failed
            } else {
                SessionSocketNotice::Disconnected
            },
        );
        self.status = SessionSocketStatus::Disconnected;
    }
    pub fn closed(&mut self, generation: &str) {
        if self.accepts(generation) {
            self.status = SessionSocketStatus::Disconnected;
        }
    }
    pub fn disconnect(&mut self) {
        if self.status == SessionSocketStatus::Connected {
            self.notice("Disconnected".into(), SessionSocketNotice::Disconnected);
        }
        self.generation = unique_id();
        self.status = SessionSocketStatus::Disconnected;
    }
    pub fn clear(&mut self) {
        self.messages.clear();
        self.bytes = 0;
    }
    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }
}
const BINARY_CONTENT_TYPE: &str = "application/octet-stream";
const HEX_CONTENT_TYPE: &str = "application/octet-stream; encoding=hex";
/// A file message whose path is typed rather than chosen.
const FILE_PATH_CONTENT_TYPE: &str = "application/x-wirebolt-file";
/// The payload a WebSocket message composer edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionMessageKind {
    Text,
    Json,
    Binary,
    File,
}
/// How the composer writes a binary message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionBinaryEncoding {
    Base64,
    Hex,
}
impl SessionBinaryEncoding {
    const fn content_type(self) -> &'static str {
        match self {
            Self::Base64 => BINARY_CONTENT_TYPE,
            Self::Hex => HEX_CONTENT_TYPE,
        }
    }
    fn encode(self, bytes: &[u8]) -> String {
        match self {
            Self::Base64 => base64::engine::general_purpose::STANDARD.encode(bytes),
            Self::Hex => bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}
/// What the composer shows for a message body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMessageDraft {
    pub kind: SessionMessageKind,
    pub encoding: SessionBinaryEncoding,
    pub text: String,
}
#[must_use]
pub fn message_draft(body: &DomainBody) -> SessionMessageDraft {
    let (kind, encoding) = match body {
        DomainBody::Json { .. } => (SessionMessageKind::Json, SessionBinaryEncoding::Base64),
        DomainBody::Text { content_type, .. } => match content_type.as_deref() {
            Some(BINARY_CONTENT_TYPE) => {
                (SessionMessageKind::Binary, SessionBinaryEncoding::Base64)
            }
            Some(HEX_CONTENT_TYPE) => (SessionMessageKind::Binary, SessionBinaryEncoding::Hex),
            Some(FILE_PATH_CONTENT_TYPE) => {
                (SessionMessageKind::File, SessionBinaryEncoding::Base64)
            }
            _ => (SessionMessageKind::Text, SessionBinaryEncoding::Base64),
        },
        DomainBody::File { .. } => (SessionMessageKind::File, SessionBinaryEncoding::Base64),
        _ => (SessionMessageKind::Text, SessionBinaryEncoding::Base64),
    };
    let text = match body {
        DomainBody::Json { value }
        | DomainBody::Text { value, .. }
        | DomainBody::Xml { value }
        | DomainBody::Html { value }
        | DomainBody::Raw { value, .. } => value.clone(),
        _ => String::new(),
    };
    SessionMessageDraft {
        kind,
        encoding,
        text,
    }
}
/// The empty message a composer starts with after choosing `kind`.
#[must_use]
pub fn switching_message(kind: SessionMessageKind) -> DomainBody {
    match kind {
        SessionMessageKind::Text => DomainBody::Text {
            content_type: Some("text/plain".into()),
            value: String::new(),
        },
        SessionMessageKind::Json => DomainBody::Json {
            value: "{\n  \n}".into(),
        },
        SessionMessageKind::Binary => DomainBody::Text {
            content_type: Some(BINARY_CONTENT_TYPE.into()),
            value: String::new(),
        },
        SessionMessageKind::File => DomainBody::File {
            path: String::new(),
            content_type: None,
        },
    }
}
/// Rewrites a binary message in `encoding`; text that does not decode is kept as typed.
#[must_use]
pub fn reencoding_message(body: &DomainBody, encoding: SessionBinaryEncoding) -> DomainBody {
    let draft = message_draft(body);
    let value = decode_binary(&draft.text, draft.encoding == SessionBinaryEncoding::Hex)
        .map_or(draft.text, |bytes| encoding.encode(&bytes));
    DomainBody::Text {
        content_type: Some(encoding.content_type().into()),
        value,
    }
}
/// Replaces the typed message, keeping its kind and binary encoding.
#[must_use]
pub fn editing_message(body: &DomainBody, text: String) -> DomainBody {
    let draft = message_draft(body);
    let content_type = match draft.kind {
        SessionMessageKind::Json => return DomainBody::Json { value: text },
        SessionMessageKind::Binary => draft.encoding.content_type(),
        SessionMessageKind::File => FILE_PATH_CONTENT_TYPE,
        SessionMessageKind::Text => "text/plain",
    };
    DomainBody::Text {
        content_type: Some(content_type.into()),
        value: text,
    }
}
/// # Errors
/// Returns a message-editor validation error for invalid Base64 or hexadecimal input.
pub fn decode_binary(text: &str, hex: bool) -> Result<Vec<u8>, String> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !hex {
        return GeneralPurpose::new(
            &alphabet::STANDARD,
            GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
        )
        .decode(compact)
        .map_err(|_| "Enter a valid Base64 binary message.".into());
    }
    let invalid = || "Enter a valid hexadecimal binary message.".to_owned();
    if !compact.len().is_multiple_of(2) || !compact.is_ascii() {
        return Err(invalid());
    }
    compact
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = char::from(pair[0]).to_digit(16).ok_or_else(invalid)?;
            let low = char::from(pair[1]).to_digit(16).ok_or_else(invalid)?;
            u8::try_from(high * 16 + low).map_err(|_| invalid())
        })
        .collect()
}
/// # Errors
/// Returns a body-validation error or a file I/O error.
pub fn message_body(body: &DomainBody) -> Result<(Vec<u8>, bool), String> {
    let result = match body {
        DomainBody::File { path, .. } => {
            let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
            if metadata.len() > MESSAGE_BYTES as u64 {
                return Err("The message exceeds the 16 MB limit.".into());
            }
            (std::fs::read(path).map_err(|e| e.to_string())?, true)
        }
        DomainBody::Text {
            content_type,
            value,
        } if content_type
            .as_ref()
            .is_some_and(|t| t.starts_with(BINARY_CONTENT_TYPE)) =>
        {
            (
                decode_binary(value, content_type.as_deref() == Some(HEX_CONTENT_TYPE))?,
                true,
            )
        }
        DomainBody::Json { value } => {
            crate::domain::body::parse_json(value).map_err(
                |()| "The data couldn’t be read because it isn’t in the correct format.",
            )?;
            (value.as_bytes().to_vec(), false)
        }
        DomainBody::Text { value, .. }
        | DomainBody::Raw { value, .. }
        | DomainBody::Xml { value }
        | DomainBody::Html { value } => (value.as_bytes().to_vec(), false),
        DomainBody::Empty => (vec![], false),
        _ => return Err("Choose a Text, JSON, Binary or File message.".into()),
    };
    if result.0.len() > MESSAGE_BYTES {
        return Err("The message exceeds the 16 MB limit.".into());
    }
    Ok(result)
}
