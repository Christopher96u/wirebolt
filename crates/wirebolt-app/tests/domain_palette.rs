use wirebolt_app::domain::palette::*;
use wirebolt_app::domain::{
    DomainAuthentication, DomainBody, DomainCollection, DomainEnvironment, DomainField,
    DomainGroup, DomainLocation, DomainRequestDraft, DomainValueSource, DomainVariable,
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
        group_index: u64::from(group == "right"),
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
        2,
    );
    let results = palette.query("health".into(), PaletteFilter::All).results;
    assert_eq!(results.len(), 2);
    let open = results.iter().find(|r| r.id == "request:api:0").unwrap();
    assert_eq!(open.subtitle, "Catalog");
    assert_eq!(open.marker, "Open");
    assert_eq!(
        open.open_tab,
        Some(PaletteTabTarget {
            group_id: "left".into(),
            tab_id: "a".into()
        })
    );
    assert!(results.iter().any(|r| r.subtitle == "Catalog / Status"));
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
    let mut draft = tab("left", "one", "Draft");
    draft.collection_id = None;
    let mut unsaved = tab("right", "two", "Unsaved");
    unsaved.collection_id = None;
    palette.update_tabs(vec![draft.clone(), unsaved], "left".into(), 2);
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
    assert!(matches!(
        palette.resolve("request:api:0"),
        PaletteResolution::Failed {
            reason: PaletteFailure::RequestRemoved
        }
    ));
    draft.title = "Renamed".into();
    palette.update_tabs(vec![draft], "left".into(), 1);
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
    assert!(matches!(
        palette.resolve("tab:right:two"),
        PaletteResolution::Failed {
            reason: PaletteFailure::TabClosed
        }
    ));
}
#[test]
fn submenu_restores_parent_query_filter_highlight_and_current_choice() {
    let mut palette = CommandPalette::default();
    palette.update_choices(
        vec![PaletteChoice {
            id: "staging".into(),
            name: "Staging".into(),
        }],
        vec![PaletteTheme {
            id: "system".into(),
            name: "Follow System".into(),
            kind: PaletteThemeKind::BuiltIn,
        }],
        PaletteEnvironmentSelection::Environment {
            id: "staging".into(),
        },
        "system".into(),
    );
    palette.open();
    let parent = palette.query(">environment".into(), PaletteFilter::Actions);
    palette.enter(PalettePage::Environments);
    assert_eq!(
        palette.refresh().selected_id.as_deref(),
        Some("environment:staging")
    );
    palette.move_selection(1);
    assert!(
        palette
            .refresh()
            .results
            .iter()
            .any(|r| r.current && r.id == "environment:staging")
    );
    assert_eq!(palette.back().unwrap(), parent);
    assert!(palette.back().is_none());
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
        PaletteEnvironmentSelection::Globals,
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

#[test]
fn open_request_keeps_folder_ranking_and_active_group_destination() {
    let mut palette = CommandPalette::default();
    let mut workspace = collection(&["Health", "Health", "health details", "my health"]);
    workspace.groups = vec![DomainGroup {
        id: "folder".into(),
        name: "Health".into(),
        parent_id: None,
        order: 0,
    }];
    for request in &mut workspace.requests {
        request.group_id = Some("folder".into());
    }
    palette.replace_workspace(vec![workspace]);
    palette.update_tabs(
        vec![
            tab("right", "right-tab", "Health"),
            tab("left", "left-tab", "Health"),
        ],
        "left".into(),
        2,
    );
    let snapshot = palette.query("health".into(), PaletteFilter::Requests);
    assert_eq!(
        snapshot
            .results
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        [
            "request:api:0",
            "request:api:1",
            "request:api:2",
            "request:api:3"
        ]
    );
    assert_eq!(snapshot.results[0].subtitle, "Catalog / Health");
    assert_eq!(snapshot.results[0].marker, "Open");
    assert_eq!(
        snapshot.results[0].open_tab.as_ref().unwrap().tab_id,
        "left-tab"
    );
    palette.update_tabs(vec![tab("right", "right-tab", "Health")], "left".into(), 2);
    assert_eq!(palette.refresh().results[0].marker, "Open in split 2");
}

#[test]
fn unsaved_group_hints_and_websocket_labels_are_domain_owned() {
    let mut palette = CommandPalette::default();
    let mut draft = tab("right", "draft", "New socket");
    draft.collection_id = None;
    draft.web_socket = true;
    palette.update_tabs(vec![draft.clone()], "right".into(), 1);
    let snapshot = palette.open();
    assert_eq!(snapshot.results[0].subtitle, "Unsaved");
    assert_eq!(snapshot.results[0].method, "WS");
    assert!(snapshot.results[0].web_socket);
    palette.update_tabs(vec![draft], "right".into(), 2);
    assert_eq!(palette.open().results[0].subtitle, "Unsaved · Split 2");
}

#[test]
fn page_policy_has_no_filler_search_terms_and_resolves_outside_current_page() {
    let mut palette = CommandPalette::default();
    palette.update_choices(
        vec![PaletteChoice {
            id: "staging".into(),
            name: "Staging".into(),
        }],
        vec![PaletteTheme {
            id: "black".into(),
            name: "Black".into(),
            kind: PaletteThemeKind::BuiltIn,
        }],
        PaletteEnvironmentSelection::Globals,
        "black".into(),
    );
    let root = palette.open();
    assert_eq!(root.page_title, "Command Palette");
    assert!(root.results.iter().all(|r| r.subtitle.is_empty()));
    let page = palette.enter(PalettePage::Environments);
    assert_eq!(page.search_placeholder, "Search environments…");
    assert_eq!(page.search_label, "Search environments");
    assert_eq!(
        page.results[0].destination,
        PaletteDestination::Environment {
            selection: PaletteEnvironmentSelection::Globals
        }
    );
    assert!(
        palette
            .query("requests".into(), PaletteFilter::All)
            .results
            .is_empty()
    );
    palette.back();
    assert!(matches!(
        palette.resolve("environment:staging"),
        PaletteResolution::Ready { .. }
    ));
    let page = palette.enter(PalettePage::Themes);
    assert_eq!(page.search_placeholder, "Search themes…");
    assert_eq!(page.results[0].subtitle, "Built-in");
    assert!(
        palette
            .query("appearance".into(), PaletteFilter::All)
            .results
            .is_empty()
    );
    palette.back();
    let snapshot = palette.query(">new".into(), PaletteFilter::Requests);
    assert_eq!(snapshot.effective_filter, PaletteFilter::Actions);
    assert_eq!(snapshot.filter, PaletteFilter::Requests);
}

#[test]
fn updates_reuse_request_normalization_and_return_typed_missing_destinations() {
    let mut palette = CommandPalette::default();
    let mut workspace = collection(&["First", "Second"]);
    palette.replace_workspace(vec![workspace.clone()]);
    assert_eq!(palette.normalized_requests(), 2);
    palette.replace_workspace(vec![workspace.clone()]);
    assert_eq!(palette.normalized_requests(), 2);
    workspace.requests[0].name = "Renamed".into();
    palette.replace_workspace(vec![workspace]);
    assert_eq!(palette.normalized_requests(), 3);
    palette.update_choices(
        vec![],
        vec![],
        PaletteEnvironmentSelection::Globals,
        String::new(),
    );
    assert_eq!(palette.normalized_requests(), 3);
    for (id, expected) in [
        ("request:api:gone", PaletteFailure::RequestRemoved),
        ("tab:g:t", PaletteFailure::TabClosed),
        ("environment:gone", PaletteFailure::EnvironmentRemoved),
        ("theme:gone", PaletteFailure::ThemeRemoved),
    ] {
        assert_eq!(
            palette.resolve(id),
            PaletteResolution::Failed { reason: expected }
        );
    }
    assert!(matches!(
        palette.resolve("command:save"),
        PaletteResolution::Failed {
            reason: PaletteFailure::CommandUnavailable { .. }
        }
    ));
}

#[test]
fn saved_and_all_split_draft_titles_search_one_active_group_row() {
    let mut palette = CommandPalette::default();
    palette.replace_workspace(vec![collection(&["Saved title"])]);
    let mut other = tab("right", "b", "Other split title");
    other.method = "POST".into();
    palette.update_tabs(
        vec![other, tab("left", "a", "Active draft title")],
        "left".into(),
        2,
    );
    for query in [
        "Saved title",
        "Other split title",
        "Active draft title",
        "POST",
        "GET",
    ] {
        let snapshot = palette.query(query.into(), PaletteFilter::Requests);
        assert_eq!(snapshot.results.len(), 1);
        assert_eq!(snapshot.results[0].title, "Active draft title");
        assert_eq!(snapshot.results[0].open_tab.as_ref().unwrap().tab_id, "a");
    }
}

#[test]
fn globals_and_an_environment_named_globals_have_distinct_stable_destinations() {
    let mut palette = CommandPalette::default();
    palette.update_choices(
        vec![PaletteChoice {
            id: "globals".into(),
            name: "Named globals".into(),
        }],
        vec![],
        PaletteEnvironmentSelection::Globals,
        String::new(),
    );
    let page = palette.enter(PalettePage::Environments);
    assert_eq!(page.results.len(), 2);
    assert_ne!(page.results[0].id, page.results[1].id);
    for result in page.results {
        let resolved = palette.resolve(&result.id);
        assert_eq!(resolved, PaletteResolution::Ready { result });
    }
    match palette.resolve("environment:globals") {
        PaletteResolution::Ready { result } => assert_eq!(
            result.destination,
            PaletteDestination::Environment {
                selection: PaletteEnvironmentSelection::Environment {
                    id: "globals".into()
                }
            }
        ),
        PaletteResolution::Failed { reason } => panic!("unexpected {reason:?}"),
    }
}

#[test]
fn workspace_rename_effect_updates_only_its_request_and_preserves_open_metadata() {
    use wirebolt_app::session::workspace::{SessionWorkspaceEdit, plan_edit};
    let workspace = wirebolt_app::domain::DomainWorkspace {
        collections: vec![DomainCollection {
            id: "api".into(),
            name: "Catalog".into(),
            requests: vec![DomainLocation {
                collection_id: "api".into(),
                request: DomainRequestDraft {
                    id: "0".into(),
                    name: "Before".into(),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let plan = plan_edit(
        &workspace,
        None,
        SessionWorkspaceEdit::RenameRequest {
            collection_id: "api".into(),
            id: "0".into(),
            name: "After".into(),
        },
    )
    .unwrap();
    assert_eq!(
        plan.palette_change,
        PaletteWorkspaceChange::RenameRequest {
            collection_id: "api".into(),
            request_id: "0".into(),
            name: "After".into(),
        }
    );
    let mut palette = CommandPalette::default();
    palette.replace_workspace(vec![collection(&["Before", "Keep"])]);
    palette.update_tabs(vec![tab("left", "open", "Draft title")], "left".into(), 1);
    let normalized = palette.normalized_requests();
    assert!(palette.apply_workspace_change(plan.palette_change));
    assert_eq!(palette.normalized_requests(), normalized + 1);
    let snapshot = palette.query("After".into(), PaletteFilter::Requests);
    assert_eq!(snapshot.results.len(), 1);
    assert_eq!(snapshot.results[0].title, "Draft title");
    assert_eq!(snapshot.results[0].marker, "Open");
    assert!(
        palette
            .query("Before".into(), PaletteFilter::Requests)
            .results
            .is_empty()
    );
    assert_eq!(
        palette
            .query("Keep".into(), PaletteFilter::Requests)
            .results
            .len(),
        1
    );
    assert!(!palette.apply_workspace_change(PaletteWorkspaceChange::Rebuild));
    assert!(
        !palette.apply_workspace_change(PaletteWorkspaceChange::RenameRequest {
            collection_id: "api".into(),
            request_id: "gone".into(),
            name: "Gone".into(),
        })
    );
    assert_eq!(palette.normalized_requests(), normalized + 1);
}
