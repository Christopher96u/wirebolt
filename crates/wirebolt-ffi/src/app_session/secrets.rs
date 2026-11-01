//! Typed secrets application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{
    Arc, DomainRequestDraft, DomainRunInput, DomainValueSource, DomainWorkspace, HashMap, Mutex,
    RuntimeContext, RuntimeObserverAdapter, SessionDocument, SessionRuntimeObserver,
    WorkspaceBridge, app_runtime, lock,
};
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum SessionSecretOperation {
    Read,
    Save,
    Remove,
}

type SecretReplySender = tokio::sync::oneshot::Sender<Result<Option<String>, ()>>;

#[derive(Debug, uniffi::Object)]
pub struct SessionSecretReply {
    pub(super) sender: Mutex<Option<SecretReplySender>>,
}

#[uniffi::export]
impl SessionSecretReply {
    pub fn finish(&self, value: Option<String>, failed: bool) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(if failed { Err(()) } else { Ok(value) });
        }
    }
}

#[uniffi::export(with_foreign)]
pub trait SessionSecretPlatform: Send + Sync {
    fn execute(
        &self,
        operation: SessionSecretOperation,
        name: String,
        value: Option<String>,
        reply: Arc<SessionSecretReply>,
    );
}

#[derive(uniffi::Object)]
pub struct AppSecretPlatform {
    pub(super) workspace: Option<Arc<WorkspaceBridge>>,
    pub(super) foreign: Option<Arc<dyn SessionSecretPlatform>>,
}

impl std::fmt::Debug for AppSecretPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppSecretPlatform").finish_non_exhaustive()
    }
}

#[uniffi::export]
impl AppSecretPlatform {
    #[uniffi::constructor]
    pub fn workspace(bridge: Arc<WorkspaceBridge>) -> Arc<Self> {
        Arc::new(Self {
            workspace: Some(bridge),
            foreign: None,
        })
    }
    #[uniffi::constructor]
    pub fn foreign(platform: Arc<dyn SessionSecretPlatform>) -> Arc<Self> {
        Arc::new(Self {
            workspace: None,
            foreign: Some(platform),
        })
    }
}

impl AppSecretPlatform {
    async fn execute(
        &self,
        operation: SessionSecretOperation,
        name: String,
        value: Option<String>,
    ) -> Result<Option<String>, ()> {
        if let Some(workspace) = &self.workspace {
            let workspace = workspace.inner.clone();
            return tokio::task::spawn_blocking(move || match operation {
                SessionSecretOperation::Read => workspace.read_secret(&name).map_err(|_| ()),
                SessionSecretOperation::Save => workspace
                    .save_secret(&name, value.as_deref().unwrap_or_default())
                    .map(|()| None)
                    .map_err(|_| ()),
                SessionSecretOperation::Remove => workspace
                    .delete_secret(&name)
                    .map(|()| None)
                    .map_err(|_| ()),
            })
            .await
            .map_err(|_| ())?;
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.foreign.as_ref().ok_or(())?.execute(
            operation,
            name,
            value,
            Arc::new(SessionSecretReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver.await.map_err(|_| ())?
    }
}

#[async_trait::async_trait]
impl wirebolt_app::session::secrets::SessionSecretPlatform for AppSecretPlatform {
    async fn read(&self, name: String) -> Result<Option<String>, ()> {
        self.execute(SessionSecretOperation::Read, name, None).await
    }
    async fn save(&self, name: String, value: String) -> Result<(), ()> {
        self.execute(SessionSecretOperation::Save, name, Some(value))
            .await
            .map(|_| ())
    }
    async fn remove(&self, name: String) -> Result<(), ()> {
        self.execute(SessionSecretOperation::Remove, name, None)
            .await
            .map(|_| ())
    }
}

#[derive(Debug, uniffi::Object)]
pub struct AppSessionSecrets {
    pub(super) inner: Arc<wirebolt_app::session::secrets::SessionSecrets>,
}

#[uniffi::export]
impl AppSessionSecrets {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(wirebolt_app::session::secrets::SessionSecrets::default()),
        })
    }
    pub fn edit_batch(&self, values: HashMap<String, String>) {
        self.inner.edit_batch(values);
    }
    pub fn reset_editor(&self) {
        self.inner.reset_editor();
    }
    pub fn clear_observed(&self) {
        self.inner.clear_observed();
    }
    pub fn observe(&self, names: Vec<String>) {
        self.inner.observe(names);
    }
    pub fn has_observed(&self) -> bool {
        self.inner.has_observed()
    }
}

fn secret_error(
    _: wirebolt_app::session::secrets::SecretAccessError,
) -> crate::WorkspaceBridgeError {
    crate::WorkspaceBridgeError::operation("secret storage failed")
}

#[uniffi::export(async_runtime = "tokio")]
impl AppSessionSecrets {
    pub async fn load(
        &self,
        name: String,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<String, crate::WorkspaceBridgeError> {
        self.inner
            .load(name, platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn flush(
        &self,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<(), crate::WorkspaceBridgeError> {
        self.inner
            .flush(platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn save(
        &self,
        values: HashMap<String, String>,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<(), crate::WorkspaceBridgeError> {
        self.inner
            .save(values, platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn copy(
        &self,
        renames: HashMap<String, String>,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<HashMap<String, String>, crate::WorkspaceBridgeError> {
        self.inner
            .copy(renames, platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn purge(
        &self,
        in_use: Vec<String>,
        others: Vec<String>,
        platform: Arc<AppSecretPlatform>,
    ) {
        self.inner.purge(in_use, others, platform.as_ref()).await;
    }
}

#[uniffi::export]
pub fn session_scan_secrets(candidates: Vec<String>, workspaces: Vec<String>) -> Vec<String> {
    wirebolt_app::session::secrets::scan_references(&candidates, &workspaces)
        .into_iter()
        .collect()
}

#[uniffi::export]
impl AppSessionSecrets {
    pub fn cached(&self) -> HashMap<String, String> {
        self.inner.cached()
    }
    pub fn dirty(&self) -> Vec<String> {
        self.inner.dirty()
    }
}

#[uniffi::export]
impl AppSessionSecrets {
    pub fn material(&self, source: DomainValueSource) -> String {
        self.inner.material(&source)
    }
    pub fn edit(&self, name: String, value: String) {
        self.inner.edit_batch(HashMap::from([(name, value)]));
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AppSessionSecrets {
    pub async fn migrate(
        &self,
        draft: DomainRequestDraft,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<DomainRequestDraft, crate::WorkspaceBridgeError> {
        self.inner
            .migrate(draft, platform.as_ref())
            .await
            .map_err(secret_error)
    }
    pub async fn purge_workspace(
        &self,
        workspace: DomainWorkspace,
        documents: Vec<SessionDocument>,
        current: Option<String>,
        others: Vec<String>,
        platform: Arc<AppSecretPlatform>,
    ) {
        self.inner
            .purge_workspace(workspace, documents, current, others, platform.as_ref())
            .await;
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AppSessionSecrets {
    pub async fn socket_input(
        &self,
        draft: DomainRequestDraft,
        context: RuntimeContext,
        platform: Option<Arc<AppSecretPlatform>>,
        observer: Arc<dyn SessionRuntimeObserver>,
    ) -> Result<DomainRunInput, crate::WorkspaceBridgeError> {
        app_runtime::socket_input(
            draft,
            context,
            app_runtime::RuntimeCredentials {
                secrets: self.inner.clone(),
                platform: platform
                    .map(|p| p as Arc<dyn wirebolt_app::session::secrets::SessionSecretPlatform>),
            },
            Some(&RuntimeObserverAdapter(observer)),
        )
        .await
        .map_err(secret_error)
    }
    pub async fn save_material(
        &self,
        name: String,
        value: String,
        platform: Arc<AppSecretPlatform>,
    ) -> Result<(), crate::WorkspaceBridgeError> {
        wirebolt_app::session::secrets::SessionSecretPlatform::save(platform.as_ref(), name, value)
            .await
            .map_err(|()| secret_error(wirebolt_app::session::secrets::SecretAccessError))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirebolt_app::session::secrets::SessionSecretPlatform as _;
    #[derive(Default)]
    struct Platform(Mutex<Vec<SessionSecretOperation>>);
    impl SessionSecretPlatform for Platform {
        fn execute(
            &self,
            operation: SessionSecretOperation,
            _: String,
            value: Option<String>,
            reply: Arc<SessionSecretReply>,
        ) {
            lock(&self.0).push(operation);
            if matches!(operation, SessionSecretOperation::Save) {
                assert_eq!(value.as_deref(), Some("value"));
            }
            reply.finish(
                matches!(operation, SessionSecretOperation::Read).then(|| "stored".into()),
                false,
            );
        }
    }
    #[tokio::test]
    async fn foreign_secret_operations_are_exhaustive_and_keep_their_meaning() {
        let platform = Arc::new(Platform::default());
        let bridge = AppSecretPlatform::foreign(platform.clone());
        assert_eq!(
            bridge.read("name".into()).await.unwrap().as_deref(),
            Some("stored")
        );
        bridge.save("name".into(), "value".into()).await.unwrap();
        bridge.remove("name".into()).await.unwrap();
        let operations = lock(&platform.0);
        assert!(matches!(
            operations.as_slice(),
            [
                SessionSecretOperation::Read,
                SessionSecretOperation::Save,
                SessionSecretOperation::Remove
            ]
        ));
    }
}
