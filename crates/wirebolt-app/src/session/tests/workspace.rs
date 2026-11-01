use super::super::workspace::{
    SessionWorkspaceCommand as Command, SessionWorkspaceEdit as Edit, SessionWorkspacePlan,
    plan_edit,
};
use crate::domain::{
    DomainCollection, DomainEnvironment, DomainGroup, DomainLocation, DomainRequestDraft,
    DomainValueSource, DomainVariable, DomainWorkspace,
};
fn nested() -> DomainWorkspace {
    DomainWorkspace {
        collections: vec![DomainCollection {
            id: "api".into(),
            name: "API".into(),
            order: 3,
            groups: vec![
                DomainGroup {
                    id: "child".into(),
                    name: "Child".into(),
                    parent_id: Some("parent".into()),
                    order: 1,
                },
                DomainGroup {
                    id: "parent".into(),
                    name: "Parent".into(),
                    parent_id: None,
                    order: 0,
                },
                DomainGroup {
                    id: "grandchild".into(),
                    name: "Grandchild".into(),
                    parent_id: Some("child".into()),
                    order: 0,
                },
            ],
            requests: vec![
                location("deep", Some("grandchild"), 4),
                location("shallow", Some("parent"), 7),
                location("root", None, 2),
            ],
        }],
        ..Default::default()
    }
}
fn location(id: &str, group: Option<&str>, order: i64) -> DomainLocation {
    DomainLocation {
        collection_id: "api".into(),
        group_id: group.map(str::to_owned),
        order,
        request: DomainRequestDraft {
            id: id.into(),
            name: id.into(),
            ..Default::default()
        },
    }
}
fn commit(workspace: &mut DomainWorkspace, plan: &SessionWorkspacePlan) {
    workspace
        .collections
        .retain(|c| !plan.removed_collection_ids.contains(&c.id));
    for c in &plan.collections {
        if let Some(i) = workspace.collections.iter().position(|old| old.id == c.id) {
            workspace.collections[i].clone_from(c);
        } else {
            workspace.collections.push(c.clone());
        }
    }
    if let Some(environments) = &plan.environments {
        workspace.environments.clone_from(environments);
    }
}
fn sorted(mut workspace: DomainWorkspace) -> DomainWorkspace {
    for c in &mut workspace.collections {
        c.groups.sort_by(|a, b| a.id.cmp(&b.id));
        c.requests.sort_by(|a, b| a.request.id.cmp(&b.request.id));
    }
    workspace
}
fn roundtrip(mut workspace: DomainWorkspace, step: Edit) {
    let initial = sorted(workspace.clone());
    let plan = plan_edit(&workspace, None, step).unwrap();
    commit(&mut workspace, &plan);
    let inverse = plan_edit(&workspace, None, plan.inverse).unwrap();
    commit(&mut workspace, &inverse);
    assert_eq!(sorted(workspace), initial);
}
#[test]
fn environment_edits_and_deletion_restore_position_and_secret_references() {
    let environment = DomainEnvironment {
        id: "local".into(),
        name: "Local".into(),
        variables: vec![DomainVariable {
            id: "token".into(),
            key: "token".into(),
            value: DomainValueSource::Secret {
                secret: "env.local.token".into(),
            },
            ..Default::default()
        }],
    };
    let workspace = DomainWorkspace {
        environments: vec![
            environment.clone(),
            DomainEnvironment {
                id: "other".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let mut changed = environment;
    changed.name = "Development".into();
    let plan = plan_edit(
        &workspace,
        None,
        Edit::RestoreEnvironment {
            environment: changed.clone(),
            index: 0,
        },
    )
    .unwrap();
    assert_eq!(
        plan.observed_secrets,
        ["env.local.token", "env.local.token"]
    );
    assert_eq!(plan.selected_environment_id.as_deref(), Some("local"));
    roundtrip(
        workspace.clone(),
        Edit::RestoreEnvironment {
            environment: changed,
            index: 0,
        },
    );
    roundtrip(workspace, Edit::RemoveEnvironment { id: "local".into() });
}
#[test]
fn folder_deletion_inverse_restores_parents_before_children_with_ids_and_order() {
    let workspace = nested();
    let plan = plan_edit(
        &workspace,
        None,
        Edit::RemoveGroup {
            collection_id: "api".into(),
            id: "parent".into(),
        },
    )
    .unwrap();
    assert!(plan.collections[0].groups.is_empty());
    assert_eq!(plan.collections[0].requests[0].request.id, "root");
    let Edit::RestoreGroups {
        groups, requests, ..
    } = &plan.inverse
    else {
        panic!("restore subtree")
    };
    assert_eq!(
        groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
        ["parent", "child", "grandchild"]
    );
    assert_eq!(requests.len(), 2);
    roundtrip(
        workspace,
        Edit::RemoveGroup {
            collection_id: "api".into(),
            id: "parent".into(),
        },
    );
}
#[test]
fn collection_deletion_inverse_restores_folders_and_requests() {
    roundtrip(nested(), Edit::RemoveCollection { id: "api".into() });
}
#[test]
fn request_deletion_inverse_preserves_saved_definition() {
    roundtrip(
        nested(),
        Edit::RemoveRequest {
            collection_id: "api".into(),
            id: "deep".into(),
        },
    );
}
#[test]
fn request_folder_collection_renames_restore_previous_names() {
    for edit in [
        Edit::RenameRequest {
            collection_id: "api".into(),
            id: "root".into(),
            name: "Renamed".into(),
        },
        Edit::RenameGroup {
            collection_id: "api".into(),
            id: "child".into(),
            name: "Renamed".into(),
        },
        Edit::RenameCollection {
            id: "api".into(),
            name: "Renamed".into(),
        },
    ] {
        roundtrip(nested(), edit);
    }
}
#[test]
fn creation_inverses_remove_requests_groups_and_collections() {
    for edit in [
        Edit::RestoreRequest {
            location: location("new", None, 0),
        },
        Edit::RestoreGroups {
            collection_id: "api".into(),
            groups: vec![DomainGroup {
                id: "new".into(),
                ..Default::default()
            }],
            requests: vec![],
        },
        Edit::RestoreCollection {
            collection: DomainCollection {
                id: "other".into(),
                ..Default::default()
            },
        },
    ] {
        roundtrip(nested(), edit);
    }
}
#[test]
fn moving_requests_and_groups_restores_original_parent_and_order() {
    let mut workspace = nested();
    workspace.collections.push(DomainCollection {
        id: "other".into(),
        ..Default::default()
    });
    roundtrip(
        workspace,
        Edit::MoveRequest {
            from_collection_id: "api".into(),
            id: "deep".into(),
            to_collection_id: "other".into(),
            group_id: None,
            order: 9,
        },
    );
    roundtrip(
        nested(),
        Edit::MoveGroup {
            collection_id: "api".into(),
            id: "child".into(),
            parent_id: None,
            order: 8,
        },
    );
}
#[test]
fn reordering_siblings_is_reversible_and_rejects_changed_membership() {
    let mut workspace = nested();
    let before = crate::domain::sidebar::ordered_children(&workspace.collections[0], None);
    let plan = plan_edit(
        &workspace,
        None,
        Edit::Reorder {
            collection_id: "api".into(),
            parent_id: None,
            items: vec!["request:root".into(), "group:parent".into()],
        },
    )
    .unwrap();
    commit(&mut workspace, &plan);
    assert_eq!(
        crate::domain::sidebar::ordered_children(&workspace.collections[0], None),
        ["request:root", "group:parent"]
    );
    let inverse = plan_edit(&workspace, None, plan.inverse).unwrap();
    commit(&mut workspace, &inverse);
    assert_eq!(
        crate::domain::sidebar::ordered_children(&workspace.collections[0], None),
        before
    );
    assert!(
        plan_edit(
            &nested(),
            None,
            Edit::Reorder {
                collection_id: "api".into(),
                parent_id: None,
                items: vec!["request:missing".into()]
            }
        )
        .is_none()
    );
}
#[test]
fn unchanged_renames_and_missing_targets_do_not_register_undo() {
    assert!(
        plan_edit(
            &nested(),
            None,
            Edit::RenameCollection {
                id: "api".into(),
                name: "API".into()
            }
        )
        .is_none()
    );
    assert!(
        plan_edit(
            &nested(),
            None,
            Edit::RemoveRequest {
                collection_id: "api".into(),
                id: "missing".into()
            }
        )
        .is_none()
    );
}
#[test]
fn planning_does_not_mutate_state_before_persistence_succeeds() {
    let workspace = nested();
    let initial = workspace.clone();
    let plan = plan_edit(
        &workspace,
        None,
        Edit::RemoveCollection { id: "api".into() },
    )
    .unwrap();
    assert_eq!(workspace, initial);
    assert!(matches!(&plan.commands[0],Command::DeleteCollection{id} if id=="api"));
}
#[test]
fn global_environment_save_does_not_select_it() {
    let plan = plan_edit(
        &DomainWorkspace::default(),
        Some("old".into()),
        Edit::RestoreEnvironment {
            environment: DomainEnvironment {
                id: "global".into(),
                ..Default::default()
            },
            index: 0,
        },
    )
    .unwrap();
    assert!(plan.selected_environment_id.is_none() && plan.changes_environment_selection);
}
#[test]
fn partial_subtree_restore_keeps_only_successfully_persisted_prefix() {
    let session = super::super::workspace::WorkspaceSession::default();
    let original = nested();
    let plan = session
        .plan(
            None,
            Edit::RestoreCollection {
                collection: original.collections[0].clone(),
            },
        )
        .unwrap();
    assert!(session.partial(plan.clone(), 0).is_none());
    let partial = session.partial(plan, 2).unwrap();
    assert_eq!(partial.collections.len(), 1);
    assert_eq!(partial.collections[0].groups.len(), 1);
    assert_eq!(partial.collections[0].groups[0].id, "parent");
    assert!(partial.collections[0].requests.is_empty());
}
#[test]
fn delete_confirmation_preserves_exact_text_and_dirty_warning() {
    let session = super::super::workspace::WorkspaceSession::default();
    session.replace(nested());
    let text = session.delete_consequences(
        super::super::workspace::SessionDeleteTarget::Collection { id: "api".into() },
        &[],
    );
    assert_eq!(
        text,
        "Its 3 requests and 3 folders will also be deleted. You can undo the deletion with Edit ▸ Undo (⌘Z)."
    );
}
#[test]
fn tab_shortcuts_wrap_and_out_of_range_indices_are_ignored() {
    let tabs = ["a", "b", "c"].map(str::to_owned);
    assert_eq!(
        super::super::workspace::tab_destination(&tabs, Some("a"), None, -1).as_deref(),
        Some("c")
    );
    assert_eq!(
        super::super::workspace::tab_destination(&tabs, Some("c"), None, 1).as_deref(),
        Some("a")
    );
    assert!(super::super::workspace::tab_destination(&tabs, None, Some(3), 0).is_none());
    assert!(super::super::workspace::tab_destination(&[], None, None, 1).is_none());
}
