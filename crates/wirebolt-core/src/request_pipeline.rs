use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{
    HeaderMap, HeaderName, HeaderValue,
    header::{AUTHORIZATION, CONTENT_TYPE},
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use url::Url;

use crate::{
    ApiKeyPlacement, Environment, PreparedRequest, Request, RequestAuthentication, RequestBody,
    RequestHeader, RequestValueField, SecretResolver, ValueSource,
    request::{parse_http_url, parse_method},
};

const MAX_TEMPLATE_DEPTH: usize = 64;
const MAX_RESOLVED_VALUE_BYTES: usize = 16 * 1024 * 1024;
/// Everything outside RFC 3986 unreserved characters is percent-encoded, so a
/// space becomes `%20` rather than `+`; servers that do not form-decode query
/// strings would otherwise read a literal plus sign.
const QUERY_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

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
    /// Every issue is reported against the field that produced it, using the
    /// indexes of the saved request rather than the position of the resolved
    /// header. Non-fatal findings are kept as warnings on the prepared request.
    ///
    /// # Errors
    ///
    /// Returns field-scoped issues without including resolved values.
    pub fn prepare(&self, request: &Request) -> Result<PreparedRequest, RequestPipelineError> {
        let mut resolver = TemplateResolver::new(self.environment, self.secrets);
        let method = parse_method(&request.method).ok();
        if method.is_none() {
            resolver.issue(
                FieldPath::Fixed("method"),
                RequestIssueKind::InvalidMethod,
                None,
            );
        }
        let Some(mut url) = resolver
            .resolve_template(&request.url, FieldPath::Fixed("url"))
            .and_then(|value| parse_http_url(&value))
        else {
            if resolver.issues.is_empty() {
                resolver.issue(FieldPath::Fixed("url"), RequestIssueKind::InvalidUrl, None);
            }
            return Err(resolver.finish());
        };

        let mut query = QueryBuilder::new(&url);
        append_query(&request.query, &mut resolver, &mut query);
        let mut headers = resolve_headers(&request.headers, &mut resolver);
        apply_authentication(
            &request.authentication,
            &mut resolver,
            &mut query,
            &mut headers,
        );
        query.apply(&mut url);
        let body = prepare_body(&request.body, &mut resolver, &mut headers);

        match (method, resolver.issues.is_empty()) {
            (Some(method), true) => Ok(PreparedRequest::from_parts(
                method,
                url,
                headers,
                body,
                resolver.warnings,
            )),
            _ => Err(resolver.finish()),
        }
    }
}

/// Accumulates query pairs and rewrites the URL query once, instead of
/// re-serializing the whole query for every appended pair.
struct QueryBuilder {
    query: String,
    changed: bool,
}

impl QueryBuilder {
    fn new(url: &Url) -> Self {
        Self {
            query: url.query().map_or_else(String::new, str::to_owned),
            changed: false,
        }
    }

    fn append(&mut self, name: &str, value: &str) {
        if !self.query.is_empty() {
            self.query.push('&');
        }
        self.query
            .extend(utf8_percent_encode(name, QUERY_COMPONENT));
        self.query.push('=');
        self.query
            .extend(utf8_percent_encode(value, QUERY_COMPONENT));
        self.changed = true;
    }

    fn apply(self, url: &mut Url) {
        if self.changed {
            url.set_query(Some(&self.query));
        }
    }
}

fn append_query<R: SecretResolver + ?Sized>(
    fields: &[RequestValueField],
    resolver: &mut TemplateResolver<'_, R>,
    query: &mut QueryBuilder,
) {
    for (index, field) in fields.iter().enumerate().filter(|(_, field)| field.enabled) {
        let name =
            resolver.resolve_template(&field.name, FieldPath::indexed("query", index, "name"));
        let value =
            resolver.resolve_source(&field.value, FieldPath::indexed("query", index, "value"));
        if let (Some(name), Some(value)) = (name, value) {
            query.append(&name, &value);
        }
    }
}

fn resolve_headers<R: SecretResolver + ?Sized>(
    fields: &[RequestHeader],
    resolver: &mut TemplateResolver<'_, R>,
) -> HeaderMap {
    let mut headers = HeaderMap::with_capacity(fields.len() + 2);
    for (index, field) in fields.iter().enumerate().filter(|(_, field)| field.enabled) {
        let name_path = FieldPath::indexed("headers", index, "name");
        let value_path = FieldPath::indexed("headers", index, "value");
        let name = resolver.resolve_template(&field.name, name_path);
        let value = resolver.resolve_source(&field.value, value_path);
        let (Some(name), Some(value)) = (name, value) else {
            continue;
        };
        let name = resolver.header_name(&name, name_path);
        let value = resolver.header_value(&value, value_path);
        if let (Some(name), Some(mut value)) = (name, value) {
            value.set_sensitive(matches!(field.value, ValueSource::Secret { .. }));
            headers.append(name, value);
        }
    }
    headers
}

fn apply_authentication<R: SecretResolver + ?Sized>(
    authentication: &RequestAuthentication,
    resolver: &mut TemplateResolver<'_, R>,
    query: &mut QueryBuilder,
    headers: &mut HeaderMap,
) {
    if !matches!(authentication, RequestAuthentication::None) && headers.contains_key(AUTHORIZATION)
    {
        resolver.issue(
            FieldPath::Fixed("authentication"),
            RequestIssueKind::ConflictingHeader,
            Some(AUTHORIZATION.as_str()),
        );
        return;
    }
    match authentication {
        RequestAuthentication::None => {}
        RequestAuthentication::Basic { username, password } => {
            let username =
                resolver.resolve_source(username, FieldPath::Fixed("authentication.username"));
            let password =
                resolver.resolve_source(password, FieldPath::Fixed("authentication.password"));
            if let (Some(username), Some(password)) = (username, password) {
                let encoded = STANDARD.encode(format!("{username}:{password}"));
                if let Some(mut value) = resolver.header_value(
                    &format!("Basic {encoded}"),
                    FieldPath::Fixed("authentication"),
                ) {
                    value.set_sensitive(true);
                    headers.insert(AUTHORIZATION, value);
                }
            }
        }
        RequestAuthentication::Bearer { token } => {
            let path = FieldPath::Fixed("authentication.token");
            if let Some(token) = resolver.resolve_source(token, path)
                && let Some(mut value) = resolver.header_value(&format!("Bearer {token}"), path)
            {
                value.set_sensitive(true);
                headers.insert(AUTHORIZATION, value);
            }
        }
        RequestAuthentication::ApiKey {
            placement,
            name,
            value,
        } => apply_api_key(*placement, name, value, resolver, query, headers),
    }
}

fn apply_api_key<R: SecretResolver + ?Sized>(
    placement: ApiKeyPlacement,
    name: &str,
    value: &ValueSource,
    resolver: &mut TemplateResolver<'_, R>,
    query: &mut QueryBuilder,
    headers: &mut HeaderMap,
) {
    let name_path = FieldPath::Fixed("authentication.name");
    let value_path = FieldPath::Fixed("authentication.value");
    let name = resolver.resolve_template(name, name_path);
    let value = resolver.resolve_source(value, value_path);
    let (Some(name), Some(value)) = (name, value) else {
        return;
    };
    match placement {
        ApiKeyPlacement::Header => {
            let name = resolver.header_name(&name, name_path);
            let value = resolver.header_value(&value, value_path);
            if let (Some(name), Some(mut value)) = (name, value) {
                value.set_sensitive(true);
                headers.append(name, value);
            }
        }
        ApiKeyPlacement::Query => query.append(&name, &value),
    }
}

fn prepare_body<R: SecretResolver + ?Sized>(
    body: &RequestBody,
    resolver: &mut TemplateResolver<'_, R>,
    headers: &mut HeaderMap,
) -> Vec<u8> {
    match body {
        RequestBody::Empty => Vec::new(),
        RequestBody::Text {
            content_type,
            value,
        } => {
            if let Some(content_type) = content_type
                && !headers.contains_key(CONTENT_TYPE)
                && let Some(value) =
                    resolver.header_value(content_type, FieldPath::Fixed("body.content_type"))
            {
                headers.insert(CONTENT_TYPE, value);
            }
            resolver
                .resolve_template(value, FieldPath::Fixed("body"))
                .map_or_else(Vec::new, |value| value.into_owned().into_bytes())
        }
        RequestBody::Json { value } => {
            ensure_content_type(headers, "application/json");
            let Some(value) = resolver.resolve_template(value, FieldPath::Fixed("body")) else {
                return Vec::new();
            };
            // Validation only walks the document; nothing is built from it.
            if serde_json::from_str::<serde::de::IgnoredAny>(&value).is_err() {
                resolver.warning(
                    FieldPath::Fixed("body"),
                    RequestIssueKind::InvalidJson,
                    None,
                );
            }
            value.into_owned().into_bytes()
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
    fields: &[RequestValueField],
    resolver: &mut TemplateResolver<'_, R>,
    serializer: &mut url::form_urlencoded::Serializer<'_, String>,
) {
    for (index, field) in fields.iter().enumerate().filter(|(_, field)| field.enabled) {
        let name = resolver.resolve_template(
            &field.name,
            FieldPath::indexed("body.fields", index, "name"),
        );
        let value = resolver.resolve_source(
            &field.value,
            FieldPath::indexed("body.fields", index, "value"),
        );
        if let (Some(name), Some(value)) = (name, value) {
            serializer.append_pair(&name, &value);
        }
    }
}

fn ensure_content_type(headers: &mut HeaderMap, value: &'static str) {
    headers
        .entry(CONTENT_TYPE)
        .or_insert_with(|| HeaderValue::from_static(value));
}

/// Where an issue belongs. Formatting the `headers[3].value` style path is
/// deferred until an issue is actually recorded.
#[derive(Clone, Copy)]
enum FieldPath {
    Fixed(&'static str),
    Indexed {
        list: &'static str,
        index: usize,
        leaf: &'static str,
    },
}

impl FieldPath {
    const fn indexed(list: &'static str, index: usize, leaf: &'static str) -> Self {
        Self::Indexed { list, index, leaf }
    }
}

impl fmt::Display for FieldPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(path) => formatter.write_str(path),
            Self::Indexed { list, index, leaf } => write!(formatter, "{list}[{index}].{leaf}"),
        }
    }
}

struct TemplateResolver<'a, R: SecretResolver + ?Sized> {
    environment: Option<&'a Environment>,
    secrets: &'a R,
    resolving: BTreeSet<String>,
    resolved_variables: BTreeMap<String, String>,
    issues: Vec<RequestIssue>,
    warnings: Vec<RequestIssue>,
}

impl<'a, R: SecretResolver + ?Sized> TemplateResolver<'a, R> {
    const fn new(environment: Option<&'a Environment>, secrets: &'a R) -> Self {
        Self {
            environment,
            secrets,
            resolving: BTreeSet::new(),
            resolved_variables: BTreeMap::new(),
            issues: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn resolve_source<'v>(
        &mut self,
        source: &'v ValueSource,
        path: FieldPath,
    ) -> Option<Cow<'v, str>> {
        match source {
            ValueSource::Literal(value) => self.resolve_template(value, path),
            ValueSource::Secret { secret } => {
                if let Ok(value) = self.secrets.resolve(secret) {
                    Some(Cow::Owned(value.expose().to_owned()))
                } else {
                    self.issue(path, RequestIssueKind::MissingSecret, Some(secret.as_str()));
                    None
                }
            }
        }
    }

    /// Expands `{{name}}` references. Inputs without a template are returned
    /// borrowed, so the common case costs no allocation.
    fn resolve_template<'v>(&mut self, input: &'v str, path: FieldPath) -> Option<Cow<'v, str>> {
        if !input.contains("{{") {
            if input.contains("}}") {
                self.issue(path, RequestIssueKind::InvalidTemplate, None);
                return None;
            }
            if input.len() > MAX_RESOLVED_VALUE_BYTES {
                self.issue(path, RequestIssueKind::ResolvedValueTooLarge, None);
                return None;
            }
            return Some(Cow::Borrowed(input));
        }

        let mut output = String::with_capacity(input.len());
        let mut remainder = input;
        while let Some(open) = remainder.find("{{") {
            let literal = &remainder[..open];
            if literal.contains("}}") {
                self.issue(path, RequestIssueKind::InvalidTemplate, None);
                return None;
            }
            if !self.append_resolved(&mut output, literal, path) {
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
        Some(Cow::Owned(output))
    }

    fn resolve_variable(&mut self, name: &str, path: FieldPath) -> Option<String> {
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
        let result = self.resolve_source(source, path).map(Cow::into_owned);
        self.resolving.remove(name);
        if let Some(value) = &result {
            self.resolved_variables
                .insert(name.to_owned(), value.clone());
        }
        result
    }

    fn append_resolved(&mut self, output: &mut String, value: &str, path: FieldPath) -> bool {
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

    fn header_name(&mut self, name: &str, path: FieldPath) -> Option<HeaderName> {
        let name = HeaderName::from_bytes(name.trim().as_bytes()).ok();
        if name.is_none() {
            self.issue(path, RequestIssueKind::InvalidHeaderName, None);
        }
        name
    }

    fn header_value(&mut self, value: &str, path: FieldPath) -> Option<HeaderValue> {
        let value = HeaderValue::from_str(value.trim()).ok();
        if value.is_none() {
            self.issue(path, RequestIssueKind::InvalidHeaderValue, None);
        }
        value
    }

    fn issue(&mut self, path: FieldPath, kind: RequestIssueKind, reference: Option<&str>) {
        self.issues.push(RequestIssue::new(path, kind, reference));
    }

    fn warning(&mut self, path: FieldPath, kind: RequestIssueKind, reference: Option<&str>) {
        self.warnings.push(RequestIssue::new(path, kind, reference));
    }

    fn finish(mut self) -> RequestPipelineError {
        // A variable referenced several times in one field reports once.
        self.issues.dedup();
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

impl RequestIssue {
    fn new(path: FieldPath, kind: RequestIssueKind, reference: Option<&str>) -> Self {
        Self {
            path: path.to_string(),
            kind,
            reference: reference.map(str::to_owned),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestPipelineError {
    pub issues: Vec<RequestIssue>,
}

impl fmt::Display for RequestPipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "request has {} issue(s)", self.issues.len())
    }
}

impl Error for RequestPipelineError {}
