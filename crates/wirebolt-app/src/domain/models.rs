//! Editable records at the application boundary. Storage owns file validation.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainValueSource {
    Literal { value: String },
    Secret { secret: String },
}
impl Serialize for DomainValueSource {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Literal { value } => value.serialize(s),
            Self::Secret { secret } => {
                std::collections::BTreeMap::from([("secret", secret)]).serialize(s)
            }
        }
    }
}
impl<'de> Deserialize<'de> for DomainValueSource {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Literal(String),
            Secret { secret: String },
        }
        Ok(match Wire::deserialize(d)? {
            Wire::Literal(value) => Self::Literal { value },
            Wire::Secret { secret } => Self::Secret { secret },
        })
    }
}
impl DomainValueSource {
    #[must_use]
    pub fn editable(&self) -> &str {
        match self {
            Self::Literal { value } => value,
            Self::Secret { secret } => secret,
        }
    }
    #[must_use]
    pub fn secret_name(&self) -> Option<&str> {
        match self {
            Self::Secret { secret } => Some(secret),
            Self::Literal { .. } => None,
        }
    }
    #[must_use]
    pub fn literal(value: impl Into<String>) -> Self {
        Self::Literal {
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainField {
    pub id: String,
    pub name: String,
    pub value: DomainValueSource,
    pub enabled: bool,
    pub sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainOAuthConfiguration {
    pub grant: DomainOAuthGrant,
    pub authorization_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret_reference: String,
    pub scopes: String,
    pub audience: String,
    pub redirect_uri: String,
    pub access_token_reference: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainMultipartPart {
    pub id: String,
    pub name: String,
    pub kind: DomainMultipartKind,
    pub value: DomainValueSource,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DomainTransport {
    pub validate_tls: bool,
    pub follow_redirects: bool,
    pub maximum_redirects: i64,
    pub total_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub client_certificate_reference: Option<String>,
    pub custom_ca_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DomainRequestDraft {
    pub id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub web_socket: bool,
    pub note: String,
    pub query: Vec<DomainField>,
    pub headers: Vec<DomainField>,
    pub authentication: DomainAuthentication,
    pub body: DomainBody,
    pub proxy: Option<DomainProxy>,
    pub transport: DomainTransport,
    pub inherits_workspace_transport: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainVariable {
    pub id: String,
    pub key: String,
    pub value: DomainValueSource,
    pub enabled: bool,
    pub order: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainEnvironment {
    pub id: String,
    pub name: String,
    pub variables: Vec<DomainVariable>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainLocation {
    pub collection_id: String,
    pub group_id: Option<String>,
    pub order: i64,
    pub request: DomainRequestDraft,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainGroup {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub order: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainCollection {
    pub id: String,
    pub name: String,
    pub order: i64,
    pub groups: Vec<DomainGroup>,
    pub requests: Vec<DomainLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainWorkspace {
    pub name: String,
    pub proxy: Option<DomainProxy>,
    pub transport: DomainTransport,
    pub collections: Vec<DomainCollection>,
    pub environments: Vec<DomainEnvironment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainProxyRoute {
    pub destination: String,
    pub endpoint: String,
    pub credentials: Option<DomainProxyCredentials>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DomainProxyCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DomainOAuthGrant {
    #[default]
    AuthorizationCodePkce,
    ClientCredentials,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DomainApiKeyPlacement {
    #[default]
    Header,
    Query,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DomainMultipartKind {
    #[default]
    Text,
    Binary,
    File,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum DomainAuthentication {
    #[default]
    None,
    Basic {
        username: DomainValueSource,
        password: DomainValueSource,
    },
    Bearer {
        token: DomainValueSource,
    },
    ApiKey {
        placement: DomainApiKeyPlacement,
        name: String,
        value: DomainValueSource,
    },
    Oauth2 {
        configuration: DomainOAuthConfiguration,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[derive(Default)]
pub enum DomainBody {
    #[default]
    Empty,
    Text {
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
        content_type: Option<String>,
        value: String,
    },
    FormUrlEncoded {
        fields: Vec<DomainField>,
    },
    Multipart {
        parts: Vec<DomainMultipartPart>,
    },
    File {
        path: String,
        content_type: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum DomainProxy {
    System,
    Direct,
    Manual { routes: Vec<DomainProxyRoute> },
}
impl Default for DomainTransport {
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
impl Default for DomainRequestDraft {
    fn default() -> Self {
        Self {
            id: "draft".into(),
            name: "Untitled Request".into(),
            method: "GET".into(),
            url: String::new(),
            web_socket: false,
            note: String::new(),
            query: vec![],
            headers: vec![],
            authentication: DomainAuthentication::None,
            body: DomainBody::Empty,
            proxy: None,
            transport: DomainTransport::default(),
            inherits_workspace_transport: true,
        }
    }
}

impl Default for DomainValueSource {
    fn default() -> Self {
        Self::literal("")
    }
}
/// Immutable input to a transport run; state is projected once before execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainRunInput {
    pub method: String,
    pub url: String,
    pub query: Vec<DomainField>,
    pub headers: Vec<DomainField>,
    pub authentication: DomainAuthentication,
    pub body: DomainBody,
    pub variables: std::collections::HashMap<String, DomainValueSource>,
    pub app_proxy: Option<DomainProxy>,
    pub workspace_proxy: Option<DomainProxy>,
    pub request_proxy: Option<DomainProxy>,
    pub total_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub max_response_bytes: Option<u64>,
    pub validate_tls: bool,
    pub follow_redirects: bool,
    pub maximum_redirects: u8,
    pub client_certificate_reference: Option<String>,
    pub custom_ca_path: Option<String>,
}

/// Composer choices are typed independently of the authentication payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainAuthenticationKind {
    None,
    Basic,
    Bearer,
    ApiKey,
    Oauth2,
}
