use super::{RuntimeContext, RuntimeTransport};
use crate::lock;
use crate::{
    domain::{
        DomainAuthentication, DomainProxy, DomainRequestDraft, DomainTransport, DomainValueSource,
        DomainWorkspace, curl, proxy, request,
    },
    session::{
        run::SessionFailure,
        secrets::{SecretAccessError, SessionSecretPlatform, SessionSecrets},
        tabs::SessionDocument,
    },
};
use async_trait::async_trait;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

impl SessionSecrets {
    /// # Errors
    /// Returns a credential-copy failure; the original draft stays intact.
    pub async fn migrate(
        &self,
        mut draft: DomainRequestDraft,
        platform: &dyn SessionSecretPlatform,
    ) -> Result<DomainRequestDraft, SecretAccessError> {
        let (authentication, renames) = request::own_auth(draft.authentication.clone(), true);
        self.copy(renames, platform).await?;
        draft.authentication = authentication;
        Ok(draft)
    }
    pub async fn purge_workspace(
        &self,
        workspace: DomainWorkspace,
        documents: Vec<SessionDocument>,
        current: Option<String>,
        others: Vec<String>,
        platform: &dyn SessionSecretPlatform,
    ) {
        let mut in_use: HashSet<_> = request::workspace_references(&workspace)
            .into_iter()
            .collect();
        for document in documents {
            in_use.extend(request::request_references(&document.draft));
            if let Some(saved) = document.saved_draft {
                in_use.extend(request::request_references(&saved));
            }
        }
        let others = others
            .into_iter()
            .filter(|p| Some(p) != current.as_ref())
            .collect::<HashSet<_>>();
        self.purge(
            in_use.into_iter().collect(),
            others.into_iter().collect(),
            platform,
        )
        .await;
    }
}
/// # Errors
/// Returns redacted value-resolution or export failures.
pub async fn curl_command(
    draft: DomainRequestDraft,
    context: RuntimeContext,
    transport: &dyn RuntimeTransport,
) -> Result<String, SessionFailure> {
    let draft = context.draft(draft);
    let proxy = draft
        .proxy
        .as_ref()
        .or(context.workspace_proxy.as_ref())
        .unwrap_or(&context.app_proxy);
    let sources = curl::value_sources(&draft);
    let mut names = curl::export(&draft, &context.variables, &HashSet::new()).secret_names;
    let mut to_resolve = sources.clone();
    to_resolve.extend(
        names
            .iter()
            .map(|n| DomainValueSource::Secret { secret: n.clone() }),
    );
    let values = transport
        .resolve(to_resolve, context.variables.clone())
        .await?;
    if values.len() != sources.len() + names.len() {
        return Err(SessionFailure::new(
            crate::response::ResponseFailureKind::Export,
        ));
    }
    let url = values
        .first()
        .cloned()
        .ok_or_else(|| SessionFailure::new(crate::response::ResponseFailureKind::Export))?;
    let mut secret_values = values.into_iter().skip(sources.len()).collect::<Vec<_>>();
    if let Some(credentials) = curl::proxy_route(proxy, &url).and_then(|r| r.credentials) {
        let proxy_names = [credentials.username, credentials.password];
        let values = transport
            .resolve(
                proxy_names
                    .iter()
                    .map(|n| DomainValueSource::Secret { secret: n.clone() })
                    .collect(),
                context.variables.clone(),
            )
            .await?;
        if values.len() != 2 {
            return Err(SessionFailure::new(
                crate::response::ResponseFailureKind::Export,
            ));
        }
        names.extend(proxy_names);
        secret_values.extend(values);
    }
    let hex = names
        .into_iter()
        .zip(secret_values)
        .filter_map(|(n, v)| curl::prints_as_hex(&v).then_some(n))
        .collect();
    Ok(curl::export(&draft, &context.variables, &hex).command
        + &curl::proxy_arguments(proxy, &url, &hex))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeOperationError {
    Unsupported,
    Validation { reason: String },
    Platform { reason: String },
    Workspace { reason: String },
}
impl std::fmt::Display for RuntimeOperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported operation"),
            Self::Validation { reason }
            | Self::Platform { reason }
            | Self::Workspace { reason } => f.write_str(reason),
        }
    }
}
impl std::error::Error for RuntimeOperationError {}
pub use wirebolt_core::ProxySource;
#[async_trait]
pub trait RuntimeSettingsPlatform: Send + Sync {
    async fn proxy(
        &self,
        scope: ProxySource,
        configuration: Option<DomainProxy>,
    ) -> Result<(), RuntimeOperationError>;
    async fn transport(&self, value: DomainTransport) -> Result<(), RuntimeOperationError>;
}
/// # Errors
/// Returns validation or storage failures before activating a policy.
#[expect(
    clippy::implicit_hasher,
    reason = "Typed application commands use the standard hasher."
)]
pub async fn apply_proxy(
    configuration: Option<DomainProxy>,
    scope: ProxySource,
    has_session: bool,
    secrets: HashMap<String, String>,
    secret_platform: Option<&dyn SessionSecretPlatform>,
    platform: &dyn RuntimeSettingsPlatform,
) -> Result<(), RuntimeOperationError> {
    if scope == ProxySource::SystemDefault {
        return Err(RuntimeOperationError::Unsupported);
    }
    if let Some(configuration) = &configuration {
        proxy::validate(configuration)
            .map_err(|reason| RuntimeOperationError::Validation { reason })?;
    }
    if scope == ProxySource::AppDefault && configuration.is_none() {
        return Err(RuntimeOperationError::Validation {
            reason: "Choose an app default.".into(),
        });
    }
    if scope == ProxySource::Request && !has_session {
        return Err(RuntimeOperationError::Unsupported);
    }
    for (name, value) in secrets {
        secret_platform
            .ok_or(RuntimeOperationError::Unsupported)?
            .save(name, value)
            .await
            .map_err(|()| RuntimeOperationError::Platform {
                reason: "secret storage failed".into(),
            })?;
    }
    platform.proxy(scope, configuration).await
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeTransportSnapshot {
    pub value: DomainTransport,
    pub failed: bool,
}
#[derive(Debug, Default)]
pub struct RuntimeTransportSettings {
    state: Mutex<Option<(u64, DomainTransport, DomainTransport)>>,
    serial: tokio::sync::Mutex<()>,
}
impl RuntimeTransportSettings {
    pub fn reset(&self) {
        *lock(&self.state) = None;
    }
    pub fn update(&self, previous: DomainTransport, value: DomainTransport) -> u64 {
        let mut state = lock(&self.state);
        let (generation, persisted) = state
            .as_ref()
            .map_or((0, previous), |(g, p, _)| (g + 1, p.clone()));
        *state = Some((generation, persisted, value));
        generation
    }
    pub async fn save_pending(
        &self,
        generation: u64,
        platform: Arc<dyn RuntimeSettingsPlatform>,
    ) -> Option<RuntimeTransportSnapshot> {
        let _serial = self.serial.lock().await;
        if lock(&self.state)
            .as_ref()
            .is_none_or(|(g, _, _)| *g != generation)
        {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let value = {
            let state = lock(&self.state);
            let (g, _, value) = state.as_ref()?;
            if *g != generation {
                return None;
            }
            value.clone()
        };
        let failed = platform.transport(value.clone()).await.is_err();
        let mut state = lock(&self.state);
        let (g, persisted, current) = state.as_mut()?;
        if !failed {
            persisted.clone_from(&value);
        }
        if *g == generation && failed && *current == value {
            current.clone_from(persisted);
        }
        Some(RuntimeTransportSnapshot {
            value: current.clone(),
            failed,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeTokenReceipt {
    pub expires_at: Option<f64>,
    pub scope: Option<String>,
}
#[async_trait]
pub trait RuntimeAuthorizer: Send + Sync {
    async fn acquire(
        &self,
        configuration: crate::domain::DomainOAuthConfiguration,
    ) -> Result<(String, RuntimeTokenReceipt), RuntimeOperationError>;
}
/// # Errors
/// Returns storage or native-localized authorization failures.
pub async fn acquire_token(
    secrets: &SessionSecrets,
    document: &Mutex<SessionDocument>,
    platform: &dyn SessionSecretPlatform,
    authorizer: &dyn RuntimeAuthorizer,
    observer: Option<&dyn super::RuntimeObserver>,
) -> Result<Option<RuntimeTokenReceipt>, RuntimeOperationError> {
    let keychain = |_| RuntimeOperationError::Workspace {
        reason: "secret storage failed".into(),
    };
    secrets.flush(platform).await.map_err(keychain)?;
    if let Some(observer) = observer
        && let Some(input) = observer
            .prepare()
            .await
            .map_err(|()| RuntimeOperationError::Unsupported)?
    {
        lock(document).edit(input.draft);
    }
    let draft = lock(document).draft.clone();
    if !matches!(draft.authentication, DomainAuthentication::Oauth2 { .. }) {
        return Ok(None);
    }
    let migrated = secrets.migrate(draft, platform).await.map_err(keychain)?;
    let DomainAuthentication::Oauth2 { configuration } = &migrated.authentication else {
        return Ok(None);
    };
    lock(document).edit(migrated.clone());
    let (token, receipt) = authorizer.acquire(configuration.clone()).await?;
    secrets.observe(vec![configuration.access_token_reference.clone()]);
    platform
        .save(configuration.access_token_reference.clone(), token)
        .await
        .map_err(|()| keychain(SecretAccessError))?;
    Ok(Some(receipt))
}
