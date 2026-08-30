use std::{error::Error, fmt};

use http::{HeaderMap, HeaderName, HeaderValue, Method};
use url::Url;

use crate::RequestIssue;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestDraft {
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderField>,
    pub body: Vec<u8>,
}

/// A validated request that the HTTP engine can send without re-parsing.
///
/// The URL is kept as a [`Url`] because that is the representation the
/// transport consumes directly; converting through strings or `http::Uri`
/// would parse the same text again on every run.
#[derive(Clone, Debug)]
pub struct PreparedRequest {
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Vec<u8>,
    warnings: Vec<RequestIssue>,
}

impl PreparedRequest {
    pub(crate) fn from_parts(
        method: Method,
        url: Url,
        headers: HeaderMap,
        body: Vec<u8>,
        warnings: Vec<RequestIssue>,
    ) -> Self {
        Self {
            method,
            url,
            headers,
            body,
            warnings,
        }
    }

    #[must_use]
    pub const fn method(&self) -> &Method {
        &self.method
    }

    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.url
    }

    #[must_use]
    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Non-fatal findings recorded while preparing the request, such as a
    /// JSON body that does not parse but is sent verbatim anyway.
    #[must_use]
    pub fn warnings(&self) -> &[RequestIssue] {
        &self.warnings
    }

    pub(crate) fn into_parts(self) -> (Method, Url, HeaderMap, Vec<u8>) {
        (self.method, self.url, self.headers, self.body)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestPreparationError {
    InvalidMethod,
    InvalidUrl,
    InvalidHeaderName { index: usize },
    InvalidHeaderValue { index: usize },
}

impl fmt::Display for RequestPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMethod => formatter.write_str("invalid HTTP method"),
            Self::InvalidUrl => formatter.write_str("invalid absolute HTTP URL"),
            Self::InvalidHeaderName { index } => {
                write!(formatter, "invalid header name at index {index}")
            }
            Self::InvalidHeaderValue { index } => {
                write!(formatter, "invalid header value at index {index}")
            }
        }
    }
}

impl Error for RequestPreparationError {}

/// Validates and normalizes a request before execution.
///
/// # Errors
///
/// Returns [`RequestPreparationError`] when the method, absolute HTTP URL, or
/// any header cannot be represented safely by the HTTP engine.
pub fn prepare_request(draft: RequestDraft) -> Result<PreparedRequest, RequestPreparationError> {
    let method = parse_method(&draft.method)?;
    let url = parse_http_url(&draft.url).ok_or(RequestPreparationError::InvalidUrl)?;

    let mut headers = HeaderMap::with_capacity(draft.headers.len());
    for (index, field) in draft.headers.into_iter().enumerate() {
        let name = HeaderName::from_bytes(field.name.as_bytes())
            .map_err(|_| RequestPreparationError::InvalidHeaderName { index })?;
        let value = HeaderValue::from_str(&field.value)
            .map_err(|_| RequestPreparationError::InvalidHeaderValue { index })?;
        headers.append(name, value);
    }

    Ok(PreparedRequest::from_parts(
        method,
        url,
        headers,
        draft.body,
        Vec::new(),
    ))
}

/// Parses a method token. ASCII letters are upper-cased first so that a
/// hand-typed `get` is sent as `GET`, which is what every server expects.
pub(crate) fn parse_method(method: &str) -> Result<Method, RequestPreparationError> {
    let method = method.trim();
    if method.bytes().any(|byte| byte.is_ascii_lowercase()) {
        Method::from_bytes(method.to_ascii_uppercase().as_bytes())
    } else {
        Method::from_bytes(method.as_bytes())
    }
    .map_err(|_| RequestPreparationError::InvalidMethod)
}

/// Parses an absolute `http` or `https` URL with a host.
pub(crate) fn parse_http_url(value: &str) -> Option<Url> {
    let url = Url::parse(value.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn representative_draft() -> RequestDraft {
        RequestDraft {
            method: "POST".to_owned(),
            url: "https://api.example.com/v1/items?limit=50".to_owned(),
            headers: vec![
                HeaderField {
                    name: "accept".to_owned(),
                    value: "application/json".to_owned(),
                },
                HeaderField {
                    name: "x-wirebolt-environment".to_owned(),
                    value: "local".to_owned(),
                },
            ],
            body: br#"{"name":"wirebolt"}"#.to_vec(),
        }
    }

    #[test]
    fn prepares_an_absolute_http_request() {
        let prepared = prepare_request(representative_draft()).expect("valid request");

        assert_eq!(prepared.method(), Method::POST);
        assert_eq!(prepared.url().scheme(), "https");
        assert_eq!(prepared.headers().len(), 2);
        assert_eq!(prepared.body(), br#"{"name":"wirebolt"}"#);
        assert!(prepared.warnings().is_empty());
    }

    #[test]
    fn upper_cases_hand_typed_methods() {
        let mut draft = representative_draft();
        draft.method = " get ".to_owned();

        let prepared = prepare_request(draft).expect("valid request");

        assert_eq!(prepared.method(), Method::GET);
    }

    #[test]
    fn rejects_a_relative_url() {
        let mut draft = representative_draft();
        draft.url = "/v1/items".to_owned();

        assert_eq!(
            prepare_request(draft).expect_err("relative URLs must fail"),
            RequestPreparationError::InvalidUrl
        );
    }

    #[test]
    fn rejects_non_http_schemes_and_missing_hosts() {
        for url in ["ftp://example.com/file", "http://", "mailto:x@y"] {
            let mut draft = representative_draft();
            draft.url = url.to_owned();
            assert_eq!(
                prepare_request(draft).expect_err("must fail"),
                RequestPreparationError::InvalidUrl,
                "{url}"
            );
        }
    }

    #[test]
    fn rejects_an_invalid_header_without_exposing_its_value() {
        let mut draft = representative_draft();
        draft.headers[1].value = "secret\nleak".to_owned();

        assert_eq!(
            prepare_request(draft).expect_err("invalid header must fail"),
            RequestPreparationError::InvalidHeaderValue { index: 1 }
        );
    }
}
