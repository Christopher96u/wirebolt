//! Typed history application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{
    SessionCompletion, SessionFailure, SessionHead, SessionHistoryEntry, SessionPrepared,
    SessionRequestIdentity,
};
#[uniffi::remote(Record)]
pub struct SessionHistoryEntry {
    pub id: String,
    pub request_id: String,
    pub identity: Option<SessionRequestIdentity>,
    pub created_at: f64,
    pub prepared: SessionPrepared,
    pub response_head: Option<SessionHead>,
    pub completion: Option<SessionCompletion>,
    pub failure: Option<SessionFailure>,
    pub body_path: String,
}

#[uniffi::remote(Record)]
pub struct SessionRequestIdentity {
    pub collection_id: String,
    pub request_id: String,
}

#[uniffi::export]
pub fn session_request_activity_key(collection_id: String, request_id: String) -> String {
    SessionRequestIdentity {
        collection_id,
        request_id,
    }
    .activity_key()
}
