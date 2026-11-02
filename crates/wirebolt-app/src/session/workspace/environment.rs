//! Field commits for the live environment inspector. Inputs may contain ephemeral
//! secret material; only references enter reversible workspace plans.
use super::{SessionEnvironmentRestoreSelection, SessionWorkspaceEdit, actions::NamedEdit};
use crate::domain::{DomainEnvironment, DomainValueSource, DomainVariable, DomainWorkspace};

#[derive(Clone, PartialEq, Eq)]
pub enum EnvironmentCommit {
    Rename { name: String },
    VariableName { id: String, name: String },
    VariableValue { id: String, value: String },
    VariableEnabled { id: String, enabled: bool },
    VariableSecret { id: String, secret: bool },
    AddVariable,
    DeleteVariable { id: String },
    ClearVariables,
    Create,
    Delete,
}

pub(super) fn validate_name(
    workspace: &DomainWorkspace,
    environment: &DomainEnvironment,
) -> Result<(), String> {
    if environment.name.trim().is_empty() {
        return Err("Enter an environment name.".into());
    }
    if workspace.environments.iter().any(|other| {
        other.id != environment.id
            && other.name.trim().to_lowercase() == environment.name.trim().to_lowercase()
    }) {
        return Err("An environment already has this name.".into());
    }
    Ok(())
}

pub(super) fn validate_key(
    environment: &DomainEnvironment,
    id: &str,
    key: &str,
) -> Result<(), String> {
    if key.is_empty()
        || key
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '{' | '}'))
    {
        return Err("Use a non-empty variable name without spaces or braces.".into());
    }
    if environment
        .variables
        .iter()
        .any(|row| row.id != id && row.key == key)
    {
        return Err("A variable already has this name.".into());
    }
    Ok(())
}

pub(super) fn row_mut<'a>(
    environment: &'a mut DomainEnvironment,
    id: &str,
) -> Result<&'a mut DomainVariable, String> {
    environment
        .variables
        .iter_mut()
        .find(|row| row.id == id)
        .ok_or_else(|| "This variable no longer exists.".into())
}

pub(super) fn save_edit(environment: DomainEnvironment, name: &str) -> NamedEdit {
    NamedEdit {
        edit: SessionWorkspaceEdit::RestoreEnvironment {
            environment,
            selection: SessionEnvironmentRestoreSelection::SelectCreated,
        },
        name: name.into(),
        created_id: None,
    }
}

pub(super) fn source(value: String) -> DomainValueSource {
    DomainValueSource::Literal { value }
}

impl std::fmt::Debug for EnvironmentCommit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Commit ingress can carry a secret value. Never print input material.
        f.write_str("EnvironmentCommit(..)")
    }
}

/// Globals has one stable place even before the first global variable is saved.
#[must_use]
pub fn inspector_environments(mut environments: Vec<DomainEnvironment>) -> Vec<DomainEnvironment> {
    let global = environments
        .iter()
        .position(|e| e.id == wirebolt_core::GLOBAL_ENVIRONMENT_ID)
        .map_or_else(global_environment, |index| environments.remove(index));
    environments.sort_by(|left, right| left.id.cmp(&right.id));
    let mut global = global;
    global.name = global_environment().name;
    environments.insert(0, global);
    environments
}

#[must_use]
pub fn global_environment() -> DomainEnvironment {
    DomainEnvironment {
        id: wirebolt_core::GLOBAL_ENVIRONMENT_ID.into(),
        name: "Globals".into(),
        ..Default::default()
    }
}

/// Inspector browsing is independent of the runtime's active environment.
#[derive(Clone, Debug)]
pub struct EnvironmentInspector {
    pub editing_id: String,
}
impl EnvironmentInspector {
    #[must_use]
    pub fn new(active: Option<String>) -> Self {
        Self {
            editing_id: active.unwrap_or_else(|| wirebolt_core::GLOBAL_ENVIRONMENT_ID.into()),
        }
    }
    pub fn browse(&mut self, id: String) {
        self.editing_id = id;
    }
    #[must_use]
    pub fn use_for_requests(&self) -> Option<String> {
        (self.editing_id != wirebolt_core::GLOBAL_ENVIRONMENT_ID).then(|| self.editing_id.clone())
    }
}
