//! Reversible workspace edits. Plans preserve command order and return inverses only
//! after persistence succeeds; native `UndoManager` only schedules these opaque steps.
pub mod actions;
pub mod documents;
pub mod files;
pub mod interface;
pub mod io;
pub mod lifecycle;
pub mod operations;
pub mod presentation;
use crate::domain::{
    DomainCollection, DomainEnvironment, DomainGroup, DomainLocation, DomainProxy, DomainTransport,
    DomainWorkspace,
};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "Typed edits carry the existing request record across UniFFI."
)]
pub enum SessionWorkspaceEdit {
    RestoreEnvironment {
        environment: DomainEnvironment,
        index: u64,
    },
    RemoveEnvironment {
        id: String,
    },
    RestoreRequest {
        location: DomainLocation,
    },
    RemoveRequest {
        collection_id: String,
        id: String,
    },
    DuplicateRequest {
        collection_id: String,
        id: String,
        new_id: String,
        name: String,
    },
    RestoreGroups {
        collection_id: String,
        groups: Vec<DomainGroup>,
        requests: Vec<DomainLocation>,
    },
    RemoveGroup {
        collection_id: String,
        id: String,
    },
    RestoreCollection {
        collection: DomainCollection,
    },
    RemoveCollection {
        id: String,
    },
    RenameRequest {
        collection_id: String,
        id: String,
        name: String,
    },
    RenameGroup {
        collection_id: String,
        id: String,
        name: String,
    },
    RenameCollection {
        id: String,
        name: String,
    },
    MoveRequest {
        from_collection_id: String,
        id: String,
        to_collection_id: String,
        group_id: Option<String>,
        order: i64,
    },
    MoveGroup {
        collection_id: String,
        id: String,
        parent_id: Option<String>,
        order: i64,
    },
    Reorder {
        collection_id: String,
        parent_id: Option<String>,
        items: Vec<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "Typed commands carry the existing request record across UniFFI."
)]
pub enum SessionWorkspaceCommand {
    SaveWorkspaceProxy {
        proxy: Option<DomainProxy>,
    },
    SaveWorkspaceSettings {
        transport: DomainTransport,
    },
    RenameWorkspace {
        name: String,
    },
    SaveEnvironment {
        environment: DomainEnvironment,
    },
    DeleteEnvironment {
        id: String,
    },
    SaveRequest {
        location: DomainLocation,
    },
    DeleteRequest {
        collection_id: String,
        id: String,
    },
    DuplicateRequest {
        collection_id: String,
        id: String,
        new_id: String,
        name: String,
    },
    CreateGroup {
        collection_id: String,
        group: DomainGroup,
    },
    DeleteGroup {
        collection_id: String,
        id: String,
    },
    CreateCollection {
        collection: DomainCollection,
    },
    DeleteCollection {
        id: String,
    },
    RenameGroup {
        collection_id: String,
        id: String,
        name: String,
    },
    RenameCollection {
        id: String,
        name: String,
    },
    MoveRequest {
        from_collection_id: String,
        request_id: String,
        to_collection_id: String,
        group_id: Option<String>,
        order: i64,
    },
    MoveGroup {
        collection_id: String,
        id: String,
        parent_id: Option<String>,
        order: i64,
    },
    ReorderChildren {
        collection_id: String,
        parent_id: Option<String>,
        items: Vec<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionDocumentEffect {
    Close {
        collection_id: String,
        request_id: Option<String>,
    },
    Rename {
        collection_id: String,
        request_id: String,
        name: String,
    },
    Relocate {
        collection_id: String,
        request_id: String,
        destination: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionWorkspacePlan {
    pub commands: Vec<SessionWorkspaceCommand>,
    pub inverse: SessionWorkspaceEdit,
    pub collections: Vec<DomainCollection>,
    pub removed_collection_ids: Vec<String>,
    pub environments: Option<Vec<DomainEnvironment>>,
    pub selected_environment_id: Option<String>,
    pub changes_environment_selection: bool,
    pub effects: Vec<SessionDocumentEffect>,
    pub observed_secrets: Vec<String>,
}
fn collection<'a>(workspace: &'a DomainWorkspace, id: &str) -> Option<&'a DomainCollection> {
    workspace.collections.iter().find(|c| c.id == id)
}
fn location<'a>(
    workspace: &'a DomainWorkspace,
    collection_id: &str,
    id: &str,
) -> Option<&'a DomainLocation> {
    collection(workspace, collection_id)?
        .requests
        .iter()
        .find(|r| r.request.id == id)
}
fn descendants(collection: &DomainCollection, root: &str) -> HashSet<String> {
    let mut result = HashSet::from([root.to_owned()]);
    loop {
        let before = result.len();
        for group in &collection.groups {
            if group
                .parent_id
                .as_ref()
                .is_some_and(|id| result.contains(id))
            {
                result.insert(group.id.clone());
            }
        }
        if result.len() == before {
            break;
        }
    }
    result
}
fn subtree(collection: &DomainCollection, root: Option<&str>) -> Vec<DomainGroup> {
    crate::domain::sidebar::subtree(collection, root)
        .into_iter()
        .filter_map(|i| collection.groups.get(usize::try_from(i).ok()?).cloned())
        .collect()
}
use crate::domain::sidebar::ordered_children;
/// Returns no plan for missing targets and no-op edits. The caller persists the
/// ordered commands before applying the projection delta and registering its inverse.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "Each edit keeps its persistence command and inverse together."
)]
pub fn plan_edit(
    workspace: &DomainWorkspace,
    selected: Option<String>,
    step: SessionWorkspaceEdit,
) -> Option<SessionWorkspacePlan> {
    use SessionDocumentEffect as F;
    use SessionWorkspaceCommand as C;
    use SessionWorkspaceEdit as E;
    let mut plan = SessionWorkspacePlan {
        commands: vec![],
        inverse: step.clone(),
        collections: vec![],
        removed_collection_ids: vec![],
        environments: None,
        selected_environment_id: selected,
        changes_environment_selection: false,
        effects: vec![],
        observed_secrets: vec![],
    };
    match step {
        E::RestoreEnvironment { environment, index } => {
            let previous = workspace
                .environments
                .iter()
                .position(|e| e.id == environment.id);
            let mut environments = workspace.environments.clone();
            plan.observed_secrets.extend(
                environment
                    .variables
                    .iter()
                    .filter_map(|v| v.value.secret_name().map(str::to_owned)),
            );
            if let Some(index) = previous {
                plan.observed_secrets.extend(
                    environments[index]
                        .variables
                        .iter()
                        .filter_map(|v| v.value.secret_name().map(str::to_owned)),
                );
            }
            plan.inverse = previous.map_or_else(
                || E::RemoveEnvironment {
                    id: environment.id.clone(),
                },
                |i| E::RestoreEnvironment {
                    environment: environments[i].clone(),
                    index: i as u64,
                },
            );
            environments.retain(|e| e.id != environment.id);
            environments.insert(
                usize::try_from(index)
                    .unwrap_or(usize::MAX)
                    .min(environments.len()),
                environment.clone(),
            );
            plan.selected_environment_id =
                (environment.id != "global").then(|| environment.id.clone());
            plan.changes_environment_selection = true;
            plan.commands.push(C::SaveEnvironment { environment });
            plan.environments = Some(environments);
        }
        E::RemoveEnvironment { id } => {
            let index = workspace.environments.iter().position(|e| e.id == id)?;
            let previous = workspace.environments[index].clone();
            plan.observed_secrets.extend(
                previous
                    .variables
                    .iter()
                    .filter_map(|v| v.value.secret_name().map(str::to_owned)),
            );
            let mut environments = workspace.environments.clone();
            environments.remove(index);
            if plan.selected_environment_id.as_deref() == Some(&id) {
                plan.selected_environment_id = None;
                plan.changes_environment_selection = true;
            }
            plan.commands.push(C::DeleteEnvironment { id });
            plan.environments = Some(environments);
            plan.inverse = E::RestoreEnvironment {
                environment: previous,
                index: index as u64,
            };
        }
        E::RestoreRequest { location } => {
            let mut c = collection(workspace, &location.collection_id)?.clone();
            if location
                .group_id
                .as_ref()
                .is_some_and(|id| !c.groups.iter().any(|g| &g.id == id))
            {
                return None;
            }
            if let Some(index) = c
                .requests
                .iter()
                .position(|r| r.request.id == location.request.id)
            {
                c.requests[index] = location.clone();
            } else {
                c.requests.push(location.clone());
            }
            plan.inverse = E::RemoveRequest {
                collection_id: location.collection_id.clone(),
                id: location.request.id.clone(),
            };
            plan.commands.push(C::SaveRequest { location });
            plan.collections.push(c);
        }
        E::RemoveRequest { collection_id, id } => {
            let previous = location(workspace, &collection_id, &id)?.clone();
            let mut c = collection(workspace, &collection_id)?.clone();
            c.requests.retain(|r| r.request.id != id);
            plan.commands.push(C::DeleteRequest {
                collection_id: collection_id.clone(),
                id: id.clone(),
            });
            plan.effects.push(F::Close {
                collection_id,
                request_id: Some(id),
            });
            plan.collections.push(c);
            plan.inverse = E::RestoreRequest { location: previous };
        }
        E::DuplicateRequest {
            collection_id,
            id,
            new_id,
            name,
        } => {
            let mut copy = location(workspace, &collection_id, &id)?.clone();
            let mut c = collection(workspace, &collection_id)?.clone();
            copy.request.id.clone_from(&new_id);
            copy.request.name.clone_from(&name);
            copy.order = i64::try_from(c.requests.len()).unwrap_or(i64::MAX);
            plan.commands.push(C::DuplicateRequest {
                collection_id: collection_id.clone(),
                id,
                new_id: new_id.clone(),
                name,
            });
            c.requests.push(copy);
            plan.collections.push(c);
            plan.inverse = E::RemoveRequest {
                collection_id,
                id: new_id,
            };
        }
        E::RestoreGroups {
            collection_id,
            groups,
            requests,
        } => {
            let first = groups.first()?;
            let mut c = collection(workspace, &collection_id)?.clone();
            plan.inverse = E::RemoveGroup {
                collection_id: collection_id.clone(),
                id: first.id.clone(),
            };
            plan.commands
                .extend(groups.iter().map(|group| C::CreateGroup {
                    collection_id: collection_id.clone(),
                    group: group.clone(),
                }));
            plan.commands
                .extend(requests.iter().map(|location| C::SaveRequest {
                    location: location.clone(),
                }));
            c.groups.extend(groups);
            c.requests.extend(requests);
            plan.collections.push(c);
        }
        E::RemoveGroup { collection_id, id } => {
            let mut c = collection(workspace, &collection_id)?.clone();
            if !c.groups.iter().any(|g| g.id == id) {
                return None;
            }
            let ids = descendants(&c, &id);
            let requests: Vec<_> = c
                .requests
                .iter()
                .filter(|r| r.group_id.as_ref().is_some_and(|id| ids.contains(id)))
                .cloned()
                .collect();
            plan.inverse = E::RestoreGroups {
                collection_id: collection_id.clone(),
                groups: subtree(&c, Some(&id)),
                requests: requests.clone(),
            };
            plan.commands.push(C::DeleteGroup {
                collection_id: collection_id.clone(),
                id,
            });
            plan.effects.extend(requests.iter().map(|r| F::Close {
                collection_id: collection_id.clone(),
                request_id: Some(r.request.id.clone()),
            }));
            c.groups.retain(|g| !ids.contains(&g.id));
            c.requests
                .retain(|r| !r.group_id.as_ref().is_some_and(|id| ids.contains(id)));
            plan.collections.push(c);
        }
        E::RestoreCollection { collection: c } => {
            if collection(workspace, &c.id).is_some() {
                return None;
            }
            let empty = DomainCollection {
                id: c.id.clone(),
                name: c.name.clone(),
                order: c.order,
                ..Default::default()
            };
            plan.commands
                .push(C::CreateCollection { collection: empty });
            plan.commands
                .extend(subtree(&c, None).into_iter().map(|group| C::CreateGroup {
                    collection_id: c.id.clone(),
                    group,
                }));
            plan.commands
                .extend(c.requests.iter().map(|location| C::SaveRequest {
                    location: location.clone(),
                }));
            plan.inverse = E::RemoveCollection { id: c.id.clone() };
            plan.collections.push(c);
        }
        E::RemoveCollection { id } => {
            let c = collection(workspace, &id)?.clone();
            plan.commands.push(C::DeleteCollection { id: id.clone() });
            plan.effects.push(F::Close {
                collection_id: id.clone(),
                request_id: None,
            });
            plan.removed_collection_ids.push(id);
            plan.inverse = E::RestoreCollection { collection: c };
        }
        E::RenameRequest {
            collection_id,
            id,
            name,
        } => {
            let mut c = collection(workspace, &collection_id)?.clone();
            let r = c.requests.iter_mut().find(|r| r.request.id == id)?;
            if r.request.name == name {
                return None;
            }
            plan.inverse = E::RenameRequest {
                collection_id: collection_id.clone(),
                id: id.clone(),
                name: r.request.name.clone(),
            };
            r.request.name.clone_from(&name);
            plan.commands.push(C::SaveRequest {
                location: r.clone(),
            });
            plan.effects.push(F::Rename {
                collection_id,
                request_id: id,
                name,
            });
            plan.collections.push(c);
        }
        E::RenameGroup {
            collection_id,
            id,
            name,
        } => {
            let mut c = collection(workspace, &collection_id)?.clone();
            let group = c.groups.iter_mut().find(|g| g.id == id)?;
            if group.name == name {
                return None;
            }
            plan.inverse = E::RenameGroup {
                collection_id: collection_id.clone(),
                id: id.clone(),
                name: group.name.clone(),
            };
            group.name.clone_from(&name);
            plan.commands.push(C::RenameGroup {
                collection_id,
                id,
                name,
            });
            plan.collections.push(c);
        }
        E::RenameCollection { id, name } => {
            let mut c = collection(workspace, &id)?.clone();
            if c.name == name {
                return None;
            }
            plan.inverse = E::RenameCollection {
                id: id.clone(),
                name: c.name.clone(),
            };
            c.name.clone_from(&name);
            plan.commands.push(C::RenameCollection { id, name });
            plan.collections.push(c);
        }
        E::MoveRequest {
            from_collection_id,
            id,
            to_collection_id,
            group_id,
            order,
        } => {
            let source = location(workspace, &from_collection_id, &id)?.clone();
            let mut from = collection(workspace, &from_collection_id)?.clone();
            from.requests.retain(|r| r.request.id != id);
            let mut to = if from_collection_id == to_collection_id {
                from.clone()
            } else {
                collection(workspace, &to_collection_id)?.clone()
            };
            to.requests.push(DomainLocation {
                collection_id: to_collection_id.clone(),
                group_id,
                order,
                request: source.request,
            });
            if from_collection_id != to_collection_id {
                plan.collections.push(from);
            }
            plan.collections.push(to);
            let E::MoveRequest { group_id, .. } = &plan.inverse else {
                return None;
            };
            plan.commands.push(C::MoveRequest {
                from_collection_id: from_collection_id.clone(),
                request_id: id.clone(),
                to_collection_id: to_collection_id.clone(),
                group_id: group_id.clone(),
                order,
            });
            plan.effects.push(F::Relocate {
                collection_id: from_collection_id.clone(),
                request_id: id.clone(),
                destination: to_collection_id.clone(),
            });
            plan.inverse = E::MoveRequest {
                from_collection_id: to_collection_id,
                id,
                to_collection_id: from_collection_id,
                group_id: source.group_id,
                order: source.order,
            };
        }
        E::MoveGroup {
            collection_id,
            id,
            parent_id,
            order,
        } => {
            let mut c = collection(workspace, &collection_id)?.clone();
            let group = c.groups.iter_mut().find(|g| g.id == id)?;
            plan.inverse = E::MoveGroup {
                collection_id: collection_id.clone(),
                id: id.clone(),
                parent_id: group.parent_id.clone(),
                order: group.order,
            };
            group.parent_id.clone_from(&parent_id);
            group.order = order;
            plan.commands.push(C::MoveGroup {
                collection_id,
                id,
                parent_id,
                order,
            });
            plan.collections.push(c);
        }
        E::Reorder {
            collection_id,
            parent_id,
            items,
        } => {
            let mut c = collection(workspace, &collection_id)?.clone();
            let previous = ordered_children(&c, parent_id.as_deref());
            if items == previous
                || items.iter().collect::<HashSet<_>>() != previous.iter().collect::<HashSet<_>>()
            {
                return None;
            }
            for (index, item) in items.iter().enumerate() {
                for g in &mut c.groups {
                    if item == &format!("group:{}", g.id) {
                        g.order = i64::try_from(index).unwrap_or(i64::MAX);
                    }
                }
                for r in &mut c.requests {
                    if item == &format!("request:{}", r.request.id) {
                        r.order = i64::try_from(index).unwrap_or(i64::MAX);
                    }
                }
            }
            plan.inverse = E::Reorder {
                collection_id: collection_id.clone(),
                parent_id: parent_id.clone(),
                items: previous,
            };
            plan.commands.push(C::ReorderChildren {
                collection_id,
                parent_id,
                items,
            });
            plan.collections.push(c);
        }
    }
    Some(plan)
}
/// The retained workspace revision used to plan edits. Native projections can stage
/// editor changes and synchronize once at the next application command.
#[derive(Debug, Default)]
pub struct WorkspaceSession {
    workspace: std::sync::Mutex<DomainWorkspace>,
}
impl WorkspaceSession {
    pub fn replace(&self, workspace: DomainWorkspace) {
        *crate::lock(&self.workspace) = workspace;
    }
    #[must_use]
    pub fn plan(
        &self,
        selected: Option<String>,
        step: SessionWorkspaceEdit,
    ) -> Option<SessionWorkspacePlan> {
        plan_edit(&crate::lock(&self.workspace), selected, step)
    }
    pub fn commit(&self, plan: SessionWorkspacePlan) {
        let mut workspace = crate::lock(&self.workspace);
        workspace
            .collections
            .retain(|c| !plan.removed_collection_ids.contains(&c.id));
        for collection in plan.collections {
            if let Some(i) = workspace
                .collections
                .iter()
                .position(|c| c.id == collection.id)
            {
                workspace.collections[i] = collection;
            } else {
                workspace.collections.push(collection);
            }
        }
        if let Some(environments) = plan.environments {
            workspace.environments = environments;
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionDeleteTarget {
    Collection { id: String },
    Group { collection_id: String, id: String },
    Request { collection_id: String, id: String },
}
impl WorkspaceSession {
    #[must_use]
    pub fn delete_consequences(
        &self,
        target: SessionDeleteTarget,
        documents: &[super::tabs::SessionTabDocument],
    ) -> String {
        let workspace = crate::lock(&self.workspace);
        let (collection_id, requests, folder_count, container) = match target {
            SessionDeleteTarget::Request { collection_id, id } => {
                let requests = location(&workspace, &collection_id, &id)
                    .map(|r| vec![r.request.id.clone()])
                    .unwrap_or_default();
                (collection_id, requests, 0, false)
            }
            SessionDeleteTarget::Group { collection_id, id } => {
                let Some(c) = collection(&workspace, &collection_id) else {
                    return String::new();
                };
                let ids = descendants(c, &id);
                let requests = c
                    .requests
                    .iter()
                    .filter(|r| r.group_id.as_ref().is_some_and(|id| ids.contains(id)))
                    .map(|r| r.request.id.clone())
                    .collect();
                (collection_id, requests, ids.len().saturating_sub(1), true)
            }
            SessionDeleteTarget::Collection { id } => {
                let Some(c) = collection(&workspace, &id) else {
                    return String::new();
                };
                (
                    id,
                    c.requests.iter().map(|r| r.request.id.clone()).collect(),
                    c.groups.len(),
                    true,
                )
            }
        };
        let mut parts = vec![];
        if container {
            let mut contents = vec![];
            if !requests.is_empty() {
                contents.push(if requests.len() == 1 {
                    "1 request".into()
                } else {
                    format!("{} requests", requests.len())
                });
            }
            if folder_count > 0 {
                contents.push(if folder_count == 1 {
                    "1 folder".into()
                } else {
                    format!("{folder_count} folders")
                });
            }
            if !contents.is_empty() {
                parts.push(format!(
                    "Its {} will also be deleted.",
                    contents.join(" and ")
                ));
            }
        }
        if documents.iter().any(|d| {
            d.dirty
                && d.collection_id.as_ref() == Some(&collection_id)
                && requests.contains(&d.request_id)
        }) {
            parts.push(
                "Unsaved changes in open tabs will be discarded and can’t be restored.".into(),
            );
        }
        if parts.is_empty() {
            return String::new();
        }
        parts.push("You can undo the deletion with Edit ▸ Undo (⌘Z).".into());
        parts.join(" ")
    }
}
#[must_use]
pub fn tab_destination(
    tabs: &[String],
    selected: Option<&str>,
    index: Option<i64>,
    offset: i64,
) -> Option<String> {
    let count = i64::try_from(tabs.len()).ok()?;
    if count == 0 {
        return None;
    }
    let current = i64::try_from(
        tabs.iter()
            .position(|id| Some(id.as_str()) == selected)
            .unwrap_or(0),
    )
    .ok()?;
    let target = index.unwrap_or_else(|| (current + offset + count) % count);
    tabs.get(usize::try_from(target).ok()?).cloned()
}
impl WorkspaceSession {
    #[must_use]
    pub fn partial(
        &self,
        mut plan: SessionWorkspacePlan,
        completed: u64,
    ) -> Option<SessionWorkspacePlan> {
        if completed == 0 {
            return None;
        }
        let workspace = crate::lock(&self.workspace);
        let mut changed: Vec<DomainCollection> = vec![];
        for command in plan
            .commands
            .iter()
            .take(usize::try_from(completed).unwrap_or(usize::MAX))
        {
            match command {
                SessionWorkspaceCommand::CreateCollection { collection } => {
                    changed.push(collection.clone());
                }
                SessionWorkspaceCommand::CreateGroup {
                    collection_id,
                    group,
                } => {
                    if !changed.iter().any(|c| &c.id == collection_id) {
                        changed.push(collection(&workspace, collection_id)?.clone());
                    }
                    changed
                        .iter_mut()
                        .find(|c| &c.id == collection_id)?
                        .groups
                        .push(group.clone());
                }
                SessionWorkspaceCommand::SaveRequest { location } => {
                    if !changed.iter().any(|c| c.id == location.collection_id) {
                        changed.push(collection(&workspace, &location.collection_id)?.clone());
                    }
                    changed
                        .iter_mut()
                        .find(|c| c.id == location.collection_id)?
                        .requests
                        .push(location.clone());
                }
                _ => return Some(plan),
            }
        }
        plan.collections = changed;
        plan.removed_collection_ids.clear();
        plan.environments = None;
        plan.effects.clear();
        plan.changes_environment_selection = false;
        Some(plan)
    }
}

#[cfg(test)]
mod tests;
