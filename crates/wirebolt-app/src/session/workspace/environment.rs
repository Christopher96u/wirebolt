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
    Move { offset: i64 },
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

pub(super) fn restore(
    workspace: &DomainWorkspace,
    environment: DomainEnvironment,
    name: &str,
) -> NamedEdit {
    NamedEdit {
        edit: SessionWorkspaceEdit::RestoreEnvironment {
            index: workspace
                .environments
                .iter()
                .position(|e| e.id == environment.id)
                .unwrap_or(workspace.environments.len()) as u64,
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
        .position(|e| e.id == "global")
        .map_or_else(
            || DomainEnvironment {
                id: "global".into(),
                name: "Global Environment".into(),
                ..Default::default()
            },
            |index| environments.remove(index),
        );
    environments.insert(0, global);
    environments
}

#[must_use]
pub fn can_move(environments: &[DomainEnvironment], id: &str, offset: i64) -> bool {
    if id == "global" {
        return false;
    }
    let ids: Vec<_> = environments
        .iter()
        .filter(|e| e.id != "global")
        .map(|e| e.id.as_str())
        .collect();
    let Some(index) = ids.iter().position(|candidate| *candidate == id) else {
        return false;
    };
    let Ok(offset) = isize::try_from(offset) else {
        return false;
    };
    index
        .checked_add_signed(offset)
        .is_some_and(|target| target < ids.len() && target != index)
}
