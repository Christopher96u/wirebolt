use super::{
    DomainWorkspace, EnvironmentSave, HashMap, NamedEdit, SessionSecretPlatform, SessionSecrets,
    SessionWorkspaceEdit, WorkspaceMutation, WorkspaceOperations, WorkspacePlatform, actions,
};
type PreparedCommit = (NamedEdit, HashMap<String, String>);
type PreparedResult = Result<Option<PreparedCommit>, String>;
use crate::domain::{DomainEnvironment, DomainValueSource, DomainVariable};
use crate::session::workspace::environment::{self, EnvironmentCommit};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentCommitOutcome {
    pub mutation: WorkspaceMutation,
    pub problem: Option<String>,
}

impl WorkspaceOperations {
    pub async fn commit_environment(
        &self,
        id: String,
        commit: EnvironmentCommit,
        selected: Option<String>,
        platform: &dyn WorkspacePlatform,
        secrets: &SessionSecrets,
        secret_platform: &dyn SessionSecretPlatform,
    ) -> EnvironmentCommitOutcome {
        let _guard = self.edits.lock().await;
        match self
            .environment_edit(&id, commit, secrets, secret_platform)
            .await
        {
            Ok(Some((edit, values))) => {
                let mutation = if let SessionWorkspaceEdit::RestoreEnvironment {
                    environment, ..
                } = &edit.edit
                {
                    self.save_environment_locked(
                        EnvironmentSave {
                            environment: environment.clone(),
                            values,
                            name: Some(edit.name),
                        },
                        selected,
                        platform,
                        secrets,
                        secret_platform,
                    )
                    .await
                } else {
                    self.commit_named(edit, selected, platform, secrets, secret_platform)
                        .await
                };
                let problem = (!mutation.succeeded).then(|| {
                    "Could not save this change. Your text is still here; try again.".into()
                });
                EnvironmentCommitOutcome { mutation, problem }
            }
            Ok(None) => EnvironmentCommitOutcome {
                mutation: WorkspaceMutation {
                    succeeded: true,
                    ..Default::default()
                },
                problem: None,
            },
            Err(problem) => EnvironmentCommitOutcome {
                problem: Some(problem),
                ..Default::default()
            },
        }
    }

    async fn environment_edit(
        &self,
        id: &str,
        commit: EnvironmentCommit,
        secrets: &SessionSecrets,
        secret_platform: &dyn SessionSecretPlatform,
    ) -> PreparedResult {
        use EnvironmentCommit as C;
        let workspace = self.workspace.snapshot();
        if commit == C::Create {
            return Ok(Some(create_environment(&workspace)));
        }
        let mut environment = workspace
            .environments
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .or_else(|| {
                (id == "global").then(|| DomainEnvironment {
                    id: "global".into(),
                    name: "Global Environment".into(),
                    ..Default::default()
                })
            })
            .ok_or("This environment no longer exists.")?;
        let original = environment.clone();
        let mut values = HashMap::new();
        let label = match commit {
            C::Rename { name } => {
                if id == "global" {
                    return Err("Globals cannot be renamed.".into());
                }
                environment.name = name;
                environment::validate_name(&workspace, &environment)?;
                "Rename Environment"
            }
            C::VariableName { id, name } => {
                environment::validate_key(&environment, &id, &name)?;
                environment::row_mut(&mut environment, &id)?.key = name;
                "Rename Variable"
            }
            C::VariableValue { id, value } => {
                let row = environment::row_mut(&mut environment, &id)?;
                if !edit_value(row, value, &mut values, secrets, secret_platform).await? {
                    return Ok(None);
                }
                "Edit Variable"
            }
            C::VariableEnabled { id, enabled } => {
                environment::row_mut(&mut environment, &id)?.enabled = enabled;
                "Edit Variable"
            }
            C::VariableSecret { id: row_id, secret } => {
                let row = environment::row_mut(&mut environment, &row_id)?;
                if !toggle_secret(row, id, secret, &mut values, secrets, secret_platform).await? {
                    return Ok(None);
                }
                "Edit Variable Secret"
            }
            C::AddVariable => {
                add_variable(&mut environment);
                "Add Variable"
            }
            C::DeleteVariable { id } => {
                environment.variables.retain(|row| row.id != id);
                "Delete Variable"
            }
            C::ClearVariables => {
                environment.variables.clear();
                "Delete Variables"
            }
            C::Delete => {
                if id == "global" {
                    return Err("Globals cannot be deleted.".into());
                }
                return Ok(Some((
                    NamedEdit {
                        edit: SessionWorkspaceEdit::RemoveEnvironment {
                            id: id.into(),
                            is_creation: false,
                        },
                        name: "Delete Environment".into(),
                        created_id: None,
                    },
                    values,
                )));
            }
            C::Move { offset } => return move_environment(workspace, id, offset),
            C::Create => unreachable!(),
        };
        if original == environment && values.is_empty() {
            return Ok(None);
        }
        Ok(Some((
            environment::restore(&workspace, environment, label),
            values,
        )))
    }
}

fn move_environment(mut workspace: DomainWorkspace, id: &str, offset: i64) -> PreparedResult {
    if id == "global" {
        return Err("Globals stays at the top.".into());
    }
    let mut environments: Vec<_> = workspace
        .environments
        .iter()
        .filter(|e| e.id != "global")
        .cloned()
        .collect();
    let index = environments
        .iter()
        .position(|e| e.id == id)
        .ok_or("This environment no longer exists.")?;
    let signed_offset =
        isize::try_from(offset).unwrap_or(if offset < 0 { isize::MIN } else { isize::MAX });
    let destination = index
        .saturating_add_signed(signed_offset)
        .min(environments.len().saturating_sub(1));
    if destination == index {
        return Ok(None);
    }
    let moved = environments.remove(index);
    environments.insert(destination, moved);
    workspace.environments.retain(|e| e.id == "global");
    workspace.environments.extend(environments);
    Ok(Some((
        NamedEdit {
            edit: SessionWorkspaceEdit::RestoreEnvironments {
                environments: workspace.environments,
            },
            name: "Reorder Environments".into(),
            created_id: None,
        },
        HashMap::new(),
    )))
}

fn create_environment(workspace: &DomainWorkspace) -> PreparedCommit {
    let mut environment = actions::new_environment();
    let mut suffix = 1;
    while environment::validate_name(workspace, &environment).is_err() {
        suffix += 1;
        environment.name = format!("New Environment {suffix}");
    }
    environment.order = workspace
        .environments
        .iter()
        .map(|e| e.order)
        .max()
        .unwrap_or(-1)
        .saturating_add(1);
    (
        environment::restore(workspace, environment, "Add Environment"),
        HashMap::new(),
    )
}
fn add_variable(environment: &mut DomainEnvironment) {
    let mut key = "variable".to_owned();
    let mut suffix = 1;
    while environment::validate_key(environment, "", &key).is_err() {
        suffix += 1;
        key = format!("variable_{suffix}");
    }
    environment.variables.push(DomainVariable {
        id: crate::domain::identity::unique_id(),
        key,
        value: environment::source(String::new()),
        enabled: true,
        order: environment
            .variables
            .iter()
            .map(|r| r.order)
            .max()
            .unwrap_or(-1)
            .saturating_add(1),
    });
}
async fn edit_value(
    row: &mut DomainVariable,
    value: String,
    values: &mut HashMap<String, String>,
    secrets: &SessionSecrets,
    secret_platform: &dyn SessionSecretPlatform,
) -> Result<bool, String> {
    if let Some(name) = row.value.secret_name() {
        let previous = secrets
            .load(name.into(), secret_platform)
            .await
            .map_err(|_| "Could not read this Keychain value. Your text is still here.")?;
        if previous == value {
            return Ok(false);
        }
        values.insert(name.to_owned(), value);
    } else {
        row.value = environment::source(value);
    }

    Ok(true)
}
async fn toggle_secret(
    row: &mut DomainVariable,
    id: &str,
    secret: bool,
    values: &mut HashMap<String, String>,
    secrets: &SessionSecrets,
    secret_platform: &dyn SessionSecretPlatform,
) -> Result<bool, String> {
    if row.value.secret_name().is_some() == secret {
        return Ok(false);
    }
    if secret {
        let name = crate::domain::request::credential_reference(id, "env");
        values.insert(name.clone(), secrets.material(&row.value));
        row.value = DomainValueSource::Secret { secret: name };
    } else {
        let name = row
            .value
            .secret_name()
            .ok_or("Missing Keychain reference.")?;
        row.value = environment::source(
            secrets
                .load(name.into(), secret_platform)
                .await
                .map_err(|_| "Could not read this Keychain value. Try again.")?,
        );
    }

    Ok(true)
}
