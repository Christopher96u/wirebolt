use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use url::Url;

use crate::{
    ApiKeyPlacement, Environment, HeaderField, PreparedRequest, Request, RequestAuthentication,
    RequestBody, RequestDraft, RequestPreparationError, SecretResolver, ValueSource,
    prepare_request,
};

const MAX_TEMPLATE_DEPTH: usize = 64;
const MAX_RESOLVED_VALUE_BYTES: usize = 16 * 1024 * 1024;

pub struct RequestPipeline<'a, R: SecretResolver + ?Sized> {
    environment: Option<&'a Environment>,
    secrets: &'a R,
}

impl<R: SecretResolver + ?Sized> fmt::Debug for RequestPipeline<'_, R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestPipeline")
            .field(
                "environment",
                &self.environment.map(|value| value.id.as_str()),
            )
            .field("secrets", &"[REDACTED]")
            .finish()
    }
}

impl<'a, R: SecretResolver + ?Sized> RequestPipeline<'a, R> {
    #[must_use]
    pub const fn new(environment: Option<&'a Environment>, secrets: &'a R) -> Self {
        Self {
            environment,
            secrets,
        }
    }

    /// Resolves and validates one saved or temporary request.
    ///
    /// # Errors
    ///
    /// Returns field-scoped issues without including resolved values.
    pub fn prepare(&self, request: &Request) -> Result<PreparedRequest, RequestPipelineError> {
        let mut resolver = TemplateResolver::new(self.environment, self.secrets);
        let Some(mut url) = resolver
            .resolve_template(&request.url, "url")
            .and_then(|value| Url::parse(&value).ok())
        else {
            if resolver.issues.is_empty() {
                resolver.issue("url", RequestIssueKind::InvalidUrl, None);
            }
            return Err(resolver.finish());
        };

        append_query(&request.query, &mut resolver, &mut url);
        let mut headers = resolve_headers(&request.headers, &mut resolver);
        apply_authentication(
            &request.authentication,
            &mut resolver,
            &mut url,
            &mut headers,
        );
        let body = prepare_body(&request.body, &mut resolver, &mut headers);

        if !resolver.issues.is_empty() {
            return Err(resolver.finish());
        }

        prepare_request(RequestDraft {
            method: request.method.clone(),
            url: url.into(),
            headers,
            body,
        })
        .map_err(|error| RequestPipelineError::from_preparation(&error))
    }
}

fn append_query<R: SecretResolver + ?Sized>(
    fields: &[crate::RequestValueField],
    resolver: &mut TemplateResolver<'_, R>,
    url: &mut Url,
) {
    for (index, field) in fields.iter().enumerate().filter(|(_, field)| field.enabled) {
        let path = format!("query[{index}]");
        let Some(name) = resolver.resolve_template(&field.name, &format!("{path}.name")) else {
            continue;
        };
        let Some(value) = resolver.resolve_source(&field.value, &format!("{path}.value")) else {
            continue;
        };
        url.query_pairs_mut().append_pair(&name, &value);
    }
}

fn resolve_headers<R: SecretResolver + ?Sized>(
    fields: &[crate::RequestHeader],
    resolver: &mut TemplateResolver<'_, R>,
) -> Vec<HeaderField> {
    fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.enabled)
        .filter_map(|(index, field)| {
            let path = format!("headers[{index}]");
            let name = resolver.resolve_template(&field.name, &format!("{path}.name"));
            let value = resolver.resolve_source(&field.value, &format!("{path}.value"));
            name.zip(value)
                .map(|(name, value)| HeaderField { name, value })
        })
        .collect()
}

fn apply_authentication<R: SecretResolver + ?Sized>(
    authentication: &RequestAuthentication,
    resolver: &mut TemplateResolver<'_, R>,
    url: &mut Url,
    headers: &mut Vec<HeaderField>,
) {
    let has_authorization = headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(AUTHORIZATION.as_str()));
    if !matches!(authentication, RequestAuthentication::None) && has_authorization {
        resolver.issue(
            "authentication",
            RequestIssueKind::ConflictingHeader,
            Some(AUTHORIZATION.as_str()),
        );
        return;
    }
    match authentication {
        RequestAuthentication::None => {}
        RequestAuthentication::Basic { username, password } => {
            let username = resolver.resolve_source(username, "authentication.username");
            let password = resolver.resolve_source(password, "authentication.password");
            if let (Some(username), Some(password)) = (username, password) {
                let encoded = STANDARD.encode(format!("{username}:{password}"));
                headers.push(HeaderField {
                    name: AUTHORIZATION.as_str().to_owned(),
                    value: format!("Basic {encoded}"),
                });
            }
        }
        RequestAuthentication::Bearer { token } => {
            if let Some(token) = resolver.resolve_source(token, "authentication.token") {
                headers.push(HeaderField {
                    name: AUTHORIZATION.as_str().to_owned(),
                    value: format!("Bearer {token}"),
                });
            }
        }
        RequestAuthentication::ApiKey {
            placement,
            name,
            value,
        } => {
            let name = resolver.resolve_template(name, "authentication.name");
            let value = resolver.resolve_source(value, "authentication.value");
            if let (Some(name), Some(value)) = (name, value) {
                match placement {
                    ApiKeyPlacement::Header => headers.push(HeaderField { name, value }),
                    ApiKeyPlacement::Query => {
                        url.query_pairs_mut().append_pair(&name, &value);
                    }
                }
            }
        }
    }
}

fn prepare_body<R: SecretResolver + ?Sized>(
    body: &RequestBody,
    resolver: &mut TemplateResolver<'_, R>,
    headers: &mut Vec<HeaderField>,
) -> Vec<u8> {
    match body {
        RequestBody::Empty => Vec::new(),
        RequestBody::Text {
            content_type,
            value,
        } => {
            if let Some(content_type) = content_type {
                ensure_content_type(headers, content_type);
            }
            resolver
                .resolve_template(value, "body")
                .map_or_else(Vec::new, String::into_bytes)
        }
        RequestBody::Json { value } => {
            ensure_content_type(headers, "application/json");
            let Some(value) = resolver.resolve_template(value, "body") else {
                return Vec::new();
            };
            if serde_json::from_str::<serde_json::Value>(&value).is_err() {
                resolver.issue("body", RequestIssueKind::InvalidJson, None);
                Vec::new()
            } else {
                value.into_bytes()
            }
        }
        RequestBody::FormUrlEncoded { fields } => {
            ensure_content_type(headers, "application/x-www-form-urlencoded");
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            append_form_fields(fields, resolver, &mut serializer);
            serializer.finish().into_bytes()
        }
    }
}

fn append_form_fields<R: SecretResolver + ?Sized>(
    fields: &[crate::RequestValueField],
    resolver: &mut TemplateResolver<'_, R>,
    serializer: &mut url::form_urlencoded::Serializer<'_, String>,
) {
    for (index, field) in fields.iter().enumerate().filter(|(_, field)| field.enabled) {
        let path = format!("body.fields[{index}]");
        let name = resolver.resolve_template(&field.name, &format!("{path}.name"));
        let value = resolver.resolve_source(&field.value, &format!("{path}.value"));
        if let (Some(name), Some(value)) = (name, value) {
            serializer.append_pair(&name, &value);
        }
    }
}

fn ensure_content_type(headers: &mut Vec<HeaderField>, value: &str) {
    if headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(CONTENT_TYPE.as_str()))
    {
        return;
    }
    headers.push(HeaderField {
        name: CONTENT_TYPE.as_str().to_owned(),
        value: value.to_owned(),
    });
}

struct TemplateResolver<'a, R: SecretResolver + ?Sized> {
    environment: Option<&'a Environment>,
    secrets: &'a R,
    resolving: BTreeSet<String>,
    resolved_variables: BTreeMap<String, String>,
    issues: Vec<RequestIssue>,
}

impl<'a, R: SecretResolver + ?Sized> TemplateResolver<'a, R> {
    const fn new(environment: Option<&'a Environment>, secrets: &'a R) -> Self {
        Self {
            environment,
            secrets,
            resolving: BTreeSet::new(),
            resolved_variables: BTreeMap::new(),
            issues: Vec::new(),
        }
    }

    fn resolve_source(&mut self, source: &ValueSource, path: &str) -> Option<String> {
        match source {
            ValueSource::Literal(value) => self.resolve_template(value, path),
            ValueSource::Secret { secret } => {
                if let Ok(value) = self.secrets.resolve(secret) {
                    Some(value.expose().to_owned())
                } else {
                    self.issue(path, RequestIssueKind::MissingSecret, Some(secret.as_str()));
                    None
                }
            }
        }
    }

    fn resolve_template(&mut self, input: &str, path: &str) -> Option<String> {
        let mut output = String::with_capacity(input.len());
        let mut remainder = input;
        while let Some(open) = remainder.find("{{") {
            if !self.append_resolved(&mut output, &remainder[..open], path) {
                return None;
            }
            let after_open = &remainder[open + 2..];
            let Some(close) = after_open.find("}}") else {
                self.issue(path, RequestIssueKind::InvalidTemplate, None);
                return None;
            };
            let name = after_open[..close].trim();
            if name.is_empty() {
                self.issue(path, RequestIssueKind::InvalidTemplate, None);
                return None;
            }
            let value = self.resolve_variable(name, path)?;
            if !self.append_resolved(&mut output, &value, path) {
                return None;
            }
            remainder = &after_open[close + 2..];
        }
        if remainder.contains("}}") {
            self.issue(path, RequestIssueKind::InvalidTemplate, None);
            return None;
        }
        if !self.append_resolved(&mut output, remainder, path) {
            return None;
        }
        Some(output)
    }

    fn resolve_variable(&mut self, name: &str, path: &str) -> Option<String> {
        if let Some(value) = self.resolved_variables.get(name) {
            return Some(value.clone());
        }
        let Some(source) = self
            .environment
            .and_then(|environment| environment.variables.get(name))
        else {
            self.issue(path, RequestIssueKind::MissingVariable, Some(name));
            return None;
        };
        if self.resolving.len() >= MAX_TEMPLATE_DEPTH {
            self.issue(path, RequestIssueKind::TemplateTooDeep, Some(name));
            return None;
        }
        if !self.resolving.insert(name.to_owned()) {
            self.issue(path, RequestIssueKind::CyclicVariable, Some(name));
            return None;
        }
        let result = self.resolve_source(source, path);
        self.resolving.remove(name);
        if let Some(value) = &result {
            self.resolved_variables
                .insert(name.to_owned(), value.clone());
        }
        result
    }

    fn append_resolved(&mut self, output: &mut String, value: &str, path: &str) -> bool {
        if output
            .len()
            .checked_add(value.len())
            .is_none_or(|length| length > MAX_RESOLVED_VALUE_BYTES)
        {
            self.issue(path, RequestIssueKind::ResolvedValueTooLarge, None);
            return false;
        }
        output.push_str(value);
        true
    }

    fn issue(&mut self, path: &str, kind: RequestIssueKind, reference: Option<&str>) {
        self.issues.push(RequestIssue {
            path: path.to_owned(),
            kind,
            reference: reference.map(str::to_owned),
        });
    }

    fn finish(self) -> RequestPipelineError {
        RequestPipelineError {
            issues: self.issues,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestIssueKind {
    InvalidMethod,
    InvalidUrl,
    InvalidHeaderName,
    InvalidHeaderValue,
    ConflictingHeader,
    InvalidJson,
    InvalidTemplate,
    TemplateTooDeep,
    ResolvedValueTooLarge,
    MissingVariable,
    CyclicVariable,
    MissingSecret,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestIssue {
    pub path: String,
    pub kind: RequestIssueKind,
    pub reference: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestPipelineError {
    pub issues: Vec<RequestIssue>,
}

impl RequestPipelineError {
    fn from_preparation(error: &RequestPreparationError) -> Self {
        let (path, kind) = match error {
            RequestPreparationError::InvalidMethod => ("method", RequestIssueKind::InvalidMethod),
            RequestPreparationError::InvalidUrl => ("url", RequestIssueKind::InvalidUrl),
            RequestPreparationError::InvalidHeaderName { index } => {
                return Self {
                    issues: vec![RequestIssue {
                        path: format!("headers[{index}].name"),
                        kind: RequestIssueKind::InvalidHeaderName,
                        reference: None,
                    }],
                };
            }
            RequestPreparationError::InvalidHeaderValue { index } => {
                return Self {
                    issues: vec![RequestIssue {
                        path: format!("headers[{index}].value"),
                        kind: RequestIssueKind::InvalidHeaderValue,
                        reference: None,
                    }],
                };
            }
        };
        Self {
            issues: vec![RequestIssue {
                path: path.to_owned(),
                kind,
                reference: None,
            }],
        }
    }
}

impl fmt::Display for RequestPipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "request has {} issue(s)", self.issues.len())
    }
}

impl Error for RequestPipelineError {}
