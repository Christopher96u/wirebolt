use super::super::secrets::{SessionSecretPlatform, SessionSecrets, scan_references};
use async_trait::async_trait;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Debug, Default)]
struct Platform {
    values: Mutex<HashMap<String, String>>,
    deleted: Mutex<Vec<String>>,
    fails: AtomicBool,
}
#[async_trait]
impl SessionSecretPlatform for Platform {
    async fn read(&self, name: String) -> Result<Option<String>, ()> {
        Ok(self.values.lock().unwrap().get(&name).cloned())
    }
    async fn save(&self, name: String, value: String) -> Result<(), ()> {
        if self.fails.load(Ordering::Relaxed) {
            return Err(());
        }
        self.values.lock().unwrap().insert(name, value);
        Ok(())
    }
    async fn remove(&self, name: String) -> Result<(), ()> {
        if self.fails.load(Ordering::Relaxed) {
            return Err(());
        }
        self.values.lock().unwrap().remove(&name);
        self.deleted.lock().unwrap().push(name);
        Ok(())
    }
}
fn owned(role: &str) -> String {
    crate::domain::request::credential_reference(role, "request")
}
#[tokio::test]
async fn duplicates_copy_credentials_to_independent_names() {
    let platform = Platform::default();
    platform
        .values
        .lock()
        .unwrap()
        .insert("old".into(), "secret".into());
    let ledger = SessionSecrets::default();
    let name = owned("password");
    ledger
        .copy(HashMap::from([("old".into(), name.clone())]), &platform)
        .await
        .unwrap();
    assert_eq!(
        platform.values.lock().unwrap().get(&name).unwrap(),
        "secret"
    );
    ledger.edit_batch(HashMap::from([(name.clone(), "new".into())]));
    ledger.flush(&platform).await.unwrap();
    assert_eq!(
        platform.values.lock().unwrap().get("old").unwrap(),
        "secret"
    );
}
#[tokio::test]
async fn deletion_keeps_items_while_undo_can_reference_them() {
    let platform = Platform::default();
    let name = owned("password");
    let ledger = SessionSecrets::default();
    ledger
        .save(HashMap::from([(name.clone(), "secret".into())]), &platform)
        .await
        .unwrap();
    ledger.purge(vec![name.clone()], vec![], &platform).await;
    assert!(platform.deleted.lock().unwrap().is_empty());
    assert!(ledger.has_observed());
}
#[tokio::test]
async fn discarded_undo_releases_owned_names_and_keeps_chosen_names() {
    let platform = Platform::default();
    let name = owned("password");
    let ledger = SessionSecrets::default();
    ledger
        .save(
            HashMap::from([
                (name.clone(), "secret".into()),
                ("chosen-name".into(), "shared".into()),
            ]),
            &platform,
        )
        .await
        .unwrap();
    ledger.purge(vec![], vec![], &platform).await;
    assert_eq!(*platform.deleted.lock().unwrap(), [name]);
    assert!(platform.values.lock().unwrap().contains_key("chosen-name"));
}
#[test]
fn scan_reads_request_environment_and_workspace_toml_files() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("collections/api/requests")).unwrap();
    std::fs::create_dir_all(root.path().join("environments")).unwrap();
    std::fs::write(
        root.path().join("collections/api/requests/a.toml"),
        "secret = 'request-secret'",
    )
    .unwrap();
    std::fs::write(
        root.path().join("environments/a.toml"),
        "secret = \"environment-secret\"",
    )
    .unwrap();
    std::fs::write(root.path().join("wirebolt.toml"), "'workspace-secret'").unwrap();
    let candidates: Vec<String> = ["request-secret", "environment-secret", "workspace-secret"]
        .map(str::to_owned)
        .into_iter()
        .collect();
    let found = scan_references(
        &candidates,
        &[
            root.path().to_string_lossy().into_owned(),
            "/missing".into(),
        ],
    );
    assert_eq!(
        found,
        HashSet::from([
            "request-secret".into(),
            "environment-secret".into(),
            "workspace-secret".into()
        ])
    );
}
#[tokio::test]
async fn another_workspace_reference_prevents_deletion() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("environments")).unwrap();
    let name = owned("token");
    std::fs::write(
        root.path().join("environments/a.toml"),
        format!("secret = '{name}'"),
    )
    .unwrap();
    let platform = Platform::default();
    let ledger = SessionSecrets::default();
    ledger
        .save(HashMap::from([(name.clone(), "secret".into())]), &platform)
        .await
        .unwrap();
    ledger
        .purge(
            vec![],
            vec![root.path().to_string_lossy().into_owned()],
            &platform,
        )
        .await;
    assert!(platform.deleted.lock().unwrap().is_empty());
    assert!(!ledger.has_observed());
}
#[tokio::test]
async fn removed_environment_secret_is_purged_once() {
    let platform = Platform::default();
    let name = owned("variable");
    let ledger = SessionSecrets::default();
    ledger
        .save(HashMap::from([(name.clone(), "secret".into())]), &platform)
        .await
        .unwrap();
    ledger.purge(vec![], vec![], &platform).await;
    ledger.purge(vec![], vec![], &platform).await;
    assert_eq!(*platform.deleted.lock().unwrap(), [name]);
}
#[tokio::test]
async fn copy_prefers_pending_edits_and_missing_values_stay_missing() {
    let platform = Platform::default();
    platform
        .values
        .lock()
        .unwrap()
        .insert("old".into(), "saved".into());
    let ledger = SessionSecrets::default();
    ledger.edit_batch(HashMap::from([("old".into(), "edited".into())]));
    let copied = ledger
        .copy(
            HashMap::from([
                ("old".into(), "new".into()),
                ("missing".into(), "absent".into()),
            ]),
            &platform,
        )
        .await
        .unwrap();
    assert_eq!(copied.get("new").unwrap(), "edited");
    assert!(!platform.values.lock().unwrap().contains_key("absent"));
}
#[tokio::test]
async fn keychain_failure_keeps_pending_edits_and_retries_flush() {
    let platform = Platform::default();
    platform.fails.store(true, Ordering::Relaxed);
    let ledger = SessionSecrets::default();
    ledger.edit_batch(HashMap::from([("token".into(), "secret".into())]));
    assert!(ledger.flush(&platform).await.is_err());
    assert_eq!(ledger.dirty(), ["token"]);
    platform.fails.store(false, Ordering::Relaxed);
    ledger.flush(&platform).await.unwrap();
    assert!(ledger.dirty().is_empty());
}
#[tokio::test]
async fn failed_delete_keeps_observed_name_for_later_retry() {
    let platform = Platform::default();
    let name = owned("password");
    let ledger = SessionSecrets::default();
    ledger.observe(vec![name.clone()]);
    platform.fails.store(true, Ordering::Relaxed);
    ledger.purge(vec![], vec![], &platform).await;
    assert!(ledger.has_observed());
    platform.fails.store(false, Ordering::Relaxed);
    ledger.purge(vec![], vec![], &platform).await;
    assert_eq!(*platform.deleted.lock().unwrap(), [name]);
}
#[tokio::test]
async fn loading_does_not_overwrite_an_edited_value() {
    let platform = Platform::default();
    platform
        .values
        .lock()
        .unwrap()
        .insert("token".into(), "saved".into());
    let ledger = SessionSecrets::default();
    ledger.edit_batch(HashMap::from([("token".into(), "edited".into())]));
    assert_eq!(
        ledger.load("token".into(), &platform).await.unwrap(),
        "edited"
    );
}

#[tokio::test]
async fn staged_tls_cancel_replace_and_remove_never_write_abandoned_keys() {
    use crate::domain::request::credential_reference;
    let platform = Platform::default();
    let ledger = SessionSecrets::default();
    let abandoned = credential_reference("client-identity", "tls");
    {
        let cancelled = SessionSecrets::default();
        cancelled.edit_batch(HashMap::from([(abandoned.clone(), "fixture-key".into())]));
    }
    assert!(platform.values.lock().unwrap().is_empty());
    let staged = SessionSecrets::default();
    let selected = credential_reference("client-identity", "tls");
    staged.edit_batch(HashMap::from([
        (abandoned.clone(), "old-key".into()),
        (selected.clone(), "new-key".into()),
    ]));
    staged
        .commit_identity(None, &ledger, &platform)
        .await
        .unwrap();
    assert!(platform.values.lock().unwrap().is_empty());
    staged
        .commit_identity(Some(&selected), &ledger, &platform)
        .await
        .unwrap();
    assert_eq!(platform.values.lock().unwrap().len(), 1);
    assert!(!platform.values.lock().unwrap().contains_key(&abandoned));
    ledger
        .purge(vec![selected.clone()], vec![], &platform)
        .await;
    assert!(
        platform.deleted.lock().unwrap().is_empty(),
        "Retain identity while saved or undoable"
    );
    ledger.purge(vec![], vec![], &platform).await;
    assert_eq!(*platform.deleted.lock().unwrap(), [selected]);
}

#[tokio::test]
async fn tls_reference_in_another_workspace_prevents_key_deletion() {
    let root = tempfile::tempdir().unwrap();
    let name = crate::domain::request::credential_reference("client-identity", "tls");
    std::fs::write(
        root.path().join("wirebolt.toml"),
        format!("[transport]\nclient_certificate_reference = '{name}'"),
    )
    .unwrap();
    let ledger = SessionSecrets::default();
    let platform = Platform::default();
    ledger
        .save(
            HashMap::from([(name.clone(), "fixture-key".into())]),
            &platform,
        )
        .await
        .unwrap();
    ledger
        .purge(
            vec![],
            vec![root.path().to_string_lossy().into_owned()],
            &platform,
        )
        .await;
    assert!(platform.deleted.lock().unwrap().is_empty());
}
