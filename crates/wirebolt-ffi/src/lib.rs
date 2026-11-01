uniffi::setup_scaffolding!();
mod app_domain;
mod app_pem;
mod app_response;
mod app_session;
mod app_syntax;
mod native_transport;

#[cfg(test)]
use wirebolt_app::session::transport::{HttpEngineCache, timeout_from_millis};
use wirebolt_app::session::transport::{clear_manual_http_engines, shared_http_engine};

use std::{
    error::Error,
    fmt,
    sync::{Arc, LazyLock},
};

#[cfg(test)]
use std::collections::BTreeMap;
use wirebolt_app::lock;
#[cfg(not(target_vendor = "apple"))]
use wirebolt_core::NoSecrets;
#[cfg(test)]
use wirebolt_core::{
    Collection, DocumentId, Environment, HttpEngine, Request, RequestHeader, ResolvedProxy,
    ValueSource, Workspace, WorkspaceDocument, WorkspaceStore,
};
use wirebolt_core::{HttpEngineConfig, ProxyMode, ProxyPolicy, SecretName};

#[derive(Debug, Eq, PartialEq, uniffi::Record)]
pub struct CoreHandshake {
    pub product: String,
    pub core_version: String,
    pub stream_abi_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct HeaderField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct BridgeRequestDraft {
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderField>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct PreparedRequestSummary {
    pub method: String,
    pub url: String,
    pub header_count: u64,
    pub body_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Error)]
pub enum RequestPreparationError {
    InvalidRequest { reason: String },
}

impl fmt::Display for RequestPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { reason } => formatter.write_str(reason),
        }
    }
}

impl Error for RequestPreparationError {}

use wirebolt_app::session::persistence::{
    SessionPersistence, WorkspaceError as WorkspaceBridgeError,
    WorkspaceGitError as GitBridgeError, WorkspaceSecrets,
};
#[uniffi::remote(Error)]
pub enum WorkspaceBridgeError {
    OperationFailed { reason: String },
}
#[uniffi::remote(Error)]
pub enum GitBridgeError {
    OperationFailed {
        kind: wirebolt_app::session::workspace::io::WorkspaceFailureKind,
        reason: String,
    },
}
#[derive(Debug, uniffi::Object)]
pub struct WorkspaceBridge {
    inner: Arc<SessionPersistence>,
}
#[uniffi::export]
impl WorkspaceBridge {
    #[uniffi::constructor]
    /// # Errors
    /// Returns the application operation error.
    pub fn open_or_create(path: String, name: String) -> Result<Arc<Self>, WorkspaceBridgeError> {
        Ok(Arc::new(Self {
            inner: SessionPersistence::open_or_create(
                path,
                name,
                Arc::new(PlatformWorkspaceSecrets),
            )?,
        }))
    }

    /// # Errors
    /// Returns the application operation error.
    pub fn read_secret(&self, name: &str) -> Result<Option<String>, WorkspaceBridgeError> {
        self.inner.read_secret(name)
    }
    /// # Errors
    /// Returns the application operation error.
    pub fn save_secret(&self, name: &str, value: &str) -> Result<(), WorkspaceBridgeError> {
        self.inner.save_secret(name, value)
    }
    /// # Errors
    /// Returns the application operation error.
    pub fn delete_secret(&self, name: &str) -> Result<(), WorkspaceBridgeError> {
        self.inner.delete_secret(name)
    }
}
#[derive(Debug)]
struct PlatformWorkspaceSecrets;
impl WorkspaceSecrets for PlatformWorkspaceSecrets {
    fn read(&self, name: &str) -> Result<Option<String>, WorkspaceBridgeError> {
        let name = SecretName::new(name)
            .map_err(|_| WorkspaceBridgeError::operation("secret name is invalid"))?;
        #[cfg(target_vendor = "apple")]
        {
            wirebolt_core::KeychainSecretStore::default()
                .read(&name)
                .map_err(|_| WorkspaceBridgeError::operation("secret could not be read"))
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = name;
            Err(WorkspaceBridgeError::operation(
                "secret storage is unavailable",
            ))
        }
    }
    fn save(&self, name: &str, value: &str) -> Result<(), WorkspaceBridgeError> {
        let name = SecretName::new(name)
            .map_err(|_| WorkspaceBridgeError::operation("secret name is invalid"))?;
        #[cfg(target_vendor = "apple")]
        {
            wirebolt_core::KeychainSecretStore::default()
                .save(&name, value)
                .map_err(|_| WorkspaceBridgeError::operation("secret could not be saved"))?;
            clear_manual_http_engines();
            Ok(())
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (name, value);
            Err(WorkspaceBridgeError::operation(
                "secret storage is unavailable",
            ))
        }
    }
    fn remove(&self, name: &str) -> Result<(), WorkspaceBridgeError> {
        let name = SecretName::new(name)
            .map_err(|_| WorkspaceBridgeError::operation("secret name is invalid"))?;
        #[cfg(target_vendor = "apple")]
        {
            let store = wirebolt_core::KeychainSecretStore::default();
            if store.remove(&name).is_err() && !matches!(store.read(&name), Ok(None)) {
                return Err(WorkspaceBridgeError::operation(
                    "secret could not be deleted",
                ));
            }
            clear_manual_http_engines();
            Ok(())
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = name;
            Err(WorkspaceBridgeError::operation(
                "secret storage is unavailable",
            ))
        }
    }
}
#[uniffi::export]
#[must_use]
pub fn core_handshake() -> CoreHandshake {
    wirebolt_core::handshake().into()
}

#[uniffi::export]
/// Prepares one coarse-grained request across the Swift–Rust boundary.
///
/// # Errors
///
/// Returns [`RequestPreparationError`] when the Rust core rejects any request
/// component.
pub fn prepare_request(
    draft: BridgeRequestDraft,
) -> Result<PreparedRequestSummary, RequestPreparationError> {
    let prepared = wirebolt_core::prepare_request(draft.into()).map_err(|error| {
        RequestPreparationError::InvalidRequest {
            reason: error.to_string(),
        }
    })?;

    Ok(PreparedRequestSummary {
        method: prepared.method().to_string(),
        url: prepared.url().to_string(),
        header_count: prepared.headers().len().try_into().unwrap_or(u64::MAX),
        body_bytes: prepared.body_byte_count(),
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn wirebolt_stream_abi_version() -> u32 {
    wirebolt_core::STREAM_ABI_VERSION
}

/// Starts the shared runtime and warms the direct and system HTTP engines in
/// the background, so the first request does not pay for TLS and proxy
/// discovery. Returns 1 when the runtime is available.
#[unsafe(no_mangle)]
pub extern "C" fn wirebolt_runtime_warmup() -> u8 {
    let Some(runtime) = SHARED_RUNTIME.as_ref() else {
        return 0;
    };
    runtime.spawn_blocking(|| {
        for mode in [ProxyMode::Direct, ProxyMode::System] {
            let proxy = ProxyPolicy::with_workspace(mode).resolve(None);
            let _ = shared_http_engine(
                &proxy,
                &HttpEngineConfig::default(),
                &wirebolt_core::NoSecrets,
            );
        }
    });
    1
}

/// Drops every pooled HTTP engine so the next run rebuilds its client. Call
/// this when the network configuration changes: the system proxy settings
/// are captured once per engine, and pooled connections may be dead.
#[uniffi::export]
pub fn reset_http_engines() {
    wirebolt_app::session::transport::reset_http_engines();
}

static SHARED_RUNTIME: LazyLock<Option<tokio::runtime::Runtime>> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("wirebolt-http")
        .enable_all()
        .build()
        .ok()
});

impl From<wirebolt_core::CoreHandshake> for CoreHandshake {
    fn from(value: wirebolt_core::CoreHandshake) -> Self {
        Self {
            product: value.product.to_owned(),
            core_version: value.core_version.to_owned(),
            stream_abi_version: value.stream_abi_version,
        }
    }
}

impl From<BridgeRequestDraft> for wirebolt_core::RequestDraft {
    fn from(value: BridgeRequestDraft) -> Self {
        Self {
            method: value.method,
            url: value.url,
            headers: value.headers.into_iter().map(Into::into).collect(),
            body: value.body,
        }
    }
}

impl From<HeaderField> for wirebolt_core::HeaderField {
    fn from(value: HeaderField) -> Self {
        Self {
            name: value.name,
            value: value.value,
        }
    }
}

#[cfg(test)]
impl WorkspaceBridge {
    /// # Errors
    /// Returns the application operation error.
    pub fn export_workspace_json(&self) -> Result<String, WorkspaceBridgeError> {
        self.inner.export_workspace_json()
    }
    /// # Errors
    /// Returns the application operation error.
    pub fn export_collection_json(&self, id: &str) -> Result<String, WorkspaceBridgeError> {
        self.inner.export_collection_json(id)
    }
    /// # Errors
    /// Returns the application operation error.
    pub fn export_request_json(
        &self,
        collection_id: &str,
        id: &str,
    ) -> Result<String, WorkspaceBridgeError> {
        self.inner.export_request_json(collection_id, id)
    }
    /// # Errors
    /// Returns the application operation error.
    pub fn save_collection(&self, id: String, name: String) -> Result<(), WorkspaceBridgeError> {
        self.inner.save_collection(id, name)
    }
    fn request(&self, collection_id: &str, id: &str) -> Result<Request, WorkspaceBridgeError> {
        self.inner.request(collection_id, id)
    }
    fn collection(&self, id: &str) -> Result<Collection, WorkspaceBridgeError> {
        self.inner.collection(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::Command,
        sync::Mutex,
        thread,
        time::Duration,
    };
    use wirebolt_app::{
        response::FailureIssue,
        session::{
            run::{SessionFailure, SessionPrepared},
            transport::{
                PreparedRunSnapshotDocument, RunFailureDocument, RunInput, TransportObserver,
            },
        },
    };
    use wirebolt_core::{RunCancellation, RunHead};

    use wirebolt_app::{
        domain::{DomainCollection, DomainGroup, DomainLocation, DomainRequestDraft},
        session::{
            persistence::ImportCommit,
            workspace::{SessionWorkspaceCommand as C, io::WorkspaceFailureKind as WK},
        },
    };
    use wirebolt_core::ImportFormat;
    fn collection_command(id: &str, name: &str) -> C {
        C::CreateCollection {
            collection: DomainCollection {
                id: id.into(),
                name: name.into(),
                order: 0,
                groups: vec![],
                requests: vec![],
            },
        }
    }
    fn group_command(collection_id: &str, id: &str, name: &str) -> C {
        C::CreateGroup {
            collection_id: collection_id.into(),
            group: DomainGroup {
                id: id.into(),
                name: name.into(),
                parent_id: None,
                order: 0,
            },
        }
    }
    fn request_command(id: &str, name: &str, group: Option<&str>, order: i64) -> C {
        C::SaveRequest {
            location: DomainLocation {
                collection_id: "api".into(),
                group_id: group.map(str::to_owned),
                order,
                request: DomainRequestDraft {
                    id: id.into(),
                    name: name.into(),
                    url: format!("https://example.com/{id}"),
                    ..Default::default()
                },
            },
        }
    }

    #[test]
    fn reorder_mixed_siblings_persists_and_rejects_incomplete_or_foreign_items() {
        let directory = tempfile::tempdir().unwrap();
        WorkspaceStore::create(directory.path(), &Workspace::new("Reorder")).unwrap();
        let path = directory.path().to_string_lossy().into_owned();
        let bridge = WorkspaceBridge::open_or_create(path.clone(), "Reorder".into()).unwrap();
        for command in [
            collection_command("api", "API"),
            group_command("api", "folder", "Folder"),
            request_command("a", "A", None, 0),
            request_command("b", "B", None, 99),
            request_command("nested", "Nested", Some("folder"), 42),
        ] {
            bridge.inner.apply_session_command(command).unwrap();
        }
        bridge
            .inner
            .apply_session_command(C::ReorderChildren {
                collection_id: "api".into(),
                parent_id: None,
                items: vec![
                    "request:b".into(),
                    "group:folder".into(),
                    "request:a".into(),
                ],
            })
            .unwrap();
        let reopened = WorkspaceBridge::open_or_create(path, "Reorder".into()).unwrap();
        assert_eq!(reopened.request("api", "b").unwrap().order, 0);
        assert_eq!(reopened.collection("api").unwrap().groups[0].order, 1);
        assert_eq!(reopened.request("api", "a").unwrap().order, 2);
        assert_eq!(reopened.request("api", "nested").unwrap().order, 42);
        assert_eq!(
            reopened.request("api", "b").unwrap().url,
            "https://example.com/b"
        );
        let before = reopened.inner.domain_snapshot().unwrap();
        for invalid in [
            vec!["request:a"],
            vec!["request:a", "request:a", "group:folder"],
            vec!["request:a", "request:nested", "group:folder"],
            vec!["request:a", "request:missing", "group:folder"],
        ] {
            assert!(
                reopened
                    .inner
                    .apply_session_command(C::ReorderChildren {
                        collection_id: "api".into(),
                        parent_id: None,
                        items: invalid.into_iter().map(str::to_owned).collect()
                    })
                    .is_err()
            );
            assert_eq!(reopened.inner.domain_snapshot().unwrap(), before);
        }
    }

    fn test_secret(name: &str) -> ValueSource {
        ValueSource::secret(SecretName::new(name).unwrap())
    }

    /// Exports a workspace with two collections, a global and a staging environment.
    fn exported_source_workspace() -> String {
        let id = |value: &str| DocumentId::new(value).unwrap();
        let secret = test_secret;
        let source_directory = tempfile::tempdir().unwrap();
        let source =
            WorkspaceStore::create(source_directory.path(), &Workspace::new("Source")).unwrap();
        let mut request = Request::new(
            id("search"),
            "Search",
            "GET",
            "https://api.example.test/items",
        );
        request.headers = vec![RequestHeader {
            sensitive: true,
            ..RequestHeader::enabled("X-Api-Token", secret("inventory.token"))
        }];
        request.proxy_override = Some(ProxyMode::Direct);
        request.transport.validate_tls = false;
        request.inherits_workspace_transport = false;
        let documents = [
            WorkspaceDocument::Collection(Collection::new(id("inventory"), "Inventory".into())),
            WorkspaceDocument::Request {
                collection_id: id("inventory"),
                request,
            },
            WorkspaceDocument::Collection(Collection::new(id("billing"), "Billing".into())),
            WorkspaceDocument::Request {
                collection_id: id("billing"),
                request: Request::new(
                    id("invoices"),
                    "Invoices",
                    "GET",
                    "https://billing.example.test",
                ),
            },
            WorkspaceDocument::Environment(Environment::new(
                id("global"),
                "Source globals".into(),
                [(
                    "baseUrl".to_owned(),
                    ValueSource::literal("https://api.example.test"),
                )]
                .into(),
            )),
            WorkspaceDocument::Environment(Environment::new(
                id("staging"),
                "Staging".into(),
                [("token".to_owned(), secret("staging.token"))].into(),
            )),
        ];
        for document in &documents {
            source.save(document).unwrap();
        }
        WorkspaceBridge::open_or_create(
            source_directory.path().to_string_lossy().into_owned(),
            "Source".into(),
        )
        .unwrap()
        .export_workspace_json()
        .unwrap()
    }

    #[test]
    fn workspace_export_file_imports_as_separate_collections_and_environments() {
        let exported = exported_source_workspace();
        let secret = test_secret;
        let target_directory = tempfile::tempdir().unwrap();
        let target =
            WorkspaceStore::create(target_directory.path(), &Workspace::new("Target")).unwrap();
        target
            .save(&WorkspaceDocument::Environment(Environment::new(
                DocumentId::new("global").unwrap(),
                "Target globals".into(),
                BTreeMap::new(),
            )))
            .unwrap();
        let bridge = WorkspaceBridge::open_or_create(
            target_directory.path().to_string_lossy().into_owned(),
            "Target".into(),
        )
        .unwrap();
        let delta = bridge
            .inner
            .commit_import_file(ImportFormat::LegacyWorkspaceV1, &exported, "source-export")
            .unwrap();
        assert_eq!(delta.delta.affected_ids.len(), 2);

        let snapshot = target.load().unwrap();
        let mut collections: Vec<_> = snapshot
            .collections
            .iter()
            .map(|entry| entry.collection.name.as_str())
            .collect();
        collections.sort_unstable();
        assert_eq!(collections, ["Billing", "Inventory"]);
        let inventory = snapshot
            .collections
            .iter()
            .find(|entry| entry.collection.name == "Inventory")
            .unwrap();
        assert!(inventory.collection.groups.is_empty());
        let imported = &inventory.requests[0];
        assert_eq!(imported.headers[0].value, secret("inventory.token"));
        assert!(imported.headers[0].sensitive);
        assert_eq!(imported.proxy_override, Some(ProxyMode::Direct));
        assert!(!imported.transport.validate_tls);
        assert!(!imported.inherits_workspace_transport);

        let environment = |name: &str| {
            snapshot
                .environments
                .iter()
                .find(|environment| environment.name == name)
                .unwrap()
        };
        assert_eq!(snapshot.environments.len(), 3);
        assert_eq!(environment("Target globals").id.as_str(), "global");
        assert_ne!(environment("Source globals").id.as_str(), "global");
        assert_eq!(
            environment("Staging").variables[0].value,
            secret("staging.token")
        );
    }

    #[test]
    fn workspace_proxy_commands_round_trip_and_reset_without_changing_transport() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_string_lossy().into_owned();
        let mut workspace = Workspace::new("Proxy fixture");
        workspace.transport.total_timeout_ms = 123;
        WorkspaceStore::create(directory.path(), &workspace).unwrap();
        let bridge = WorkspaceBridge::open_or_create(path.clone(), "Proxy fixture".into()).unwrap();
        let proxy = serde_json::json!({"mode":"manual","routes":[{"destination":"all","endpoint":"http://localhost:8080/"}]});
        bridge
            .inner
            .apply_session_command(C::SaveWorkspaceProxy {
                proxy: Some(serde_json::from_value(proxy.clone()).unwrap()),
            })
            .unwrap();
        let reopened = WorkspaceBridge::open_or_create(path, "Proxy fixture".into()).unwrap();
        let snapshot = reopened.inner.domain_snapshot().unwrap();
        assert_eq!(snapshot.proxy, Some(serde_json::from_value(proxy).unwrap()));
        assert_eq!(snapshot.transport.total_timeout_ms, 123);
        reopened
            .inner
            .apply_session_command(C::SaveWorkspaceProxy { proxy: None })
            .unwrap();
        let reset = reopened.inner.domain_snapshot().unwrap();
        assert!(reset.proxy.is_none());
        assert_eq!(reset.transport.total_timeout_ms, 123);
    }

    #[test]
    fn a_failed_app_proxy_never_retries_the_destination_directly() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let unused = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_address = unused.local_addr().unwrap();
        drop(unused);
        let input = serde_json::json!({"method":"GET", "url":format!("http://{}/must-not-arrive", destination.local_addr().unwrap()),
            "app_proxy":{"mode":"manual", "routes":[{"destination":"all", "endpoint":format!("http://{proxy_address}")}]}});
        let (events, state) = run_to_completion(&input.to_string());
        assert_eq!(events, ["prepared", "error"]);
        assert_eq!(
            state.failure.lock().unwrap().as_ref().unwrap().kind,
            wirebolt_app::response::ResponseFailureKind::Connection
        );
        assert!(
            matches!(destination.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }

    #[test]
    fn app_proxy_routes_over_the_stream_bridge_and_request_direct_bypasses_it() {
        for direct in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let server = thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    if let Ok((stream, _)) = listener.accept() {
                        break stream;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "proxy was never reached"
                    );
                    thread::sleep(Duration::from_millis(5));
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = [0_u8; 4096];
                let count = stream.read(&mut bytes).unwrap();
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npong",
                    )
                    .unwrap();
                String::from_utf8_lossy(&bytes[..count]).into_owned()
            });
            let endpoint = if direct {
                "http://127.0.0.1:9".to_owned()
            } else {
                format!("http://{address}")
            };
            let url = if direct {
                format!("http://{address}/proxy-test")
            } else {
                "http://127.0.0.1:9/proxy-test".to_owned()
            };
            let mut input = serde_json::json!({"method":"GET","url":url,"app_proxy":{"mode":"manual","routes":[{"destination":"all","endpoint":endpoint}]}});
            if direct {
                input["request_proxy"] = serde_json::json!({"mode":"direct"});
            }
            let (events, state) = run_to_completion(&input.to_string());
            assert_eq!(events, ["prepared", "head", "chunk:pong", "complete"]);
            let snapshot = state.prepared.lock().unwrap().clone().unwrap();
            assert_eq!(
                snapshot.proxy.as_ref().unwrap().source,
                if direct {
                    wirebolt_app::domain::proxy::DomainProxySource::Request
                } else {
                    wirebolt_app::domain::proxy::DomainProxySource::AppDefault
                }
            );
            assert_eq!(
                matches!(
                    snapshot.proxy.unwrap().configuration,
                    wirebolt_app::domain::DomainProxy::Direct
                ),
                direct
            );
            let request = server.join().unwrap();
            assert!(request.starts_with(if direct {
                "GET /proxy-test "
            } else {
                "GET http://127.0.0.1:9/proxy-test "
            }));
        }
    }

    #[test]
    fn imports_after_reopening_keep_previously_imported_collections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_string_lossy().into_owned();
        let first = WorkspaceBridge::open_or_create(path.clone(), "Fixture".into()).unwrap();
        let first_delta = first
            .inner
            .commit_import(ImportFormat::Curl, "curl 'https://example.com/first'")
            .unwrap();
        drop(first);
        let reopened = WorkspaceBridge::open_or_create(path, "Fixture".into()).unwrap();
        let second_delta = reopened
            .inner
            .commit_import(ImportFormat::Curl, "curl 'https://example.com/second'")
            .unwrap();
        assert_ne!(
            first_delta.delta.affected_ids,
            second_delta.delta.affected_ids
        );
        let snapshot = reopened.inner.domain_snapshot().unwrap();
        assert_eq!(snapshot.collections.len(), 2);
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(serialized.contains("https://example.com/first"));
        assert!(serialized.contains("https://example.com/second"));
        assert!(
            reopened
                .inner
                .commit_import(
                    ImportFormat::LegacyWorkspaceV1,
                    r#"{"version":2,"nodes":[]}"#
                )
                .is_err()
        );
        assert_eq!(reopened.inner.domain_snapshot().unwrap(), snapshot);
    }

    #[test]
    fn bruno_file_import_reports_warnings_and_keeps_variable_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let bridge = WorkspaceBridge::open_or_create(
            directory.path().to_string_lossy().into_owned(),
            "Fixture".into(),
        )
        .unwrap();
        let source = r#"{
          "name": "Demo", "version": "1",
          "items": [{"type": "http", "name": "Me", "seq": 1, "request": {
            "url": "{{baseUrl}}/me", "method": "GET",
            "auth": {"mode": "bearer", "bearer": {"token": "{{token}}"}}
          }}],
          "environments": [{"name": "Local", "variables": [
            {"name": "baseUrl", "value": "http://127.0.0.1:18990", "enabled": true, "secret": false},
            {"name": "token", "value": "", "enabled": true, "secret": true}
          ]}]
        }"#;
        let committed = bridge
            .inner
            .commit_import_file(ImportFormat::Bruno, source, "Demo")
            .unwrap();
        let summary = &committed.summary;
        assert_eq!(summary.collection_names, ["Demo"]);
        assert_eq!(summary.environments[0].name, "Demo \u{2013} Local");
        assert!(
            summary
                .warnings
                .iter()
                .any(|warning| warning.contains("secret values"))
        );
        let snapshot = bridge.inner.domain_snapshot().unwrap();
        let serialized = serde_json::to_string(&snapshot.environments).unwrap();
        assert!(serialized.contains("http://127.0.0.1:18990"));
        assert!(serialized.contains("demo.local.token"));
        // A templated credential stays a variable reference instead of a Keychain copy.
        assert!(
            serde_json::to_string(&snapshot.collections)
                .unwrap()
                .contains("{{token}}")
        );
        assert!(
            bridge
                .inner
                .commit_import_file(ImportFormat::Bruno, "{}", "Broken")
                .is_err()
        );
        let after = bridge.inner.domain_snapshot().unwrap();
        assert_eq!(after.environments, snapshot.environments);
    }

    #[test]
    fn committed_imports_return_a_summary_and_create_their_environment() {
        let directory = tempfile::tempdir().unwrap();
        let bridge = WorkspaceBridge::open_or_create(
            directory.path().to_string_lossy().into_owned(),
            "Fixture".into(),
        )
        .unwrap();
        let source = r#"{
          "info": {"name": "Demo", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
          "variable": [{"key": "baseUrl", "value": "https://api.example.test"}],
          "event": [{"listen": "prerequest", "script": {"exec": ["console.log(1)"]}}],
          "item": [{"name": "Folder", "item": [
            {"name": "List", "request": {"method": "GET", "url": "{{baseUrl}}/items"}}
          ]}]
        }"#;

        let committed = bridge
            .inner
            .commit_import_file(ImportFormat::PostmanV2, source, "demo")
            .unwrap();

        let summary = &committed.summary;
        assert_eq!(summary.collection_names, ["Demo"]);
        assert_eq!(summary.request_count, 1);
        assert_eq!(summary.group_count, 1);
        assert_eq!(summary.environments[0].name, "Demo");
        assert!(summary.warnings[0].contains("Scripts aren't supported"));
        let environment_id = summary.environments[0].id.as_str();
        let snapshot = bridge.inner.domain_snapshot().unwrap();
        assert!(
            serde_json::to_string(&snapshot)
                .unwrap()
                .contains(environment_id)
        );
        assert!(
            serde_json::to_string(&snapshot)
                .unwrap()
                .contains("https://api.example.test")
        );
        assert_eq!(committed.delta.affected_ids.len(), 1);
    }

    #[test]
    fn imported_curl_transport_options_apply_to_the_request() {
        let directory = tempfile::tempdir().unwrap();
        let bridge = WorkspaceBridge::open_or_create(
            directory.path().to_string_lossy().into_owned(),
            "Fixture".into(),
        )
        .unwrap();

        bridge
            .inner
            .commit_import(
                ImportFormat::Curl,
                "curl -k -L \\\n  https://api.example.test/items",
            )
            .unwrap();

        let snapshot = bridge.inner.domain_snapshot().unwrap();
        let request = &snapshot.collections[0].requests[0].request;
        assert_eq!(request.url, "https://api.example.test/items");
        assert!(!request.inherits_workspace_transport);
        assert!(!request.transport.validate_tls);
        assert!(request.transport.follow_redirects);
    }

    #[test]
    fn failed_imports_report_why_without_changing_the_workspace() {
        let directory = tempfile::tempdir().unwrap();
        let bridge = WorkspaceBridge::open_or_create(
            directory.path().to_string_lossy().into_owned(),
            "Fixture".into(),
        )
        .unwrap();
        let before = bridge.inner.domain_snapshot().unwrap();
        let reason = |result: Result<ImportCommit, WorkspaceBridgeError>| match result {
            Err(WorkspaceBridgeError::OperationFailed { reason }) => reason,
            Ok(_) => panic!("import must fail"),
        };

        assert!(
            reason(bridge.inner.commit_import_file(
                ImportFormat::PostmanV2,
                r#"{"openapi": "3.1.0", "info": {"title": "Pets"}, "paths": {}}"#,
                "pets"
            ))
            .contains("OpenAPI")
        );
        assert!(reason(bridge.inner.commit_import(ImportFormat::Har, "{}")).contains("HAR"));
        assert_eq!(bridge.inner.domain_snapshot().unwrap(), before);
    }

    #[test]
    fn both_bridge_surfaces_share_the_same_version() {
        let handshake = core_handshake();

        assert_eq!(handshake.product, "Wirebolt");
        assert_eq!(handshake.stream_abi_version, wirebolt_stream_abi_version());
    }

    #[test]
    fn prepares_a_request_through_the_coarse_bridge() {
        let summary = prepare_request(BridgeRequestDraft {
            method: "POST".to_owned(),
            url: "https://api.example.com/v1/items".to_owned(),
            headers: vec![HeaderField {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
            }],
            body: br#"{"fast":true}"#.to_vec(),
        })
        .expect("valid request");

        assert_eq!(summary.method, "POST");
        assert_eq!(summary.url, "https://api.example.com/v1/items");
        assert_eq!(summary.header_count, 1);
        assert_eq!(summary.body_bytes, 13);
    }

    #[test]
    fn transport_observer_delivers_head_chunks_and_completion_in_order() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept HTTP client");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read HTTP request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nX-Wirebolt: ffi\r\nConnection: close\r\n\r\npong",
                )
                .expect("write HTTP response");
        });
        let input = serde_json::json!({
            "method": "GET",
            "url": format!("http://{address}/ffi"),
            "workspace_proxy": { "mode": "direct" }
        })
        .to_string();
        let (events, _state) = run_to_completion(&input);
        server.join().expect("HTTP server");
        assert_eq!(events, ["prepared", "head", "chunk:pong", "complete"]);
    }

    #[test]
    fn transport_failures_retain_kind_and_empty_issues() {
        let input = serde_json::json!({
            "method": "GET",
            "url": "http://127.0.0.1:1/refused",
            "workspace_proxy": { "mode": "direct" }
        })
        .to_string();
        let (events, state) = run_to_completion(&input);

        assert_eq!(events, ["prepared", "error"]);
        let failure = state
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("failure document");
        assert!(failure.issues.is_empty());
        assert_eq!(
            failure.kind,
            wirebolt_app::response::ResponseFailureKind::Connection
        );
    }

    #[test]
    fn workspace_bridge_round_trips_hierarchy_without_secret_values() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        let mut workspace = Workspace::new("Demo");
        workspace.proxy = Some(ProxyMode::Direct);
        WorkspaceStore::create(temporary.path(), &workspace).expect("create workspace");
        let bridge = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Demo".to_owned(),
        )
        .expect("workspace bridge");
        bridge
            .save_collection("api".to_owned(), "API".to_owned())
            .expect("save collection");
        let C::SaveRequest { mut location } = request_command("health", "Health", None, 0) else {
            unreachable!()
        };
        location.request.url = "{{endpoint}}".into();
        location.request.web_socket = true;
        location.request.note = "café 東京 🚀".into();
        location.request.authentication = wirebolt_app::domain::DomainAuthentication::Bearer {
            token: wirebolt_app::domain::DomainValueSource::Secret {
                secret: "api.token".into(),
            },
        };
        bridge
            .inner
            .apply_session_command(C::SaveRequest { location })
            .expect("save request");
        std::fs::write(
            temporary.path().join("environments/broken.toml"),
            "<<<<<<< HEAD\nid = \"broken\"\n=======\n",
        )
        .expect("write conflicted environment");

        let snapshot = bridge.inner.domain_snapshot().expect("workspace snapshot");
        let document = &snapshot;

        assert!(document.collections[0].requests[0].request.web_socket);
        assert_eq!(
            document.collections[0].requests[0].request.note,
            "café 東京 🚀"
        );
        let reopened = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Demo".to_owned(),
        )
        .expect("reopen");
        let restored = reopened.inner.domain_snapshot().unwrap();
        assert!(restored.collections[0].requests[0].request.web_socket);
        assert_eq!(
            restored.collections[0].requests[0].request.note,
            "café 東京 🚀"
        );
        assert_eq!(document.name, "Demo");
        assert_eq!(
            document.proxy,
            Some(wirebolt_app::domain::DomainProxy::Direct)
        );
        assert_eq!(document.collections[0].id, "api");
        assert_eq!(document.collections[0].requests[0].request.id, "health");
        assert_eq!(
            document.collections[0].requests[0].request.authentication,
            wirebolt_app::domain::DomainAuthentication::Bearer {
                token: wirebolt_app::domain::DomainValueSource::Secret {
                    secret: "api.token".into()
                }
            }
        );
        let problems = bridge.inner.store.load().unwrap().problems;
        assert_eq!(
            problems[0].path,
            std::path::PathBuf::from("environments/broken.toml")
        );
        assert_eq!(
            problems[0].kind,
            wirebolt_core::DocumentProblemKind::InvalidToml
        );
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("secret-value")
        );
        let request_export = bridge
            .export_request_json("api", "health")
            .expect("export request");
        let collection_export = bridge
            .export_collection_json("api")
            .expect("export collection");
        let workspace_export = bridge.export_workspace_json().expect("export workspace");
        for exported in [request_export, collection_export, workspace_export] {
            assert!(exported.contains("api.token"));
            assert!(!exported.contains("secret-value"));
        }
    }

    #[test]
    fn workspace_commands_persist_groups_and_return_monotonic_deltas() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        WorkspaceStore::create(temporary.path(), &Workspace::new("Commands"))
            .expect("create workspace");
        let bridge = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Commands".to_owned(),
        )
        .expect("workspace bridge");

        let first = bridge
            .inner
            .apply_session_command(collection_command("api", "API"))
            .expect("create collection");
        let second = bridge
            .inner
            .apply_session_command(group_command("api", "auth", "Authentication"))
            .expect("create group");
        let C::SaveRequest { mut location } = request_command("login", "Login", Some("auth"), 0)
        else {
            unreachable!()
        };
        location.request.method = "POST".into();
        bridge
            .inner
            .apply_session_command(C::SaveRequest { location })
            .expect("save grouped request");
        assert_eq!(first.version, 1);
        assert_eq!(second.version, 2);

        let snapshot = bridge.inner.domain_snapshot().expect("workspace snapshot");
        assert_eq!(snapshot.collections[0].groups[0].id, "auth");
        assert_eq!(
            snapshot.collections[0].requests[0].group_id.as_deref(),
            Some("auth")
        );
    }

    #[test]
    fn workspace_bridge_exposes_typed_git_operations() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        WorkspaceStore::create(temporary.path(), &Workspace::new("Demo"))
            .expect("create workspace");
        let bridge = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Demo".to_owned(),
        )
        .expect("workspace bridge");
        for arguments in [
            &["init", "-b", "main"][..],
            &["config", "user.name", "Wirebolt Tests"][..],
            &["config", "user.email", "wirebolt@example.invalid"][..],
            &["config", "commit.gpgsign", "false"][..],
        ] {
            let output = Command::new("git")
                .arg("-C")
                .arg(temporary.path())
                .args(arguments)
                .output()
                .expect("run Git");
            assert!(output.status.success());
        }

        let before = bridge.inner.git_status().expect("Git status");
        assert_eq!(before.branch.as_deref(), Some("main"));
        assert_eq!(before.changes[0].path, "wirebolt.toml");
        assert_eq!(
            before.changes[0].unstaged,
            wirebolt_core::GitDelta::Untracked
        );
        let operation = bridge
            .inner
            .git_commit("initial workspace")
            .expect("Git commit");
        assert_eq!(
            operation.outcome,
            wirebolt_app::session::workspace::io::WorkspaceGitOutcome::Committed
        );
        assert!(operation.revision.is_some());
        assert!(operation.status.changes.is_empty());
        assert!(!operation.status.merging);
        let Err(GitBridgeError::OperationFailed { kind, .. }) = bridge.inner.git_abort_merge()
        else {
            panic!("aborting without a merge must fail")
        };
        assert_eq!(kind, WK::NoMergeInProgress);
    }

    #[test]
    fn workspace_bridge_initializes_a_repository_at_the_workspace_root() {
        let temporary = tempfile::tempdir().expect("temporary parent");
        let root = temporary.path().join("Payments API");
        let bridge = WorkspaceBridge::open_or_create(
            root.to_string_lossy().into_owned(),
            "Payments API".to_owned(),
        )
        .expect("workspace bridge");
        let Err(GitBridgeError::OperationFailed { kind, .. }) = bridge.inner.git_status() else {
            panic!("a new workspace folder is not a repository");
        };
        assert_eq!(kind, WK::NotRepository);

        let status = bridge.inner.git_initialize().expect("initialize Git");
        assert!(!status.merging);
        assert!(root.join(".git").exists());
        assert!(bridge.inner.git_status().is_ok());
    }

    #[test]
    fn rename_workspace_persists_a_trimmed_name_and_rejects_blank_names() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        let path = temporary.path().to_string_lossy().into_owned();
        let bridge = WorkspaceBridge::open_or_create(path.clone(), "Checkout".into())
            .expect("workspace bridge");

        let delta = bridge
            .inner
            .apply_session_command(C::RenameWorkspace {
                name: "  Checkout Team  ".into(),
            })
            .expect("rename workspace");
        assert_eq!(
            delta.kind,
            wirebolt_app::session::persistence::SessionWorkspaceNodeKind::Workspace
        );
        assert!(
            bridge
                .inner
                .apply_session_command(C::RenameWorkspace { name: "   ".into() })
                .is_err()
        );

        let reopened = WorkspaceBridge::open_or_create(path, "Ignored".into()).expect("reopen");
        let snapshot = reopened.inner.domain_snapshot().expect("snapshot");
        assert_eq!(snapshot.name, "Checkout Team");
    }

    /// Each test owns its cache, so resets in one test cannot race lookups
    /// in another the way a shared static would.
    fn engine_for(cache: &HttpEngineCache, proxy: &ResolvedProxy) -> Arc<HttpEngine> {
        cache
            .engine(proxy.mode(), || {
                HttpEngine::with_proxy(
                    &HttpEngineConfig::default(),
                    proxy,
                    &wirebolt_core::NoSecrets,
                )
            })
            .expect("HTTP engine")
    }

    fn manual_proxy_mode() -> ProxyMode {
        let route = wirebolt_core::ProxyRoute::new(
            wirebolt_core::ProxyDestination::All,
            wirebolt_core::ProxyEndpoint::new("http://proxy.internal:8080")
                .expect("proxy endpoint"),
        );
        ProxyMode::Manual(wirebolt_core::ManualProxy::new(vec![route]).expect("manual proxy"))
    }

    #[test]
    fn shared_direct_engine_reuses_the_same_client_pool() {
        let cache = HttpEngineCache::default();
        let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);

        let first = engine_for(&cache, &proxy);
        let second = engine_for(&cache, &proxy);

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn shared_manual_engine_is_keyed_by_mode_and_reset_on_demand() {
        let cache = HttpEngineCache::default();
        let manual = manual_proxy_mode();
        let from_workspace = ProxyPolicy::with_workspace(manual.clone()).resolve(None);
        let from_request = ProxyPolicy::default().resolve(Some(&manual));

        let first = engine_for(&cache, &from_workspace);
        let second = engine_for(&cache, &from_request);
        assert!(Arc::ptr_eq(&first, &second));

        cache.reset();
        let replacement = engine_for(&cache, &from_workspace);
        assert!(!Arc::ptr_eq(&first, &replacement));
    }

    #[test]
    fn reset_retires_every_slot_at_once() {
        let cache = HttpEngineCache::default();
        let direct = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);
        let system = ProxyPolicy::with_workspace(ProxyMode::System).resolve(None);
        let manual = ProxyPolicy::with_workspace(manual_proxy_mode()).resolve(None);
        let before = [
            engine_for(&cache, &direct),
            engine_for(&cache, &system),
            engine_for(&cache, &manual),
        ];

        cache.reset();

        let after = [
            engine_for(&cache, &direct),
            engine_for(&cache, &system),
            engine_for(&cache, &manual),
        ];
        for (old, new) in before.iter().zip(&after) {
            assert!(!Arc::ptr_eq(old, new));
        }
        for (old, new) in after.iter().zip([
            engine_for(&cache, &direct),
            engine_for(&cache, &system),
            engine_for(&cache, &manual),
        ]) {
            assert!(Arc::ptr_eq(old, &new));
        }
    }

    #[test]
    fn an_engine_built_while_a_reset_ran_is_used_once_but_never_pooled() {
        let cache = HttpEngineCache::default();
        let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);

        let raced = cache
            .engine(proxy.mode(), || {
                // Simulates a network change landing while the client is
                // still being built with the pre-change proxy settings.
                cache.reset();
                HttpEngine::with_proxy(
                    &HttpEngineConfig::default(),
                    &proxy,
                    &wirebolt_core::NoSecrets,
                )
            })
            .expect("engine for this run");

        let pooled = engine_for(&cache, &proxy);
        assert!(!Arc::ptr_eq(&raced, &pooled));
        assert!(Arc::ptr_eq(&pooled, &engine_for(&cache, &proxy)));
    }

    #[test]
    fn concurrent_lookups_and_resets_never_hand_out_a_retired_engine() {
        let cache = Arc::new(HttpEngineCache::default());
        let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let proxy = proxy.clone();
                thread::spawn(move || {
                    for _ in 0..50 {
                        drop(engine_for(&cache, &proxy));
                    }
                })
            })
            .collect();
        for _ in 0..20 {
            cache.reset();
            thread::sleep(Duration::from_millis(1));
        }
        for worker in workers {
            worker.join().expect("worker thread");
        }

        let retired = engine_for(&cache, &proxy);
        cache.reset();
        let fresh = engine_for(&cache, &proxy);
        assert!(!Arc::ptr_eq(&retired, &fresh));
        assert!(Arc::ptr_eq(&fresh, &engine_for(&cache, &proxy)));
    }

    #[test]
    fn zero_timeouts_disable_the_deadline() {
        assert_eq!(timeout_from_millis(0), None);
        assert_eq!(timeout_from_millis(250), Some(Duration::from_millis(250)));
    }

    fn run_to_completion(input: &str) -> (Vec<String>, Arc<TestTransportObserver>) {
        let observer = Arc::new(TestTransportObserver::default());
        let input: RunInput = serde_json::from_str(input).unwrap();
        SHARED_RUNTIME
            .as_ref()
            .unwrap()
            .block_on(wirebolt_app::session::transport::execute(
                input,
                RunCancellation::new(),
                observer.clone(),
                wirebolt_core::NoSecrets,
            ));
        let events = lock(&observer.events).clone();
        assert!(
            events
                .last()
                .is_some_and(|event| matches!(event.as_str(), "complete" | "error"))
        );
        (events, observer)
    }
    #[derive(Debug, Default)]
    struct TestTransportObserver {
        events: Mutex<Vec<String>>,
        failure: Mutex<Option<SessionFailure>>,
        prepared: Mutex<Option<SessionPrepared>>,
    }
    impl TransportObserver for TestTransportObserver {
        fn prepared(&self, snapshot: &PreparedRunSnapshotDocument) {
            assert_eq!(snapshot.method, "GET");
            *lock(&self.prepared) = Some(SessionPrepared::from(snapshot));
            lock(&self.events).push("prepared".into());
        }
        fn head(&self, head: &RunHead, _: &[FailureIssue]) {
            assert_eq!(head.status, 200);
            lock(&self.events).push("head".into());
        }
        fn chunk(&self, bytes: &[u8]) -> bool {
            lock(&self.events).push(format!("chunk:{}", String::from_utf8_lossy(bytes)));
            true
        }
        fn cookies(&self, _: &str, _: Vec<(String, String)>) {}
        fn complete(&self, received: u64, decoded: u64, _: Duration) {
            assert_eq!(received, 4);
            assert_eq!(decoded, 4);
            lock(&self.events).push("complete".into());
        }
        fn failure(&self, failure: &RunFailureDocument) {
            *lock(&self.failure) = Some(SessionFailure::from(failure));
            lock(&self.events).push("error".into());
        }
    }
}
