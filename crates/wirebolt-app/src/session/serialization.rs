//! Native preference and interchange codecs. Interfaces never duplicate field defaults.
use super::persistence::WorkspaceError;
use crate::domain::{DomainBody, DomainEnvironment, DomainProxy, DomainRunInput};
pub(crate) fn encode(
    value: &impl serde::Serialize,
    record: &str,
) -> Result<Vec<u8>, WorkspaceError> {
    serde_json::to_vec(value)
        .map_err(|_| WorkspaceError::operation(&format!("{record} could not be encoded")))
}
pub(crate) fn decode<T: serde::de::DeserializeOwned>(
    data: &[u8],
    record: &str,
) -> Result<T, WorkspaceError> {
    serde_json::from_slice(data)
        .map_err(|_| WorkspaceError::operation(&format!("{record} could not be decoded")))
}
/// # Errors
/// Returns an invalid interchange record error.
pub fn encode_input(value: &DomainRunInput) -> Result<Vec<u8>, WorkspaceError> {
    encode(value, "run input")
}
/// # Errors
/// Returns an invalid preference record error.
pub fn encode_proxy(value: &DomainProxy) -> Result<Vec<u8>, WorkspaceError> {
    let mut document = serde_json::to_value(value)
        .map_err(|_| WorkspaceError::operation("proxy preferences could not be encoded"))?;
    if let Some(routes) = document
        .get_mut("routes")
        .and_then(serde_json::Value::as_array_mut)
    {
        for route in routes {
            if route
                .get("credentials")
                .is_some_and(serde_json::Value::is_null)
                && let Some(route) = route.as_object_mut()
            {
                route.remove("credentials");
            }
        }
    }
    encode(&document, "proxy preferences")
}
/// # Errors
/// Returns an invalid preference record error.
pub fn decode_proxy(data: &[u8]) -> Result<DomainProxy, WorkspaceError> {
    decode(data, "proxy preferences")
}
/// # Errors
/// Returns an invalid environment record error.
pub fn encode_environment(value: &DomainEnvironment) -> Result<Vec<u8>, WorkspaceError> {
    encode(value, "environment")
}
/// # Errors
/// Returns an invalid environment record error.
pub fn decode_environment(data: &[u8]) -> Result<DomainEnvironment, WorkspaceError> {
    decode(data, "environment")
}
/// # Errors
/// Returns an invalid message body error.
pub fn encode_body(value: &DomainBody) -> Result<Vec<u8>, WorkspaceError> {
    encode(
        &super::conversions::body_to_core(value.clone())?,
        "message body",
    )
}
/// # Errors
/// Returns an invalid message body error.
pub fn decode_body(data: &[u8]) -> Result<DomainBody, WorkspaceError> {
    decode(data, "message body").map(super::conversions::body_from_core)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn body_codec_uses_storage_defaults_and_omissions() {
        let body = decode_body(br#"{"kind":"form_url_encoded","fields":[{"name":"q","value":"hello","enabled":true}]}"#).unwrap();
        let DomainBody::FormUrlEncoded { fields } = &body else {
            panic!("form")
        };
        assert!(!fields[0].id.is_empty());
        assert!(!fields[0].sensitive);
        let encoded = encode_body(&body).unwrap();
        let json = String::from_utf8(encoded.clone()).unwrap();
        assert!(!json.contains("sensitive"));
        assert_eq!(decode_body(&encoded).unwrap(), body);
        assert!(decode_body(b"{}").is_err());
    }
    #[test]
    fn proxy_preferences_preserve_existing_strings_and_optional_credentials() {
        let proxy = DomainProxy::Manual {
            routes: vec![crate::domain::DomainProxyRoute {
                destination: wirebolt_core::ProxyDestination::All,
                endpoint: "http://localhost:8080".into(),
                credentials: None,
            }],
        };
        let encoded = encode_proxy(&proxy).unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("credentials"));
        assert_eq!(decode_proxy(&encoded).unwrap(), proxy);
        assert!(decode_proxy(b"{}").is_err());
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn decode_errors_identify_the_record_without_exposing_input() {
        for (error, record) in [
            (decode_proxy(b"private").unwrap_err(), "proxy preferences"),
            (decode_environment(b"private").unwrap_err(), "environment"),
            (decode_body(b"private").unwrap_err(), "message body"),
            (
                crate::session::tabs::decode_layout(b"private").unwrap_err(),
                "session layout",
            ),
        ] {
            assert_eq!(error.to_string(), format!("{record} could not be decoded"));
        }
    }
    #[test]
    fn encode_errors_identify_the_record() {
        struct Invalid;
        impl serde::Serialize for Invalid {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("private"))
            }
        }
        for record in [
            "proxy preferences",
            "environment",
            "message body",
            "session layout",
        ] {
            assert_eq!(
                encode(&Invalid, record).unwrap_err().to_string(),
                format!("{record} could not be encoded")
            );
        }
    }
}
