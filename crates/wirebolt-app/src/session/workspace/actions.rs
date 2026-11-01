//! Workspace command policy and sidebar validation, over the retained workspace.
use super::{SessionWorkspaceEdit as Edit, collection, descendants, location};
use crate::domain::{
    DomainCollection, DomainEnvironment, DomainGroup, DomainLocation, DomainRequestDraft,
    DomainWorkspace, identity::unique_id, sidebar::ordered_children,
};

pub const ROOT_COLLECTION: &str = "workspace-root";
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "Typed environment edits retain their variable rows."
)]
pub enum WorkspaceAction {
    SaveEnvironment {
        environment: DomainEnvironment,
    },
    DeleteEnvironment {
        id: String,
    },
    CreateRequest {
        web_socket: bool,
        collection_id: Option<String>,
        group_id: Option<String>,
    },
    CreateCollection {
        name: String,
    },
    CreateGroup {
        collection_id: String,
        parent_id: Option<String>,
        name: String,
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
    DeleteRequest {
        collection_id: String,
        id: String,
    },
    DeleteGroup {
        collection_id: String,
        id: String,
    },
    DeleteCollection {
        id: String,
    },
    DuplicateRequest {
        collection_id: String,
        id: String,
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
        identifier: String,
        target: String,
        after: bool,
        collection_id: String,
        parent_id: Option<String>,
    },
    MoveBy {
        identifier: String,
        delta: i64,
    },
    MoveTo {
        identifier: String,
        collection_id: String,
        parent_id: Option<String>,
    },
}
#[derive(Clone, Debug)]
pub struct NamedEdit {
    pub edit: Edit,
    pub name: String,
    pub created_id: Option<String>,
}
fn named(edit: Edit, name: impl Into<String>) -> NamedEdit {
    NamedEdit {
        edit,
        name: name.into(),
        created_id: None,
    }
}
#[must_use]
pub fn document_id(name: &str, fallback: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let stem = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let stem: String = stem.chars().take(48).collect();
    format!(
        "{}-{}",
        if stem.is_empty() { fallback } else { &stem },
        &unique_id()[..8]
    )
}
#[must_use]
pub fn new_environment() -> DomainEnvironment {
    DomainEnvironment {
        id: unique_id(),
        name: "New Environment".into(),
        variables: vec![],
    }
}
#[must_use]
pub fn new_request(
    workspace: &DomainWorkspace,
    web_socket: bool,
    collection_id: String,
    group_id: Option<String>,
) -> DomainLocation {
    let order = collection(workspace, &collection_id)
        .and_then(|c| c.requests.iter().map(|r| r.order).min())
        .unwrap_or(0)
        .clamp(i64::MIN + 1, 0)
        - 1;
    DomainLocation {
        collection_id,
        group_id,
        order,
        request: DomainRequestDraft {
            id: unique_id(),
            name: if web_socket {
                "Untitled WebSocket Request"
            } else {
                "Untitled Request"
            }
            .into(),
            web_socket,
            ..Default::default()
        },
    }
}
#[must_use]
pub fn root_collection() -> DomainCollection {
    DomainCollection {
        id: ROOT_COLLECTION.into(),
        name: "Requests".into(),
        order: i64::MIN,
        ..Default::default()
    }
}
fn parts(identifier: &str) -> Option<(&str, &str, &str)> {
    let mut parts = identifier.split('|');
    let result = (parts.next()?, parts.next()?, parts.next()?);
    parts.next().is_none().then_some(result)
}
#[must_use]
pub fn item_name(workspace: &DomainWorkspace, identifier: &str) -> String {
    let Some((kind, c, id)) = parts(identifier) else {
        return String::new();
    };
    if kind == "request" {
        location(workspace, c, id)
            .map(|r| r.request.name.clone())
            .unwrap_or_default()
    } else {
        collection(workspace, c)
            .and_then(|c| c.groups.iter().find(|g| g.id == id))
            .map(|g| g.name.clone())
            .unwrap_or_default()
    }
}
#[must_use]
pub fn can_reorder(
    workspace: &DomainWorkspace,
    identifier: &str,
    target: &str,
    collection_id: &str,
    parent: Option<&str>,
) -> bool {
    let Some((kind, c, id)) = parts(identifier) else {
        return false;
    };
    let Some(collection) = collection(workspace, collection_id) else {
        return false;
    };
    let source = format!("{kind}:{id}");
    let siblings = ordered_children(collection, parent);
    c == collection_id
        && source != target
        && siblings.contains(&source)
        && siblings.iter().any(|s| s == target)
}
#[must_use]
pub fn can_move_to(
    workspace: &DomainWorkspace,
    identifier: &str,
    collection_id: &str,
    parent: Option<&str>,
) -> bool {
    let Some((kind, c, id)) = parts(identifier) else {
        return false;
    };
    let Some(destination) = collection(workspace, collection_id) else {
        return false;
    };
    if parent.is_some_and(|id| !destination.groups.iter().any(|g| g.id == id)) {
        return false;
    }
    match kind {
        "request" => location(workspace, c, id)
            .is_some_and(|r| r.collection_id != collection_id || r.group_id.as_deref() != parent),
        "group" if c == collection_id => destination
            .groups
            .iter()
            .find(|g| g.id == id)
            .is_some_and(|g| {
                g.parent_id.as_deref() != parent
                    && !parent.is_some_and(|p| descendants(destination, id).contains(p))
            }),
        _ => false,
    }
}
#[must_use]
pub fn next_order(workspace: &DomainWorkspace, collection_id: &str, parent: Option<&str>) -> i64 {
    collection(workspace, collection_id).map_or(0, |c| {
        c.groups
            .iter()
            .filter(|g| g.parent_id.as_deref() == parent)
            .map(|g| g.order)
            .chain(
                c.requests
                    .iter()
                    .filter(|r| r.group_id.as_deref() == parent)
                    .map(|r| r.order),
            )
            .max()
            .unwrap_or(-1)
            .saturating_add(1)
    })
}
#[must_use]
pub fn neighbor(
    workspace: &DomainWorkspace,
    identifier: &str,
    delta: i64,
) -> Option<(String, Option<String>, String)> {
    let (kind, c, id) = parts(identifier)?;
    if delta == 0 {
        return None;
    }
    let c = collection(workspace, c)?;
    let parent = match kind {
        "request" => c
            .requests
            .iter()
            .find(|r| r.request.id == id)?
            .group_id
            .clone(),
        "group" => c.groups.iter().find(|g| g.id == id)?.parent_id.clone(),
        _ => return None,
    };
    let siblings = ordered_children(c, parent.as_deref());
    let index = i64::try_from(siblings.iter().position(|s| s == &format!("{kind}:{id}"))?)
        .ok()?
        .checked_add(delta)?;
    Some((
        c.id.clone(),
        parent,
        siblings.get(usize::try_from(index).ok()?)?.clone(),
    ))
}
#[allow(
    clippy::needless_pass_by_value,
    reason = "This planner consumes owned action inputs alongside the generated edit."
)]
fn reorder(
    workspace: &DomainWorkspace,
    identifier: String,
    target: String,
    after: bool,
    collection_id: String,
    parent_id: Option<String>,
) -> Option<NamedEdit> {
    if !can_reorder(
        workspace,
        &identifier,
        &target,
        &collection_id,
        parent_id.as_deref(),
    ) {
        return None;
    }
    let (kind, _, id) = parts(&identifier)?;
    let source = format!("{kind}:{id}");
    let previous = ordered_children(collection(workspace, &collection_id)?, parent_id.as_deref());
    let mut items: Vec<_> = previous.iter().filter(|s| *s != &source).cloned().collect();
    let index = items.iter().position(|s| s == &target)?;
    items.insert(index + usize::from(after), source);
    if items == previous {
        return None;
    }
    Some(named(
        Edit::Reorder {
            collection_id,
            parent_id,
            items,
        },
        format!("Move “{}”", item_name(workspace, &identifier)),
    ))
}
/// Converts an interface action into exactly one reversible persistence plan.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "Each action keeps its name and edit together."
)]
pub fn action(workspace: &DomainWorkspace, command: WorkspaceAction) -> Option<NamedEdit> {
    use WorkspaceAction as A;
    Some(match command {
        A::SaveEnvironment { environment } => {
            let index = workspace
                .environments
                .iter()
                .position(|e| e.id == environment.id);
            if index.is_some_and(|i| workspace.environments[i] == environment) {
                return None;
            }
            named(
                Edit::RestoreEnvironment {
                    environment,
                    index: index.unwrap_or(workspace.environments.len()) as u64,
                },
                "Edit Environment",
            )
        }
        A::DeleteEnvironment { id } => named(Edit::RemoveEnvironment { id }, "Delete Environment"),
        A::CreateRequest {
            web_socket,
            collection_id,
            group_id,
        } => {
            let collection_id = collection_id.unwrap_or_else(|| ROOT_COLLECTION.into());
            collection(workspace, &collection_id)?;
            let location = new_request(workspace, web_socket, collection_id, group_id);
            NamedEdit {
                created_id: Some(location.request.id.clone()),
                ..named(Edit::RestoreRequest { location }, "New Request")
            }
        }
        A::CreateCollection { name } => {
            let collection = DomainCollection {
                id: document_id(&name, "collection"),
                name,
                order: i64::try_from(workspace.collections.len()).unwrap_or(i64::MAX),
                ..Default::default()
            };
            NamedEdit {
                created_id: Some(collection.id.clone()),
                ..named(Edit::RestoreCollection { collection }, "New Collection")
            }
        }
        A::CreateGroup {
            collection_id,
            parent_id,
            name,
        } => {
            let c = collection(workspace, &collection_id)?;
            let group = DomainGroup {
                id: document_id(&name, "group"),
                name,
                order: i64::try_from(c.groups.iter().filter(|g| g.parent_id == parent_id).count())
                    .unwrap_or(i64::MAX),
                parent_id,
            };
            NamedEdit {
                created_id: Some(group.id.clone()),
                ..named(
                    Edit::RestoreGroups {
                        collection_id,
                        groups: vec![group],
                        requests: vec![],
                    },
                    "New Folder",
                )
            }
        }
        A::RenameRequest {
            collection_id,
            id,
            name,
        } => named(
            Edit::RenameRequest {
                collection_id,
                id,
                name,
            },
            "Rename",
        ),
        A::RenameGroup {
            collection_id,
            id,
            name,
        } => named(
            Edit::RenameGroup {
                collection_id,
                id,
                name,
            },
            "Rename",
        ),
        A::RenameCollection { id, name } => named(Edit::RenameCollection { id, name }, "Rename"),
        A::DeleteRequest { collection_id, id } => {
            let name = item_name(workspace, &format!("request|{collection_id}|{id}"));
            named(
                Edit::RemoveRequest { collection_id, id },
                format!("Delete “{name}”"),
            )
        }
        A::DeleteGroup { collection_id, id } => {
            let name = item_name(workspace, &format!("group|{collection_id}|{id}"));
            named(
                Edit::RemoveGroup { collection_id, id },
                format!("Delete “{name}”"),
            )
        }
        A::DeleteCollection { id } => {
            let name = collection(workspace, &id)
                .map(|c| c.name.as_str())
                .unwrap_or_default();
            named(
                Edit::RemoveCollection { id: id.clone() },
                format!("Delete “{name}”"),
            )
        }
        A::DuplicateRequest { collection_id, id } => {
            let source = location(workspace, &collection_id, &id)?;
            named(
                Edit::DuplicateRequest {
                    collection_id,
                    id: id.clone(),
                    new_id: document_id(&format!("{id}-copy"), "request"),
                    name: format!("{} Copy", source.request.name),
                },
                format!("Duplicate “{}”", source.request.name),
            )
        }
        A::MoveRequest {
            from_collection_id,
            id,
            to_collection_id,
            group_id,
            order,
        } => {
            let name = item_name(workspace, &format!("request|{from_collection_id}|{id}"));
            named(
                Edit::MoveRequest {
                    from_collection_id,
                    id,
                    to_collection_id,
                    group_id,
                    order,
                },
                format!("Move “{name}”"),
            )
        }
        A::MoveGroup {
            collection_id,
            id,
            parent_id,
            order,
        } => {
            let name = item_name(workspace, &format!("group|{collection_id}|{id}"));
            named(
                Edit::MoveGroup {
                    collection_id,
                    id,
                    parent_id,
                    order,
                },
                format!("Move “{name}”"),
            )
        }
        A::Reorder {
            identifier,
            target,
            after,
            collection_id,
            parent_id,
        } => {
            return reorder(
                workspace,
                identifier,
                target,
                after,
                collection_id,
                parent_id,
            );
        }
        A::MoveBy { identifier, delta } => {
            let (collection_id, parent_id, target) = neighbor(workspace, &identifier, delta)?;
            return reorder(
                workspace,
                identifier,
                target,
                delta > 0,
                collection_id,
                parent_id,
            );
        }
        A::MoveTo {
            identifier,
            collection_id,
            parent_id,
        } => {
            if !can_move_to(workspace, &identifier, &collection_id, parent_id.as_deref()) {
                return None;
            }
            let (kind, c, id) = parts(&identifier)?;
            let order = next_order(workspace, &collection_id, parent_id.as_deref());
            let command = if kind == "request" {
                A::MoveRequest {
                    from_collection_id: c.into(),
                    id: id.into(),
                    to_collection_id: collection_id,
                    group_id: parent_id,
                    order,
                }
            } else {
                A::MoveGroup {
                    collection_id,
                    id: id.into(),
                    parent_id,
                    order,
                }
            };
            return action(workspace, command);
        }
    })
}

/// Compatibility move identifiers are parsed in the application core, not the bridge.
#[must_use]
pub fn move_row_id(identifier: &str) -> Option<String> {
    let (kind, collection, id) = parts(identifier)?;
    Some(if kind == "request" {
        format!("request:{collection}/{id}")
    } else {
        format!("group:{collection}:{id}")
    })
}
