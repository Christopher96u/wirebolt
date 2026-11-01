//! WebSocket document lifecycle and bounded message retention.
use crate::domain::{DomainBody, identity::unique_id};
use crate::response::{ResponseHeaderValue, reference_time};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::VecDeque;
pub const MESSAGE_BYTES: usize = 16 * 1024 * 1024;
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
            && (self.messages.len() >= 1000 || self.bytes + data.len() > MESSAGE_BYTES)
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
        self.error = error;
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
/// # Errors
/// Returns the existing message-editor validation text.
pub fn decode_binary(text: &str, hex: bool) -> Result<Vec<u8>, String> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !hex {
        return STANDARD
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
/// Returns the existing body-validation or file error.
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
            .is_some_and(|t| t.starts_with("application/octet-stream")) =>
        {
            (
                decode_binary(
                    value,
                    content_type.as_deref() == Some("application/octet-stream; encoding=hex"),
                )?,
                true,
            )
        }
        DomainBody::Json { value } => {
            serde_json::from_str::<serde_json::Value>(value).map_err(|e| e.to_string())?;
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
