use std::{collections::BTreeMap, fs};

use tempfile::tempdir;
use wirebolt_core::{
    Collection, DocumentId, Environment, Request, RequestBody, RequestHeader, SaveOutcome,
    SecretName, StorageError, ValueSource, Workspace, WorkspaceDocument, WorkspaceStore,
};

fn id(value: &str) -> DocumentId {
    DocumentId::new(value).expect("valid fixture ID")
}

#[test]
fn round_trips_a_stable_git_friendly_workspace() {
    let temporary = tempdir().expect("temporary workspace");
    let workspace = Workspace::new("Wirebolt API");
    let store = WorkspaceStore::create(temporary.path(), &workspace).expect("create workspace");

    let collection = Collection::new(id("users"), "Users".to_owned());
    assert_eq!(
        store
            .save(&WorkspaceDocument::Collection(collection.clone()))
            .expect("save collection"),
        SaveOutcome::Created
    );

    let mut request = Request::new(
        id("create-user"),
        "Create user",
        "POST",
        "{{base_url}}/users",
    );
    request.headers = vec![
        RequestHeader {
            name: "content-type".to_owned(),
            value: ValueSource::literal("application/json"),
            enabled: true,
        },
        RequestHeader {
            name: "authorization".to_owned(),
            value: ValueSource::secret(SecretName::new("API_TOKEN").expect("secret name")),
            enabled: true,
        },
    ];
    request.body = RequestBody::Text {
        content_type: Some("application/json".to_owned()),
        value: "{\n  \"name\": \"Chris\"\n}".to_owned(),
    };
    let request_document = WorkspaceDocument::Request {
        collection_id: collection.id.clone(),
        request: request.clone(),
    };
    assert_eq!(
        store.save(&request_document).expect("save request"),
        SaveOutcome::Created
    );

    let mut variables = BTreeMap::new();
    variables.insert(
        "base_url".to_owned(),
        ValueSource::literal("https://api.example.com"),
    );
    variables.insert(
        "api_token".to_owned(),
        ValueSource::secret(SecretName::new("API_TOKEN").expect("secret name")),
    );
    let environment = Environment::new(id("local"), "Local".to_owned(), variables);
    assert_eq!(
        store
            .save(&WorkspaceDocument::Environment(environment.clone()))
            .expect("save environment"),
        SaveOutcome::Created
    );

    let snapshot = WorkspaceStore::open(temporary.path())
        .expect("open workspace")
        .load()
        .expect("load workspace");
    assert_eq!(snapshot.workspace, workspace);
    assert_eq!(snapshot.collections.len(), 1);
    assert_eq!(snapshot.collections[0].collection, collection);
    assert_eq!(snapshot.collections[0].requests, vec![request]);
    assert_eq!(snapshot.environments, vec![environment]);

    assert_eq!(
        store.save(&request_document).expect("save request again"),
        SaveOutcome::Unchanged
    );

    let request_path = temporary
        .path()
        .join("collections/users/requests/create-user.toml");
    let environment_path = temporary.path().join("environments/local.toml");
    assert!(temporary.path().join("wirebolt.toml").is_file());
    assert!(
        temporary
            .path()
            .join("collections/users/collection.toml")
            .is_file()
    );
    assert!(request_path.is_file());
    assert!(environment_path.is_file());

    let request_toml = fs::read_to_string(request_path).expect("request TOML");
    let environment_toml = fs::read_to_string(environment_path).expect("environment TOML");
    assert!(request_toml.contains("secret = \"API_TOKEN\""));
    assert!(!request_toml.contains("sk-live-do-not-write"));
    assert!(environment_toml.contains("api_token"));
    assert!(environment_toml.contains("base_url"));
}

#[test]
fn refuses_to_save_a_request_before_its_collection() {
    let temporary = tempdir().expect("temporary workspace");
    let store = WorkspaceStore::create(temporary.path(), &Workspace::new("Wirebolt"))
        .expect("create workspace");
    let collection_id = id("missing");
    let request = Request::new(id("health"), "Health", "GET", "https://example.com/health");

    let error = store
        .save(&WorkspaceDocument::Request {
            collection_id: collection_id.clone(),
            request,
        })
        .expect_err("missing collection must fail");

    assert!(matches!(
        error,
        StorageError::MissingCollection { id } if id == collection_id
    ));
}

#[test]
fn identifiers_cannot_escape_the_workspace() {
    for invalid in ["../secrets", "nested/path", "UPPERCASE", "-leading"] {
        assert!(DocumentId::new(invalid).is_err(), "accepted {invalid}");
    }
    assert!(SecretName::new("../../keychain").is_err());
}

#[test]
fn corrupt_toml_errors_never_echo_document_contents() {
    let temporary = tempdir().expect("temporary workspace");
    let store = WorkspaceStore::create(temporary.path(), &Workspace::new("Wirebolt"))
        .expect("create workspace");
    fs::write(
        temporary.path().join("environments/broken.toml"),
        "schema_version = 1\nid = \"broken\"\nname = \"TOP-SECRET\"\nvariables = [",
    )
    .expect("write corrupt fixture");

    let error = store.load().expect_err("corrupt TOML must fail");

    assert!(matches!(error, StorageError::InvalidToml { .. }));
    assert!(!error.to_string().contains("TOP-SECRET"));
}

#[test]
fn rejects_future_schema_versions() {
    let temporary = tempdir().expect("temporary workspace");
    fs::write(
        temporary.path().join("wirebolt.toml"),
        "schema_version = 999\nname = \"Future\"\n",
    )
    .expect("write future fixture");

    let error = WorkspaceStore::open(temporary.path()).expect_err("future schema must fail");

    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            found: 999,
            supported: 1,
            ..
        }
    ));
}

#[test]
fn explicitly_migrates_unversioned_documents_and_is_restartable() {
    let temporary = tempdir().expect("temporary workspace");
    fs::create_dir_all(temporary.path().join("collections/users"))
        .expect("create collection directory");
    fs::write(
        temporary.path().join("wirebolt.toml"),
        "name = \"Legacy workspace\"\n",
    )
    .expect("write legacy workspace");
    fs::write(
        temporary.path().join("collections/users/collection.toml"),
        "id = \"users\"\nname = \"Users\"\n",
    )
    .expect("write legacy collection");

    let store = WorkspaceStore::open(temporary.path()).expect("open legacy workspace");
    assert_eq!(
        store
            .load()
            .expect("load legacy workspace")
            .collections
            .len(),
        1
    );
    assert_eq!(
        store
            .migrate()
            .expect("migrate workspace")
            .migrated_documents,
        2
    );
    assert_eq!(
        store
            .migrate()
            .expect("repeat migration")
            .migrated_documents,
        0
    );

    let workspace_toml =
        fs::read_to_string(temporary.path().join("wirebolt.toml")).expect("workspace TOML");
    let collection_toml =
        fs::read_to_string(temporary.path().join("collections/users/collection.toml"))
            .expect("collection TOML");
    assert!(workspace_toml.starts_with("schema_version = 1\n"));
    assert!(collection_toml.starts_with("schema_version = 1\n"));
}
