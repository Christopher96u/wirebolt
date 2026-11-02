use std::{collections::BTreeMap, fs, io::Write as _};

use tempfile::tempdir;
use wirebolt_core::{
    CURRENT_SCHEMA_VERSION, Collection, DocumentId, DocumentProblemKind, Environment, ManualProxy,
    ProxyCredentials, ProxyDestination, ProxyEndpoint, ProxyMode, ProxyPolicy, ProxyRoute,
    ProxySource, Request, RequestBody, RequestHeader, SaveOutcome, SecretName, StorageError,
    TransportSettings, ValueSource, Workspace, WorkspaceDocument, WorkspaceStore,
};

fn id(value: &str) -> DocumentId {
    DocumentId::new(value).expect("valid fixture ID")
}

#[test]
fn round_trips_a_stable_git_friendly_workspace() {
    let temporary = tempdir().expect("temporary workspace");
    let mut workspace = Workspace::new("Wirebolt API");
    workspace.transport = TransportSettings {
        validate_tls: false,
        follow_redirects: true,
        maximum_redirects: 4,
        ..TransportSettings::default()
    };
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
            id: "content-type".to_owned(),
            name: "content-type".to_owned(),
            value: ValueSource::literal("application/json"),
            enabled: true,
            sensitive: false,
        },
        RequestHeader {
            id: "authorization".to_owned(),
            name: "authorization".to_owned(),
            value: ValueSource::secret(SecretName::new("API_TOKEN").expect("secret name")),
            enabled: true,
            sensitive: true,
        },
    ];
    request.body = RequestBody::Text {
        content_type: Some("application/json".to_owned()),
        value: "{\n  \"name\": \"Chris\"\n}".to_owned(),
    };
    request.inherits_workspace_transport = false;
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
fn broken_documents_are_reported_without_hiding_the_rest_of_the_workspace() {
    let temporary = tempdir().expect("temporary workspace");
    let store = WorkspaceStore::create(temporary.path(), &Workspace::new("Wirebolt"))
        .expect("create workspace");
    let collection = Collection::new(id("users"), "Users".to_owned());
    store
        .save(&WorkspaceDocument::Collection(collection.clone()))
        .expect("save collection");
    let request = Request::new(id("list"), "List", "GET", "https://example.com/users");
    store
        .save(&WorkspaceDocument::Request {
            collection_id: collection.id.clone(),
            request: request.clone(),
        })
        .expect("save request");
    fs::write(
        temporary.path().join("environments/broken.toml"),
        "<<<<<<< HEAD\nschema_version = 3\nid = \"broken\"\nname = \"TOP-SECRET\"\n=======\n",
    )
    .expect("write conflicted fixture");
    fs::write(
        temporary
            .path()
            .join("collections/users/requests/future.toml"),
        "schema_version = 999\nid = \"future\"\nname = \"Future\"\nnew_field = 1\n",
    )
    .expect("write future fixture");

    let snapshot = store.load().expect("load tolerates broken documents");

    assert_eq!(snapshot.collections[0].requests, vec![request]);
    assert!(snapshot.environments.is_empty());
    let kinds: Vec<_> = snapshot
        .problems
        .iter()
        .map(|problem| (problem.path.to_string_lossy().into_owned(), problem.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                "collections/users/requests/future.toml".to_owned(),
                DocumentProblemKind::UnsupportedSchema
            ),
            (
                "environments/broken.toml".to_owned(),
                DocumentProblemKind::InvalidToml
            ),
        ]
    );
    assert!(
        snapshot
            .problems
            .iter()
            .all(|problem| !problem.reason.contains("TOP-SECRET"))
    );
    assert!(
        fs::read_to_string(temporary.path().join("environments/broken.toml"))
            .expect("conflicted fixture")
            .starts_with("<<<<<<< HEAD"),
        "broken documents must be left untouched"
    );
}

#[test]
fn reloads_reflect_external_edits_and_deletions() {
    let temporary = tempdir().expect("temporary workspace");
    let store = WorkspaceStore::create(temporary.path(), &Workspace::new("Wirebolt"))
        .expect("create workspace");
    let environment = Environment::new(id("local"), "Local".to_owned(), BTreeMap::new());
    store
        .save(&WorkspaceDocument::Environment(environment))
        .expect("save environment");
    assert_eq!(
        store.load().expect("first load").environments[0].name,
        "Local"
    );

    let path = temporary.path().join("environments/local.toml");
    fs::write(
        &path,
        format!("schema_version = {CURRENT_SCHEMA_VERSION}\nid = \"local\"\nname = \"Edited by hand\"\n"),
    )
    .expect("edit environment externally");
    assert_eq!(
        store.load().expect("second load").environments[0].name,
        "Edited by hand"
    );

    fs::remove_file(&path).expect("delete environment");
    assert!(store.load().expect("third load").environments.is_empty());
}

#[test]
fn reloads_reflect_an_edit_that_keeps_the_size_and_modification_time() {
    let temporary = tempdir().expect("temporary workspace");
    let store = WorkspaceStore::create(temporary.path(), &Workspace::new("Wirebolt"))
        .expect("create workspace");
    let environment = Environment::new(id("local"), "Local".to_owned(), BTreeMap::new());
    store
        .save(&WorkspaceDocument::Environment(environment))
        .expect("save environment");
    assert_eq!(
        store.load().expect("first load").environments[0].name,
        "Local"
    );

    let path = temporary.path().join("environments/local.toml");
    let original = fs::read_to_string(&path).expect("environment TOML");
    let modified = fs::metadata(&path)
        .expect("environment metadata")
        .modified()
        .expect("modification time");
    let edited = original.replace("\"Local\"", "\"Edits\"");
    assert_eq!(edited.len(), original.len());
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open environment for editing");
    (&file).write_all(edited.as_bytes()).expect("edit in place");
    file.set_modified(modified)
        .expect("restore modification time");
    drop(file);

    assert_eq!(
        store.load().expect("second load").environments[0].name,
        "Edits"
    );
}

#[test]
fn an_unsupported_document_with_a_quoted_key_is_left_unchanged() {
    let temporary = tempdir().expect("temporary workspace");
    let store = WorkspaceStore::create(temporary.path(), &Workspace::new("Wirebolt"))
        .expect("create workspace");
    let path = temporary.path().join("environments/future.toml");
    let future = "\"schema_version\" = 999\nid = \"future\"\nname = \"Future\"\n\n[variables]\n";
    fs::write(&path, future).expect("write future environment");

    let snapshot = store.load().expect("load tolerates future documents");
    assert!(snapshot.environments.is_empty());
    assert_eq!(snapshot.problems.len(), 1);
    assert_eq!(
        snapshot.problems[0].kind,
        DocumentProblemKind::UnsupportedSchema
    );
    assert_eq!(
        fs::read_to_string(&path).expect("future environment"),
        future,
        "a document from the future must not be downgraded"
    );
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
            supported: CURRENT_SCHEMA_VERSION,
            ..
        }
    ));
}




#[test]
fn round_trips_workspace_and_request_proxy_overrides_without_secret_values() {
    let temporary = tempdir().expect("temporary workspace");
    let username = SecretName::new("CORP_PROXY_USER").expect("username secret name");
    let password = SecretName::new("CORP_PROXY_PASSWORD").expect("password secret name");
    let route = ProxyRoute::new(
        ProxyDestination::All,
        ProxyEndpoint::new("socks5h://proxy.internal:1080").expect("proxy endpoint"),
    )
    .with_credentials(ProxyCredentials::new(username, password));
    let mut workspace = Workspace::new("Corporate API");
    workspace.proxy = Some(ProxyMode::Manual(
        ManualProxy::new(vec![route]).expect("manual proxy"),
    ));
    let store = WorkspaceStore::create(temporary.path(), &workspace).expect("create workspace");
    let collection = Collection::new(id("status"), "Status".to_owned());
    store
        .save(&WorkspaceDocument::Collection(collection.clone()))
        .expect("save collection");
    let mut request = Request::new(id("health"), "Health", "GET", "https://api.internal/health");
    request.proxy_override = Some(ProxyMode::Direct);
    store
        .save(&WorkspaceDocument::Request {
            collection_id: collection.id,
            request: request.clone(),
        })
        .expect("save request");

    let snapshot = WorkspaceStore::open(temporary.path())
        .expect("open workspace")
        .load()
        .expect("load workspace");

    assert_eq!(snapshot.workspace, workspace);
    assert_eq!(snapshot.collections[0].requests, vec![request]);
    let policy = ProxyPolicy::new(snapshot.workspace.proxy.clone());
    let resolved = policy.resolve(snapshot.collections[0].requests[0].proxy_override.as_ref());
    assert_eq!(resolved.source(), ProxySource::Request);
    assert_eq!(resolved.mode(), &ProxyMode::Direct);
    let workspace_toml =
        fs::read_to_string(temporary.path().join("wirebolt.toml")).expect("workspace TOML");
    assert!(workspace_toml.contains("CORP_PROXY_USER"));
    assert!(workspace_toml.contains("CORP_PROXY_PASSWORD"));
    assert!(!workspace_toml.contains("super-secret"));
}

#[test]
fn oversized_save_preserves_readable_request() {
    let directory = tempdir().unwrap();
    let store = WorkspaceStore::create(directory.path(), &Workspace::new("Fixture")).unwrap();
    store
        .save(&WorkspaceDocument::Collection(Collection::new(
            id("api"),
            "API".into(),
        )))
        .unwrap();
    let mut request = Request::new(id("request"), "Original", "POST", "https://example.com");
    store
        .save(&WorkspaceDocument::Request {
            collection_id: id("api"),
            request: request.clone(),
        })
        .unwrap();
    request.body = RequestBody::Text {
        content_type: None,
        value: "x".repeat(16 * 1024 * 1024),
    };
    assert!(matches!(
        store.save(&WorkspaceDocument::Request {
            collection_id: id("api"),
            request
        }),
        Err(StorageError::DocumentTooLarge { .. })
    ));
    let snapshot = WorkspaceStore::open(directory.path())
        .unwrap()
        .load()
        .unwrap();
    assert!(snapshot.problems.is_empty());
    assert_eq!(snapshot.collections[0].requests[0].name, "Original");
}

#[test]
fn delete_collection_preserves_unmanaged_and_unreadable_files() {
    let directory = tempdir().unwrap();
    let store = WorkspaceStore::create(directory.path(), &Workspace::new("Fixture")).unwrap();
    store
        .save(&WorkspaceDocument::Collection(Collection::new(
            id("api"),
            "API".into(),
        )))
        .unwrap();
    store
        .save(&WorkspaceDocument::Request {
            collection_id: id("api"),
            request: Request::new(id("saved"), "Saved", "GET", "https://example.com"),
        })
        .unwrap();
    let collection = directory.path().join("collections/api");
    let attachment = collection.join("attachment.bin");
    let malformed = collection.join("requests/broken.toml");
    fs::write(&attachment, b"attachment").unwrap();
    fs::write(&malformed, b"unreadable = [").unwrap();
    fs::create_dir(collection.join("requests/notes")).unwrap();
    fs::write(collection.join("requests/notes/readme.txt"), b"notes").unwrap();
    store.delete_collection(&id("api")).unwrap();
    assert_eq!(fs::read(attachment).unwrap(), b"attachment");
    assert_eq!(fs::read(malformed).unwrap(), b"unreadable = [");
    assert!(collection.join("requests/notes/readme.txt").is_file());
    assert!(!collection.join("requests/saved.toml").exists());
    assert!(!collection.join("collection.toml").exists());
    let loaded = store.load().unwrap();
    assert!(loaded.collections.is_empty());
    assert!(loaded.problems.is_empty());
}

#[test]
fn request_move_never_replaces_a_destination_and_rolls_back_missing_source() {
    let root = tempdir().unwrap();
    let store = WorkspaceStore::create(root.path(), &Workspace::new("Fixture")).unwrap();
    for name in ["source", "target"] {
        store
            .save(&WorkspaceDocument::Collection(Collection::new(
                id(name),
                name.into(),
            )))
            .unwrap();
    }
    let request = Request::new(id("same"), "Target", "GET", "https://example.com");
    store
        .save(&WorkspaceDocument::Request {
            collection_id: id("target"),
            request: request.clone(),
        })
        .unwrap();
    let mut changed = request.clone();
    changed.name = "Source".into();
    assert!(matches!(
        store.move_request(&id("source"), &id("target"), &changed),
        Err(StorageError::AlreadyExists { .. })
    ));
    assert_eq!(
        store
            .load()
            .unwrap()
            .collections
            .iter()
            .find(|c| c.collection.id == id("target"))
            .unwrap()
            .requests[0]
            .name,
        "Target"
    );
    changed.id = id("missing");
    assert!(matches!(
        store.move_request(&id("source"), &id("target"), &changed),
        Err(StorageError::Missing { .. })
    ));
    assert!(
        !root
            .path()
            .join("collections/target/requests/missing.toml")
            .exists()
    );
}

#[test]
fn collection_deletion_refuses_symlinked_directories() {
    let root = tempdir().unwrap();
    let external = tempdir().unwrap();
    fs::write(external.path().join("attachment.toml"), b"unmanaged").unwrap();
    let store = WorkspaceStore::create(root.path(), &Workspace::new("Fixture")).unwrap();
    store
        .save(&WorkspaceDocument::Collection(Collection::new(
            id("api"),
            "API".into(),
        )))
        .unwrap();
    let requests = root.path().join("collections/api/requests");
    fs::remove_dir(&requests).unwrap();
    std::os::unix::fs::symlink(external.path(), &requests).unwrap();
    assert!(store.delete_collection(&id("api")).is_err());
    assert!(
        root.path()
            .join("collections/api/collection.toml")
            .is_file()
    );
    assert_eq!(
        fs::read(external.path().join("attachment.toml")).unwrap(),
        b"unmanaged"
    );
}

#[test]
fn rejects_every_noncurrent_workspace_schema() {
    for version in 0..CURRENT_SCHEMA_VERSION {
        let root = tempdir().unwrap();
        fs::write(root.path().join("wirebolt.toml"), format!("schema_version = {version}\nname = \"Old\"\n")).unwrap();
        assert!(matches!(WorkspaceStore::open(root.path()), Err(StorageError::UnsupportedSchema { found, .. }) if found == version));
    }
    let root = tempdir().unwrap();
    fs::write(root.path().join("wirebolt.toml"), "name = \"Unversioned\"\n").unwrap();
    assert!(matches!(WorkspaceStore::open(root.path()), Err(StorageError::UnsupportedSchema { found: 0, .. })));
}
