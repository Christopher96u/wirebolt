use crate::domain::{
    DomainEnvironment, DomainProxy, DomainRequestDraft, DomainRunInput, DomainTransport,
    DomainValueSource, request, variables,
};
use crate::session::{run::SessionFailure, transport};
use std::collections::HashMap;
use wirebolt_core::{DocumentId, Environment, RequestPipeline, SecretResolver, ValueSource};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeContext {
    pub transport: DomainTransport,
    pub workspace_proxy: Option<DomainProxy>,
    pub app_proxy: DomainProxy,
    pub variables: HashMap<String, DomainValueSource>,
}
#[must_use]
pub fn active_variables(
    environments: &[DomainEnvironment],
    selected: Option<&str>,
) -> HashMap<String, DomainValueSource> {
    let mut values = HashMap::new();
    for environment in [
        environments.iter().find(|e| e.id == "global"),
        environments
            .iter()
            .find(|e| Some(e.id.as_str()) == selected),
    ]
    .into_iter()
    .flatten()
    {
        values.extend(variables::enabled_values(environment));
    }
    values
}
impl RuntimeContext {
    #[must_use]
    pub fn draft(&self, mut draft: DomainRequestDraft) -> DomainRequestDraft {
        if draft.inherits_workspace_transport {
            draft.transport.clone_from(&self.transport);
        }
        draft
    }
    #[must_use]
    pub fn input(&self, draft: DomainRequestDraft) -> DomainRunInput {
        request::run_input(
            self.draft(draft),
            self.variables.clone(),
            self.workspace_proxy.clone(),
            Some(self.app_proxy.clone()),
        )
    }
}
/// # Errors
/// Returns a redacted invalid-input failure.
pub fn transport_input(input: DomainRunInput) -> Result<transport::RunInput, SessionFailure> {
    serde_json::to_value(input)
        .and_then(serde_json::from_value)
        .map_err(|_| SessionFailure::new("invalid_request"))
}
/// # Errors
/// Returns the same redacted template issues as request preparation.
#[expect(
    clippy::implicit_hasher,
    reason = "Typed application commands use the standard hasher."
)]
pub fn resolve_values(
    values: Vec<DomainValueSource>,
    variables: HashMap<String, DomainValueSource>,
    secrets: &dyn SecretResolver,
) -> Result<Vec<String>, SessionFailure> {
    let invalid = || SessionFailure::new("invalid_request");
    let values: Vec<ValueSource> = serde_json::to_value(values)
        .and_then(serde_json::from_value)
        .map_err(|_| invalid())?;
    let variables = serde_json::to_value(variables)
        .and_then(serde_json::from_value)
        .map_err(|_| invalid())?;
    let environment = Environment::new(
        DocumentId::new("active").map_err(|_| invalid())?,
        "Active".into(),
        variables,
    );
    RequestPipeline::new(Some(&environment), secrets)
        .resolve_values(&values)
        .map_err(|error| SessionFailure {
            kind: "invalid_request".into(),
            issues: error
                .issues
                .into_iter()
                .map(|issue| crate::session::run::SessionIssue {
                    path: issue.path,
                    kind: transport::request_issue_kind(issue.kind).into(),
                    reference: issue.reference,
                })
                .collect(),
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeSendAction {
    Http,
    SocketConnect,
    SocketSend,
    Ignore,
}
#[must_use]
pub fn send_action(
    kind: &crate::session::tabs::SessionKind,
    socket: &crate::session::websocket::SessionSocketStatus,
    running: bool,
) -> RuntimeSendAction {
    if *kind == crate::session::tabs::SessionKind::WebSocket {
        if *socket == crate::session::websocket::SessionSocketStatus::Connected {
            RuntimeSendAction::SocketSend
        } else {
            RuntimeSendAction::SocketConnect
        }
    } else if running {
        RuntimeSendAction::Ignore
    } else {
        RuntimeSendAction::Http
    }
}
/// # Errors
/// Returns a credential error before constructing a connection input.
pub async fn socket_input(
    draft: DomainRequestDraft,
    context: RuntimeContext,
    credentials: super::RuntimeCredentials,
    observer: Option<&dyn super::RuntimeObserver>,
) -> Result<DomainRunInput, crate::session::secrets::SecretAccessError> {
    credentials.flush().await?;
    if let Some(observer) = observer
        && let Some(input) = observer
            .prepare()
            .await
            .map_err(|()| crate::session::secrets::SecretAccessError)?
    {
        return Ok(input.context.input(input.draft));
    }
    Ok(context.input(draft))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeRequestError {
    Failure { failure: SessionFailure },
}
impl std::fmt::Display for RuntimeRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failure { failure } => f.write_str(&failure.kind),
        }
    }
}
impl std::error::Error for RuntimeRequestError {}
impl From<SessionFailure> for RuntimeRequestError {
    fn from(failure: SessionFailure) -> Self {
        Self::Failure { failure }
    }
}
