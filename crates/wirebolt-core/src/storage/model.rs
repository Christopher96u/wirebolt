use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::proxy::ProxyMode;

pub const CURRENT_SCHEMA_VERSION: u32 = 6;
static ROW_ID_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DocumentId(String);

impl DocumentId {
    /// Creates a path-safe identifier.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError`] unless the value contains 1–64 lowercase
    /// ASCII letters, digits, or internal hyphens.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        let valid_length = (1..=64).contains(&value.len());
        let valid_edges = value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric);
        let valid_characters = value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');

        if valid_length && valid_edges && valid_characters {
            Ok(Self(value))
        } else {
            Err(IdentifierError::document())
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DocumentId {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for DocumentId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DocumentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SecretName(String);

impl SecretName {
    /// Creates a Keychain reference name, never secret material.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError`] when the name is empty, longer than 128
    /// bytes, or contains path separators, whitespace, or control characters.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        let valid_length = (1..=128).contains(&value.len());
        let valid_characters = value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));

        if valid_length && valid_characters {
            Ok(Self(value))
        } else {
            Err(IdentifierError::secret())
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for SecretName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SecretName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SecretName::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentifierError {
    message: &'static str,
}

impl IdentifierError {
    const fn document() -> Self {
        Self {
            message: "document ID must be 1–64 lowercase ASCII letters, digits, or internal hyphens",
        }
    }

    const fn secret() -> Self {
        Self {
            message: "secret name must be 1–128 ASCII letters, digits, dots, hyphens, or underscores",
        }
    }
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for IdentifierError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    #[serde(default)]
    pub(super) schema_version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyMode>,
    #[serde(default)]
    pub transport: TransportSettings,
}

impl Workspace {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: name.into(),
            proxy: None,
            transport: TransportSettings::default(),
        }
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    #[serde(default)]
    pub(super) schema_version: u32,
    pub id: DocumentId,
    pub name: String,
    #[serde(default)]
    pub order: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
}

impl Collection {
    #[must_use]
    pub const fn new(id: DocumentId, name: String) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id,
            name,
            order: 0,
            groups: Vec::new(),
        }
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: DocumentId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<DocumentId>,
    #[serde(default)]
    pub order: i64,
}

impl Group {
    #[must_use]
    pub const fn new(
        id: DocumentId,
        name: String,
        parent_id: Option<DocumentId>,
        order: i64,
    ) -> Self {
        Self {
            id,
            name,
            parent_id,
            order,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(default)]
    pub(super) schema_version: u32,
    pub id: DocumentId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<DocumentId>,
    #[serde(default)]
    pub order: i64,
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub web_socket: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<RequestValueField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<RequestHeader>,
    #[serde(default, skip_serializing_if = "RequestAuthentication::is_none")]
    pub authentication: RequestAuthentication,
    pub body: RequestBody,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_override: Option<ProxyMode>,
    #[serde(default)]
    pub transport: TransportSettings,
    #[serde(default = "enabled_by_default")]
    pub inherits_workspace_transport: bool,
}

impl Request {
    #[must_use]
    pub fn new(
        id: DocumentId,
        name: impl Into<String>,
        method: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id,
            name: name.into(),
            group_id: None,
            order: 0,
            method: method.into(),
            url: url.into(),
            web_socket: false,
            note: String::new(),
            query: Vec::new(),
            headers: Vec::new(),
            authentication: RequestAuthentication::None,
            body: RequestBody::Empty,
            proxy_override: None,
            transport: TransportSettings::default(),
            inherits_workspace_transport: true,
        }
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportSettings {
    #[serde(default = "enabled_by_default")]
    pub validate_tls: bool,
    #[serde(default)]
    pub follow_redirects: bool,
    #[serde(default = "default_maximum_redirects")]
    pub maximum_redirects: u8,
    #[serde(default = "default_total_timeout_ms")]
    pub total_timeout_ms: u64,
    #[serde(default = "default_read_timeout_ms")]
    pub read_timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_certificate_reference: Option<SecretName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_ca_path: Option<String>,
}

impl Default for TransportSettings {
    fn default() -> Self {
        Self {
            validate_tls: true,
            follow_redirects: false,
            maximum_redirects: 10,
            total_timeout_ms: 30_000,
            read_timeout_ms: 10_000,
            client_certificate_reference: None,
            custom_ca_path: None,
        }
    }
}

const fn default_maximum_redirects() -> u8 {
    10
}
const fn default_total_timeout_ms() -> u64 {
    30_000
}
const fn default_read_timeout_ms() -> u64 {
    10_000
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestValueField {
    #[serde(default = "new_row_id")]
    pub id: String,
    pub name: String,
    pub value: ValueSource,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sensitive: bool,
}

impl RequestValueField {
    #[must_use]
    pub fn enabled(name: impl Into<String>, value: ValueSource) -> Self {
        Self {
            id: new_row_id(),
            name: name.into(),
            value,
            enabled: true,
            sensitive: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestHeader {
    #[serde(default = "new_row_id")]
    pub id: String,
    pub name: String,
    pub value: ValueSource,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sensitive: bool,
}

impl RequestHeader {
    #[must_use]
    pub fn enabled(name: impl Into<String>, value: ValueSource) -> Self {
        Self {
            id: new_row_id(),
            name: name.into(),
            value,
            enabled: true,
            sensitive: false,
        }
    }
}

fn new_row_id() -> String {
    let sequence = ROW_ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("row-{}-{sequence}", std::process::id())
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum RequestAuthentication {
    #[default]
    None,
    Basic {
        username: ValueSource,
        password: ValueSource,
    },
    Bearer {
        token: ValueSource,
    },
    ApiKey {
        placement: ApiKeyPlacement,
        name: String,
        value: ValueSource,
    },
    Oauth2 {
        configuration: Oauth2Configuration,
    },
}

impl RequestAuthentication {
    fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyPlacement {
    Header,
    Query,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Oauth2Grant {
    AuthorizationCodePkce,
    ClientCredentials,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Oauth2Configuration {
    pub grant: Oauth2Grant,
    pub authorization_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret_reference: SecretName,
    pub scopes: String,
    pub audience: String,
    pub redirect_uri: String,
    pub access_token_reference: SecretName,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum RequestBody {
    Empty,
    Text {
        #[serde(skip_serializing_if = "Option::is_none")]
        content_type: Option<String>,
        value: String,
    },
    Json {
        value: String,
    },
    Xml {
        value: String,
    },
    Html {
        value: String,
    },
    Raw {
        #[serde(skip_serializing_if = "Option::is_none")]
        content_type: Option<String>,
        value: String,
    },
    FormUrlEncoded {
        fields: Vec<RequestValueField>,
    },
    Multipart {
        parts: Vec<MultipartPart>,
    },
    File {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content_type: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultipartPart {
    pub id: String,
    pub name: String,
    pub kind: MultipartPartKind,
    pub value: ValueSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultipartPartKind {
    Text,
    /// Embedded bytes stored as Base64, without an external file dependency.
    Binary,
    File,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ValueSource {
    Literal(String),
    Secret { secret: SecretName },
}

impl ValueSource {
    #[must_use]
    pub fn literal(value: impl Into<String>) -> Self {
        Self::Literal(value.into())
    }

    #[must_use]
    pub const fn secret(secret: SecretName) -> Self {
        Self::Secret { secret }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    #[serde(default)]
    pub(super) schema_version: u32,
    pub id: DocumentId,
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_environment_variables")]
    pub variables: Vec<EnvironmentVariable>,
}

impl Environment {
    #[must_use]
    pub fn new(id: DocumentId, name: String, variables: BTreeMap<String, ValueSource>) -> Self {
        Self::from_values(id, name, variables)
    }

    #[must_use]
    pub fn from_values(
        id: DocumentId,
        name: String,
        variables: BTreeMap<String, ValueSource>,
    ) -> Self {
        let variables = variables
            .into_iter()
            .enumerate()
            .map(|(order, (key, value))| EnvironmentVariable {
                id: format!("variable-{order}"),
                key,
                value,
                enabled: true,
                order: i64::try_from(order).unwrap_or(i64::MAX),
            })
            .collect();
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id,
            name,
            variables,
        }
    }

    #[must_use]
    pub const fn from_rows(
        id: DocumentId,
        name: String,
        variables: Vec<EnvironmentVariable>,
    ) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id,
            name,
            variables,
        }
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentVariable {
    pub id: String,
    pub key: String,
    pub value: ValueSource,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub order: i64,
}

const fn enabled_by_default() -> bool {
    true
}

fn deserialize_environment_variables<'de, D>(
    deserializer: D,
) -> Result<Vec<EnvironmentVariable>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StoredVariables {
        Rows(Vec<EnvironmentVariable>),
        Legacy(BTreeMap<String, ValueSource>),
    }

    match StoredVariables::deserialize(deserializer)? {
        StoredVariables::Rows(rows) => Ok(rows),
        StoredVariables::Legacy(values) => Ok(values
            .into_iter()
            .enumerate()
            .map(|(order, (key, value))| EnvironmentVariable {
                id: format!("variable-{order}"),
                key,
                value,
                enabled: true,
                order: i64::try_from(order).unwrap_or(i64::MAX),
            })
            .collect()),
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceDocument {
    Workspace(Workspace),
    Collection(Collection),
    Request {
        collection_id: DocumentId,
        request: Request,
    },
    Environment(Environment),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionSnapshot {
    pub collection: Collection,
    pub requests: Vec<Request>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSnapshot {
    pub workspace: Workspace,
    pub collections: Vec<CollectionSnapshot>,
    pub environments: Vec<Environment>,
    /// Documents that were skipped because they could not be loaded.
    pub problems: Vec<super::DocumentProblem>,
}
