//! Typed history orchestration over the response repository. The storage format
//! keeps the existing Foundation reference-date timestamps and snapshot keys.
use super::run::{SessionCompletion, SessionFailure, SessionHead, SessionPrepared};
use crate::response::VIEWPORT_BYTES;
use serde::{Deserialize, Serialize};
use std::io::Read;

/// Collection and request IDs survive workspace relocation. Copies share history.
/// Unlocated requests use an empty collection ID, stable across launches.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionRequestIdentity {
    pub collection_id: String,
    pub request_id: String,
}
impl SessionRequestIdentity {
    #[must_use]
    pub fn new(collection_id: &str, request_id: &str) -> Self {
        Self {
            collection_id: collection_id.into(),
            request_id: request_id.into(),
        }
    }
    #[must_use]
    pub fn activity_key(&self) -> String {
        format!("{}/{}", self.collection_id, self.request_id)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryEntry {
    pub id: String,
    #[serde(rename = "requestID")]
    pub request_id: String,
    /// Absent only for legacy entries whose collection is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<SessionRequestIdentity>,
    pub created_at: f64,
    pub prepared: SessionPrepared,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_head: Option<SessionHead>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<SessionCompletion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<SessionFailure>,
    pub body_path: String,
}
#[must_use]
pub fn viewport(path: &str) -> Vec<u8> {
    let mut data = vec![];
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(VIEWPORT_BYTES as u64).read_to_end(&mut data);
    }
    data
}
