use super::{RuntimeContext, RuntimeState};
use crate::lock;
use crate::response::FailureIssue;
use crate::{
    domain::{DomainRunInput, DomainValueSource},
    response::ResponseHeaderValue,
    session::{
        run::{
            DocumentRun, PUBLICATION_INTERVAL, SessionCompletion, SessionFailure, SessionHead,
            SessionPrepared, SessionRunEvent, SessionRunSnapshot, SharedBody,
        },
        tabs::SessionDocument,
        transport::{PreparedRunSnapshotDocument, RunFailureDocument, TransportObserver},
    },
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wirebolt_core::RunCancellation;

#[async_trait]
pub trait RuntimeTransport: Send + Sync {
    fn cancel(&self, _id: &str) {}
    async fn resolve(
        &self,
        values: Vec<DomainValueSource>,
        variables: HashMap<String, DomainValueSource>,
    ) -> Result<Vec<String>, SessionFailure>;
    async fn execute(
        &self,
        input: DomainRunInput,
        id: String,
        cancellation: RunCancellation,
        observer: Arc<RunObserver>,
    );
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInput {
    pub draft: crate::domain::DomainRequestDraft,
    pub context: RuntimeContext,
}
#[async_trait]
pub trait RuntimeObserver: Send + Sync {
    fn changed(&self);
    async fn prepare(&self) -> Result<Option<RuntimeInput>, ()> {
        Ok(None)
    }
}
struct ActiveRun {
    id: String,
    cancellation: RunCancellation,
    transport: Arc<dyn RuntimeTransport>,
    observer: Option<Arc<RunObserver>>,
}
impl std::fmt::Debug for ActiveRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveRun")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
pub struct RuntimeCredentials {
    pub secrets: Arc<crate::session::secrets::SessionSecrets>,
    pub platform: Option<Arc<dyn crate::session::secrets::SessionSecretPlatform>>,
}
impl std::fmt::Debug for RuntimeCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeCredentials").finish_non_exhaustive()
    }
}
impl RuntimeCredentials {
    /// # Errors
    /// Dirty material requires a credential store. Clean previews need none.
    pub async fn flush(&self) -> Result<(), crate::session::secrets::SecretAccessError> {
        if let Some(platform) = &self.platform {
            self.secrets.flush(platform.as_ref()).await
        } else if self.secrets.dirty().is_empty() {
            Ok(())
        } else {
            Err(crate::session::secrets::SecretAccessError)
        }
    }
}
pub struct RuntimeSend {
    pub state: Arc<RuntimeState>,
    pub document: Arc<Mutex<SessionDocument>>,
    pub run: Arc<Mutex<DocumentRun>>,
    pub context: RuntimeContext,
    pub id: String,
    pub directory: PathBuf,
    pub transport: Arc<dyn RuntimeTransport>,
    pub observer: Arc<dyn RuntimeObserver>,
    pub credentials: RuntimeCredentials,
}
impl std::fmt::Debug for RuntimeSend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeSend")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Default)]
pub struct RuntimeRuns {
    active: Mutex<HashMap<String, ActiveRun>>,
}
impl RuntimeRuns {
    pub fn cancel(&self, document: &str, run: &Mutex<DocumentRun>) {
        if let Some(active) = lock(&self.active).remove(document) {
            active.cancellation.cancel();
            if let Some(observer) = active.observer {
                observer.cancel();
            } else {
                lock(run).cancel(&active.id);
            }
            active.transport.cancel(&active.id);
        } else {
            let mut run = lock(run);
            if let Some(id) = run.snapshot().active_run_id {
                run.cancel(&id);
            }
        }
    }
    pub async fn send(&self, command: RuntimeSend) -> Option<SessionFailure> {
        let RuntimeSend {
            state,
            document,
            run,
            mut context,
            id,
            directory,
            transport,
            observer,
            credentials,
        } = command;
        let (document_id, request_id) = {
            let document = lock(&document);
            if lock(&run).snapshot().active_run_id.is_some() {
                return None;
            }
            (document.id.clone(), document.request_id.clone())
        };
        let cancellation = RunCancellation::new();
        // Installation and begin share the active-map lock with cancellation.
        {
            let mut active = lock(&self.active);
            if active.contains_key(&document_id) {
                return None;
            }
            active.insert(
                document_id.clone(),
                ActiveRun {
                    id: id.clone(),
                    cancellation: cancellation.clone(),
                    transport: transport.clone(),
                    observer: None,
                },
            );
        }
        if credentials.flush().await.is_err() {
            self.remove(&document_id, &id);
            return Some(SessionFailure::new(
                crate::response::ResponseFailureKind::Keychain,
            ));
        }
        let Ok(latest) = observer.prepare().await else {
            self.remove(&document_id, &id);
            return None;
        };
        let draft = {
            let active = lock(&self.active);
            if active.get(&document_id).is_none_or(|run| run.id != id) {
                return None;
            }
            let mut document = lock(&document);
            if let Some(latest) = latest {
                document.edit(latest.draft);
                context = latest.context;
            }
            document.touched = true;
            lock(&run).begin(&id, &directory);
            context.draft(document.draft.clone())
        };
        observer.changed();
        let run_observer = Arc::new(RunObserver::new(
            id.clone(),
            run.clone(),
            state.clone(),
            observer,
        ));
        if let Some(active) = lock(&self.active)
            .get_mut(&document_id)
            .filter(|run| run.id == id)
        {
            active.observer = Some(run_observer.clone());
        }
        let draft = match prepare_cookies(draft, &context, transport.as_ref(), &run_observer).await
        {
            Ok(draft) => draft,
            Err(failure) => {
                run_observer.finish(failure);
                self.remove(&document_id, &id);
                return None;
            }
        };
        let done = tokio_util::sync::CancellationToken::new();
        let _publication_guard = done.clone().drop_guard();
        let publisher = spawn_publisher(run_observer.clone(), done.clone());
        transport
            .execute(
                context.input(draft),
                id.clone(),
                cancellation,
                run_observer.clone(),
            )
            .await;
        done.cancel();
        let _ = publisher.await;
        run_observer.finish(None);
        record_terminal(state, request_id, id.clone(), &run_observer).await;
        self.remove(&document_id, &id);
        None
    }
    fn remove(&self, document: &str, id: &str) {
        let mut active = lock(&self.active);
        if active.get(document).is_some_and(|run| run.id == id) {
            active.remove(document);
        }
    }
}

pub struct RunObserver {
    id: String,
    run: Arc<Mutex<DocumentRun>>,
    state: Arc<RuntimeState>,
    cookies: super::RuntimeCookies,
    observer: Arc<dyn RuntimeObserver>,
    dirty: AtomicBool,
    terminal: Mutex<Option<(SessionRunSnapshot, SharedBody)>>,
}
impl std::fmt::Debug for RunObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunObserver")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
impl RunObserver {
    fn new(
        id: String,
        run: Arc<Mutex<DocumentRun>>,
        state: Arc<RuntimeState>,
        observer: Arc<dyn RuntimeObserver>,
    ) -> Self {
        // Retain the starting workspace's jar for the complete run.
        let cookies = state.cookie_jar();
        Self {
            id,
            run,
            state,
            cookies,
            observer,
            dirty: AtomicBool::new(false),
            terminal: Mutex::new(None),
        }
    }

    pub fn event(&self, event: SessionRunEvent) {
        let mut run = lock(&self.run);
        if !run.accepts(&self.id) {
            return;
        }
        let completed = matches!(event, SessionRunEvent::Complete { .. });
        run.consume(&self.id, event);
        if completed {
            self.capture(&run);
        }
        drop(run);
        self.observer.changed();
    }
    pub fn bytes(&self, bytes: &[u8]) -> bool {
        let mut run = lock(&self.run);
        if !run.accepts(&self.id) {
            return false;
        }
        run.append_batch(&self.id, &[bytes.to_vec()], Instant::now());
        self.dirty.store(true, Ordering::Release);
        true
    }
    pub fn store_cookies(&self, url: &str, headers: &[ResponseHeaderValue]) {
        if !lock(&self.run).accepts(&self.id) {
            return;
        }
        let received = lock(&self.cookies).store(headers, url, crate::response::unix_time());
        self.state.received_cookies(&received);
        let mut run = lock(&self.run);
        if run.accepts(&self.id) {
            run.append_cookies(received);
        }
        drop(run);
        self.observer.changed();
    }
    fn publish(&self) {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return;
        }
        let mut run = lock(&self.run);
        if !run.accepts(&self.id) {
            return;
        }
        run.publish();
        drop(run);
        self.observer.changed();
    }
    fn cancel(&self) {
        let mut run = lock(&self.run);
        if !run.accepts(&self.id) {
            return;
        }
        run.cancel(&self.id);
        self.capture(&run);
    }
    pub fn abandon(&self) {
        self.finish(Some(SessionFailure::new(
            crate::response::ResponseFailureKind::Cancelled,
        )));
        lock(&self.terminal).take();
    }
    pub fn finish(&self, failure: Option<SessionFailure>) {
        let mut run = lock(&self.run);
        if !run.accepts(&self.id) {
            return;
        }
        run.finish(&self.id, failure);
        self.capture(&run);
        drop(run);
        self.observer.changed();
    }
    fn capture(&self, run: &DocumentRun) {
        if let Some(body) = run.body() {
            *lock(&self.terminal) = Some((run.snapshot(), body));
        }
    }
}
impl TransportObserver for RunObserver {
    fn prepared(&self, snapshot: &PreparedRunSnapshotDocument) {
        self.event(SessionRunEvent::Prepared {
            snapshot: SessionPrepared::from(snapshot),
        });
    }
    fn head(&self, head: &wirebolt_core::RunHead, _: &[FailureIssue]) {
        self.event(SessionRunEvent::Head {
            head: SessionHead::from(head),
        });
    }
    fn chunk(&self, bytes: &[u8]) -> bool {
        self.bytes(bytes)
    }
    fn cookies(&self, url: &str, headers: Vec<(String, String)>) {
        self.store_cookies(
            url,
            &headers
                .into_iter()
                .map(|(name, value)| ResponseHeaderValue { name, value })
                .collect::<Vec<_>>(),
        );
    }
    fn complete(&self, bytes_received: u64, _: u64, total_time: Duration) {
        self.event(SessionRunEvent::Complete {
            completion: SessionCompletion::from_transport(bytes_received, total_time),
        });
    }
    fn failure(&self, failure: &RunFailureDocument) {
        self.finish(Some(SessionFailure::from(failure)));
    }
}

fn spawn_publisher(
    observer: Arc<RunObserver>,
    done: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(PUBLICATION_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            tokio::select! { ()=done.cancelled()=>break, _=interval.tick()=>observer.publish() }
        }
    })
}

async fn prepare_cookies(
    mut draft: crate::domain::DomainRequestDraft,
    context: &RuntimeContext,
    transport: &dyn RuntimeTransport,
    observer: &RunObserver,
) -> Result<crate::domain::DomainRequestDraft, Option<SessionFailure>> {
    let values = transport
        .resolve(
            vec![DomainValueSource::literal(&draft.url)],
            context.variables.clone(),
        )
        .await
        .map_err(Some)?;
    if !lock(&observer.run).accepts(&observer.id) {
        return Err(None);
    }
    if !draft
        .headers
        .iter()
        .any(|h| h.enabled && h.name.eq_ignore_ascii_case("cookie"))
        && let Some(url) = values.first()
        && let Some(header) =
            lock(&observer.cookies).header(url.trim(), crate::response::unix_time())
    {
        draft.headers.push(crate::domain::DomainField {
            id: crate::domain::identity::unique_id(),
            name: "Cookie".into(),
            value: DomainValueSource::literal(header),
            enabled: true,
            sensitive: true,
        });
    }
    Ok(draft)
}

async fn record_terminal(
    state: Arc<RuntimeState>,
    request_id: String,
    id: String,
    observer: &RunObserver,
) {
    let terminal = lock(&observer.terminal).take();
    if let Some((snapshot, body)) = terminal {
        let _ = tokio::task::spawn_blocking(move || {
            state.record(&request_id, &id, snapshot, &body);
        })
        .await;
    }
}
