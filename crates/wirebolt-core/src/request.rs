use std::{error::Error, fmt};

use http::{HeaderMap, HeaderName, HeaderValue, Method, Uri};

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

#[derive(Clone, Debug)]
pub struct PreparedRequest {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl PreparedRequest {
    #[must_use]
    pub fn method(&self) -> &Method {
        &self.method
    }

    #[must_use]
    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    #[must_use]
    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub(crate) fn into_parts(self) -> (Method, Uri, HeaderMap, Vec<u8>) {
        (self.method, self.uri, self.headers, self.body)
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
    let method = Method::from_bytes(draft.method.as_bytes())
        .map_err(|_| RequestPreparationError::InvalidMethod)?;
    let uri = draft
        .url
        .parse::<Uri>()
        .map_err(|_| RequestPreparationError::InvalidUrl)?;

    if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.authority().is_none() {
        return Err(RequestPreparationError::InvalidUrl);
    }

    let mut headers = HeaderMap::with_capacity(draft.headers.len());
    for (index, field) in draft.headers.into_iter().enumerate() {
        let name = HeaderName::from_bytes(field.name.as_bytes())
            .map_err(|_| RequestPreparationError::InvalidHeaderName { index })?;
        let value = HeaderValue::from_str(&field.value)
            .map_err(|_| RequestPreparationError::InvalidHeaderValue { index })?;
        headers.append(name, value);
    }

    Ok(PreparedRequest {
        method,
        uri,
        headers,
        body: draft.body,
    })
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
        assert_eq!(prepared.uri().scheme_str(), Some("https"));
        assert_eq!(prepared.headers().len(), 2);
        assert_eq!(prepared.body(), br#"{"name":"wirebolt"}"#);
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
    fn rejects_an_invalid_header_without_exposing_its_value() {
        let mut draft = representative_draft();
        draft.headers[1].value = "secret\nleak".to_owned();

        assert_eq!(
            prepare_request(draft).expect_err("invalid header must fail"),
            RequestPreparationError::InvalidHeaderValue { index: 1 }
        );
    }
}
