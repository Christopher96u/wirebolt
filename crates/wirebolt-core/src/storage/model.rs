use std::{collections::BTreeMap, error::Error, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::proxy::ProxyMode;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

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
    pub(super) schema_version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyMode>,
}

impl Workspace {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: name.into(),
            proxy: None,
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
    pub(super) schema_version: u32,
    pub id: DocumentId,
    pub name: String,
}

impl Collection {
    #[must_use]
    pub const fn new(id: DocumentId, name: String) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id,
            name,
        }
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub(super) schema_version: u32,
    pub id: DocumentId,
    pub name: String,
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<RequestHeader>,
    pub body: RequestBody,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_override: Option<ProxyMode>,
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
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            body: RequestBody::Empty,
            proxy_override: None,
        }
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestHeader {
    pub name: String,
    pub value: ValueSource,
    pub enabled: bool,
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
    pub(super) schema_version: u32,
    pub id: DocumentId,
    pub name: String,
    pub variables: BTreeMap<String, ValueSource>,
}

impl Environment {
    #[must_use]
    pub const fn new(
        id: DocumentId,
        name: String,
        variables: BTreeMap<String, ValueSource>,
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
}
