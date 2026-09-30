//! Import fidelity for Postman, HAR and cURL sources, using synthetic
//! fixtures shaped like real exports.

use wirebolt_core::{
    ApiKeyPlacement, ImportEngine, ImportFormat, ImportedCollection, ImportedRequest,
    MultipartPartKind, Oauth2Grant, ParsedImport, RequestAuthentication, RequestBody,
    TransportSettings, ValueSource,
};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/import/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn request<'a>(collection: &'a ImportedCollection, name: &str) -> &'a ImportedRequest {
    collection
        .requests
        .iter()
        .find(|request| request.name == name)
        .unwrap_or_else(|| panic!("no request named {name}"))
}

fn literal(value: &ValueSource) -> &str {
    match value {
        ValueSource::Literal(value) => value,
        ValueSource::Secret { .. } => panic!("expected a literal"),
    }
}

fn warning<'a>(collection: &'a ImportedCollection, fragment: &str) -> &'a str {
    collection
        .warnings
        .iter()
        .find(|warning| warning.contains(fragment))
        .unwrap_or_else(|| {
            panic!(
                "no warning containing {fragment:?} in {:#?}",
                collection.warnings
            )
        })
}

fn postman_v21_import() -> ParsedImport {
    ImportEngine::parse_import(
        ImportFormat::PostmanV2,
        &fixture("postman_v21_shop.postman_collection.json"),
        Some("shop"),
    )
    .expect("parse Postman v2.1")
}

fn postman_v21() -> ImportedCollection {
    postman_v21_import().workspace.collections.remove(0)
}

fn secret_material<'a>(parsed: &'a ParsedImport, value: &ValueSource) -> &'a str {
    let ValueSource::Secret { secret } = value else {
        panic!("expected a secret reference, found {value:?}");
    };
    parsed
        .secrets
        .iter()
        .find(|imported| &imported.reference == secret)
        .map(|imported| imported.material.as_str())
        .expect("secret material travels with the import")
}

fn transport<'a>(parsed: &'a ParsedImport, source_id: &str) -> Option<&'a TransportSettings> {
    parsed
        .workspace
        .request_settings
        .get(source_id)
        .map(|settings| {
            assert!(!settings.inherits_workspace_transport);
            &settings.transport
        })
}

#[test]
fn postman_auth_is_mapped_and_inherited_through_folders() {
    let collection = postman_v21();

    // Folder basic auth reaches a request that inherits it.
    assert_eq!(
        request(&collection, "Get customer").authentication,
        RequestAuthentication::Basic {
            username: ValueSource::literal("demo-user"),
            password: ValueSource::literal("demo-password"),
        }
    );
    let RequestAuthentication::ApiKey {
        placement,
        name,
        value,
    } = &request(&collection, "Create customer").authentication
    else {
        panic!("expected API key auth");
    };
    assert_eq!(*placement, ApiKeyPlacement::Header);
    assert_eq!(name, "X-Api-Key");
    assert_eq!(literal(value), "demo-api-key");
    // `noauth` on a folder stops inheritance for its children.
    assert_eq!(
        request(&collection, "Search customers").authentication,
        RequestAuthentication::None
    );
    // Collection bearer auth applies to top-level requests without auth.
    assert_eq!(
        request(&collection, "Import price list").authentication,
        RequestAuthentication::Bearer {
            token: ValueSource::literal("{{accessToken}}"),
        }
    );
}

#[test]
fn postman_oauth2_keeps_its_configuration_and_hands_over_the_client_secret() {
    let parsed = postman_v21_import();
    let collection = &parsed.workspace.collections[0];

    let graphql = request(collection, "Catalog GraphQL");
    let RequestAuthentication::Oauth2 { configuration } = &graphql.authentication else {
        panic!("expected OAuth 2.0");
    };
    assert_eq!(configuration.grant, Oauth2Grant::ClientCredentials);
    assert_eq!(
        configuration.token_url,
        "https://auth.example.test/oauth/token"
    );
    assert_eq!(configuration.client_id, "shop-cli");
    assert_eq!(configuration.scopes, "catalog:read");
    assert_eq!(
        secret_material(
            &parsed,
            &ValueSource::secret(configuration.client_secret_reference.clone())
        ),
        "demo-client-secret"
    );

    let browser = request(collection, "Sign in with browser");
    let RequestAuthentication::Oauth2 { configuration } = &browser.authentication else {
        panic!("expected OAuth 2.0");
    };
    assert_eq!(configuration.grant, Oauth2Grant::AuthorizationCodePkce);
    assert_eq!(configuration.redirect_uri, "wirebolt://oauth/callback");
    assert!(
        parsed
            .secrets
            .iter()
            .all(|secret| secret.reference != configuration.client_secret_reference)
    );
    warning(collection, "client secrets that reference variables");
    warning(collection, "now uses PKCE");

    assert_eq!(
        request(collection, "Legacy password grant").authentication,
        RequestAuthentication::None
    );
    warning(collection, "OAuth 2.0 password credentials grant");
    assert!(warning(collection, "Digest authentication").contains("Upload avatar"));
}

#[test]
fn postman_bodies_keep_their_type() {
    let collection = postman_v21();

    let RequestBody::Json { value } = &request(&collection, "Catalog GraphQL").body else {
        panic!("GraphQL becomes JSON");
    };
    let graphql: serde_json::Value = serde_json::from_str(value).expect("valid GraphQL JSON");
    assert!(
        graphql["query"]
            .as_str()
            .unwrap()
            .contains("products(first: $first)")
    );
    assert_eq!(graphql["variables"]["first"], 10);

    assert_eq!(
        request(&collection, "Import price list").body,
        RequestBody::File {
            path: "/Users/demo/Documents/prices.csv".to_owned(),
            content_type: None,
        }
    );
    assert!(matches!(
        request(&collection, "Export XML feed").body,
        RequestBody::Xml { .. }
    ));
    // Declared JSON stays JSON even while `{{tier}}` makes it invalid.
    let RequestBody::Json { value } = &request(&collection, "Create customer").body else {
        panic!("raw JSON language maps to a JSON body");
    };
    assert!(value.contains("{{tier}}"));
    assert_eq!(
        request(&collection, "Archived draft").body,
        RequestBody::Empty
    );
    warning(&collection, "Disabled bodies");
}

#[test]
fn postman_disabled_rows_stay_disabled() {
    let collection = postman_v21();

    let get = request(&collection, "Get customer");
    let debug_header = get
        .headers
        .iter()
        .find(|header| header.name == "X-Debug")
        .unwrap();
    assert!(!debug_header.enabled);
    let query: Vec<_> = get
        .query
        .iter()
        .map(|field| (field.name.as_str(), literal(&field.value), field.enabled))
        .collect();
    assert_eq!(
        query,
        [
            ("expand", "orders", true),
            ("debug", "true", false),
            ("q", "hello world", true),
        ]
    );
    // Query rows are not duplicated in the URL.
    assert_eq!(get.url, "{{baseUrl}}/customers/{{customerId}}");

    let RequestBody::FormUrlEncoded { fields } = &request(&collection, "Search customers").body
    else {
        panic!("urlencoded body");
    };
    assert_eq!(
        fields.iter().map(|field| field.enabled).collect::<Vec<_>>(),
        [true, false]
    );
    let RequestBody::Multipart { parts } = &request(&collection, "Upload avatar").body else {
        panic!("form-data body");
    };
    assert_eq!(parts[0].kind, MultipartPartKind::File);
    assert_eq!(
        parts[0].file_path.as_deref(),
        Some("/Users/demo/Pictures/avatar.png")
    );
    assert!(!parts[2].enabled);
    assert_eq!(parts[3].file_path, None);
    warning(&collection, "File uploads without a file path");
}

#[test]
fn postman_path_variables_descriptions_and_transport_are_kept() {
    let parsed = postman_v21_import();
    let collection = &parsed.workspace.collections[0];

    assert_eq!(
        request(collection, "Upload avatar").url,
        "{{baseUrl}}/customers/{{customerId}}/avatar"
    );
    warning(collection, "Path variables without a value");
    assert_eq!(
        request(collection, "Get customer").note,
        "Returns one customer with their **orders**."
    );
    let search = request(collection, "Search customers");
    let settings = transport(&parsed, &search.source_id)
        .expect("protocol profile behaviour maps to transport");
    assert!(!settings.validate_tls);
    assert!(settings.follow_redirects);
    assert!(transport(&parsed, &request(collection, "Get customer").source_id).is_none());
}

#[test]
fn postman_variables_become_an_environment_with_secrets_in_keychain_material() {
    let parsed = postman_v21_import();

    let [environment] = parsed.workspace.environments.as_slice() else {
        panic!("one environment");
    };
    assert_eq!(environment.name, "Shop API");
    assert!(!environment.global);
    let variables: Vec<_> = environment
        .variables
        .iter()
        .map(|variable| {
            (
                variable.key.as_str(),
                variable.enabled,
                matches!(variable.value, ValueSource::Secret { .. }),
            )
        })
        .collect();
    assert_eq!(
        variables,
        [
            ("baseUrl", true, false),
            ("accessToken", true, true),
            ("tenant", true, true),
            ("legacyFlag", false, false),
            ("customerId", true, false),
        ]
    );
    // A folder cannot silently change a collection variable.
    assert_eq!(
        environment.variables[0].value,
        ValueSource::literal("https://shop.example.test/v1")
    );
    assert_eq!(
        secret_material(&parsed, &environment.variables[1].value),
        "demo-access-token"
    );
    warning(
        &parsed.workspace.collections[0],
        "Folder variables that redefine",
    );
    // Debug output never prints secret material.
    let debug = format!("{parsed:?}");
    assert!(!debug.contains("demo-access-token"));
    assert!(!debug.contains("demo-client-secret"));
}

#[test]
fn postman_reports_scripts_examples_and_descriptions() {
    let collection = postman_v21();

    let scripts = warning(&collection, "Scripts aren't supported");
    assert!(scripts.contains("Shop API (pre-request)"));
    assert!(scripts.contains("Customers / Get customer (test)"));
    // Empty script stubs are not reported.
    assert!(!scripts.contains("Shop API (test)"));
    assert!(warning(&collection, "example responses").contains("Get customer (1)"));
    warning(&collection, "folder descriptions");
    assert_eq!(collection.groups.len(), 3);
}

#[test]
fn postman_v20_reads_object_auth_string_requests_and_header_blocks() {
    let collection = ImportEngine::parse(
        ImportFormat::PostmanV2,
        &fixture("postman_v20_legacy.postman_collection.json"),
    )
    .expect("parse Postman v2.0");

    let stock = request(&collection, "List stock");
    assert_eq!(stock.url, "https://inventory.example.test/v2/stock");
    assert_eq!(stock.query.len(), 2);
    assert_eq!(stock.headers.len(), 2);
    assert_eq!(stock.note, "Paged stock levels.");
    assert!(matches!(
        stock.authentication,
        RequestAuthentication::Basic { .. }
    ));
    let health = request(&collection, "Health");
    assert_eq!(health.url, "https://inventory.example.test/health");
    assert_eq!(
        health.authentication,
        RequestAuthentication::Bearer {
            token: ValueSource::literal("demo-v20-token"),
        }
    );
}

#[test]
fn har_drops_connection_headers_and_maps_form_bodies() {
    let collection = ImportEngine::parse_file(
        ImportFormat::Har,
        &fixture("chrome_session.har"),
        "chrome_session",
    )
    .expect("parse HAR");

    assert_eq!(collection.name, "chrome_session");
    let items = request(&collection, "GET /api/items");
    assert!(
        items
            .headers
            .iter()
            .all(|header| !header.name.starts_with(':')),
        "HTTP/2 pseudo-headers are not request headers"
    );
    assert_eq!(
        items.authentication,
        RequestAuthentication::Bearer {
            token: ValueSource::literal("demo-session-token"),
        }
    );
    let create = request(&collection, "POST /api/items");
    let names: Vec<_> = create
        .headers
        .iter()
        .map(|header| header.name.as_str())
        .collect();
    assert_eq!(names, ["Content-Type", "Accept-Encoding"]);
    assert!(matches!(create.body, RequestBody::Json { .. }));

    let RequestBody::FormUrlEncoded { fields } = &request(&collection, "POST /login").body else {
        panic!("urlencoded HAR body");
    };
    assert_eq!(literal(&fields[1].value), "hello world!");

    let upload = request(&collection, "POST /upload");
    let RequestBody::Multipart { parts } = &upload.body else {
        panic!("multipart HAR body");
    };
    assert_eq!(parts[0].kind, MultipartPartKind::File);
    assert_eq!(parts[0].file_name.as_deref(), Some("photo.png"));
    assert_eq!(literal(&parts[1].value), "Holiday");
    assert!(
        upload.headers.is_empty(),
        "the stale boundary header is dropped"
    );
    warning(&collection, "don't include uploaded files");

    let live = request(&collection, "GET /live");
    assert!(live.web_socket);
    assert!(live.headers.is_empty());
    warning(&collection, "Skipped 1 entry");
}

#[test]
fn curl_reads_multiline_chrome_copies() {
    let collection = ImportEngine::parse(ImportFormat::Curl, &fixture("chrome_copy_as_curl.sh"))
        .expect("parse cURL");

    let [request] = collection.requests.as_slice() else {
        panic!("one request");
    };
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.url,
        "https://api.example.test/v1/orders?status=open"
    );
    assert_eq!(request.name, "POST /v1/orders");
    assert_eq!(
        request.authentication,
        RequestAuthentication::Bearer {
            token: ValueSource::literal("demo-session-token"),
        }
    );
    let cookie = request
        .headers
        .iter()
        .find(|header| header.name == "Cookie")
        .expect("-b becomes a Cookie header");
    assert_eq!(literal(&cookie.value), "theme=dark; region=eu");
    let RequestBody::Json { value } = &request.body else {
        panic!("JSON body");
    };
    let body: serde_json::Value = serde_json::from_str(value).expect("valid JSON");
    assert_eq!(body["note"], "it's urgent\n");
    assert!(collection.warnings.is_empty(), "{:?}", collection.warnings);
}

#[test]
fn curl_imports_every_command_with_its_options() {
    let parsed =
        ImportEngine::parse_import(ImportFormat::Curl, &fixture("several_commands.sh"), None)
            .expect("parse cURL");
    let collection = &parsed.workspace.collections[0];

    assert_eq!(collection.requests.len(), 5);
    let upload = &collection.requests[0];
    assert_eq!(upload.method, "POST");
    assert_eq!(
        upload.authentication,
        RequestAuthentication::Basic {
            username: ValueSource::literal("demo-user"),
            password: ValueSource::literal("demo-password"),
        }
    );
    let RequestBody::Multipart { parts } = &upload.body else {
        panic!("-F becomes multipart");
    };
    assert_eq!(parts[0].file_path.as_deref(), Some("/tmp/demo/report.pdf"));
    assert_eq!(parts[0].content_type.as_deref(), Some("application/pdf"));
    assert_eq!(literal(&parts[1].value), "Quarterly report");

    let listing = &collection.requests[1];
    assert_eq!(listing.url, "http://api.example.test/v1/items?limit=5");
    let settings = transport(&parsed, &listing.source_id).expect("transport options");
    assert!(!settings.validate_tls);
    assert!(settings.follow_redirects);
    assert_eq!(settings.maximum_redirects, 3);
    assert_eq!(settings.total_timeout_ms, 5_000);
    assert!(transport(&parsed, &upload.source_id).is_none());
    let header = |name: &str| {
        listing
            .headers
            .iter()
            .find(|header| header.name == name)
            .map(|header| literal(&header.value).to_owned())
    };
    assert_eq!(header("User-Agent").as_deref(), Some("Wirebolt-Test/1.0"));
    assert_eq!(
        header("Referer").as_deref(),
        Some("https://app.example.test")
    );

    let search = &collection.requests[2];
    assert_eq!(search.method, "HEAD");
    assert_eq!(
        search.url,
        "https://api.example.test/v1/search?q=desk lamp&tag=a%26b"
    );
    assert_eq!(search.body, RequestBody::Empty);

    let put = &collection.requests[3];
    assert_eq!(put.method, "PUT");
    assert_eq!(
        put.body,
        RequestBody::File {
            path: "items.csv".to_owned(),
            content_type: None,
        }
    );

    let RequestBody::FormUrlEncoded { fields } = &collection.requests[4].body else {
        panic!("-d pairs become a form");
    };
    let fields: Vec<_> = fields
        .iter()
        .map(|field| (field.name.as_str(), literal(&field.value)))
        .collect();
    assert_eq!(fields, [("a", "1"), ("b", "two words"), ("c", "x&y")]);

    assert!(warning(collection, "options aren't supported").contains("--proxy"));
    warning(collection, "Relative file paths");
    warning(collection, "other than curl");
}

#[test]
fn curl_defaults_data_to_a_form_content_type() {
    let collection = ImportEngine::parse(
        ImportFormat::Curl,
        "curl https://api.example.test -d 'raw text'",
    )
    .expect("parse cURL");

    assert_eq!(
        collection.requests[0].body,
        RequestBody::Text {
            content_type: Some("application/x-www-form-urlencoded".to_owned()),
            value: "raw text".to_owned(),
        }
    );
}

#[test]
fn failures_name_the_problem() {
    let reason = |format, source: &str| {
        ImportEngine::parse(format, source)
            .expect_err("must fail")
            .reason()
            .to_owned()
    };
    let openapi = r#"{"openapi":"3.0.3","info":{"title":"Pets","version":"1"},"paths":{}}"#;
    assert!(reason(ImportFormat::PostmanV2, openapi).contains("OpenAPI"));
    assert!(
        reason(ImportFormat::Har, "openapi: 3.1.0\ninfo:\n  title: Pets\n").contains("OpenAPI")
    );
    assert!(
        reason(
            ImportFormat::PostmanV2,
            r#"{"id":"1","name":"Old","order":[],"requests":[]}"#
        )
        .contains("Collection v1")
    );
    assert!(
        reason(
            ImportFormat::PostmanV2,
            r#"{"name":"Env","values":[],"_postman_variable_scope":"environment"}"#
        )
        .contains("Postman environment")
    );
    assert!(reason(ImportFormat::Curl, "curl -H 'Accept: x'").contains("no URL"));
    assert!(reason(ImportFormat::Curl, "curl 'https://x.test").contains("unterminated"));
}
