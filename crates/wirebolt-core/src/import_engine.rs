use std::{
    borrow::Cow,
    error::Error,
    fmt::{self, Write as _},
};

use serde_json::Value;

mod curl;
mod har;
mod legacy_v1;
mod postman;
mod workspace;

pub use workspace::{ImportedEnvironment, ImportedRequestSettings, ImportedWorkspace};

use crate::{
    EnvironmentVariable, Request, RequestAuthentication, RequestBody, RequestHeader,
    RequestValueField, SecretName, ValueSource,
};

/// One supported import source. Each variant is parsed by its own module and
/// dispatched from [`ImportEngine::parse_import`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportFormat {
    Curl,
    Har,
    LegacyWorkspaceV1,
    PostmanV2,
}

impl ImportFormat {
    /// Parses the stable bridge token for one supported import format.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] for unknown format tokens.
    pub fn parse(value: &str) -> Result<Self, ImportError> {
        match value {
            "curl" => Ok(Self::Curl),
            "har" => Ok(Self::Har),
            "legacy_workspace_v1" => Ok(Self::LegacyWorkspaceV1),
            "postman_v2" => Ok(Self::PostmanV2),
            _ => Err(ImportError::new("This import format isn't supported.")),
        }
    }
}

/// One imported collection. Importers report anything they could not carry
/// over exactly in `warnings`, as short user-facing sentences that never
/// quote secret material; the app shows them after the import.
#[derive(Clone, Debug, Default)]
pub struct ImportedCollection {
    pub name: String,
    pub groups: Vec<ImportedGroup>,
    pub requests: Vec<ImportedRequest>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportedGroup {
    pub source_id: String,
    pub name: String,
    pub parent_source_id: Option<String>,
    pub order: i64,
}

/// Literal credentials in `authentication` are moved to Keychain by the
/// bridge before the request is written; `{{variable}}` references stay.
#[derive(Clone, Debug)]
pub struct ImportedRequest {
    pub source_id: String,
    pub name: String,
    pub group_source_id: Option<String>,
    pub order: i64,
    pub method: String,
    pub url: String,
    pub headers: Vec<RequestHeader>,
    pub query: Vec<RequestValueField>,
    pub authentication: RequestAuthentication,
    pub body: RequestBody,
    pub web_socket: bool,
    pub note: String,
}

impl Default for ImportedRequest {
    fn default() -> Self {
        Self {
            source_id: String::new(),
            name: String::new(),
            group_source_id: None,
            order: 0,
            method: "GET".to_owned(),
            url: String::new(),
            headers: Vec::new(),
            query: Vec::new(),
            authentication: RequestAuthentication::None,
            body: RequestBody::Empty,
            web_socket: false,
            note: String::new(),
        }
    }
}

impl ImportedRequest {
    #[must_use]
    pub fn into_request(
        self,
        id: crate::DocumentId,
        group_id: Option<crate::DocumentId>,
    ) -> Request {
        let mut request = Request::new(id, self.name, self.method, self.url);
        request.group_id = group_id;
        request.order = self.order;
        request.headers = self.headers;
        request.query = self.query;
        request.authentication = self.authentication;
        request.body = self.body;
        request.web_socket = self.web_socket;
        request.note = self.note;
        request
    }
}

/// Everything one import creates, plus credential material that the bridge
/// must store in Keychain before any document is written.
#[derive(Clone, Debug)]
pub struct ParsedImport {
    pub workspace: ImportedWorkspace,
    /// Material for the placeholder secret names used by the workspace's
    /// environment variables and OAuth 2.0 client secret references. The
    /// bridge gives each placeholder a unique Keychain name.
    pub secrets: Vec<ImportedSecret>,
}

impl ParsedImport {
    /// The collections' warnings, in order, for the post-import summary.
    pub fn warnings(&self) -> impl Iterator<Item = &str> {
        self.workspace
            .collections
            .iter()
            .flat_map(|collection| collection.warnings.iter().map(String::as_str))
    }
}

impl From<ImportedCollection> for ParsedImport {
    fn from(collection: ImportedCollection) -> Self {
        ImportedWorkspace::from(collection).into()
    }
}

impl From<ImportedWorkspace> for ParsedImport {
    fn from(workspace: ImportedWorkspace) -> Self {
        Self {
            workspace,
            secrets: Vec::new(),
        }
    }
}

/// Credential material found in an import, under a placeholder reference.
#[derive(Clone)]
pub struct ImportedSecret {
    pub reference: SecretName,
    pub material: String,
}

impl fmt::Debug for ImportedSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImportedSecret")
            .field("reference", &self.reference)
            .field("material", &"[REDACTED]")
            .finish()
    }
}

/// Collects credential material under unique placeholder names while an
/// importer runs.
#[derive(Debug, Default)]
struct SecretCollector {
    secrets: Vec<ImportedSecret>,
}

impl SecretCollector {
    /// Stores `material` and returns its placeholder reference.
    fn add(&mut self, label: &str, material: String) -> Option<SecretName> {
        let label = slug(label);
        let reference = SecretName::new(format!("{label}-{}", self.secrets.len())).ok()?;
        self.secrets.push(ImportedSecret {
            reference: reference.clone(),
            material,
        });
        Some(reference)
    }
}

/// Builds environment rows; credential-like values become secret references.
fn environment_variable(
    order: usize,
    key: String,
    value: String,
    enabled: bool,
    secret: bool,
    secrets: &mut SecretCollector,
) -> EnvironmentVariable {
    let value = if (secret || looks_like_credential_name(&key))
        && !value.is_empty()
        && !value.contains("{{")
    {
        secrets
            .add(&format!("variable-{key}"), value.clone())
            .map_or_else(|| ValueSource::literal(value), ValueSource::secret)
    } else {
        ValueSource::literal(value)
    };
    EnvironmentVariable {
        id: format!("variable-{order}"),
        key,
        value,
        enabled,
        order: i64::try_from(order).unwrap_or(i64::MAX),
    }
}

/// Why an import failed, as a user-facing sentence without source contents.
#[derive(Debug, Eq, PartialEq)]
pub struct ImportError {
    reason: Cow<'static, str>,
}

impl ImportError {
    const fn new(reason: &'static str) -> Self {
        Self {
            reason: Cow::Borrowed(reason),
        }
    }

    fn message(reason: String) -> Self {
        Self {
            reason: Cow::Owned(reason),
        }
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl Error for ImportError {}

#[derive(Debug, Default)]
pub struct ImportEngine;

impl ImportEngine {
    /// Parses a source completely, with every collection, environment,
    /// request setting and Keychain secret it creates. This is the single
    /// dispatch point for import formats.
    ///
    /// `file_name` names the collection after the imported file when the
    /// format has no collection name of its own.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] with a user-facing reason, never source contents.
    pub fn parse_import(
        format: ImportFormat,
        source: &str,
        file_name: Option<&str>,
    ) -> Result<ParsedImport, ImportError> {
        let result = match format {
            ImportFormat::Curl => curl::parse(source, file_name),
            ImportFormat::Har => har::parse(source, file_name).map(ParsedImport::from),
            ImportFormat::PostmanV2 => postman::parse(source),
            ImportFormat::LegacyWorkspaceV1 => {
                match (file_name, serde_json::from_str::<Value>(source)) {
                    (Some(name), Ok(root)) if root.get("nodes").is_some() => {
                        Self::parse_workspace_file(format, source, name).map(ParsedImport::from)
                    }
                    _ => parse_legacy_workspace(source).map(ParsedImport::from),
                }
            }
        };
        result.map_err(|error| unsupported_document(source).unwrap_or(error))
    }

    /// Imports a file while retaining its top-level folders under the filename.
    ///
    /// # Errors
    /// Returns [`ImportError`] for invalid input without including source contents.
    pub fn parse_file(
        format: ImportFormat,
        source: &str,
        name: &str,
    ) -> Result<ImportedCollection, ImportError> {
        if format == ImportFormat::LegacyWorkspaceV1
            && let Ok(root) = serde_json::from_str::<Value>(source)
            && root.get("nodes").is_some()
        {
            return legacy_v1::parse_named(&root, Some(name));
        }
        first_collection(Self::parse_import(format, source, Some(name))?)
    }

    /// Imports a file with everything it describes: a Wirebolt workspace export
    /// yields one collection per exported collection plus its environments;
    /// other formats yield the single collection of [`Self::parse_file`].
    ///
    /// # Errors
    /// Returns [`ImportError`] for invalid input without including source contents.
    pub fn parse_workspace_file(
        format: ImportFormat,
        source: &str,
        name: &str,
    ) -> Result<ImportedWorkspace, ImportError> {
        if format == ImportFormat::LegacyWorkspaceV1 {
            let root: Value = serde_json::from_str(source)
                .map_err(|_| ImportError::new("legacy workspace JSON is invalid"))?;
            if root.get("nodes").is_some() {
                return legacy_v1::parse_workspace(&root, Some(name));
            }
        }
        Self::parse_file(format, source, name).map(ImportedWorkspace::from)
    }

    /// Parses a source into its first collection.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] without including source contents.
    pub fn parse(format: ImportFormat, source: &str) -> Result<ImportedCollection, ImportError> {
        first_collection(Self::parse_import(format, source, None)?)
    }
}

fn first_collection(parsed: ParsedImport) -> Result<ImportedCollection, ImportError> {
    parsed
        .workspace
        .collections
        .into_iter()
        .next()
        .ok_or(ImportError::new("The document has nothing to import."))
}

/// Recognizes documents Wirebolt cannot import so a failed parse can name
/// the actual problem instead of the chosen importer's generic error.
fn unsupported_document(source: &str) -> Option<ImportError> {
    let trimmed = source.trim_start();
    let yaml_key = |key: &str| {
        trimmed.lines().take(40).any(|line| {
            line.strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with(':'))
        })
    };
    if yaml_key("openapi") || yaml_key("swagger") {
        return Some(ImportError::new(
            "OpenAPI and Swagger documents aren't supported yet.",
        ));
    }
    if !trimmed.starts_with('{') {
        return None;
    }
    let root: Value = serde_json::from_str(trimmed).ok()?;
    if root.get("openapi").is_some() || root.get("swagger").is_some() {
        return Some(ImportError::new(
            "OpenAPI and Swagger documents aren't supported yet.",
        ));
    }
    if root.get("_postman_variable_scope").is_some() {
        return Some(ImportError::new(
            "This is a Postman environment, not a collection. Import a Postman Collection v2.0 or v2.1 file.",
        ));
    }
    None
}

/// Joins a warning with the affected item names, keeping the list short.
fn listed_warning(message: &str, items: &[String]) -> String {
    const LIMIT: usize = 6;
    let mut listed = items
        .iter()
        .take(LIMIT)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > LIMIT {
        let _ = write!(listed, " and {} more", items.len() - LIMIT);
    }
    format!("{message}: {listed}.")
}

/// Credential-like names are imported as Keychain secrets, not plain values.
fn looks_like_credential_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "apikey",
        "api_key",
        "api-key",
        "credential",
        "private",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

/// Decodes `%XX` escapes so values are not encoded twice when sent. Invalid
/// sequences or non-UTF-8 results keep the original text.
fn percent_decoded(value: &str) -> String {
    if !value.contains('%') {
        return value.to_owned();
    }
    percent_encoding::percent_decode_str(value)
        .decode_utf8()
        .map_or_else(|_| value.to_owned(), Cow::into_owned)
}

/// Headers that describe one captured connection rather than the request.
fn is_connection_header(name: &str) -> bool {
    name.starts_with(':')
        || [
            "host",
            "content-length",
            "connection",
            "keep-alive",
            "proxy-connection",
            "transfer-encoding",
            "upgrade",
        ]
        .iter()
        .any(|header| name.eq_ignore_ascii_case(header))
}

/// Moves a literal `Authorization: Bearer …` or `Basic …` header into typed
/// authentication, so the bridge stores the credential in Keychain instead
/// of the workspace.
fn authorization_header_authentication(headers: &mut Vec<RequestHeader>) -> RequestAuthentication {
    let mut matching = headers
        .iter()
        .enumerate()
        .filter(|(_, header)| header.enabled && header.name.eq_ignore_ascii_case("authorization"));
    let (Some((index, header)), None) = (matching.next(), matching.next()) else {
        return RequestAuthentication::None;
    };
    let ValueSource::Literal(value) = &header.value else {
        return RequestAuthentication::None;
    };
    let Some((scheme, credentials)) = value.trim().split_once(' ') else {
        return RequestAuthentication::None;
    };
    let credentials = credentials.trim();
    let authentication = if scheme.eq_ignore_ascii_case("bearer") && !credentials.is_empty() {
        RequestAuthentication::Bearer {
            token: ValueSource::literal(credentials),
        }
    } else if scheme.eq_ignore_ascii_case("basic")
        && let Some((username, password)) =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, credentials)
                .ok()
                .and_then(|decoded| String::from_utf8(decoded).ok())
                .and_then(|decoded| {
                    decoded
                        .split_once(':')
                        .map(|(username, password)| (username.to_owned(), password.to_owned()))
                })
    {
        RequestAuthentication::Basic {
            username: ValueSource::literal(username),
            password: ValueSource::literal(password),
        }
    } else {
        return RequestAuthentication::None;
    };
    headers.remove(index);
    authentication
}

/// A readable request name such as `GET /users/42`.
fn method_and_path_name(method: &str, url: &str) -> String {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = without_scheme
        .find('/')
        .map_or("/", |index| &without_scheme[index..]);
    let path = path.split(['?', '#']).next().unwrap_or("/");
    let path = if path.is_empty() { "/" } else { path };
    format!("{method} {path}")
}

fn parse_legacy_workspace(source: &str) -> Result<ImportedCollection, ImportError> {
    let root: Value = serde_json::from_str(source)
        .map_err(|_| ImportError::new("legacy workspace JSON is invalid"))?;
    if root.get("nodes").is_some() {
        return legacy_v1::parse(&root);
    }
    let name = root
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Imported Workspace")
        .to_owned();
    let requests = root
        .get("requests")
        .and_then(Value::as_array)
        .or_else(|| {
            root.pointer("/collection/requests")
                .and_then(Value::as_array)
        })
        .ok_or_else(|| ImportError::new("legacy workspace document has no requests"))?;
    let requests = requests
        .iter()
        .enumerate()
        .filter_map(|(index, request)| {
            let url = request.get("url")?.as_str()?.to_owned();
            let method = request
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET")
                .to_owned();
            Some(ImportedRequest {
                source_id: format!("legacy-workspace-{index}"),
                name: request
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled")
                    .to_owned(),
                group_source_id: None,
                order: i64::try_from(index).unwrap_or(i64::MAX),
                method,
                url,
                headers: request
                    .get("headers")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(header_from_name_value)
                    .collect(),
                ..ImportedRequest::default()
            })
        })
        .collect::<Vec<_>>();
    if requests.is_empty() {
        return Err(ImportError::new(
            "legacy workspace document has no importable requests",
        ));
    }
    Ok(ImportedCollection {
        name,
        requests,
        ..ImportedCollection::default()
    })
}

fn header_from_name_value(value: &Value) -> Option<RequestHeader> {
    Some(RequestHeader::enabled(
        value.get("name").or_else(|| value.get("key"))?.as_str()?,
        ValueSource::literal(value.get("value").and_then(Value::as_str).unwrap_or("")),
    ))
}

fn slug(value: &str) -> String {
    let value = value
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let value = value
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if value.is_empty() {
        "item".to_owned()
    } else {
        value.chars().take(40).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{ImportEngine, ImportFormat};
    use crate::{RequestBody, ValueSource};

    #[test]
    fn imports_quoted_curl_without_exposing_or_losing_values() {
        let collection = ImportEngine::parse(
            ImportFormat::Curl,
            r#"curl --request patch 'https://example.com/items?q=hello world' -H 'X-Token: secret ref' --data-raw '{"ok":true}'"#,
        )
        .expect("parse cURL");

        let request = &collection.requests[0];
        assert_eq!(request.method, "PATCH");
        assert_eq!(request.url, "https://example.com/items?q=hello world");
        assert_eq!(request.headers[0].value, ValueSource::literal("secret ref"));
        assert!(matches!(request.body, RequestBody::Json { .. }));
    }

    #[test]
    fn preserves_nested_postman_groups_custom_methods_and_multipart_files() {
        let source = r#"{
          "info": {"name": "Payments"},
          "item": [{"name": "Admin", "item": [{
            "name": "Upload", "request": {
              "method": "PURGE", "url": {"raw": "https://example.com/upload"},
              "body": {"mode": "formdata", "formdata": [
                {"key": "asset", "type": "file", "src": "/tmp/asset.bin"}
              ]}
            }
          }]}]
        }"#;
        let collection =
            ImportEngine::parse(ImportFormat::PostmanV2, source).expect("parse Postman");

        assert_eq!(collection.name, "Payments");
        assert_eq!(collection.groups.len(), 1);
        assert_eq!(collection.requests[0].method, "PURGE");
        assert_eq!(
            collection.requests[0].group_source_id.as_deref(),
            Some(collection.groups[0].source_id.as_str())
        );
        assert!(matches!(
            collection.requests[0].body,
            RequestBody::Multipart { .. }
        ));
    }

    #[test]
    fn rejects_invalid_sources_before_returning_any_documents() {
        assert!(ImportEngine::parse(ImportFormat::Curl, "curl 'unterminated").is_err());
        assert!(ImportEngine::parse(ImportFormat::Har, r#"{"log":{"entries":[]}}"#).is_err());
        assert!(ImportEngine::parse(ImportFormat::PostmanV2, "not json").is_err());
    }
}
