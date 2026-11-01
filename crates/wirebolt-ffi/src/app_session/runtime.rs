//! Typed runtime application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{
    AppDocumentSession, AppSecretPlatform, AppSessionSecrets, Arc, DomainEnvironment,
    DomainOAuthConfiguration, DomainProxy, DomainRequestDraft, DomainRunInput, DomainTransport,
    DomainValueSource, HashMap, Mutex, RequestActivity, ResponseHeaderValue, RunObserver,
    RuntimeAuthorizer, RuntimeContext, RuntimeInput, RuntimeOAuthSnapshot, RuntimeObserver,
    RuntimeOperationError, RuntimeProxyScope, RuntimeRequestError, RuntimeRequestKey, RuntimeRuns,
    RuntimeSendAction, RuntimeSettingsPlatform, RuntimeSnapshot, RuntimeState, RuntimeTokenReceipt,
    RuntimeTransport, RuntimeTransportSettings, RuntimeTransportSnapshot, SessionFailure,
    SessionHistoryEntry, SessionKind, SessionRunEvent, SessionSocketStatus,
    SessionWorkspaceCommand, StoredCookie, WorkspaceBridge, app_runtime, lock,
};
#[uniffi::remote(Record)]
pub struct RuntimeContext {
    pub transport: DomainTransport,
    pub workspace_proxy: Option<DomainProxy>,
    pub app_proxy: DomainProxy,
    pub variables: HashMap<String, DomainValueSource>,
}

#[uniffi::remote(Record)]
pub struct RuntimeSnapshot {
    pub history_entries: Vec<SessionHistoryEntry>,
    pub history_revision: u64,
    pub request_activity: HashMap<String, RequestActivity>,
    pub cookie_revision: u64,
}

#[uniffi::remote(Error)]
pub enum RuntimeOperationError {
    Unsupported,
    Validation { reason: String },
    Platform { reason: String },
    Workspace { reason: String },
}

#[uniffi::remote(Record)]
pub struct RuntimeTransportSnapshot {
    pub value: DomainTransport,
    pub failed: bool,
}

#[uniffi::remote(Record)]
pub struct RuntimeTokenReceipt {
    pub expires_at: Option<f64>,
    pub scope: Option<String>,
}

pub(super) struct RuntimeObserverAdapter(pub(super) Arc<dyn SessionRuntimeObserver>);

#[async_trait::async_trait]
impl RuntimeObserver for RuntimeObserverAdapter {
    fn changed(&self) {
        self.0.changed();
    }
    async fn prepare(&self) -> Result<Option<app_runtime::RuntimeInput>, ()> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0.prepare(Arc::new(SessionInputReply {
            sender: Mutex::new(Some(sender)),
        }));
        receiver.await.map_err(|_| ())?.map(Some).ok_or(())
    }
}

type ValuesReplySender = tokio::sync::oneshot::Sender<Result<Vec<String>, SessionFailure>>;

#[derive(Debug, uniffi::Object)]
pub struct SessionValuesReply {
    pub(super) sender: Mutex<Option<ValuesReplySender>>,
}

#[uniffi::export]
impl SessionValuesReply {
    pub fn finish(&self, values: Vec<String>, failure: Option<SessionFailure>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(failure.map_or(Ok(values), Err));
        }
    }
}

#[derive(uniffi::Object)]
pub struct AppRuntimeTransport {
    pub(super) foreign: Option<Arc<dyn SessionRuntimeTransport>>,
}

impl std::fmt::Debug for AppRuntimeTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppRuntimeTransport")
            .finish_non_exhaustive()
    }
}

#[uniffi::export]
impl AppRuntimeTransport {
    #[uniffi::constructor]
    pub fn native() -> Arc<Self> {
        Arc::new(Self { foreign: None })
    }
    #[uniffi::constructor]
    pub fn foreign(platform: Arc<dyn SessionRuntimeTransport>) -> Arc<Self> {
        Arc::new(Self {
            foreign: Some(platform),
        })
    }
}

#[cfg(target_vendor = "apple")]
pub(crate) fn runtime_secrets() -> impl wirebolt_core::SecretResolver + Clone {
    wirebolt_core::KeychainSecretResolver::default()
}

#[cfg(not(target_vendor = "apple"))]
pub(crate) fn runtime_secrets() -> impl wirebolt_core::SecretResolver + Clone {
    wirebolt_core::NoSecrets
}

#[async_trait::async_trait]
impl RuntimeTransport for AppRuntimeTransport {
    fn cancel(&self, id: &str) {
        if let Some(foreign) = &self.foreign {
            foreign.cancel(id.into());
        }
    }
    async fn resolve(
        &self,
        values: Vec<DomainValueSource>,
        variables: HashMap<String, DomainValueSource>,
    ) -> Result<Vec<String>, SessionFailure> {
        if let Some(foreign) = &self.foreign {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            foreign.resolve(
                values,
                variables,
                Arc::new(SessionValuesReply {
                    sender: Mutex::new(Some(sender)),
                }),
            );
            receiver
                .await
                .unwrap_or_else(|_| Err(SessionFailure::new("bridge")))
        } else {
            tokio::task::spawn_blocking(move || {
                app_runtime::resolve_values(values, variables, &runtime_secrets())
            })
            .await
            .unwrap_or_else(|_| Err(SessionFailure::new("invalid_request")))
        }
    }
    async fn execute(
        &self,
        input: DomainRunInput,
        id: String,
        cancellation: wirebolt_core::RunCancellation,
        observer: Arc<RunObserver>,
    ) {
        if let Some(foreign) = &self.foreign {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            foreign.execute(
                input,
                id,
                Arc::new(SessionRunSink {
                    observer,
                    sender: Mutex::new(Some(sender)),
                }),
            );
            let _ = receiver.await;
        } else {
            match app_runtime::transport_input(input) {
                Ok(input) => {
                    wirebolt_app::session::transport::execute(
                        input,
                        cancellation,
                        observer,
                        runtime_secrets(),
                    )
                    .await;
                }
                Err(failure) => observer.finish(Some(failure)),
            }
        }
    }
}

#[derive(Debug, uniffi::Object)]
pub struct AppSessionRuntime {
    pub(super) state: Arc<RuntimeState>,
    pub(super) runs: RuntimeRuns,
    pub(super) transport: RuntimeTransportSettings,
    pub(super) oauth: app_runtime::RuntimeOAuth,
}

#[uniffi::export]
impl AppSessionRuntime {
    #[uniffi::constructor]
    pub fn new(
        history: Arc<crate::app_response::AppRunHistory>,
        cookies: Arc<crate::app_response::AppCookieJar>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(RuntimeState::new(
                history.inner.clone(),
                cookies.inner.clone(),
            )),
            runs: RuntimeRuns::default(),
            transport: RuntimeTransportSettings::default(),
            oauth: app_runtime::RuntimeOAuth::default(),
        })
    }
    pub fn cookie_revision(&self) -> u64 {
        self.state.cookie_revision()
    }
    pub fn snapshot(&self) -> RuntimeSnapshot {
        self.state.snapshot()
    }
    pub fn select_environment(&self, id: Option<String>) {
        self.state.select_environment(id);
    }
    pub fn active_variables(
        &self,
        environments: Vec<DomainEnvironment>,
    ) -> HashMap<String, DomainValueSource> {
        self.state.active_variables(&environments)
    }
    pub fn configure_cookies(&self, workspace: Option<String>, directory: Option<String>) {
        self.state
            .configure_cookies(workspace.as_deref(), directory.as_deref());
    }
    pub fn cookies(&self) -> Vec<StoredCookie> {
        self.state.cookies()
    }
    pub fn delete_cookie(&self, id: String) {
        self.state.delete_cookie(&id);
    }
    pub fn clear_cookies(&self) {
        self.state.clear_cookies();
    }
    pub fn reset_workspace(&self) {
        self.state.reset_workspace();
        self.transport.reset();
        self.oauth.reset();
    }
    pub fn cancel(&self, document: Arc<AppDocumentSession>) {
        let id = lock(&document.document).id.clone();
        self.runs.cancel(&id, &document.run);
    }
    pub fn reset_transport(&self) {
        self.transport.reset();
    }
    pub fn update_transport(&self, previous: DomainTransport, value: DomainTransport) -> u64 {
        self.transport.update(previous, value)
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AppSessionRuntime {
    pub async fn send(
        &self,
        document: Arc<AppDocumentSession>,
        options: RuntimeRunOptions,
        transport: Arc<AppRuntimeTransport>,
        observer: Arc<dyn SessionRuntimeObserver>,
        secrets: Arc<AppSessionSecrets>,
        platform: Option<Arc<AppSecretPlatform>>,
    ) -> Option<SessionFailure> {
        self.runs
            .send(app_runtime::RuntimeSend {
                state: self.state.clone(),
                document: document.document.clone(),
                run: document.run.clone(),
                context: options.context,
                id: options.id,
                directory: options.directory.into(),
                transport,
                observer: Arc::new(RuntimeObserverAdapter(observer)),
                credentials: app_runtime::RuntimeCredentials {
                    secrets: secrets.inner.clone(),
                    platform: platform.map(|p| {
                        p as Arc<dyn wirebolt_app::session::secrets::SessionSecretPlatform>
                    }),
                },
            })
            .await
    }
    pub async fn history_entries(&self, id: String) -> Vec<SessionHistoryEntry> {
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || state.history_entries(&id))
            .await
            .unwrap_or_default()
    }
    pub async fn history_count(&self, id: String) -> u64 {
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || state.history_count(&id))
            .await
            .unwrap_or_default()
    }
    pub async fn load_history(&self, id: String) {
        let state = self.state.clone();
        let _ = tokio::task::spawn_blocking(move || state.load_history(&id)).await;
    }
    pub async fn clear_history(&self, id: String) {
        let state = self.state.clone();
        let _ = tokio::task::spawn_blocking(move || state.clear_history(&id)).await;
    }
    pub async fn restore_history(
        &self,
        entry: SessionHistoryEntry,
        document: Arc<AppDocumentSession>,
    ) {
        let state = self.state.clone();
        let _ = tokio::task::spawn_blocking(move || state.restore(entry, &document.run)).await;
    }
    pub async fn restore_latest(
        &self,
        document: Arc<AppDocumentSession>,
        observer: Arc<dyn SessionRuntimeObserver>,
    ) {
        let id = lock(&document.document).request_id.clone();
        self.state
            .clone()
            .restore_latest(id, document.run.clone(), &RuntimeObserverAdapter(observer))
            .await;
    }
    pub async fn curl_command(
        &self,
        draft: DomainRequestDraft,
        context: RuntimeContext,
        transport: Arc<AppRuntimeTransport>,
    ) -> Result<String, RuntimeRequestError> {
        app_runtime::curl_command(draft, context, transport.as_ref())
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug, uniffi::Object)]
pub struct SessionOperationReply {
    pub(super) sender: Mutex<Option<OperationReplySender>>,
}

#[uniffi::export]
impl SessionOperationReply {
    pub fn finish(&self, reason: Option<String>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(reason.map_or(Ok(()), |reason| {
                Err(RuntimeOperationError::Platform { reason })
            }));
        }
    }
}

struct RuntimeSettingsAdapter(Arc<dyn SessionRuntimeSettings>);

#[async_trait::async_trait]
impl RuntimeSettingsPlatform for RuntimeSettingsAdapter {
    async fn proxy(
        &self,
        scope: RuntimeProxyScope,
        configuration: Option<DomainProxy>,
    ) -> Result<(), RuntimeOperationError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0.proxy(
            scope,
            configuration,
            Arc::new(SessionOperationReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported))
    }
    async fn transport(&self, value: DomainTransport) -> Result<(), RuntimeOperationError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0.transport(
            value,
            Arc::new(SessionOperationReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported))
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AppSessionRuntime {
    pub async fn apply_proxy(
        &self,
        configuration: Option<DomainProxy>,
        scope: RuntimeProxyScope,
        has_session: bool,
        secrets: HashMap<String, String>,
        secret_platform: Option<Arc<AppSecretPlatform>>,
        platform: Arc<AppRuntimeSettings>,
    ) -> Result<(), RuntimeOperationError> {
        app_runtime::apply_proxy(
            configuration,
            scope,
            has_session,
            secrets,
            secret_platform
                .as_deref()
                .map(|s| s as &dyn wirebolt_app::session::secrets::SessionSecretPlatform),
            platform.as_ref(),
        )
        .await
    }
    pub async fn save_pending_transport(
        &self,
        generation: u64,
        platform: Arc<AppRuntimeSettings>,
    ) -> Option<RuntimeTransportSnapshot> {
        self.transport.save_pending(generation, platform).await
    }
}

#[uniffi::export]
impl SessionTokenReply {
    pub fn finish(&self, token: String, receipt: RuntimeTokenReceipt, reason: Option<String>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(reason.map_or(Ok((token, receipt)), |reason| {
                Err(RuntimeOperationError::Platform { reason })
            }));
        }
    }
}

struct RuntimeAuthorizerAdapter(Arc<dyn SessionRuntimeAuthorizer>);

#[async_trait::async_trait]
impl RuntimeAuthorizer for RuntimeAuthorizerAdapter {
    async fn acquire(
        &self,
        configuration: DomainOAuthConfiguration,
    ) -> Result<(String, RuntimeTokenReceipt), RuntimeOperationError> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.0.acquire(
            configuration,
            Arc::new(SessionTokenReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported))
    }
}

#[uniffi::export]
pub fn session_runtime_input(draft: DomainRequestDraft, context: RuntimeContext) -> DomainRunInput {
    context.input(draft)
}

#[uniffi::export(async_runtime = "tokio")]
impl AppSessionRuntime {
    pub async fn save_transport(
        &self,
        value: DomainTransport,
        platform: Arc<AppRuntimeSettings>,
    ) -> Result<(), RuntimeOperationError> {
        platform.transport(value).await
    }
}

#[uniffi::remote(Enum)]
pub enum RuntimeSendAction {
    Http,
    SocketConnect,
    SocketSend,
    Ignore,
}

#[uniffi::export]
pub fn session_send_action(
    kind: SessionKind,
    socket: SessionSocketStatus,
    running: bool,
) -> RuntimeSendAction {
    app_runtime::send_action(&kind, &socket, running)
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeRunOptions {
    pub context: RuntimeContext,
    pub id: String,
    pub directory: String,
}

#[uniffi::remote(Record)]
pub struct RuntimeInput {
    pub draft: DomainRequestDraft,
    pub context: RuntimeContext,
}

#[derive(Debug, uniffi::Object)]
pub struct SessionInputReply {
    pub(super) sender: Mutex<Option<tokio::sync::oneshot::Sender<Option<RuntimeInput>>>>,
}

#[uniffi::export]
impl SessionInputReply {
    pub fn finish(&self, input: Option<RuntimeInput>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(input);
        }
    }
}

#[uniffi::remote(Error)]
pub enum RuntimeRequestError {
    Failure { failure: SessionFailure },
}

#[uniffi::export(async_runtime = "tokio")]
impl AppRuntimeTransport {
    pub async fn resolve_values(
        &self,
        values: Vec<DomainValueSource>,
        variables: HashMap<String, DomainValueSource>,
    ) -> Result<Vec<String>, RuntimeRequestError> {
        RuntimeTransport::resolve(self, values, variables)
            .await
            .map_err(Into::into)
    }
}

/// Native workspaces execute directly; only app preferences and injected test stores use the foreign relay.
#[derive(uniffi::Object)]
pub struct AppRuntimeSettings {
    pub(super) workspace: Option<Arc<WorkspaceBridge>>,
    pub(super) foreign: Arc<dyn SessionRuntimeSettings>,
}

impl std::fmt::Debug for AppRuntimeSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppRuntimeSettings").finish_non_exhaustive()
    }
}

#[uniffi::export]
impl AppRuntimeSettings {
    #[uniffi::constructor]
    pub fn new(
        bridge: Option<Arc<WorkspaceBridge>>,
        platform: Arc<dyn SessionRuntimeSettings>,
    ) -> Arc<Self> {
        Arc::new(Self {
            workspace: bridge,
            foreign: platform,
        })
    }
}

#[async_trait::async_trait]
impl RuntimeSettingsPlatform for AppRuntimeSettings {
    async fn proxy(
        &self,
        scope: RuntimeProxyScope,
        configuration: Option<DomainProxy>,
    ) -> Result<(), RuntimeOperationError> {
        if scope == RuntimeProxyScope::Request {
            return Ok(());
        }
        if scope == RuntimeProxyScope::Workspace
            && let Some(workspace) = &self.workspace
        {
            let workspace = workspace.inner.clone();
            return tokio::task::spawn_blocking(move || {
                workspace
                    .apply_session_command(SessionWorkspaceCommand::SaveWorkspaceProxy {
                        proxy: configuration,
                    })
                    .map(|_| ())
                    .map_err(|error| RuntimeOperationError::Workspace {
                        reason: error.to_string(),
                    })
            })
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported));
        }
        RuntimeSettingsAdapter(self.foreign.clone())
            .proxy(scope, configuration)
            .await
    }
    async fn transport(&self, value: DomainTransport) -> Result<(), RuntimeOperationError> {
        if let Some(workspace) = &self.workspace {
            let workspace = workspace.inner.clone();
            return tokio::task::spawn_blocking(move || {
                workspace
                    .apply_session_command(SessionWorkspaceCommand::SaveWorkspaceSettings {
                        transport: value,
                    })
                    .map(|_| ())
                    .map_err(|error| RuntimeOperationError::Workspace {
                        reason: error.to_string(),
                    })
            })
            .await
            .unwrap_or(Err(RuntimeOperationError::Unsupported));
        }
        RuntimeSettingsAdapter(self.foreign.clone())
            .transport(value)
            .await
    }
}

#[uniffi::remote(Record)]
pub struct RuntimeRequestKey {
    pub collection_id: String,
    pub request_id: String,
}

#[uniffi::export]
pub fn session_active_request_key(
    collection_id: Option<String>,
    request_id: Option<String>,
) -> Option<RuntimeRequestKey> {
    app_runtime::active_request_key(collection_id, request_id)
}

#[uniffi::remote(Record)]
pub struct RuntimeOAuthSnapshot {
    pub busy: bool,
    pub failure: Option<RuntimeOperationError>,
    pub receipts: HashMap<String, RuntimeTokenReceipt>,
}

#[uniffi::export]
impl AppSessionRuntime {
    pub fn oauth_snapshot(&self) -> RuntimeOAuthSnapshot {
        self.oauth.snapshot()
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AppSessionRuntime {
    pub async fn acquire_token(
        &self,
        document: Arc<AppDocumentSession>,
        secrets: Arc<AppSessionSecrets>,
        platform: Arc<AppSecretPlatform>,
        authorizer: Arc<dyn SessionRuntimeAuthorizer>,
        observer: Arc<dyn SessionRuntimeObserver>,
    ) {
        self.oauth
            .acquire(
                &secrets.inner,
                &document.document,
                platform.as_ref(),
                &RuntimeAuthorizerAdapter(authorizer),
                &RuntimeObserverAdapter(observer),
            )
            .await;
    }
}

#[uniffi::remote(Enum)]
pub enum RuntimeProxyScope {
    App,
    Workspace,
    Request,
}

#[uniffi::remote(Record)]
pub struct RequestActivity {
    pub date: f64,
    pub status: Option<u16>,
}

pub(super) type OperationReplySender =
    tokio::sync::oneshot::Sender<Result<(), RuntimeOperationError>>;

type TokenReplySender =
    tokio::sync::oneshot::Sender<Result<(String, RuntimeTokenReceipt), RuntimeOperationError>>;
#[derive(Debug, uniffi::Object)]
pub struct SessionTokenReply {
    pub(super) sender: Mutex<Option<TokenReplySender>>,
}

#[uniffi::export(with_foreign)]
pub trait SessionRuntimeObserver: Send + Sync {
    fn changed(&self);
    fn prepare(&self, reply: Arc<SessionInputReply>);
}

#[derive(Debug, uniffi::Object)]
pub struct SessionRunSink {
    pub(super) observer: Arc<RunObserver>,
    pub(super) sender: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[uniffi::export]
impl SessionRunSink {
    pub fn abandon(&self) {
        self.observer.abandon();
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(());
        }
    }
    pub fn event(&self, event: SessionRunEvent) {
        self.observer.event(event);
    }
    pub fn bytes(&self, data: Vec<u8>) {
        self.observer.bytes(&data);
    }
    pub fn cookies(&self, url: String, headers: Vec<ResponseHeaderValue>) {
        self.observer.store_cookies(&url, &headers);
    }
    pub fn finish(&self, failure: Option<SessionFailure>) {
        self.observer.finish(failure);
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(());
        }
    }
}

#[uniffi::export(with_foreign)]
pub trait SessionRuntimeTransport: Send + Sync {
    fn resolve(
        &self,
        values: Vec<DomainValueSource>,
        variables: HashMap<String, DomainValueSource>,
        reply: Arc<SessionValuesReply>,
    );
    fn execute(&self, input: DomainRunInput, id: String, sink: Arc<SessionRunSink>);
    fn cancel(&self, id: String);
}

#[uniffi::export(with_foreign)]
pub trait SessionRuntimeSettings: Send + Sync {
    fn proxy(
        &self,
        scope: RuntimeProxyScope,
        configuration: Option<DomainProxy>,
        reply: Arc<SessionOperationReply>,
    );
    fn transport(&self, value: DomainTransport, reply: Arc<SessionOperationReply>);
}

#[uniffi::export(with_foreign)]
pub trait SessionRuntimeAuthorizer: Send + Sync {
    fn acquire(&self, configuration: DomainOAuthConfiguration, reply: Arc<SessionTokenReply>);
}
