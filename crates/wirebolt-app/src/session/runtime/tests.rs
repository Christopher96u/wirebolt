//! Runtime ports of the native model, redirect-cookie, environment, secret and proxy cases.
mod history;
mod secrets;
mod sends;
mod settings;
use super::*;
use crate::{
    domain::*,
    response::*,
    session::{run::*, secrets::*, tabs::*},
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

fn context() -> RuntimeContext {
    RuntimeContext {
        transport: DomainTransport::default(),
        workspace_proxy: None,
        app_proxy: DomainProxy::System,
        variables: HashMap::new(),
    }
}
fn document(id: &str) -> Arc<Mutex<SessionDocument>> {
    Arc::new(Mutex::new(SessionDocument::new(
        id.into(),
        SessionKind::Http,
        Some("api".into()),
        None,
        DomainRequestDraft {
            id: id.into(),
            url: format!("https://example.com/{id}"),
            ..Default::default()
        },
        None,
        "",
    )))
}
fn state(root: &std::path::Path) -> Arc<RuntimeState> {
    Arc::new(RuntimeState::new(
        Arc::new(Mutex::new(RunHistory::new(root.join("history")))),
        Arc::new(Mutex::new(ResponseCookieJar::new(None))),
    ))
}
fn prepared(input: &DomainRunInput) -> SessionPrepared {
    SessionPrepared {
        proxy: None,
        method: input.method.clone(),
        url: input.url.clone(),
        headers: vec![],
        body: SessionPreparedBody {
            byte_count: 0,
            content_type: None,
            text_preview: None,
            redacted: false,
        },
        transport: DomainTransport::default(),
    }
}
#[derive(Debug, Default)]
struct Changes(AtomicUsize);
impl RuntimeObserver for Changes {
    fn changed(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
#[derive(Debug, Default)]
struct Platform {
    values: Mutex<HashMap<String, String>>,
    deleted: Mutex<Vec<String>>,
    fails: AtomicBool,
}
#[async_trait]
impl SessionSecretPlatform for Platform {
    async fn read(&self, name: String) -> Result<Option<String>, ()> {
        Ok(lock(&self.values).get(&name).cloned())
    }
    async fn save(&self, name: String, value: String) -> Result<(), ()> {
        if self.fails.load(Ordering::Relaxed) {
            return Err(());
        }
        lock(&self.values).insert(name, value);
        Ok(())
    }
    async fn remove(&self, name: String) -> Result<(), ()> {
        lock(&self.values).remove(&name);
        lock(&self.deleted).push(name);
        Ok(())
    }
}
impl wirebolt_core::SecretResolver for Platform {
    fn resolve(
        &self,
        name: &wirebolt_core::SecretName,
    ) -> Result<wirebolt_core::ResolvedSecret, wirebolt_core::SecretResolutionError> {
        lock(&self.values)
            .get(name.as_str())
            .cloned()
            .map(wirebolt_core::ResolvedSecret::new)
            .ok_or_else(|| {
                wirebolt_core::SecretResolutionError::new(
                    wirebolt_core::SecretResolutionErrorKind::Unavailable,
                )
            })
    }
}
#[derive(Debug, Default)]
struct Transport {
    inputs: Mutex<Vec<DomainRunInput>>,
    body: Vec<u8>,
    failure: Option<SessionFailure>,
    cookies: Vec<(String, Vec<ResponseHeaderValue>)>,
    platform: Arc<Platform>,
    hold: bool,
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
    cancelled: AtomicBool,
}
#[async_trait]
impl RuntimeTransport for Transport {
    fn cancel(&self, _id: &str) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.release.notify_one();
    }
    async fn resolve(
        &self,
        values: Vec<DomainValueSource>,
        variables: HashMap<String, DomainValueSource>,
    ) -> Result<Vec<String>, SessionFailure> {
        resolve_values(values, variables, self.platform.as_ref())
    }
    async fn execute(
        &self,
        input: DomainRunInput,
        _id: String,
        _cancellation: wirebolt_core::RunCancellation,
        observer: Arc<RunObserver>,
    ) {
        observer.event(SessionRunEvent::Prepared {
            snapshot: prepared(&input),
        });
        lock(&self.inputs).push(input);
        self.started.notify_one();
        if self.hold {
            self.release.notified().await;
        }
        if self.cancelled.load(Ordering::Relaxed) {
            observer.abandon();
            return;
        }
        for (url, headers) in &self.cookies {
            observer.store_cookies(url, headers);
        }
        if let Some(failure) = &self.failure {
            observer.finish(Some(failure.clone()));
            return;
        }
        observer.event(SessionRunEvent::Head {
            head: SessionHead {
                status: 200,
                version: "HTTP/2".into(),
                headers: vec![],
                time_to_headers_ns: 10,
            },
        });
        observer.bytes(&self.body);
        observer.event(SessionRunEvent::Complete {
            completion: SessionCompletion {
                bytes_received: self.body.len() as u64,
                total_time_ns: 20,
            },
        });
    }
}
fn command(
    state: Arc<RuntimeState>,
    document: Arc<Mutex<SessionDocument>>,
    run: Arc<Mutex<DocumentRun>>,
    transport: Arc<Transport>,
    directory: &std::path::Path,
) -> RuntimeSend {
    let platform = transport.platform.clone();
    RuntimeSend {
        state,
        document,
        run,
        context: context(),
        id: identity::unique_id(),
        directory: directory.into(),
        transport,
        observer: Arc::new(Changes::default()),
        credentials: RuntimeCredentials {
            secrets: Arc::new(SessionSecrets::default()),
            platform: Some(platform),
        },
    }
}
