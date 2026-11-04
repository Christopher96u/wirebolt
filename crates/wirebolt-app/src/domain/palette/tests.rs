use super::*;
use crate::domain::{
    DomainAuthentication, DomainBody, DomainCollection, DomainEnvironment, DomainField,
    DomainLocation, DomainRequestDraft, DomainValueSource, DomainVariable,
};

fn collection(names: &[&str]) -> PaletteCollection {
    PaletteCollection {
        id: "api".into(),
        name: "Catalog".into(),
        groups: vec![],
        requests: names
            .iter()
            .enumerate()
            .map(|(i, name)| PaletteRequest {
                id: i.to_string(),
                name: (*name).into(),
                method: "GET".into(),
                web_socket: false,
                group_id: None,
            })
            .collect(),
    }
}
fn tab(group: &str, id: &str, title: &str) -> PaletteTab {
    PaletteTab {
        group_id: group.into(),
        tab_id: id.into(),
        collection_id: Some("api".into()),
        request_id: "0".into(),
        title: title.into(),
        method: "GET".into(),
        web_socket: false,
        group_name: group.into(),
    }
}
#[test]
fn exact_prefix_substring_unicode_and_secondary_ranking() {
    let mut palette = CommandPalette::default();
    palette.replace_workspace(vec![collection(&[
        "my café",
        "café listing",
        "Café",
        "Straße",
        "東京 🚀",
    ])]);
    let results = palette
        .query("CAFE".into(), PaletteFilter::Requests)
        .results;
    assert_eq!(
        results.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["Café", "café listing", "my café"]
    );
    assert_eq!(
        palette
            .query("STRASSE".into(), PaletteFilter::Requests)
            .results[0]
            .title,
        "Straße"
    );
    assert_eq!(
        palette
            .query("東京".into(), PaletteFilter::Requests)
            .results
            .len(),
        1
    );
    assert_eq!(
        palette
            .query("catalog".into(), PaletteFilter::Requests)
            .results
            .len(),
        5
    );
    assert_eq!(
        palette
            .query("get cafe".into(), PaletteFilter::Requests)
            .results
            .len(),
        3
    );
}
#[test]
fn homonyms_folders_groups_open_tie_break_and_exact_destinations() {
    let mut palette = CommandPalette::default();
    let mut second = collection(&["Health"]);
    second.id = "second".into();
    second.groups = vec![DomainGroup {
        id: "folder".into(),
        name: "Status".into(),
        parent_id: None,
        order: 0,
    }];
    second.requests[0].group_id = Some("folder".into());
    palette.replace_workspace(vec![collection(&["Health"]), second]);
    palette.update_tabs(
        vec![tab("right", "b", "Health"), tab("left", "a", "Health")],
        "left".into(),
    );
    let results = palette.query("health".into(), PaletteFilter::All).results;
    assert!(
        matches!(&results[0].destination, PaletteDestination::Tab { group_id, tab_id } if group_id == "right" && tab_id == "b")
    );
    assert!(results.iter().any(|r| r.subtitle == "Catalog / Status"));
    assert_eq!(
        palette.request_tab("api", "0"),
        Some(PaletteDestination::Tab {
            group_id: "left".into(),
            tab_id: "a".into()
        })
    );
    assert_eq!(
        palette
            .query("status".into(), PaletteFilter::All)
            .results
            .len(),
        1
    );
}
#[test]
fn cap_empty_order_filters_removed_and_updated_destinations() {
    let mut palette = CommandPalette::default();
    palette.replace_workspace(vec![collection(&vec!["Request"; 100])]);
    palette.update_tabs(
        vec![tab("left", "one", "Draft"), tab("right", "two", "Unsaved")],
        "left".into(),
    );
    let root = palette.open();
    assert_eq!(root.results[0].title, "Draft");
    assert_eq!(root.results[1].title, "Unsaved");
    assert_eq!(root.results[2].title, "New Request");
    let results = palette.query("request".into(), PaletteFilter::Requests);
    assert_eq!(results.results.len(), 50);
    assert_eq!(results.total, 100);
    assert!(
        palette
            .query("> request".into(), PaletteFilter::Requests)
            .results
            .iter()
            .all(|r| matches!(r.destination, PaletteDestination::Command { .. }))
    );
    palette.replace_workspace(vec![]);
    palette.query("Request".into(), PaletteFilter::Requests);
    assert!(palette.resolve("request:api:0").is_none());
    palette.update_tabs(vec![tab("left", "one", "Renamed")], "left".into());
    assert!(
        palette
            .query("draft".into(), PaletteFilter::Requests)
            .results
            .is_empty()
    );
    assert_eq!(
        palette
            .query("renamed".into(), PaletteFilter::Requests)
            .results
            .len(),
        1
    );
    assert!(palette.resolve("tab:right:two").is_none());
}
#[test]
fn submenu_restores_parent_query_filter_highlight_and_current_choice() {
    let mut palette = CommandPalette::default();
    palette.update_choices(
        vec![PaletteChoice {
            id: "staging".into(),
            name: "Staging".into(),
        }],
        vec![PaletteChoice {
            id: "system".into(),
            name: "Follow System".into(),
        }],
        Some("staging".into()),
        "system".into(),
    );
    palette.open();
    let parent = palette.query(">environment".into(), PaletteFilter::Actions);
    palette.enter(PalettePage::Environments);
    assert_eq!(
        palette.state.selected_id.as_deref(),
        Some("environment:staging")
    );
    palette.move_selection(1);
    assert_eq!(palette.selected_environment.as_deref(), Some("staging"));
    assert_eq!(palette.back().unwrap(), parent);
    assert!(palette.back().is_none());
}
#[test]
fn command_catalog_availability_and_safe_scope() {
    let mut context = CommandContext::default();
    for command in [
        WorkspaceCommand::Save,
        WorkspaceCommand::Back,
        WorkspaceCommand::NextTab,
        WorkspaceCommand::ResponseBody,
    ] {
        assert!(command.descriptor(&context).unavailable_reason.is_some());
    }
    context.has_session = true;
    context.has_name = true;
    context.has_collections = true;
    assert!(
        WorkspaceCommand::Save
            .descriptor(&context)
            .unavailable_reason
            .is_none()
    );
    context.web_socket = true;
    assert_eq!(
        WorkspaceCommand::RequestBody.descriptor(&context).title,
        "Message"
    );
    assert!(
        WorkspaceCommand::ResponseBody
            .descriptor(&context)
            .unavailable_reason
            .is_some()
    );
    assert_eq!(WorkspaceCommand::ALL.len(), 30);
}
#[test]
fn privacy_projection_excludes_all_content_sentinels() {
    let field = |text: &str| DomainField {
        name: text.into(),
        value: DomainValueSource::literal(text),
        ..DomainField::default()
    };
    let request = DomainRequestDraft {
        id: "safe".into(),
        name: "Safe title".into(),
        url: "https://url-sentinel.invalid/?query-sentinel".into(),
        body: DomainBody::Raw {
            value: "body-sentinel".into(),
            content_type: Some("text/plain".into()),
        },
        note: "notes-sentinel".into(),
        headers: vec![field("header-sentinel")],
        query: vec![field("parameter-sentinel")],
        authentication: DomainAuthentication::Bearer {
            token: DomainValueSource::Secret {
                secret: "auth-secret-sentinel".into(),
            },
        },
        ..DomainRequestDraft::default()
    };
    let collection = DomainCollection {
        id: "api".into(),
        name: "Safe collection".into(),
        requests: vec![DomainLocation {
            collection_id: "api".into(),
            group_id: None,
            order: 0,
            request,
        }],
        ..DomainCollection::default()
    };
    let mut palette = CommandPalette::default();
    palette.replace_workspace(vec![PaletteCollection::from(&collection)]);
    let environment = DomainEnvironment {
        id: "env".into(),
        name: "Staging".into(),
        variables: vec![DomainVariable {
            key: "environment-variable-sentinel".into(),
            value: DomainValueSource::Secret {
                secret: "secret-material-sentinel".into(),
            },
            ..DomainVariable::default()
        }],
    };
    palette.update_choices(
        vec![PaletteChoice {
            id: environment.id,
            name: environment.name,
        }],
        vec![],
        None,
        String::new(),
    );
    for sentinel in [
        "url-sentinel",
        "query-sentinel",
        "body-sentinel",
        "notes-sentinel",
        "header-sentinel",
        "parameter-sentinel",
        "auth-secret-sentinel",
        "environment-variable-sentinel",
        "secret-material-sentinel",
    ] {
        assert!(
            palette
                .query(sentinel.into(), PaletteFilter::All)
                .results
                .is_empty(),
            "{sentinel}"
        );
        palette.enter(PalettePage::Environments);
        assert!(
            palette
                .query(sentinel.into(), PaletteFilter::All)
                .results
                .is_empty()
        );
        palette.back();
    }
    palette.open();
    assert!(!format!("{palette:?}").contains("sentinel"));
}
