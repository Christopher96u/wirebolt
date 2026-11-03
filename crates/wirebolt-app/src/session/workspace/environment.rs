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
#[derive(Clone)]
pub struct EnvironmentInspector {
    pub editing_id: String,
    drafts: Vec<EnvironmentFieldDraft>,
    notice: Option<String>,
}
impl std::fmt::Debug for EnvironmentInspector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvironmentInspector")
            .field("editing_id", &self.editing_id)
            .field("drafts", &self.drafts)
            .field("has_notice", &self.notice.is_some())
            .finish()
    }
}
fn available_active_id(environments: &[DomainEnvironment], active: Option<String>) -> String {
    active
        .filter(|id| environments.iter().any(|e| e.id == *id))
        .unwrap_or_else(|| wirebolt_core::GLOBAL_ENVIRONMENT_ID.into())
}
impl EnvironmentInspector {
    #[must_use]
    pub fn new(active: Option<String>) -> Self {
        Self {
            editing_id: active.unwrap_or_else(|| wirebolt_core::GLOBAL_ENVIRONMENT_ID.into()),
            drafts: Vec::new(),
            notice: None,
        }
    }
    pub fn browse_active(&mut self, environments: Vec<DomainEnvironment>, active: Option<String>) {
        self.editing_id = available_active_id(&inspector_environments(environments), active);
    }
    pub fn browse(&mut self, id: String) {
        self.editing_id = id;
    }
    #[must_use]
    pub fn use_for_requests(&self) -> Option<String> {
        (self.editing_id != wirebolt_core::GLOBAL_ENVIRONMENT_ID).then(|| self.editing_id.clone())
    }
}

/// An unfinished native field, identified independently of its current row index.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EnvironmentField {
    Name { environment_id: String },
    Key { environment_id: String, id: String },
    Value { environment_id: String, id: String },
}
#[derive(Clone, PartialEq, Eq)]
pub struct EnvironmentFieldDraft {
    pub field: EnvironmentField,
    pub text: String,
}
// Draft values may be secrets; diagnostics must never print their contents.
impl std::fmt::Debug for EnvironmentFieldDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvironmentFieldDraft")
            .field("field", &self.field)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Debug)]
pub struct EnvironmentFieldCommit {
    pub environment_id: String,
    pub commit: EnvironmentCommit,
}
impl EnvironmentField {
    fn environment_id(&self) -> &str {
        match self {
            Self::Name { environment_id }
            | Self::Key { environment_id, .. }
            | Self::Value { environment_id, .. } => environment_id,
        }
    }
    #[must_use]
    pub fn commit(&self, text: String) -> EnvironmentFieldCommit {
        let commit = match self {
            Self::Name { .. } => EnvironmentCommit::Rename { name: text },
            Self::Key { id, .. } => EnvironmentCommit::VariableName {
                id: id.clone(),
                name: text,
            },
            Self::Value { id, .. } => EnvironmentCommit::VariableValue {
                id: id.clone(),
                value: text,
            },
        };
        EnvironmentFieldCommit {
            environment_id: self.environment_id().into(),
            commit,
        }
    }
    fn exists(&self, environments: &[DomainEnvironment]) -> bool {
        environments
            .iter()
            .find(|e| e.id == self.environment_id())
            .is_some_and(|e| match self {
                Self::Name { .. } => true,
                Self::Key { id, .. } | Self::Value { id, .. } => {
                    e.variables.iter().any(|v| v.id == *id)
                }
            })
    }
}
impl EnvironmentInspector {
    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }
    #[must_use]
    pub fn drafts(&self) -> Vec<EnvironmentFieldDraft> {
        self.drafts.clone()
    }
    pub fn edit(&mut self, field: EnvironmentField, text: String) {
        if let Some(draft) = self.drafts.iter_mut().find(|d| d.field == field) {
            draft.text = text;
        } else {
            self.drafts.push(EnvironmentFieldDraft { field, text });
        }
    }
    pub fn committed(&mut self, field: &EnvironmentField, text: &str) {
        self.drafts.retain(|d| d.field != *field || d.text != text);
    }
    /// Orphaned buffers cannot be committed and never block navigation. The returned
    /// notice is for the local UI only and includes all discarded text for copying.
    pub fn reconcile(
        &mut self,
        environments: Vec<DomainEnvironment>,
        active: Option<String>,
    ) -> Option<String> {
        let environments = inspector_environments(environments);
        let mut notices = Vec::new();
        self.drafts.retain(|draft| {
            if draft.field.exists(&environments) {
                return true;
            }
            let removed = if environments
                .iter()
                .any(|e| e.id == draft.field.environment_id())
            {
                "variable"
            } else {
                "environment"
            };
            notices.push(format!(
                "Discarded an unfinished edit to “{}” because its {removed} was removed.",
                draft.text
            ));
            false
        });
        if !environments.iter().any(|e| e.id == self.editing_id) {
            self.editing_id = available_active_id(&environments, active);
        }
        if notices.is_empty() {
            return None;
        }
        let discarded = notices.join("\n");
        self.notice = Some(self.notice.take().map_or_else(
            || discarded.clone(),
            |previous| format!("{previous}\n{discarded}"),
        ));
        self.notice.clone()
    }
}
#[must_use]
pub fn environment_use_label(id: &str) -> String {
    if id == wirebolt_core::GLOBAL_ENVIRONMENT_ID {
        "Use Globals Only"
    } else {
        "Use for Requests"
    }
    .into()
}
