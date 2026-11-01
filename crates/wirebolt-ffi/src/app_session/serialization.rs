//! Typed serialization application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::DomainProxy;
#[uniffi::export]
pub fn session_encode_proxy(value: DomainProxy) -> Result<Vec<u8>, crate::WorkspaceBridgeError> {
    wirebolt_app::session::serialization::encode_proxy(&value)
}
#[uniffi::export]
pub fn session_decode_proxy(data: Vec<u8>) -> Result<DomainProxy, crate::WorkspaceBridgeError> {
    wirebolt_app::session::serialization::decode_proxy(&data)
}
