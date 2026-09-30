//! Wirebolt JSON export → import round trips.
//!
//! Imported requests get new document IDs and their order is renumbered to the
//! sibling position, so both are normalized before comparing; everything else,
//! including field-row IDs, must survive unchanged.

use std::collections::BTreeMap;

use wirebolt_core::{
    ApiKeyPlacement, Collection, CollectionSnapshot, DocumentId, Environment, EnvironmentVariable,
    Group, ImportEngine, ImportFormat, ImportedCollection, ImportedWorkspace, ManualProxy,
    MultipartPart, MultipartPartKind, ProxyDestination, ProxyEndpoint, ProxyMode, ProxyRoute,
    Request, RequestAuthentication, RequestBody, RequestHeader, RequestValueField, SecretName,
    ValueSource, Workspace, WorkspaceSnapshot, export_legacy_v1_collection,
    export_legacy_v1_request, export_legacy_v1_workspace,
};

fn id(value: &str) -> DocumentId {
    DocumentId::new(value).unwrap()
}

fn secret(name: &str) -> ValueSource {
    ValueSource::secret(SecretName::new(name).unwrap())
}

fn sensitive_header(name: &str, value: ValueSource) -> RequestHeader {
    RequestHeader {
        sensitive: true,
        ..RequestHeader::enabled(name, value)
    }
}

fn sensitive_field(name: &str, value: ValueSource) -> RequestValueField {
    RequestValueField {
        sensitive: true,
        ..RequestValueField::enabled(name, value)
    }
}

fn file_part(name: &str, path: &str, content_type: Option<&str>) -> MultipartPart {
    MultipartPart {
        id: format!("{name}-part"),
        name: name.into(),
        kind: MultipartPartKind::File,
        value: ValueSource::literal(""),
        file_path: Some(path.into()),
        file_name: None,
        content_type: content_type.map(str::to_owned),
        enabled: true,
    }
}

fn api_collection() -> (Collection, Vec<Request>) {
    let mut collection = Collection::new(id("api"), "Inventory API".into());
    collection.groups = vec![
        Group::new(id("orders"), "Orders".into(), None, 1),
        Group::new(id("archive"), "Archive".into(), Some(id("orders")), 0),
    ];
    let mut search = Request::new(
        id("search"),
        "Search items",
        "GET",
        "https://api.example.test/items?view=compact",
    );
    search.note = "Filters **active** items".into();
    search.headers = vec![
        sensitive_header("X-Api-Token", secret("inventory.token")),
        RequestHeader {
            enabled: false,
            ..RequestHeader::enabled("X-Debug", ValueSource::literal("1"))
        },
    ];
    search.query = vec![
        RequestValueField::enabled("page", ValueSource::literal("2")),
        sensitive_field("signature", secret("inventory.signature")),
    ];
    search.proxy_override = Some(ProxyMode::Direct);
    search.transport.validate_tls = false;
    search.transport.follow_redirects = true;
    search.transport.maximum_redirects = 3;
    search.transport.total_timeout_ms = 120_000;
    search.transport.read_timeout_ms = 45_000;
    search.transport.client_certificate_reference = Some(SecretName::new("client.cert").unwrap());
    search.transport.custom_ca_path = Some("/Users/example/certs/internal-ca.pem".into());
    search.inherits_workspace_transport = false;

    let mut create = Request::new(id("create"), "Create order", "POST", "{{baseUrl}}/orders");
    create.group_id = Some(id("orders"));
    create.authentication = RequestAuthentication::Bearer {
        token: secret("orders.token"),
    };
    create.body = RequestBody::Json {
        value: "{\"quantity\": {{quantity}}}".into(),
    };
    create.proxy_override = Some(ProxyMode::Manual(
        ManualProxy::new(vec![
            ProxyRoute::new(
                ProxyDestination::All,
                ProxyEndpoint::new("http://proxy.example.test:8080").unwrap(),
            )
            .with_credentials(wirebolt_core::ProxyCredentials::new(
                SecretName::new("proxy.user").unwrap(),
                SecretName::new("proxy.password").unwrap(),
            )),
        ])
        .unwrap(),
    ));

    let mut purge = Request::new(
        id("purge"),
        "Purge archive",
        "PURGE",
        "https://api.example.test/purge",
    );
    purge.group_id = Some(id("archive"));
    purge.authentication = RequestAuthentication::Basic {
        username: ValueSource::literal("operator"),
        password: secret("archive.password"),
    };
    purge.body = RequestBody::Text {
        content_type: Some("text/csv".into()),
        value: "sku,qty\nA-1,2".into(),
    };

    let mut requests = vec![search, create, purge];
    requests.extend(body_requests());
    for (order, request) in requests.iter_mut().enumerate() {
        request.order = i64::try_from(order).unwrap();
    }
    (collection, requests)
}

/// One request per body kind, plus a WebSocket and an API key request.
fn body_requests() -> Vec<Request> {
    let upload = env!("CARGO_MANIFEST_DIR").to_owned() + "/Cargo.toml";

    let mut raw = Request::new(
        id("raw"),
        "Raw payload",
        "POST",
        "https://api.example.test/raw",
    );
    raw.body = RequestBody::Raw {
        content_type: Some("application/vnd.inventory+json".into()),
        value: "{\"raw\":true}".into(),
    };

    let mut file = Request::new(
        id("file"),
        "Upload manifest",
        "PUT",
        "https://api.example.test/manifest",
    );
    file.body = RequestBody::File {
        path: upload.clone(),
        content_type: Some("application/toml".into()),
    };

    let mut multipart = Request::new(
        id("multipart"),
        "Upload photos",
        "POST",
        "https://api.example.test/photos",
    );
    multipart.body = RequestBody::Multipart {
        parts: vec![
            file_part("manifest", &upload, Some("application/toml")),
            MultipartPart {
                id: "token-part".into(),
                name: "token".into(),
                kind: MultipartPartKind::Text,
                value: secret("upload.token"),
                file_path: None,
                file_name: None,
                content_type: None,
                enabled: false,
            },
            MultipartPart {
                id: "bytes-part".into(),
                name: "thumbnail".into(),
                kind: MultipartPartKind::Binary,
                value: ValueSource::literal("AAH/"),
                file_path: None,
                file_name: Some("thumb.bin".into()),
                content_type: Some("application/octet-stream".into()),
                enabled: true,
            },
        ],
    };

    let mut form = Request::new(
        id("form"),
        "Sign in",
        "POST",
        "https://api.example.test/session",
    );
    form.body = RequestBody::FormUrlEncoded {
        fields: vec![
            RequestValueField::enabled("user", ValueSource::literal("operator")),
            sensitive_field("password", secret("session.password")),
        ],
    };

    let mut socket = Request::new(
        id("socket"),
        "Stock feed",
        "GET",
        "wss://api.example.test/feed",
    );
    socket.web_socket = true;

    let mut api_key = Request::new(
        id("key"),
        "Report",
        "GET",
        "https://api.example.test/report",
    );
    api_key.authentication = RequestAuthentication::ApiKey {
        placement: ApiKeyPlacement::Query,
        name: "key".into(),
        value: secret("report.key"),
    };

    vec![raw, file, multipart, form, socket, api_key]
}

fn snapshot() -> WorkspaceSnapshot {
    let (collection, requests) = api_collection();
    let mut root = Collection::new(id("workspace-root"), "Requests".into());
    root.order = i64::MIN;
    let mut billing = Collection::new(id("billing"), "Billing".into());
    billing.groups = vec![Group::new(id("invoices"), "Invoices".into(), None, 0)];
    let mut invoice = Request::new(
        id("invoice"),
        "List invoices",
        "GET",
        "https://billing.example.test/invoices",
    );
    invoice.group_id = Some(id("invoices"));
    let variable = |key: &str, value: ValueSource, order: i64| EnvironmentVariable {
        id: format!("variable-{key}"),
        key: key.into(),
        value,
        enabled: true,
        order,
    };
    WorkspaceSnapshot {
        workspace: Workspace::new("Store"),
        collections: vec![
            CollectionSnapshot {
                collection: root,
                requests: vec![Request::new(
                    id("health"),
                    "Health",
                    "GET",
                    "https://api.example.test/health",
                )],
            },
            CollectionSnapshot {
                collection,
                requests,
            },
            CollectionSnapshot {
                collection: billing,
                requests: vec![invoice],
            },
        ],
        environments: vec![
            Environment::from_rows(
                id("global"),
                "Global".into(),
                vec![variable(
                    "baseUrl",
                    ValueSource::literal("https://api.example.test"),
                    0,
                )],
            ),
            Environment::from_rows(
                id("staging"),
                "Staging".into(),
                vec![
                    variable("quantity", ValueSource::literal("2"), 0),
                    EnvironmentVariable {
                        enabled: false,
                        ..variable("apiToken", secret("staging.api-token"), 1)
                    },
                ],
            ),
        ],
        problems: vec![],
    }
}

/// Converts imported requests back into saved requests, keyed by name.
fn restored(
    collection: &ImportedCollection,
    settings: &ImportedWorkspace,
    originals: &[Request],
    original_groups: &[Group],
) -> BTreeMap<String, Request> {
    collection
        .requests
        .iter()
        .map(|imported| {
            let original = originals
                .iter()
                .find(|request| request.name == imported.name)
                .expect("imported request exists in the source");
            let group_name = imported.group_source_id.as_ref().map(|source| {
                collection
                    .groups
                    .iter()
                    .find(|group| &group.source_id == source)
                    .unwrap()
                    .name
                    .clone()
            });
            let original_group = original.group_id.as_ref().map(|group| {
                original_groups
                    .iter()
                    .find(|candidate| &candidate.id == group)
                    .unwrap()
                    .name
                    .clone()
            });
            assert_eq!(group_name, original_group, "{}", imported.name);
            let mut request = imported
                .clone()
                .into_request(original.id.clone(), original.group_id.clone());
            if let Some(settings) = settings.request_settings.get(&imported.source_id) {
                settings.clone().apply(&mut request);
            }
            request.order = original.order;
            (request.name.clone(), request)
        })
        .collect()
}

fn assert_same_requests(restored: &BTreeMap<String, Request>, originals: &[Request]) {
    assert_eq!(restored.len(), originals.len());
    for original in originals {
        assert_eq!(&restored[&original.name], original, "{}", original.name);
    }
}

#[test]
fn workspace_export_reimports_collections_environments_and_complete_requests() {
    let snapshot = snapshot();
    let exported = export_legacy_v1_workspace(&snapshot).unwrap();
    for secret in ["staging.api-token", "inventory.token", "proxy.password"] {
        assert!(exported.contains(secret), "reference {secret} is exported");
    }

    let imported = ImportEngine::parse_workspace_file(
        ImportFormat::LegacyWorkspaceV1,
        &exported,
        "store-export",
    )
    .unwrap();
    let names: Vec<_> = imported
        .collections
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["store-export", "Inventory API", "Billing"]);
    assert!(imported.collections.iter().all(|c| c.warnings.is_empty()));

    for (collection, source) in imported.collections.iter().zip(&snapshot.collections) {
        let group_names: Vec<_> = collection.groups.iter().map(|g| g.name.as_str()).collect();
        let source_names: Vec<_> = source
            .collection
            .groups
            .iter()
            .map(|g| g.name.as_str())
            .collect();
        assert_eq!(group_names, source_names, "no synthetic collection folder");
        assert_same_requests(
            &restored(
                collection,
                &imported,
                &source.requests,
                &source.collection.groups,
            ),
            &source.requests,
        );
    }
    let api = &imported.collections[1];
    let orders = api.groups.iter().find(|g| g.name == "Orders").unwrap();
    let archive = api.groups.iter().find(|g| g.name == "Archive").unwrap();
    assert_eq!(orders.parent_source_id, None);
    assert_eq!(archive.parent_source_id.as_ref(), Some(&orders.source_id));

    assert_eq!(imported.environments.len(), 2);
    for (environment, source) in imported.environments.iter().zip(&snapshot.environments) {
        assert_eq!(environment.name, source.name);
        assert_eq!(environment.variables, source.variables);
    }
    assert!(imported.environments[0].global);
    assert!(!imported.environments[1].global);
}

#[test]
fn collection_and_request_exports_reimport_without_an_extra_folder() {
    let (collection, requests) = api_collection();
    let exported = export_legacy_v1_collection(&collection, &requests).unwrap();
    for imported in [
        ImportEngine::parse_workspace_file(
            ImportFormat::LegacyWorkspaceV1,
            &exported,
            "renamed-file",
        )
        .unwrap(),
        ImportEngine::parse_file(ImportFormat::LegacyWorkspaceV1, &exported, "renamed-file")
            .unwrap()
            .into(),
    ] {
        assert_eq!(imported.collections.len(), 1);
        let restored_collection = &imported.collections[0];
        assert_eq!(restored_collection.name, "Inventory API");
        assert_eq!(restored_collection.groups.len(), collection.groups.len());
        if !imported.request_settings.is_empty() {
            assert_same_requests(
                &restored(
                    restored_collection,
                    &imported,
                    &requests,
                    &collection.groups,
                ),
                &requests,
            );
        }
    }

    for request in &requests {
        let exported = export_legacy_v1_request(request).unwrap();
        let imported = ImportEngine::parse_workspace_file(
            ImportFormat::LegacyWorkspaceV1,
            &exported,
            "request",
        )
        .unwrap();
        assert_eq!(imported.collections.len(), 1);
        let mut restored = imported.collections[0].requests[0]
            .clone()
            .into_request(request.id.clone(), request.group_id.clone());
        imported.request_settings[&imported.collections[0].requests[0].source_id]
            .clone()
            .apply(&mut restored);
        restored.order = request.order;
        assert_eq!(&restored, request);
    }
}

#[test]
fn upload_files_are_exported_as_paths_and_missing_files_only_warn() {
    let missing = "/Users/example/Missing/large-video.mov";
    let mut request = Request::new(
        id("upload"),
        "Upload video",
        "POST",
        "https://api.example.test/videos",
    );
    request.body = RequestBody::Multipart {
        parts: vec![file_part("video", missing, Some("video/quicktime"))],
    };
    let collection = Collection::new(id("media"), "Media".into());
    let exported = export_legacy_v1_collection(&collection, std::slice::from_ref(&request))
        .expect("a missing upload file never fails the export");
    assert!(exported.contains(missing));

    let imported =
        ImportEngine::parse_workspace_file(ImportFormat::LegacyWorkspaceV1, &exported, "media")
            .unwrap();
    let collection = &imported.collections[0];
    assert_eq!(collection.requests[0].body, request.body);
    assert_eq!(collection.warnings.len(), 1);
    assert!(collection.warnings[0].contains(missing));
    assert!(collection.warnings[0].contains("Upload video"));
}

#[test]
fn exports_written_before_the_complete_definition_still_import() {
    let fixture = include_str!("fixtures/wirebolt-collection-export-0.1.0.json");
    let imported = ImportEngine::parse_workspace_file(
        ImportFormat::LegacyWorkspaceV1,
        fixture,
        "My API export",
    )
    .unwrap();
    assert_eq!(imported.collections.len(), 1);
    let collection = &imported.collections[0];
    assert_eq!(collection.name, "My API");
    let groups: Vec<_> = collection.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(groups, ["Outer", "Inner"]);
    assert!(imported.request_settings.is_empty());

    let request = |name: &str| collection.requests.iter().find(|r| r.name == name).unwrap();
    let search = request("Headers+query");
    assert_eq!(search.url, "https://api.test/items?fixed=1");
    assert_eq!(search.query.len(), 2);
    assert_eq!(
        request("Bearer secret").authentication,
        RequestAuthentication::Bearer {
            token: secret("tok.ref")
        }
    );
    assert!(
        request("Bearer secret")
            .group_source_id
            .as_ref()
            .is_some_and(|group| collection
                .groups
                .iter()
                .any(|g| &g.source_id == group && g.name == "Inner"))
    );
    assert_eq!(
        collection.warnings,
        [
            "“File body” uploads /Users/example/Documents/upload.txt, which does not exist on this Mac. Choose the file again before sending."
        ]
    );
}
