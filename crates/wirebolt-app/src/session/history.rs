//! Typed history orchestration over the response repository. The storage format
//! keeps the existing Foundation reference-date timestamps and snapshot keys.
use super::run::{SessionCompletion, SessionFailure, SessionHead, SessionPrepared};
use crate::response::{
    BodyStore, HistoryEntry, ResponseError, ResponseResult, RunHistory, VIEWPORT_BYTES,
};
use serde::{Deserialize, Serialize};
use std::io::Read;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryEntry {
    pub id: String,
    #[serde(rename = "requestID")]
    pub request_id: String,
    pub created_at: f64,
    pub prepared: SessionPrepared,
    pub response_head: Option<SessionHead>,
    pub completion: Option<SessionCompletion>,
    pub failure: Option<SessionFailure>,
    pub body_path: String,
}
/// # Errors
/// Returns a history storage error.
pub fn record(
    repository: &mut RunHistory,
    entry: SessionHistoryEntry,
    body: &BodyStore,
) -> ResponseResult<()> {
    let entry: HistoryEntry = serde_json::to_value(entry)
        .and_then(serde_json::from_value)
        .map_err(|e| ResponseError::Io {
            reason: e.to_string(),
        })?;
    repository.record(entry, body)
}
#[must_use]
pub fn list(repository: &mut RunHistory, request_id: &str) -> Vec<SessionHistoryEntry> {
    repository
        .list(request_id)
        .into_iter()
        .map(|entry| serde_json::to_value(entry).and_then(serde_json::from_value))
        .collect::<Result<_, _>>()
        .unwrap_or_default()
}
#[must_use]
pub fn viewport(path: &str) -> Vec<u8> {
    let mut data = vec![];
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(VIEWPORT_BYTES as u64).read_to_end(&mut data);
    }
    data
}
