uniffi::setup_scaffolding!();

use std::{error::Error, fmt};

#[derive(Debug, Eq, PartialEq, uniffi::Record)]
pub struct CoreHandshake {
    pub product: String,
    pub core_version: String,
    pub stream_abi_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct HeaderField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct RequestDraft {
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderField>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct PreparedRequestSummary {
    pub method: String,
    pub url: String,
    pub header_count: u64,
    pub body_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Error)]
pub enum RequestPreparationError {
    InvalidRequest { reason: String },
}

impl fmt::Display for RequestPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { reason } => formatter.write_str(reason),
        }
    }
}

impl Error for RequestPreparationError {}

#[uniffi::export]
#[must_use]
pub fn core_handshake() -> CoreHandshake {
    wirebolt_core::handshake().into()
}

#[uniffi::export]
/// Prepares one coarse-grained request across the Swift–Rust boundary.
///
/// # Errors
///
/// Returns [`RequestPreparationError`] when the Rust core rejects any request
/// component.
pub fn prepare_request(
    draft: RequestDraft,
) -> Result<PreparedRequestSummary, RequestPreparationError> {
    let prepared = wirebolt_core::prepare_request(draft.into()).map_err(|error| {
        RequestPreparationError::InvalidRequest {
            reason: error.to_string(),
        }
    })?;

    Ok(PreparedRequestSummary {
        method: prepared.method().to_string(),
        url: prepared.uri().to_string(),
        header_count: prepared.headers().len().try_into().unwrap_or(u64::MAX),
        body_bytes: prepared.body().len().try_into().unwrap_or(u64::MAX),
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn wirebolt_stream_abi_version() -> u32 {
    wirebolt_core::STREAM_ABI_VERSION
}

impl From<wirebolt_core::CoreHandshake> for CoreHandshake {
    fn from(value: wirebolt_core::CoreHandshake) -> Self {
        Self {
            product: value.product.to_owned(),
            core_version: value.core_version.to_owned(),
            stream_abi_version: value.stream_abi_version,
        }
    }
}

impl From<RequestDraft> for wirebolt_core::RequestDraft {
    fn from(value: RequestDraft) -> Self {
        Self {
            method: value.method,
            url: value.url,
            headers: value.headers.into_iter().map(Into::into).collect(),
            body: value.body,
        }
    }
}

impl From<HeaderField> for wirebolt_core::HeaderField {
    fn from(value: HeaderField) -> Self {
        Self {
            name: value.name,
            value: value.value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_bridge_surfaces_share_the_same_version() {
        let handshake = core_handshake();

        assert_eq!(handshake.product, "Wirebolt");
        assert_eq!(handshake.stream_abi_version, wirebolt_stream_abi_version());
    }

    #[test]
    fn prepares_a_request_through_the_coarse_bridge() {
        let summary = prepare_request(RequestDraft {
            method: "POST".to_owned(),
            url: "https://api.example.com/v1/items".to_owned(),
            headers: vec![HeaderField {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
            }],
            body: br#"{"fast":true}"#.to_vec(),
        })
        .expect("valid request");

        assert_eq!(summary.method, "POST");
        assert_eq!(summary.url, "https://api.example.com/v1/items");
        assert_eq!(summary.header_count, 1);
        assert_eq!(summary.body_bytes, 13);
    }
}
