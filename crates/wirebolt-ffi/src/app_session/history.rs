//! Typed history application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{SessionCompletion, SessionFailure, SessionHead, SessionHistoryEntry, SessionPrepared};
#[uniffi::remote(Record)]
pub struct SessionHistoryEntry {
    pub id: String,
    pub request_id: String,
    pub created_at: f64,
    pub prepared: SessionPrepared,
    pub response_head: Option<SessionHead>,
    pub completion: Option<SessionCompletion>,
    pub failure: Option<SessionFailure>,
    pub body_path: String,
}
