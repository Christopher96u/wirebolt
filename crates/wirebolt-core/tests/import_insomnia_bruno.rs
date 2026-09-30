//! Insomnia and Bruno imports from realistic exports (synthetic data).

use std::path::Path;

use wirebolt_core::{
    ApiKeyPlacement, EnvironmentVariable, ImportEngine, ImportFormat, ImportedCollection,
    ImportedRequest, ImportedWorkspace, MultipartPartKind, Oauth2Grant, RequestAuthentication,
    RequestBody, RequestHeader, RequestValueField, ValueSource,
};

fn fixture(path: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(path),
    )
    .expect("fixture")
}

fn request<'a>(collection: &'a ImportedCollection, name: &str) -> &'a ImportedRequest {
    collection
        .requests
        .iter()
        .find(|request| request.name == name)
        .unwrap_or_else(|| panic!("request {name}"))
}

fn group_name<'a>(
    collection: &'a ImportedCollection,
    request: &ImportedRequest,
) -> Option<&'a str> {
    let id = request.group_source_id.as_deref()?;
    collection
        .groups
        .iter()
        .find(|group| group.source_id == id)
        .map(|group| group.name.as_str())
}

fn literal(value: &str) -> ValueSource {
    ValueSource::literal(value)
}

fn header<'a>(request: &'a ImportedRequest, name: &str) -> &'a RequestHeader {
    request
        .headers
        .iter()
        .find(|header| header.name == name)
        .unwrap_or_else(|| panic!("header {name}"))
}

fn rows(fields: &[RequestValueField]) -> Vec<(&str, &ValueSource, bool)> {
    fields
        .iter()
        .map(|field| (field.name.as_str(), &field.value, field.enabled))
        .collect()
}

fn variables(variables: &[EnvironmentVariable]) -> Vec<(&str, &ValueSource, bool)> {
    variables
        .iter()
        .map(|variable| (variable.key.as_str(), &variable.value, variable.enabled))
        .collect()
}

fn warned(collection: &ImportedCollection, needle: &str) -> bool {
    collection
        .warnings
        .iter()
        .any(|warning| warning.contains(needle))
}

fn only_collection(workspace: &ImportedWorkspace) -> &ImportedCollection {
    assert_eq!(workspace.collections.len(), 1);
    &workspace.collections[0]
}

// MARK: - Insomnia v4

fn insomnia_v4() -> ImportedWorkspace {
    ImportEngine::parse_workspace_file(
        ImportFormat::Insomnia,
        &fixture("insomnia/insomnia-v4-export.json"),
        "insomnia-v4-export",
    )
    .expect("parse Insomnia v4")
}

#[test]
fn insomnia_v4_keeps_folders_order_and_request_details() {
    let workspace = insomnia_v4();
    let collection = only_collection(&workspace);
    assert_eq!(collection.name, "Storefront API");
    let groups = collection
        .groups
        .iter()
        .map(|group| group.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(groups, ["Orders", "Fulfilment"]);
    assert_eq!(
        collection.groups[1].parent_source_id.as_deref(),
        Some(collection.groups[0].source_id.as_str())
    );

    let create = request(collection, "Create order");
    assert_eq!(group_name(collection, create), Some("Orders"));
    assert_eq!(create.method, "POST");
    assert_eq!(create.url, "{{base_url}}/v1/orders");
    assert_eq!(
        create.note,
        "Creates an order for the **current** customer."
    );
    assert_eq!(
        rows(&create.query),
        [
            ("dry_run", &literal("false"), true),
            ("trace", &literal("1"), false)
        ]
    );
    assert!(!header(create, "X-Request-Source").enabled);
    // Folder headers are inherited; `{}` authentication inherits the folder's bearer token.
    assert_eq!(header(create, "X-Tenant").value, literal("{{tenant}}"));
    assert_eq!(
        create.authentication,
        RequestAuthentication::Bearer {
            token: literal("{{access_token}}")
        }
    );
    let RequestBody::Json { value } = &create.body else {
        panic!("JSON body");
    };
    assert!(value.contains("\"quantity\": {{quantity}}"));
    assert!(value.contains("{% uuid 'v4' %}"));

    // Sibling order follows metaSortKey, with folders and requests sharing one order.
    let orders = collection
        .requests
        .iter()
        .filter(|request| group_name(collection, request) == Some("Orders"))
        .map(|request| (request.name.as_str(), request.order))
        .collect::<Vec<_>>();
    assert_eq!(orders, [("Create order", 0), ("Get order", 1)]);
    assert_eq!(collection.groups[1].order, 2);

    let get = request(collection, "Get order");
    assert_eq!(get.url, "{{base_url}}/v1/orders/{{last_order_id}}");
    assert_eq!(get.authentication, RequestAuthentication::None);
    assert_eq!(get.body, RequestBody::Empty);
}

#[test]
fn insomnia_v4_maps_every_body_and_authentication_type() {
    let workspace = insomnia_v4();
    let collection = only_collection(&workspace);

    let shipment = request(collection, "Book shipment");
    assert_eq!(group_name(collection, shipment), Some("Fulfilment"));
    // The nearest folder's authentication wins over the outer folder's.
    assert_eq!(
        shipment.authentication,
        RequestAuthentication::Basic {
            username: literal("warehouse"),
            password: literal("demo-password"),
        }
    );
    let RequestBody::FormUrlEncoded { fields } = &shipment.body else {
        panic!("form body");
    };
    assert_eq!(
        rows(fields),
        [
            ("carrier", &literal("dhl"), true),
            ("express", &literal("true"), false),
            ("weight_kg", &literal("{{parcel.weight}}"), true),
        ]
    );

    let upload = request(collection, "Upload product image");
    let RequestBody::Multipart { parts } = &upload.body else {
        panic!("multipart body");
    };
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[1].kind, MultipartPartKind::File);
    assert_eq!(
        parts[1].file_path.as_deref(),
        Some("/Users/demo/Pictures/lamp.png")
    );
    assert_eq!(parts[1].file_name.as_deref(), Some("lamp.png"));
    assert!(!parts[2].enabled);
    assert_eq!(
        upload.authentication,
        RequestAuthentication::ApiKey {
            placement: ApiKeyPlacement::Query,
            name: "api_key".to_owned(),
            value: literal("demo-api-key"),
        }
    );

    let search = request(collection, "Search products");
    let RequestBody::Json { value } = &search.body else {
        panic!("GraphQL as JSON");
    };
    let document: serde_json::Value = serde_json::from_str(value).expect("GraphQL JSON");
    assert_eq!(document["variables"]["term"], "lamp");
    let RequestAuthentication::Oauth2 { configuration } = &search.authentication else {
        panic!("OAuth 2.0");
    };
    assert_eq!(configuration.grant, Oauth2Grant::ClientCredentials);
    assert_eq!(
        configuration.token_url,
        "https://auth.example.test/oauth/token"
    );
    assert_eq!(configuration.client_id, "storefront-cli");
    assert_eq!(configuration.audience, "https://api.example.test");
    assert!(
        configuration
            .client_secret_reference
            .as_str()
            .starts_with("oauth.")
    );
    assert!(warned(collection, "client secrets aren’t imported"));

    let file = request(collection, "Import catalog file");
    assert_eq!(
        file.body,
        RequestBody::File {
            path: "/Users/demo/Documents/catalog.csv".to_owned(),
            content_type: None,
        }
    );
    assert_eq!(file.authentication, RequestAuthentication::None);
    assert!(warned(collection, "Digest authentication isn’t supported"));

    let soap = request(collection, "Legacy inventory (SOAP)");
    assert!(matches!(&soap.body, RequestBody::Xml { value } if value.starts_with("<?xml")));
    assert_eq!(
        header(soap, "Authorization").value,
        literal(
            "Bearer {% response 'body', 'req_e5b4a6f8c0d27e4b1f5a6b7c8d9eafb3', 'b64::JC5hY2Nlc3NfdG9rZW4=::46b', 'never', 60 %}"
        )
    );
    assert!(warned(collection, "template tags"));

    let socket = request(collection, "Order events");
    assert!(socket.web_socket);
    assert_eq!(socket.method, "GET");
    assert_eq!(socket.url, "wss://stream.example.test/orders");
    assert_eq!(rows(&socket.query), [("since", &literal("now"), true)]);
}

#[test]
fn insomnia_v4_warns_about_what_it_cannot_import() {
    let workspace = insomnia_v4();
    let collection = only_collection(&workspace);
    assert!(
        !collection
            .requests
            .iter()
            .any(|request| request.name.contains("gRPC"))
    );
    for needle in [
        "gRPC requests aren’t supported: “Inventory stream (gRPC)”",
        "Scripts aren’t run by Wirebolt and were not imported: “Create order”",
        "Folder environment variables were not imported: “Orders”",
        "Saved WebSocket messages were not imported",
        "Insomnia unit tests were not imported",
        "Cookies were not imported",
    ] {
        assert!(warned(collection, needle), "missing warning: {needle}");
    }
}

#[test]
fn insomnia_v4_layers_sub_environments_over_the_base_environment() {
    let workspace = insomnia_v4();
    let names = workspace
        .environments
        .iter()
        .map(|environment| environment.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["Storefront API – Staging", "Storefront API – Production"]
    );
    assert!(
        workspace
            .environments
            .iter()
            .all(|environment| !environment.global)
    );

    let staging = &workspace.environments[0];
    assert_eq!(
        variables(&staging.variables),
        [
            ("base_url", &literal("https://staging.example.test"), true),
            ("legacy_url", &literal("{{base_url}}/legacy"), true),
            ("quantity", &literal("1"), true),
            ("parcel.weight", &literal("2.5"), true),
            ("tenant", &literal("demo"), true),
            ("access_token", &literal("{{staging_token}}"), true),
            ("last_order_id", &literal("ord_1001"), true),
        ]
    );

    // Table (kv) environments keep disabled rows; vault secrets become references.
    let production = &workspace.environments[1];
    let debug = production
        .variables
        .iter()
        .find(|variable| variable.key == "debug")
        .expect("debug");
    assert!(!debug.enabled);
    let token = production
        .variables
        .iter()
        .find(|variable| variable.key == "access_token")
        .expect("token");
    assert!(
        matches!(&token.value, ValueSource::Secret { secret } if secret.as_str() == "storefront-api.production.access_token")
    );
}

// MARK: - Insomnia v5

#[test]
fn insomnia_v5_yaml_collection_imports_folders_auth_and_environments() {
    let workspace = ImportEngine::parse_workspace_file(
        ImportFormat::Insomnia,
        &fixture("insomnia/insomnia-v5-collection.yaml"),
        "payments",
    )
    .expect("parse Insomnia v5");
    let collection = only_collection(&workspace);
    assert_eq!(collection.name, "Payments API");
    assert_eq!(collection.groups.len(), 1);
    assert_eq!(collection.groups[0].name, "Customers");

    let cards = request(collection, "List cards");
    assert_eq!(cards.url, "{{baseUrl}}/customers/cus_42/cards");
    assert_eq!(cards.note, "Lists the saved cards for a customer.");
    assert_eq!(
        rows(&cards.query),
        [
            ("limit", &literal("20"), true),
            ("expand", &literal("brand"), false)
        ]
    );
    assert_eq!(header(cards, "X-Api-Version").value, literal("2026-01-01"));
    assert_eq!(
        cards.authentication,
        RequestAuthentication::ApiKey {
            placement: ApiKeyPlacement::Header,
            name: "X-Api-Key".to_owned(),
            value: literal("{{api_key}}"),
        }
    );

    let create = request(collection, "Create customer");
    assert!(
        matches!(&create.body, RequestBody::Json { value } if value.contains("\"name\": \"{{customer.name}}\""))
    );
    assert_eq!(
        create.authentication,
        RequestAuthentication::Basic {
            username: literal("{{api_user}}"),
            password: literal("demo-password"),
        }
    );

    let pkce = request(collection, "Authorize with PKCE");
    let RequestAuthentication::Oauth2 { configuration } = &pkce.authentication else {
        panic!("OAuth 2.0");
    };
    assert_eq!(configuration.grant, Oauth2Grant::AuthorizationCodePkce);
    assert_eq!(
        configuration.authorization_url,
        "https://auth.example.test/authorize"
    );
    assert_eq!(configuration.redirect_uri, "http://127.0.0.1:8765/callback");
    assert_eq!(configuration.scopes, "payments:read offline_access");

    let refund = request(collection, "Refund payment");
    assert_eq!(refund.method, "DELETE");
    assert_eq!(
        refund.body,
        RequestBody::Text {
            content_type: Some("text/plain".to_owned()),
            value: "duplicate charge".to_owned(),
        }
    );
    assert!(request(collection, "Payment events").web_socket);
    assert!(warned(
        collection,
        "Scripts aren’t run by Wirebolt and were not imported: “Create customer”"
    ));
    assert!(warned(collection, "Cookies were not imported"));

    let names = workspace
        .environments
        .iter()
        .map(|environment| environment.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Payments API – Local", "Payments API – Production"]);
    assert_eq!(
        variables(&workspace.environments[0].variables),
        [
            ("baseUrl", &literal("http://127.0.0.1:18990"), true),
            ("api_user", &literal("demo-user"), true),
            ("customer.name", &literal("Ada Lovelace"), true),
            ("token", &literal("local-demo-token"), true),
        ]
    );
    // A filter expression is kept verbatim and reported.
    assert!(
        workspace.environments[1]
            .variables
            .iter()
            .any(|variable| variable.value == literal("{{ _.prod_token | trim }}"))
    );
    assert!(warned(collection, "template tags"));
}

#[test]
fn insomnia_rejects_other_documents_with_a_reason() {
    for (source, reason) in [
        (r#"{"__export_format": 3, "resources": []}"#, "too old"),
        (
            "type: environment.insomnia.rest/5.0\nname: Globals\n",
            "only Insomnia collections",
        ),
        (r#"{"info": {"name": "Postman"}}"#, "not an Insomnia export"),
        (
            r#"{"_type":"export","__export_format":4,"resources":[{"_id":"wrk_1","_type":"workspace","name":"Empty"}]}"#,
            "no requests",
        ),
    ] {
        let error = ImportEngine::parse(ImportFormat::Insomnia, source).expect_err(reason);
        assert!(error.to_string().contains(reason), "{error}");
    }
}

// MARK: - Bruno folder

fn bruno_folder_source(root: &Path) -> String {
    fn collect(root: &Path, directory: &Path, files: &mut Vec<(String, String)>) {
        let mut entries = std::fs::read_dir(directory)
            .expect("read directory")
            .map(|entry| entry.expect("entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                collect(root, &path, files);
            } else {
                let relative = path.strip_prefix(root).expect("relative");
                files.push((
                    relative.to_string_lossy().into_owned(),
                    std::fs::read_to_string(&path).expect("file"),
                ));
            }
        }
    }
    let mut files = Vec::new();
    collect(root, root, &mut files);
    ImportEngine::bruno_folder_source("/Users/demo/Library API", &files)
}

fn bruno_folder() -> ImportedWorkspace {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bruno/collection");
    ImportEngine::parse_workspace_file(
        ImportFormat::BrunoFolder,
        &bruno_folder_source(&root),
        "collection",
    )
    .expect("parse Bruno folder")
}

#[test]
fn bruno_folder_keeps_folders_sequence_and_inheritance() {
    let workspace = bruno_folder();
    let collection = only_collection(&workspace);
    assert_eq!(collection.name, "Library API");
    let groups = collection
        .groups
        .iter()
        .map(|group| (group.name.as_str(), group.order))
        .collect::<Vec<_>>();
    assert_eq!(groups, [("Members", 0), ("Uploads", 1), ("Misc", 2)]);

    let members = collection
        .requests
        .iter()
        .filter(|request| group_name(collection, request) == Some("Members"))
        .map(|request| request.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(members, ["Get member", "Create member"]);

    let create = request(collection, "Create member");
    assert_eq!(create.method, "POST");
    // Query rows replace the URL's query text, including disabled rows.
    assert_eq!(create.url, "{{baseUrl}}/{{apiVersion}}/members");
    assert_eq!(
        rows(&create.query),
        [
            ("notify", &literal("true"), true),
            ("draft", &literal("1"), false)
        ]
    );
    // Collection and folder headers are inherited; the description decorator is skipped.
    let headers = create
        .headers
        .iter()
        .map(|header| (header.name.as_str(), header.enabled))
        .collect::<Vec<_>>();
    assert_eq!(
        headers,
        [
            ("X-Client", true),
            ("X-Section", true),
            ("Idempotency-Key", true),
            ("X-Debug", false),
        ]
    );
    assert_eq!(
        create.authentication,
        RequestAuthentication::Basic {
            username: literal("librarian"),
            password: literal("{{librarianPassword}}"),
        }
    );
    assert_eq!(
        create.body,
        RequestBody::Json {
            value: "{\n  \"name\": \"Ada Lovelace\",\n  \"tags\": [\"math\", \"poetry\"]\n}"
                .to_owned()
        }
    );
    assert_eq!(
        create.note,
        "Creates a library member.\n\nReturns the new member `id`."
    );

    let get = request(collection, "Get member");
    assert_eq!(get.url, "{{baseUrl}}/{{apiVersion}}/members/{{memberId}}");
    assert_eq!(get.authentication, RequestAuthentication::None);

    // A top-level request inherits the collection's bearer token.
    let ping = request(collection, "Ping");
    assert_eq!(ping.group_source_id, None);
    assert_eq!(
        ping.authentication,
        RequestAuthentication::Bearer {
            token: literal("{{accessToken}}")
        }
    );
}

#[test]
fn bruno_folder_maps_bodies_and_authentication() {
    let workspace = bruno_folder();
    let collection = only_collection(&workspace);

    let cover = request(collection, "Upload cover");
    assert_eq!(cover.url, "{{baseUrl}}/books/9780000000001/cover");
    let RequestBody::Multipart { parts } = &cover.body else {
        panic!("multipart");
    };
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].value, literal("Demo cover"));
    assert_eq!(parts[1].kind, MultipartPartKind::File);
    // Upload paths are resolved against the collection folder.
    assert_eq!(
        parts[1].file_path.as_deref(),
        Some("/Users/demo/Library API/assets/cover.png")
    );
    assert_eq!(parts[1].content_type.as_deref(), Some("image/png"));
    assert!(!parts[2].enabled);
    assert_eq!(
        cover.authentication,
        RequestAuthentication::ApiKey {
            placement: ApiKeyPlacement::Header,
            name: "X-Api-Key".to_owned(),
            value: literal("{{apiKey}}"),
        }
    );

    let catalogue = request(collection, "Replace catalogue");
    assert_eq!(
        catalogue.body,
        RequestBody::File {
            path: "/Users/demo/Library API/data/catalogue.csv".to_owned(),
            content_type: Some("text/csv".to_owned()),
        }
    );
    let RequestAuthentication::Oauth2 { configuration } = &catalogue.authentication else {
        panic!("OAuth 2.0");
    };
    assert_eq!(configuration.grant, Oauth2Grant::ClientCredentials);
    assert_eq!(configuration.client_id, "library-importer");
    assert_eq!(configuration.scopes, "catalogue:write");

    let login = request(collection, "Login form");
    let RequestBody::FormUrlEncoded { fields } = &login.body else {
        panic!("form");
    };
    assert_eq!(
        rows(fields),
        [
            ("username", &literal("reader"), true),
            ("password", &literal("{{readerPassword}}"), true),
            ("remember", &literal("yes"), false),
        ]
    );

    let search = request(collection, "Search books");
    let RequestBody::Json { value } = &search.body else {
        panic!("GraphQL JSON");
    };
    let document: serde_json::Value = serde_json::from_str(value).expect("JSON");
    assert!(
        document["query"]
            .as_str()
            .unwrap()
            .starts_with("query Search($term: String!)")
    );
    assert_eq!(document["variables"]["term"], "engine");

    let report = request(collection, "Stock report");
    assert_eq!(report.method, "REPORT");
    assert_eq!(
        report.body,
        RequestBody::Xml {
            value: "<report><branch>central</branch></report>".to_owned()
        }
    );
    assert_eq!(report.authentication, RequestAuthentication::None);

    let loans = request(collection, "Live loans");
    assert!(loans.web_socket);
    assert_eq!(loans.url, "wss://library.example.test/loans");
    assert!(matches!(
        loans.authentication,
        RequestAuthentication::Bearer { .. }
    ));

    for needle in [
        "gRPC requests aren’t supported: “Book service”",
        "Scripts, tests and assertions aren’t run by Wirebolt and were not imported: “Library API”, “Create member”",
        "Request variables were not imported: “Create member”",
        "Digest authentication isn’t supported",
        "Saved WebSocket messages were not imported: “Live loans”",
        "Bruno dynamic variables",
        "OAuth 2.0 client secrets aren’t imported",
        "Collection and folder documentation was not imported",
    ] {
        assert!(
            warned(collection, needle),
            "missing warning: {needle}\n{:#?}",
            collection.warnings
        );
    }
}

#[test]
fn bruno_folder_environments_layer_collection_variables_and_reference_secrets() {
    let workspace = bruno_folder();
    let names = workspace
        .environments
        .iter()
        .map(|environment| environment.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Library API – Local", "Library API – Production"]);
    let local = &workspace.environments[0];
    assert_eq!(
        variables(&local.variables)[..4],
        [
            ("apiVersion", &literal("v2-local"), true),
            ("legacyFlag", &literal("true"), false),
            ("baseUrl", &literal("http://127.0.0.1:18990"), true),
            ("verbose", &literal("true"), false),
        ]
    );
    let certificate = &local.variables[4];
    assert_eq!(
        certificate.value,
        literal("-----BEGIN DEMO-----\nnot-a-real-certificate\n-----END DEMO-----")
    );
    let token = &local.variables[5];
    assert_eq!(token.key, "accessToken");
    assert!(
        matches!(&token.value, ValueSource::Secret { secret } if secret.as_str() == "library-api.local.accessToken")
    );

    let production = &workspace.environments[1];
    let api_key = production
        .variables
        .iter()
        .find(|variable| variable.key == "apiKey")
        .expect("apiKey");
    assert!(!api_key.enabled);
    assert!(matches!(api_key.value, ValueSource::Secret { .. }));
}

#[test]
fn bruno_folder_rejects_folders_without_bru_files() {
    let source = ImportEngine::bruno_folder_source(
        "/tmp/not-bruno",
        &[("README.md".to_owned(), "# Notes".to_owned())],
    );
    assert!(ImportEngine::parse(ImportFormat::BrunoFolder, &source).is_err());
    let yaml = ImportEngine::bruno_folder_source(
        "/tmp/yaml",
        &[("opencollection.yml".to_owned(), "name: x".to_owned())],
    );
    let error = ImportEngine::parse(ImportFormat::BrunoFolder, &yaml).expect_err("YAML");
    assert!(error.to_string().contains("YAML"));
}

#[test]
fn a_single_bru_file_imports_as_one_request() {
    let source = ImportEngine::bruno_folder_source(
        "/Users/demo/Downloads",
        &[("Ping.bru".to_owned(), fixture("bruno/collection/Ping.bru"))],
    );
    let workspace = ImportEngine::parse_workspace_file(ImportFormat::BrunoFolder, &source, "Ping")
        .expect("single file");
    let collection = only_collection(&workspace);
    assert_eq!(collection.name, "Ping");
    assert_eq!(collection.requests.len(), 1);
    assert_eq!(collection.requests[0].url, "{{baseUrl}}/ping");
}

// MARK: - Bruno export JSON

fn bruno_export() -> ImportedWorkspace {
    ImportEngine::parse_workspace_file(
        ImportFormat::Bruno,
        &fixture("bruno/bruno-collection-export.json"),
        "Weather Service",
    )
    .expect("parse Bruno export")
}

#[test]
fn bruno_export_json_imports_items_and_inherited_settings() {
    let workspace = bruno_export();
    let collection = only_collection(&workspace);
    assert_eq!(collection.name, "Weather Service");
    assert_eq!(collection.groups.len(), 1);
    let order = collection
        .requests
        .iter()
        .map(|request| (request.name.as_str(), request.order))
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        [
            ("Current conditions", 0),
            ("Daily forecast", 1),
            ("Report observation", 1),
            ("Alerts query", 2),
            ("Upload station photos", 3),
        ]
    );

    let daily = request(collection, "Daily forecast");
    assert_eq!(daily.url, "{{baseUrl}}/forecast/lisbon");
    assert_eq!(
        rows(&daily.query),
        [
            ("days", &literal("7"), true),
            ("hourly", &literal("true"), false)
        ]
    );
    assert_eq!(daily.note, "Seven-day forecast for a city.");
    assert_eq!(
        daily.authentication,
        RequestAuthentication::ApiKey {
            placement: ApiKeyPlacement::Query,
            name: "appid".to_owned(),
            value: literal("{{owmKey}}"),
        }
    );
    assert_eq!(header(daily, "User-Agent").value, literal("bruno-demo"));
    assert_eq!(header(daily, "Accept-Language").value, literal("en"));
    assert!(!header(daily, "X-Debug").enabled);

    let current = request(collection, "Current conditions");
    assert_eq!(
        current.authentication,
        RequestAuthentication::Bearer {
            token: literal("{{weatherToken}}")
        }
    );

    let report = request(collection, "Report observation");
    assert!(
        matches!(&report.body, RequestBody::Json { value } if value.contains("{{temperature}}"))
    );
    let RequestAuthentication::Oauth2 { configuration } = &report.authentication else {
        panic!("OAuth 2.0");
    };
    assert_eq!(configuration.grant, Oauth2Grant::AuthorizationCodePkce);
    assert_eq!(
        configuration.redirect_uri,
        "https://oauth.example.test/callback"
    );

    let photos = request(collection, "Upload station photos");
    let RequestBody::Multipart { parts } = &photos.body else {
        panic!("multipart");
    };
    let files = parts
        .iter()
        .filter(|part| part.kind == MultipartPartKind::File)
        .map(|part| part.file_path.as_deref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(files, ["photos/sky.jpg", "/Users/demo/sky-2.jpg"]);
}

#[test]
fn bruno_export_json_reports_unsupported_items_and_imports_environments() {
    let workspace = bruno_export();
    let collection = only_collection(&workspace);
    for needle in [
        "Upload file paths are relative to the Bruno collection folder",
        "AWS Signature v4 authentication isn’t supported",
        "Folder variables were not imported: “Forecasts”",
        "Post-response variables were not imported: “Current conditions”",
        "Bruno doesn’t export secret values",
        "Scripts, tests and assertions aren’t run by Wirebolt and were not imported: “Weather Service”, “Current conditions”",
    ] {
        assert!(
            warned(collection, needle),
            "missing warning: {needle}\n{:#?}",
            collection.warnings
        );
    }

    assert_eq!(workspace.environments.len(), 1);
    let staging = &workspace.environments[0];
    assert_eq!(staging.name, "Weather Service – Staging");
    assert_eq!(staging.variables[0].key, "baseUrl");
    assert_eq!(
        staging.variables[0].value,
        literal("https://staging.weather.example.test")
    );
    assert!(matches!(
        staging.variables[1].value,
        ValueSource::Secret { .. }
    ));
    assert!(!staging.variables[2].enabled);
}

#[test]
fn bruno_export_rejects_other_json() {
    assert!(ImportEngine::parse(ImportFormat::Bruno, r#"{"version": 1, "nodes": []}"#).is_err());
    assert!(ImportEngine::parse(ImportFormat::Bruno, "not json").is_err());
}
