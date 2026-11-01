//! Typed application-domain bridge. All computation lives in wirebolt-app.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI functions lift owned records and strings across the bridge."
)]
use crate::{WorkspaceBridge, app_response::lock};
use std::{collections::HashMap, sync::Arc};
pub(crate) use wirebolt_app::domain::request::DomainGitChangeKind;
use wirebolt_app::domain::theme::{DensityTokens, GeometryTokens};
use wirebolt_app::domain::{
    self,
    body::{BodyKeyOrdering, DomainBodyKind},
    curl::CurlExport,
    imports::{CollectionSourceError, PemError},
    models::{
        DomainApiKeyPlacement, DomainAuthentication, DomainAuthenticationKind, DomainBody,
        DomainCollection, DomainEnvironment, DomainField, DomainGroup, DomainLocation,
        DomainMultipartKind, DomainMultipartPart, DomainOAuthConfiguration, DomainOAuthGrant,
        DomainProxy, DomainProxyCredentials, DomainProxyRoute, DomainRequestDraft, DomainTransport,
        DomainValueSource, DomainVariable, DomainWorkspace,
    },
    oauth::{
        DomainNetworkFailure, DomainOAuthError, DomainOAuthPlatform, DomainOAuthProxy,
        DomainTokenReceipt,
    },
    proxy::{
        ConnectionProbe, DomainProxySource, ProbeState, ProxyFormInput, ProxyFormMode,
        ProxyRouteInput,
    },
    sidebar::{
        DomainSidebarCollection, DomainSidebarRequest, SidebarIndex, SidebarOutcome, SidebarRow,
    },
    theme::{
        AdaptiveColor, DomainMethodChip, DomainSemanticColor, DomainStatusClass, DomainThemePreset,
        ResolvedTokens, RgbColor, ThemeTokens, TypographyTokens,
    },
    variables::{CatalogEntry, OpenTemplateReference, TemplateReference, TemplateSegment},
};
use wirebolt_app::response::FailureCategory;
#[cfg(test)]
use wirebolt_core::GitDelta as WorkspaceGitDelta;
pub(crate) use wirebolt_core::ImportFormat as DomainImportFormat;

#[uniffi::remote(Record)]
pub struct ProxyRouteInput {
    pub id: String,
    pub destination: String,
    pub scheme: String,
    pub host: String,
    pub port: String,
    pub authenticated: bool,
    pub replace_credentials: bool,
    pub username: String,
    pub password: String,
    pub credentials: Option<DomainProxyCredentials>,
}

#[uniffi::remote(Record)]
pub struct TemplateReference {
    pub name: String,
    pub location: u64,
    pub length: u64,
}

#[uniffi::remote(Record)]
pub struct OpenTemplateReference {
    pub prefix: String,
    pub location: u64,
    pub length: u64,
    pub is_closed: bool,
}

#[uniffi::remote(Record)]
pub struct CatalogEntry {
    pub name: String,
    pub value: DomainValueSource,
    pub environment_name: String,
    pub display_value: String,
}

#[uniffi::remote(Enum)]
pub enum TemplateSegment {
    Text { value: String },
    Variable { name: String },
}

#[uniffi::remote(Record)]
pub struct CurlExport {
    pub command: String,
    pub secret_names: Vec<String>,
}

#[uniffi::remote(Enum)]
pub enum DomainValueSource {
    Literal { value: String },
    Secret { secret: String },
}

#[uniffi::remote(Record)]
pub struct DomainField {
    pub id: String,
    pub name: String,
    pub value: DomainValueSource,
    pub enabled: bool,
    pub sensitive: bool,
}

#[uniffi::remote(Record)]
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

#[uniffi::remote(Record)]
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

#[uniffi::remote(Record)]
pub struct DomainTransport {
    pub validate_tls: bool,
    pub follow_redirects: bool,
    pub maximum_redirects: i64,
    pub total_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub client_certificate_reference: Option<String>,
    pub custom_ca_path: Option<String>,
}

#[uniffi::remote(Record)]
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

#[uniffi::remote(Record)]
pub struct DomainVariable {
    pub id: String,
    pub key: String,
    pub value: DomainValueSource,
    pub enabled: bool,
    pub order: i64,
}

#[uniffi::remote(Record)]
pub struct DomainEnvironment {
    pub id: String,
    pub name: String,
    pub variables: Vec<DomainVariable>,
}

#[uniffi::remote(Record)]
pub struct DomainLocation {
    pub collection_id: String,
    pub group_id: Option<String>,
    pub order: i64,
    pub request: DomainRequestDraft,
}

#[uniffi::remote(Record)]
pub struct DomainGroup {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub order: i64,
}

#[uniffi::remote(Record)]
pub struct DomainCollection {
    pub id: String,
    pub name: String,
    pub order: i64,
    pub groups: Vec<DomainGroup>,
    pub requests: Vec<DomainLocation>,
}

#[uniffi::remote(Record)]
pub struct DomainWorkspace {
    pub name: String,
    pub proxy: Option<DomainProxy>,
    pub transport: DomainTransport,
    pub collections: Vec<DomainCollection>,
    pub environments: Vec<DomainEnvironment>,
}

#[uniffi::remote(Record)]
pub struct DomainProxyRoute {
    pub destination: String,
    pub endpoint: String,
    pub credentials: Option<DomainProxyCredentials>,
}

#[uniffi::remote(Record)]
pub struct DomainProxyCredentials {
    pub username: String,
    pub password: String,
}

#[uniffi::remote(Enum)]
pub enum DomainOAuthProxy {
    Direct,
    Proxy { endpoint: String },
}

#[uniffi::remote(Enum)]
pub enum DomainOAuthGrant {
    AuthorizationCodePkce,
    ClientCredentials,
}

#[uniffi::remote(Enum)]
pub enum DomainApiKeyPlacement {
    Header,
    Query,
}

#[uniffi::remote(Enum)]
pub enum DomainMultipartKind {
    Text,
    Binary,
    File,
}

#[uniffi::remote(Enum)]
pub enum DomainAuthentication {
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

#[uniffi::remote(Enum)]
pub enum DomainBody {
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

#[uniffi::remote(Enum)]
pub enum DomainProxy {
    System,
    Direct,
    Manual { routes: Vec<DomainProxyRoute> },
}

#[uniffi::remote(Record)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[uniffi::remote(Record)]
pub struct AdaptiveColor {
    pub light: RgbColor,
    pub dark: RgbColor,
    pub light_high_contrast: RgbColor,
    pub dark_high_contrast: RgbColor,
}

#[uniffi::remote(Record)]
pub struct ThemeTokens {
    pub preset: DomainThemePreset,
    pub window: RgbColor,
    pub sidebar: RgbColor,
    pub card: RgbColor,
    pub selected: RgbColor,
    pub tab_active: RgbColor,
    pub overlay: RgbColor,
    pub hairline: RgbColor,
    pub card_border: RgbColor,
    pub primary: RgbColor,
    pub secondary: RgbColor,
    pub muted: RgbColor,
    pub dim: RgbColor,
    pub line_number: RgbColor,
    pub send_background: RgbColor,
    pub send_foreground: RgbColor,
    pub variable_background: RgbColor,
    pub variable_foreground: RgbColor,
    pub syntax_key: RgbColor,
    pub syntax_string: RgbColor,
    pub syntax_number: RgbColor,
    pub syntax_keyword: RgbColor,
    pub syntax_punctuation: RgbColor,
    pub success: RgbColor,
    pub warning: RgbColor,
    pub error: RgbColor,
    pub method_backgrounds: Vec<RgbColor>,
    pub method_foregrounds: Vec<RgbColor>,
    pub control: RgbColor,
}

#[uniffi::remote(Record)]
pub struct TypographyTokens {
    pub body: f64,
    pub detail: f64,
    pub tab: f64,
    pub field: f64,
    pub settings_label: f64,
    pub settings_detail: f64,
    pub brand: f64,
    pub chip: f64,
    pub title: f64,
}

#[uniffi::remote(Record)]
pub struct DomainMethodChip {
    pub background: RgbColor,
    pub foreground: RgbColor,
}

#[uniffi::remote(Record)]
pub struct ResolvedTokens {
    pub definition: ThemeTokens,
    pub primary: RgbColor,
    pub muted: RgbColor,
    pub hairline: RgbColor,
    pub card_border: RgbColor,
    pub focus_border: RgbColor,
    pub chips: std::collections::HashMap<String, DomainMethodChip>,
}

#[uniffi::remote(Record)]
pub struct SidebarRow {
    pub id: String,
    pub depth: u64,
    pub parent_id: Option<String>,
    pub collection_index: u64,
    pub group_index: Option<u64>,
    pub request_index: Option<u64>,
    pub collection_id: String,
    pub title: String,
    pub move_identifier: Option<String>,
    pub request_path: String,
    pub ancestors: Vec<u64>,
    pub search: String,
}

#[uniffi::remote(Error)]
pub enum DomainOAuthError {
    InvalidConfiguration,
    AuthorizationCancelled,
    AuthorizationFailed,
    StateMismatch,
    MissingAuthorizationCode,
    ClientSecretUnavailable,
    TokenEndpointRejected { status: u16 },
    InvalidTokenResponse,
    TransportFailure { kind: DomainNetworkFailure },
}

#[uniffi::remote(Record)]
pub struct DomainTokenReceipt {
    pub token: String,
    pub expires_at: Option<f64>,
    pub scope: Option<String>,
}

/// Native services start a browser hop and complete its Rust-owned result once.
#[uniffi::export(with_foreign)]
pub trait OAuthPlatform: Send + Sync {
    fn system_proxies(&self, url: String) -> Vec<DomainOAuthProxy>;
    fn authorize(&self, url: String, callback_scheme: String, completion: Arc<OAuthAuthorization>);
    fn read_secret(&self, name: String) -> Option<String>;
}
#[derive(Debug, uniffi::Object)]
pub struct OAuthAuthorization {
    sender:
        std::sync::Mutex<Option<tokio::sync::oneshot::Sender<Result<String, DomainOAuthError>>>>,
}
#[uniffi::export]
impl OAuthAuthorization {
    pub fn complete(&self, callback: Option<String>, cancelled: bool) {
        let result = callback.ok_or(if cancelled {
            DomainOAuthError::AuthorizationCancelled
        } else {
            DomainOAuthError::AuthorizationFailed
        });
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(result);
        }
    }
}
struct PlatformAdapter(Arc<dyn OAuthPlatform>);
#[async_trait::async_trait]
impl DomainOAuthPlatform for PlatformAdapter {
    fn system_proxies(&self, url: &str) -> Vec<DomainOAuthProxy> {
        self.0.system_proxies(url.to_owned())
    }
    async fn authorize(
        &self,
        url: String,
        callback_scheme: String,
    ) -> Result<String, DomainOAuthError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0.authorize(
            url,
            callback_scheme,
            Arc::new(OAuthAuthorization {
                sender: std::sync::Mutex::new(Some(sender)),
            }),
        );
        receiver
            .await
            .unwrap_or(Err(DomainOAuthError::AuthorizationFailed))
    }
    async fn read_secret(&self, name: String) -> Option<String> {
        self.0.read_secret(name)
    }
}
#[uniffi::export(async_runtime = "tokio")]
pub async fn domain_acquire_token(
    configuration: DomainOAuthConfiguration,
    platform: Arc<dyn OAuthPlatform>,
) -> Result<DomainTokenReceipt, DomainOAuthError> {
    domain::oauth::acquire(configuration, Arc::new(PlatformAdapter(platform))).await
}

#[derive(Debug, Clone, uniffi::Error)]
pub enum DomainError {
    Invalid { reason: String },
}
impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid { reason } => f.write_str(reason),
        }
    }
}
impl std::error::Error for DomainError {}
fn invalid(reason: String) -> DomainError {
    DomainError::Invalid { reason }
}
#[derive(Debug, uniffi::Object)]
pub struct DomainSidebar {
    index: SidebarIndex,
}
#[uniffi::export]
impl DomainSidebar {
    #[uniffi::constructor]
    #[must_use]
    pub fn new(collections: Vec<DomainSidebarCollection>) -> Arc<Self> {
        Arc::new(Self {
            index: SidebarIndex::from_sources(&collections),
        })
    }
    #[must_use]
    pub fn rows(&self) -> Vec<SidebarRow> {
        self.index.rows.clone()
    }
    #[must_use]
    pub fn visible(
        &self,
        query: String,
        collapsed: Vec<String>,
        expanded: Vec<String>,
    ) -> Vec<u64> {
        self.index.visible(
            &query,
            &collapsed.into_iter().collect(),
            &expanded.into_iter().collect(),
        )
    }
    #[must_use]
    pub fn sibling(&self, id: String, delta: i64) -> Option<u64> {
        self.index.sibling(&id, delta)
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct OwnedAuthentication {
    pub authentication: DomainAuthentication,
    pub renames: HashMap<String, String>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct ResolvedDomainProxy {
    pub configuration: DomainProxy,
    pub source: DomainProxySource,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct ProxyTestResult {
    pub succeeded: bool,
    pub message: String,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct DomainPemIdentity {
    pub certificates: Vec<String>,
    pub private_key: String,
    pub combined: String,
}

#[uniffi::export]
#[must_use]
pub fn domain_normalize_method(value: String) -> Option<String> {
    domain::request::normalize_method(&value)
}

#[uniffi::export]
#[must_use]
pub fn domain_credential_reference(role: String, scope: String) -> String {
    domain::request::credential_reference(&role, &scope)
}

#[uniffi::export]
#[must_use]
pub fn domain_owned_reference(name: String) -> bool {
    domain::request::owned_reference(&name)
}

#[uniffi::export]
#[must_use]
pub fn domain_legacy_reference(name: String) -> bool {
    domain::request::legacy_reference(&name)
}

#[uniffi::export]
#[must_use]
pub fn domain_auth_references(authentication: DomainAuthentication) -> Vec<String> {
    domain::request::auth_references(&authentication)
}

#[uniffi::export]
#[must_use]
pub fn domain_rename_auth(
    authentication: DomainAuthentication,
    names: HashMap<String, String>,
) -> DomainAuthentication {
    domain::request::rename_auth(authentication, &names)
}

#[uniffi::export]
#[must_use]
pub fn domain_own_auth(
    authentication: DomainAuthentication,
    only_legacy: bool,
) -> OwnedAuthentication {
    {
        let (authentication, renames) = domain::request::own_auth(authentication, only_legacy);
        OwnedAuthentication {
            authentication,
            renames,
        }
    }
}

#[uniffi::export]
#[must_use]
pub fn domain_auth_kind(authentication: DomainAuthentication) -> DomainAuthenticationKind {
    domain::request::auth_kind(&authentication)
}

#[uniffi::export]
#[must_use]
pub fn domain_new_auth(kind: DomainAuthenticationKind) -> DomainAuthentication {
    domain::request::new_auth(kind)
}

#[uniffi::export]
#[must_use]
pub fn domain_display_url(url: String, query: Vec<DomainField>) -> String {
    domain::request::display_url_query(url, query)
}

#[uniffi::export]
#[must_use]
pub fn domain_edit_url(url: String, query: Vec<DomainField>, text: String) -> UrlQueryEdit {
    domain::request::edit_url_query(url, query, &text)
}

#[uniffi::export]
#[must_use]
pub fn domain_parse_bulk(source: String, previous: Vec<DomainField>) -> Vec<DomainField> {
    domain::request::parse_bulk(&source, previous)
}

#[uniffi::export]
#[must_use]
pub fn domain_request_references(draft: DomainRequestDraft) -> Vec<String> {
    domain::request::request_references(&draft)
        .into_iter()
        .collect()
}

#[uniffi::export]
#[must_use]
pub fn domain_workspace_references(workspace: DomainWorkspace) -> Vec<String> {
    domain::request::workspace_references(&workspace)
}

#[uniffi::export]
#[must_use]
pub fn domain_workspace_name(name: String) -> String {
    domain::request::workspace_name(&name)
}

#[uniffi::export]
#[must_use]
pub fn domain_window_title(workspace: String, request: Option<String>) -> String {
    domain::request::window_title(&workspace, request.as_deref())
}

#[uniffi::export]
#[must_use]
pub fn domain_tab_description(
    method: String,
    web_socket: bool,
    dirty: bool,
    preview: bool,
) -> String {
    domain::request::tab_description(&method, web_socket, dirty, preview)
}

#[uniffi::export]
#[must_use]
pub fn domain_template_references(text: String) -> Vec<TemplateReference> {
    domain::variables::references(&text)
}

#[uniffi::export]
#[must_use]
pub fn domain_open_reference(text: String, caret: u64) -> Option<OpenTemplateReference> {
    domain::variables::open_reference(&text, caret)
}

#[uniffi::export]
#[must_use]
pub fn domain_template_segments(text: String) -> Vec<TemplateSegment> {
    domain::variables::segments(&text)
}

#[uniffi::export]
#[must_use]
pub fn domain_catalog(
    environments: Vec<DomainEnvironment>,
    selected: Option<String>,
) -> Vec<CatalogEntry> {
    domain::variables::catalog(&environments, selected.as_deref())
}

#[uniffi::export]
#[must_use]
pub fn domain_completions(entries: Vec<CatalogEntry>, prefix: String) -> Vec<CatalogEntry> {
    domain::variables::completions(&entries, &prefix)
}

#[uniffi::export]
#[must_use]
pub fn domain_variable_summary(entries: Vec<CatalogEntry>, text: String) -> Option<String> {
    domain::variables::summary(&entries, &text)
}

#[uniffi::export]
#[must_use]
pub fn domain_enabled_values(environment: DomainEnvironment) -> HashMap<String, DomainValueSource> {
    domain::variables::enabled_values(&environment)
        .into_iter()
        .collect()
}

#[uniffi::export]
#[must_use]
pub fn domain_normalize_search(text: String) -> String {
    domain::sidebar::normalize(&text)
}

#[uniffi::export]
#[must_use]
pub fn domain_request_path(url: String) -> String {
    domain::sidebar::request_path(&url)
}

#[uniffi::export]
#[must_use]
pub fn domain_ordered_children(
    collection: DomainCollection,
    parent: Option<String>,
) -> Vec<String> {
    domain::sidebar::ordered_children(&collection, parent.as_deref())
}

#[uniffi::export]
#[must_use]
pub fn domain_subtree(collection: DomainCollection, root: Option<String>) -> Vec<u64> {
    domain::sidebar::subtree(&collection, root.as_deref())
}

#[uniffi::export]
#[must_use]
pub fn domain_folder_outline(collection: DomainCollection) -> Vec<Vec<u64>> {
    domain::sidebar::folder_outline(&collection)
}

#[uniffi::export]
pub fn domain_validate_proxy(configuration: DomainProxy) -> Result<(), DomainError> {
    domain::proxy::validate(&configuration).map_err(invalid)
}

#[uniffi::export]
#[must_use]
pub fn domain_resolve_proxy(
    request: Option<DomainProxy>,
    workspace: Option<DomainProxy>,
    app: DomainProxy,
) -> ResolvedDomainProxy {
    {
        let (configuration, source) = domain::proxy::resolve(request, workspace, app);
        ResolvedDomainProxy {
            configuration,
            source,
        }
    }
}

#[uniffi::export]
#[must_use]
pub fn domain_proxy_summary(configuration: DomainProxy, url: Option<String>) -> String {
    domain::proxy::summary(&configuration, url.as_deref())
}

#[uniffi::export]
#[must_use]
pub fn domain_proxy_route_input(route: Option<DomainProxyRoute>) -> ProxyRouteInput {
    domain::proxy::route_input(route)
}

#[uniffi::export]
#[must_use]
pub fn domain_proxy_needs_credentials(input: ProxyRouteInput) -> bool {
    domain::proxy::needs_credentials(&input)
}

#[uniffi::export]
#[must_use]
pub fn domain_proxy_credential_references(
    input: ProxyRouteInput,
) -> Option<DomainProxyCredentials> {
    domain::proxy::credential_references(&input)
}

#[uniffi::export]
pub fn domain_proxy_route_document(
    input: ProxyRouteInput,
) -> Result<DomainProxyRoute, DomainError> {
    domain::proxy::route_document(&input).map_err(invalid)
}

#[uniffi::export]
#[must_use]
pub fn domain_valid_proxy_target(url: String) -> bool {
    domain::proxy::valid_target(&url)
}

#[uniffi::export]
#[must_use]
pub fn domain_proxy_test_result(
    status: Option<u16>,
    failure: Option<FailureCategory>,
) -> ProxyTestResult {
    {
        let (succeeded, message) = domain::proxy::test_result(status, failure);
        ProxyTestResult { succeeded, message }
    }
}

#[uniffi::export]
pub fn domain_proxy_test_draft(
    configuration: DomainProxy,
    url: String,
) -> Result<DomainRequestDraft, DomainError> {
    domain::proxy::test_draft(configuration, url).map_err(invalid)
}

#[uniffi::export]
#[must_use]
pub fn domain_theme(preset: DomainThemePreset) -> ThemeTokens {
    domain::theme::theme(preset)
}

#[uniffi::export]
#[must_use]
pub fn domain_resolve_theme(preset: DomainThemePreset, increased: bool) -> ResolvedTokens {
    domain::theme::resolve(preset, increased)
}

#[uniffi::export]
#[must_use]
pub fn domain_typography(compact: bool) -> TypographyTokens {
    domain::theme::typography(compact)
}

#[uniffi::export]
#[must_use]
pub fn domain_select_preset(selection: String, dark: bool) -> DomainThemePreset {
    domain::theme::select_preset(&selection, dark)
}

#[uniffi::export]
#[must_use]
pub fn domain_semantic(name: DomainSemanticColor) -> AdaptiveColor {
    domain::theme::semantic(name)
}

#[uniffi::export]
#[must_use]
pub fn domain_method_color(method: String, web_socket: bool) -> AdaptiveColor {
    domain::theme::method_color(&method, web_socket)
}

#[uniffi::export]
#[must_use]
pub fn domain_status_class(status: u16) -> DomainStatusClass {
    domain::theme::status_class(status)
}

#[uniffi::export]
#[must_use]
pub fn domain_status_color(status: u16) -> AdaptiveColor {
    domain::theme::status_color(status)
}

#[uniffi::export]
#[must_use]
pub fn domain_luminance(color: RgbColor) -> f64 {
    color.luminance()
}

#[uniffi::export]
#[must_use]
pub fn domain_contrast(color: RgbColor, other: RgbColor) -> f64 {
    color.contrast(other)
}

#[uniffi::export]
#[must_use]
pub fn domain_encode_oauth_form(form: HashMap<String, String>) -> String {
    domain::oauth::encode_form(&form.into_iter().collect())
}

#[uniffi::export]
#[must_use]
pub fn domain_curl_export(
    draft: DomainRequestDraft,
    variables: HashMap<String, DomainValueSource>,
    hex: Vec<String>,
) -> CurlExport {
    domain::curl::export(&draft, &variables, &hex.into_iter().collect())
}

#[uniffi::export]
#[must_use]
pub fn domain_shell_quote(text: String) -> String {
    domain::curl::shell_quote(&text)
}

#[uniffi::export]
#[must_use]
pub fn domain_prints_as_hex(text: String) -> bool {
    domain::curl::prints_as_hex(&text)
}

#[uniffi::export]
#[must_use]
pub fn domain_curl_proxy_route(
    configuration: DomainProxy,
    url: String,
) -> Option<DomainProxyRoute> {
    domain::curl::proxy_route(&configuration, &url)
}

#[uniffi::export]
#[must_use]
pub fn domain_curl_proxy_arguments(
    configuration: DomainProxy,
    url: String,
    hex: Vec<String>,
) -> String {
    domain::curl::proxy_arguments(&configuration, &url, &hex.into_iter().collect())
}

#[uniffi::export]
#[must_use]
pub fn domain_detect_import(extension: String, contents: String) -> Option<DomainImportFormat> {
    domain::imports::detect(&extension, &contents)
}

#[uniffi::export]
#[must_use]
pub fn domain_detect_collection(
    extension: String,
    contents: String,
    root_json: Option<String>,
) -> Option<DomainImportFormat> {
    domain::imports::detect_collection(
        &extension,
        &contents,
        root_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .as_ref(),
    )
}

#[uniffi::export]
#[must_use]
pub fn domain_is_bruno_collection(path: String) -> bool {
    domain::imports::is_bruno_collection(std::path::Path::new(&path))
}

#[uniffi::export]
pub fn domain_bruno_bundle(
    path: String,
    policy: Arc<dyn CollectionPackageServices>,
) -> Result<String, CollectionSourceError> {
    domain::imports::bruno_bundle_with_policy(std::path::Path::new(&path), &PackageAdapter(policy))
}

#[uniffi::export]
pub fn domain_parse_pem(sources: Vec<String>) -> Result<DomainPemIdentity, PemError> {
    domain::imports::parse_pem(&sources).map(|p| DomainPemIdentity {
        combined: p.combined(),
        certificates: p.certificates,
        private_key: p.private_key,
    })
}
#[uniffi::export]
#[must_use]
pub fn domain_delimiter_ranges(text: String, location: u64, length: u64, name: String) -> Vec<u64> {
    domain::variables::delimiter_ranges(&text, location, length, &name)
}
#[uniffi::remote(Enum)]
pub enum SidebarOutcome {
    Select { id: String },
    Expand { id: String },
    Collapse { id: String },
    None,
}
#[uniffi::export]
#[must_use]
pub fn domain_sidebar_step(
    rows: Vec<SidebarRow>,
    current: Option<String>,
    delta: i64,
) -> Option<String> {
    domain::sidebar::step(&rows, current.as_deref(), delta)
}
#[uniffi::export]
#[must_use]
pub fn domain_sidebar_arrow(
    rows: Vec<SidebarRow>,
    current: Option<String>,
    right: bool,
    expanded: bool,
) -> SidebarOutcome {
    domain::sidebar::arrow(&rows, current.as_deref(), right, expanded)
}
#[uniffi::export]
#[must_use]
pub fn domain_sidebar_type_select(
    rows: Vec<SidebarRow>,
    current: Option<String>,
    prefix: String,
) -> Option<String> {
    domain::sidebar::type_select(&rows, current.as_deref(), &prefix)
}
#[uniffi::export]
#[must_use]
pub fn domain_proxy_route_secrets(input: ProxyRouteInput) -> HashMap<String, String> {
    domain::proxy::route_secrets(&input)
}
#[uniffi::export]
#[must_use]
pub fn domain_backgrounds(dark: bool) -> Vec<RgbColor> {
    domain::theme::backgrounds(dark)
}
#[uniffi::export]
#[must_use]
pub fn domain_oauth_error_message(error: DomainOAuthError) -> String {
    error.to_string()
}
#[uniffi::export]
#[must_use]
pub fn domain_curl_value_sources(draft: DomainRequestDraft) -> Vec<DomainValueSource> {
    domain::curl::value_sources(&draft)
}
#[uniffi::export]
impl WorkspaceBridge {
    /// Loads typed application records without a JSON round trip through Swift.
    ///
    /// # Errors
    /// Returns an error if the workspace cannot be read or projected.
    pub fn domain_snapshot(&self) -> Result<DomainWorkspace, crate::WorkspaceBridgeError> {
        self.inner.domain_snapshot()
    }
}
#[uniffi::remote(Record)]
pub struct ProbeState {
    pub is_running: bool,
    pub message: Option<String>,
    pub succeeded: bool,
    pub run_id: Option<String>,
}
#[uniffi::remote(Record)]
pub struct ProxyFormInput {
    pub mode: ProxyFormMode,
    pub routes: Vec<ProxyRouteInput>,
}
#[derive(Debug, uniffi::Object)]
pub struct DomainProxyProbe {
    probe: std::sync::Mutex<ConnectionProbe>,
}
#[uniffi::export]
impl DomainProxyProbe {
    #[uniffi::constructor]
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            probe: std::sync::Mutex::new(ConnectionProbe::default()),
        })
    }
    #[must_use]
    pub fn snapshot(&self) -> ProbeState {
        lock(&self.probe).state.clone()
    }
    pub fn start(&self, configuration: DomainProxy, url: String) -> Option<DomainRequestDraft> {
        lock(&self.probe).start(configuration, url)
    }
    #[must_use]
    pub fn active(&self, id: String) -> bool {
        lock(&self.probe).active(&id)
    }
    pub fn cancel(&self) {
        lock(&self.probe).cancel();
    }
    pub fn reset(&self) {
        lock(&self.probe).reset();
    }
    pub fn complete(&self, id: String, status: Option<u16>, failure: Option<FailureCategory>) {
        lock(&self.probe).complete(&id, status, failure);
    }
}
#[uniffi::export]
#[must_use]
pub fn domain_proxy_form(configuration: Option<DomainProxy>) -> ProxyFormInput {
    domain::proxy::form_input(configuration)
}
#[uniffi::export]
pub fn domain_proxy_form_document(
    input: ProxyFormInput,
) -> Result<Option<DomainProxy>, DomainError> {
    domain::proxy::form_document(&input).map_err(invalid)
}
#[uniffi::export]
#[must_use]
pub fn domain_proxy_form_secrets(input: ProxyFormInput) -> HashMap<String, String> {
    domain::proxy::form_secrets(&input)
}
use domain::request::TimeoutPresentation;
#[uniffi::remote(Record)]
pub struct TimeoutPresentation {
    pub text: Option<String>,
    pub seconds: Option<f64>,
    pub suffix: String,
}
#[uniffi::export]
#[must_use]
pub fn domain_timeout_presentation(milliseconds: u64) -> TimeoutPresentation {
    domain::request::timeout_presentation(milliseconds)
}
#[uniffi::export]
#[must_use]
pub fn domain_auth_title(kind: DomainAuthenticationKind) -> String {
    domain::request::auth_title(kind)
}
#[uniffi::export]
#[must_use]
pub fn domain_active_count(fields: Vec<DomainField>) -> u64 {
    domain::request::active_count(&fields)
}
#[uniffi::export]
#[must_use]
pub fn domain_proxy_needs_indicator(configuration: DomainProxy, source: DomainProxySource) -> bool {
    domain::proxy::needs_indicator(&configuration, source)
}
#[uniffi::export]
#[must_use]
pub fn domain_sanitized(text: String) -> String {
    domain::request::sanitized(&text)
}
use domain::request::PushAvailability;
#[uniffi::remote(Enum)]
pub enum PushAvailability {
    Push,
    Publish { branch: String },
    Unavailable { reason: String },
}
#[uniffi::export]
#[must_use]
pub fn domain_import_headline(names: Vec<String>, requests: i64, groups: i64) -> String {
    domain::request::import_headline(&names, requests, groups)
}
#[uniffi::export]
#[must_use]
pub fn domain_import_environment_line(
    names: Vec<String>,
    localized_list: String,
) -> Option<String> {
    domain::request::import_environment_line(&names, &localized_list)
}
#[uniffi::export]
#[must_use]
pub fn domain_push_availability(
    branch: Option<String>,
    upstream: Option<String>,
    ahead: u64,
) -> PushAvailability {
    domain::request::push_availability(branch, upstream.as_deref(), ahead)
}
#[uniffi::export]
#[must_use]
pub fn domain_separate_url_query(url: String, query: Vec<DomainField>) -> UrlQueryEdit {
    domain::request::separate_url_query_values(url, query)
}
#[uniffi::export]
#[must_use]
pub fn domain_legacy_environment(
    id: String,
    name: String,
    values: HashMap<String, DomainValueSource>,
) -> DomainEnvironment {
    domain::request::legacy_environment(id, name, values.into_iter().collect())
}
#[uniffi::export]
#[must_use]
pub fn domain_switch_auth(
    authentication: DomainAuthentication,
    kind: DomainAuthenticationKind,
) -> DomainAuthentication {
    domain::request::switch_auth(authentication, kind)
}
#[uniffi::export]
#[must_use]
pub fn domain_unique_id() -> String {
    domain::request::unique_id()
}
#[uniffi::export]
#[must_use]
pub fn domain_clamp_redirects(value: i64) -> i64 {
    domain::request::clamp_redirects(value)
}
#[uniffi::remote(Record)]
pub struct GeometryTokens {
    pub chip: f64,
    pub tab: f64,
    pub row: f64,
    pub card: f64,
    pub window: f64,
}
#[uniffi::remote(Record)]
pub struct DensityTokens {
    pub control_height: f64,
    pub row_height: f64,
    pub body_size: f64,
    pub detail_size: f64,
}
#[uniffi::export]
#[must_use]
pub fn domain_geometry() -> GeometryTokens {
    domain::theme::geometry()
}
#[uniffi::export]
#[must_use]
pub fn domain_density(compact: bool) -> DensityTokens {
    domain::theme::density(compact)
}
use domain::models::DomainRunInput;
#[uniffi::remote(Record)]
pub struct DomainRunInput {
    pub method: String,
    pub url: String,
    pub query: Vec<DomainField>,
    pub headers: Vec<DomainField>,
    pub authentication: DomainAuthentication,
    pub body: DomainBody,
    pub variables: HashMap<String, DomainValueSource>,
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
#[uniffi::export]
#[must_use]
pub fn domain_run_input(
    draft: DomainRequestDraft,
    variables: HashMap<String, DomainValueSource>,
    workspace_proxy: Option<DomainProxy>,
    app_proxy: Option<DomainProxy>,
) -> DomainRunInput {
    domain::request::run_input(draft, variables, workspace_proxy, app_proxy)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_snapshot_preserves_storage_identity_fields_and_secret_references() {
        let temporary = tempfile::tempdir().unwrap();
        let bridge = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Fixture".into(),
        )
        .unwrap();
        bridge
            .save_collection("collection".into(), "Collection".into())
            .unwrap();
        bridge
            .inner
            .apply_session_command(
                wirebolt_app::session::workspace::SessionWorkspaceCommand::SaveRequest {
                    location: wirebolt_app::domain::DomainLocation {
                        collection_id: "collection".into(),
                        group_id: None,
                        order: 0,
                        request: DomainRequestDraft {
                            id: "request".into(),
                            name: "Request".into(),
                            method: "QUERY".into(),
                            url: "https://example.invalid".into(),
                            headers: vec![DomainField {
                                id: "header-id".into(),
                                name: "Authorization".into(),
                                value: DomainValueSource::Secret {
                                    secret: "fixture.token".into(),
                                },
                                enabled: true,
                                sensitive: true,
                            }],
                            ..Default::default()
                        },
                    },
                },
            )
            .unwrap();
        let workspace = bridge.domain_snapshot().unwrap();
        assert_eq!(workspace.name, "Fixture");
        let draft = &workspace.collections[0].requests[0].request;
        assert_eq!(draft.method, "QUERY");
        assert_eq!(draft.headers[0].id, "header-id");
        assert_eq!(
            draft.headers[0].value,
            DomainValueSource::Secret {
                secret: "fixture.token".into()
            }
        );
        assert!(draft.headers[0].sensitive);
        assert_eq!(draft.transport, DomainTransport::default());
        assert!(draft.inherits_workspace_transport);
    }
    struct Browser;
    impl OAuthPlatform for Browser {
        fn system_proxies(&self, _: String) -> Vec<DomainOAuthProxy> {
            vec![DomainOAuthProxy::Direct]
        }
        fn authorize(&self, _url: String, _scheme: String, completion: Arc<OAuthAuthorization>) {
            completion.complete(
                Some("wirebolt://oauth/callback?state=s&code=first".into()),
                false,
            );
            completion.complete(None, true);
        }
        fn read_secret(&self, _name: String) -> Option<String> {
            Some("fixture".into())
        }
    }
    #[tokio::test]
    async fn foreign_browser_completion_is_consumed_once() {
        let adapter = PlatformAdapter(Arc::new(Browser));
        assert_eq!(
            adapter
                .authorize("https://example.invalid".into(), "wirebolt".into())
                .await
                .unwrap(),
            "wirebolt://oauth/callback?state=s&code=first"
        );
        assert_eq!(
            adapter.read_secret("reference".into()).await.as_deref(),
            Some("fixture")
        );
    }
}
use domain::variables::{CatalogIndex, NameMatch, VariableComparison};
#[uniffi::remote(Record)]
pub struct NameMatch {
    pub has_prefix: bool,
    pub contains: bool,
}
#[uniffi::export(with_foreign)]
pub trait VariableComparisonServices: Send + Sync {
    fn sorted_names(&self, names: Vec<String>) -> Vec<String>;
    fn name_matches(&self, names: Vec<String>, prefix: String) -> Vec<NameMatch>;
}
struct ComparisonAdapter(Arc<dyn VariableComparisonServices>);
impl VariableComparison for ComparisonAdapter {
    fn sorted_names(&self, names: Vec<String>) -> Vec<String> {
        self.0.sorted_names(names)
    }
    fn name_matches(&self, names: Vec<String>, prefix: String) -> Vec<NameMatch> {
        self.0.name_matches(names, prefix)
    }
}
#[derive(Debug, uniffi::Object)]
pub struct DomainCatalog {
    index: CatalogIndex,
}
#[uniffi::export]
impl DomainCatalog {
    #[uniffi::constructor]
    pub fn new(
        environments: Vec<DomainEnvironment>,
        selected: Option<String>,
        comparison: Arc<dyn VariableComparisonServices>,
    ) -> Arc<Self> {
        Arc::new(Self {
            index: CatalogIndex::new(
                &environments,
                selected.as_deref(),
                Arc::new(ComparisonAdapter(comparison)),
            ),
        })
    }
    #[must_use]
    pub fn entries(&self) -> Vec<CatalogEntry> {
        self.index.entries.clone()
    }
    #[must_use]
    pub fn completions(&self, prefix: String) -> Vec<u64> {
        self.index.completions(&prefix)
    }
    #[must_use]
    pub fn summary(&self, text: String) -> Option<String> {
        self.index.summary(&text)
    }
}

#[uniffi::export(with_foreign)]
pub trait CollectionPackageServices: Send + Sync {
    fn is_package(&self, path: String) -> bool;
}
struct PackageAdapter(Arc<dyn CollectionPackageServices>);
impl domain::imports::CollectionDirectoryPolicy for PackageAdapter {
    fn is_package(&self, path: &std::path::Path) -> bool {
        self.0.is_package(path.to_string_lossy().into_owned())
    }
}

#[uniffi::remote(Enum)]
pub enum DomainNetworkFailure {
    Timeout,
    HostNotFound,
    CannotConnect,
    Offline,
    ConnectionLost,
    Certificate,
    Tls,
    Decode,
}

use domain::request::UrlQueryEdit;
#[uniffi::remote(Record)]
pub struct UrlQueryEdit {
    pub url: String,
    pub query: Vec<DomainField>,
    pub display_url: String,
}
#[uniffi::export]
#[must_use]
pub fn domain_effective_transport(
    request: DomainTransport,
    workspace: DomainTransport,
    inherits: bool,
) -> DomainTransport {
    domain::request::effective_transport(request, workspace, inherits)
}
#[uniffi::export]
#[must_use]
pub fn domain_proxy_scope_title(scope: DomainProxySource) -> String {
    domain::proxy::scope_title(scope)
}
#[uniffi::export]
#[must_use]
pub fn domain_pem_error_message(kind: PemError) -> String {
    domain::imports::pem_error_message(kind)
}

#[uniffi::remote(Record)]
pub struct DomainSidebarRequest {
    pub id: String,
    pub collection_id: String,
    pub group_id: Option<String>,
    pub order: i64,
    pub name: String,
    pub method: String,
    pub url: String,
}
#[uniffi::remote(Record)]
pub struct DomainSidebarCollection {
    pub id: String,
    pub name: String,
    pub order: i64,
    pub groups: Vec<DomainGroup>,
    pub requests: Vec<DomainSidebarRequest>,
}

#[uniffi::remote(Enum)]
pub enum DomainAuthenticationKind {
    None,
    Basic,
    Bearer,
    ApiKey,
    Oauth2,
}
#[uniffi::remote(Error)]
pub enum PemError {
    NoCertificate,
    NoPrivateKey,
    MultiplePrivateKeys,
    EncryptedPrivateKey,
}

#[uniffi::remote(Enum)]
pub enum DomainBodyKind {
    Empty,
    Text,
    Json,
    Xml,
    Html,
    Raw,
    Form,
    Multipart,
    File,
}
#[uniffi::export(with_foreign)]
pub trait AppBodyKeyOrdering: Send + Sync {
    fn sorted_keys(&self, keys: Vec<String>) -> Vec<String>;
}
struct BodyOrderingAdapter(Arc<dyn AppBodyKeyOrdering>);
impl BodyKeyOrdering for BodyOrderingAdapter {
    fn sorted_keys(&self, keys: Vec<String>) -> Vec<String> {
        self.0.sorted_keys(keys)
    }
}
#[uniffi::export]
#[must_use]
pub fn domain_body_kind(body: DomainBodyKind) -> DomainBodyKind {
    domain::body::composer_kind(body)
}
#[uniffi::export]
#[must_use]
pub fn domain_switch_body(body: DomainBody, kind: DomainBodyKind) -> DomainBody {
    domain::body::switching(body, kind)
}
#[uniffi::export]
#[must_use]
pub fn domain_format_body(body: DomainBody, ordering: Arc<dyn AppBodyKeyOrdering>) -> DomainBody {
    domain::body::formatted(body, &BodyOrderingAdapter(ordering))
}
#[uniffi::export]
#[must_use]
pub fn domain_body_can_format(kind: DomainBodyKind) -> bool {
    domain::body::kind_can_format(kind)
}

#[uniffi::remote(Enum)]
pub enum DomainImportFormat {
    Curl,
    Har,
    LegacyWorkspaceV1,
    PostmanV2,
    Insomnia,
    Bruno,
    BrunoFolder,
}
#[uniffi::remote(Enum)]
pub enum DomainProxySource {
    Request,
    Workspace,
    AppDefault,
    SystemDefault,
}
#[uniffi::remote(Enum)]
pub enum DomainGitChangeKind {
    New,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Conflicted,
}
#[uniffi::export]
pub fn domain_git_change_title(kind: DomainGitChangeKind) -> String {
    kind.title()
}

#[uniffi::remote(Enum)]
pub enum ProxyFormMode {
    Inherit,
    System,
    Direct,
    Manual,
}

#[uniffi::remote(Enum)]
pub enum DomainThemePreset {
    Black,
    Paper,
    Ember,
}

#[uniffi::remote(Enum)]
pub enum DomainSemanticColor {
    Green,
    Blue,
    Orange,
    Purple,
    Red,
    Teal,
    Pink,
    Indigo,
    Yellow,
    Neutral,
}

#[uniffi::remote(Enum)]
pub enum DomainStatusClass {
    Informational,
    Success,
    Redirection,
    ClientError,
    ServerError,
}

#[uniffi::remote(Error)]
pub enum CollectionSourceError {
    NotACollection,
    TooLarge,
}

#[cfg(test)]
mod choice_tests {
    use super::*;
    #[test]
    fn import_formats_and_proxy_sources_cross_as_typed_choices() {
        for (extension, source, expected) in [
            ("txt", "curl https://example.com", DomainImportFormat::Curl),
            ("json", r#"{"log":{"entries":[]}}"#, DomainImportFormat::Har),
            (
                "json",
                r#"{"info":{"schema":"v2"}}"#,
                DomainImportFormat::PostmanV2,
            ),
            (
                "json",
                r#"{"version":1,"nodes":[]}"#,
                DomainImportFormat::LegacyWorkspaceV1,
            ),
            (
                "yaml",
                "type: collection.insomnia.rest/5",
                DomainImportFormat::Insomnia,
            ),
            (
                "json",
                r#"{"version":"1","name":"C","items":[]}"#,
                DomainImportFormat::Bruno,
            ),
            ("bru", "meta {}", DomainImportFormat::BrunoFolder),
        ] {
            assert_eq!(
                domain_detect_import(extension.into(), source.into()),
                Some(expected)
            );
        }
        assert_eq!(domain_detect_import("txt".into(), "unknown".into()), None);
        for (request, workspace, expected) in [
            (None, None, DomainProxySource::AppDefault),
            (
                None,
                Some(DomainProxy::System),
                DomainProxySource::Workspace,
            ),
            (
                Some(DomainProxy::Direct),
                Some(DomainProxy::System),
                DomainProxySource::Request,
            ),
        ] {
            assert_eq!(
                domain_resolve_proxy(request, workspace, DomainProxy::Direct).source,
                expected
            );
        }
    }
    #[test]
    fn git_choices_and_conflict_precedence_keep_their_labels() {
        for (delta, expected) in [
            (WorkspaceGitDelta::None, DomainGitChangeKind::Modified),
            (WorkspaceGitDelta::Modified, DomainGitChangeKind::Modified),
            (WorkspaceGitDelta::Added, DomainGitChangeKind::New),
            (WorkspaceGitDelta::Untracked, DomainGitChangeKind::New),
            (WorkspaceGitDelta::Deleted, DomainGitChangeKind::Deleted),
            (WorkspaceGitDelta::Renamed, DomainGitChangeKind::Renamed),
            (WorkspaceGitDelta::Copied, DomainGitChangeKind::Copied),
            (
                WorkspaceGitDelta::TypeChanged,
                DomainGitChangeKind::TypeChanged,
            ),
            (WorkspaceGitDelta::Unmerged, DomainGitChangeKind::Conflicted),
        ] {
            assert_eq!(
                crate::app_session::session_git_change_kind(delta, WorkspaceGitDelta::None, false),
                expected
            );
            assert_eq!(
                crate::app_session::session_git_change_kind(WorkspaceGitDelta::None, delta, false),
                expected
            );
            assert_eq!(
                crate::app_session::session_git_change_kind(delta, WorkspaceGitDelta::None, true),
                DomainGitChangeKind::Conflicted
            );
        }
    }
}
