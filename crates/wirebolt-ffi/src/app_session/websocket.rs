//! Typed websocket application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{
    Arc, DomainBody, Mutex, ResponseHeaderValue, SessionSocket, SessionSocketMessage,
    SessionSocketNotice, SessionSocketSnapshot, SessionSocketStatus, decode_binary, message_body,
};
use crate::lock;
#[uniffi::remote(Enum)]
pub enum SessionSocketStatus {
    Disconnected,
    Connecting,
    Connected,
}

#[uniffi::remote(Enum)]
pub enum SessionSocketNotice {
    Connected,
    Disconnected,
    Failed,
}

#[uniffi::remote(Record)]
pub struct SessionSocketMessage {
    id: String,
    timestamp: f64,
    data: Vec<u8>,
    binary: bool,
    outgoing: bool,
    notice: Option<SessionSocketNotice>,
    control: Option<String>,
    text: String,
}

#[uniffi::remote(Record)]
pub struct SessionSocketSnapshot {
    first_sequence: u64,
    next_sequence: u64,
    generation: String,
    status: SessionSocketStatus,
    messages: Vec<SessionSocketMessage>,
    error: Option<String>,
    headers: Vec<ResponseHeaderValue>,
}

#[derive(Debug, uniffi::Object)]
pub struct AppSessionSocket {
    pub(crate) inner: Arc<Mutex<SessionSocket>>,
}

#[uniffi::export]
impl AppSessionSocket {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Mutex::new(SessionSocket::default())),
        })
    }
    pub fn snapshot(&self, since: u64) -> SessionSocketSnapshot {
        lock(&self.inner).snapshot_since(since)
    }
    pub fn begin(&self) -> Option<String> {
        lock(&self.inner).begin()
    }
    pub fn connected(&self, generation: String, headers: Vec<ResponseHeaderValue>, url: String) {
        lock(&self.inner).connected(&generation, headers, &url);
    }
    pub fn append(
        &self,
        generation: String,
        data: Vec<u8>,
        binary: bool,
        outgoing: bool,
        control: Option<String>,
    ) {
        lock(&self.inner).append(&generation, data, binary, outgoing, control);
    }
    pub fn closed(&self, generation: String) {
        lock(&self.inner).closed(&generation);
    }
    pub fn finish(&self, generation: String, error: Option<String>) {
        lock(&self.inner).finish(&generation, error);
    }
    pub fn disconnect(&self) {
        lock(&self.inner).disconnect();
    }
    pub fn clear(&self) {
        lock(&self.inner).clear();
    }
    pub fn set_error(&self, error: Option<String>) {
        lock(&self.inner).set_error(error);
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SessionMessageBody {
    pub data: Vec<u8>,
    pub binary: bool,
}

#[uniffi::export]
pub fn session_decode_binary(
    text: String,
    hex: bool,
) -> Result<Vec<u8>, crate::WorkspaceBridgeError> {
    decode_binary(&text, hex)
        .map_err(|reason| crate::WorkspaceBridgeError::OperationFailed { reason })
}

#[uniffi::export]
pub fn session_message_body(
    body: DomainBody,
) -> Result<SessionMessageBody, crate::WorkspaceBridgeError> {
    message_body(&body)
        .map(|(data, binary)| SessionMessageBody { data, binary })
        .map_err(|reason| crate::WorkspaceBridgeError::OperationFailed { reason })
}
