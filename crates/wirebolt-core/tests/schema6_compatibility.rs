use std::fs;

use tempfile::tempdir;
use wirebolt_core::{
    Collection, Environment, Request, Workspace, WorkspaceDocument, WorkspaceStore,
};

// Frozen schema-6 documents accepted by b3f943e. Groups are embedded in the
// collection file, not separate documents. Compare the complete TOML value so
// added fields (including nested fields) cannot hide behind current deserializers.
const WORKSPACE: &str = include_str!("fixtures/schema6/workspace.toml");
const COLLECTION: &str = include_str!("fixtures/schema6/collection.toml");
const ENVIRONMENT: &str = include_str!("fixtures/schema6/environment.toml");
const REQUEST: &str = include_str!("fixtures/schema6/request.toml");

#[test]
fn written_workspace_documents_remain_schema6_compatible() {
    let root = tempdir().unwrap();
    let workspace: Workspace = toml::from_str(WORKSPACE).unwrap();
    let collection: Collection = toml::from_str(COLLECTION).unwrap();
    let environment: Environment = toml::from_str(ENVIRONMENT).unwrap();
    let request: Request = toml::from_str(REQUEST).unwrap();
    let store = WorkspaceStore::create(root.path(), &workspace).unwrap();
    store
        .save(&WorkspaceDocument::Collection(collection.clone()))
        .unwrap();
    store
        .save(&WorkspaceDocument::Environment(environment))
        .unwrap();
    store
        .save(&WorkspaceDocument::Request {
            collection_id: collection.id,
            request,
        })
        .unwrap();

    for (path, golden) in [
        ("wirebolt.toml", WORKSPACE),
        ("collections/api/collection.toml", COLLECTION),
        ("environments/local.toml", ENVIRONMENT),
        ("collections/api/requests/health.toml", REQUEST),
    ] {
        let written = fs::read_to_string(root.path().join(path)).unwrap();
        assert_eq!(
            toml::from_str::<toml::Value>(&written).unwrap(),
            toml::from_str::<toml::Value>(golden).unwrap(),
            "{path} must retain the fields accepted by b3f943e (schema 6)"
        );
    }
}
