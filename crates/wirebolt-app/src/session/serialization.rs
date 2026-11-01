//! Native preference and interchange codecs. Interfaces never duplicate field defaults.
use super::persistence::WorkspaceError;
use crate::domain::{DomainBody, DomainEnvironment, DomainProxy, DomainRunInput};
fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, WorkspaceError> {
    serde_json::to_vec(value)
        .map_err(|_| WorkspaceError::operation("workspace snapshot could not be projected"))
}
fn decode<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, WorkspaceError> {
    serde_json::from_slice(data)
        .map_err(|_| WorkspaceError::operation("workspace snapshot could not be projected"))
}
/// # Errors
/// Returns an invalid interchange record error.
pub fn encode_input(value: &DomainRunInput) -> Result<Vec<u8>, WorkspaceError> {
    encode(value)
}
/// # Errors
/// Returns an invalid preference record error.
pub fn encode_proxy(value: &DomainProxy) -> Result<Vec<u8>, WorkspaceError> {
    let mut document = serde_json::to_value(value)
        .map_err(|_| WorkspaceError::operation("workspace snapshot could not be projected"))?;
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
    encode(&document)
}
/// # Errors
/// Returns an invalid preference record error.
pub fn decode_proxy(data: &[u8]) -> Result<DomainProxy, WorkspaceError> {
    decode(data)
}
/// # Errors
/// Returns an invalid environment record error.
pub fn encode_environment(value: &DomainEnvironment) -> Result<Vec<u8>, WorkspaceError> {
    encode(value)
}
/// # Errors
/// Returns an invalid environment record error.
pub fn decode_environment(data: &[u8]) -> Result<DomainEnvironment, WorkspaceError> {
    decode(data)
}
/// # Errors
/// Returns an invalid message body error.
pub fn encode_body(value: &DomainBody) -> Result<Vec<u8>, WorkspaceError> {
    encode(&super::conversions::body_to_core(value.clone())?)
}
/// # Errors
/// Returns an invalid message body error.
pub fn decode_body(data: &[u8]) -> Result<DomainBody, WorkspaceError> {
    decode(data).map(super::conversions::body_from_core)
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
                destination: "all".into(),
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
