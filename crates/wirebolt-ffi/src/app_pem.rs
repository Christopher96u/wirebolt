//! PEM selection bridge; filesystem limits and validation are application policy.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use crate::app_domain::DomainError;
use std::path::Path;
use wirebolt_app::domain::pem_file;
#[uniffi::export]
pub fn app_read_pem_files(paths: Vec<String>) -> Result<Vec<String>, DomainError> {
    paths
        .iter()
        .map(|path| pem_file::read(Path::new(path)))
        .collect::<Result<_, _>>()
        .map_err(|error| DomainError::Invalid {
            reason: error.to_string(),
        })
}
#[uniffi::export]
pub fn app_validate_ca_pem_file(path: String) -> Result<(), DomainError> {
    pem_file::validate_ca(Path::new(&path)).map_err(|error| DomainError::Invalid {
        reason: error.to_string(),
    })
}
