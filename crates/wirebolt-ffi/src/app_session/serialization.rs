//! Typed serialization application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{
    DomainBody, DomainEnvironment, DomainProxy, DomainRunInput, WorkspaceGitOperation,
    WorkspaceGitStatus,
};
#[uniffi::export]
pub fn session_decode_git_status(
    data: Vec<u8>,
) -> Result<WorkspaceGitStatus, crate::WorkspaceBridgeError> {
    serde_json::from_slice(&data)
        .map_err(|_| crate::WorkspaceBridgeError::operation("Git returned an invalid response."))
}

#[uniffi::export]
pub fn session_decode_git_operation(
    data: Vec<u8>,
) -> Result<WorkspaceGitOperation, crate::WorkspaceBridgeError> {
    serde_json::from_slice(&data)
        .map_err(|_| crate::WorkspaceBridgeError::operation("Git returned an invalid response."))
}

#[uniffi::export]
pub fn session_encode_input(value: DomainRunInput) -> Result<Vec<u8>, crate::WorkspaceBridgeError> {
    wirebolt_app::session::serialization::encode_input(&value)
}
#[uniffi::export]
pub fn session_encode_proxy(value: DomainProxy) -> Result<Vec<u8>, crate::WorkspaceBridgeError> {
    wirebolt_app::session::serialization::encode_proxy(&value)
}
#[uniffi::export]
pub fn session_decode_proxy(data: Vec<u8>) -> Result<DomainProxy, crate::WorkspaceBridgeError> {
    wirebolt_app::session::serialization::decode_proxy(&data)
}
#[uniffi::export]
pub fn session_encode_environment(
    value: DomainEnvironment,
) -> Result<Vec<u8>, crate::WorkspaceBridgeError> {
    wirebolt_app::session::serialization::encode_environment(&value)
}
#[uniffi::export]
pub fn session_decode_environment(
    data: Vec<u8>,
) -> Result<DomainEnvironment, crate::WorkspaceBridgeError> {
    wirebolt_app::session::serialization::decode_environment(&data)
}
#[uniffi::export]
pub fn session_encode_body(value: DomainBody) -> Result<Vec<u8>, crate::WorkspaceBridgeError> {
    wirebolt_app::session::serialization::encode_body(&value)
}
#[uniffi::export]
pub fn session_decode_body(data: Vec<u8>) -> Result<DomainBody, crate::WorkspaceBridgeError> {
    wirebolt_app::session::serialization::decode_body(&data)
}
