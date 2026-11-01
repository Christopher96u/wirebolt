//! Typed run application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{
    AppResponseBody, Arc, DocumentRun, DomainRequestDraft, DomainTransport, Instant, Mutex, Path,
    PathBuf, ResolvedDomainProxy, ResponseHeaderValue, SessionCompletion, SessionDocument,
    SessionFailure, SessionHead, SessionKind, SessionPrepared, SessionPreparedBody,
    SessionPreparedHeader, SessionRunEvent, SessionRunSnapshot, StoredCookie,
};
use crate::lock;
use std::sync::atomic::{AtomicU64, Ordering};
use wirebolt_app::response::FailureIssue;

/// Counts full document/draft records at the bridge, excluding bounded metadata
/// and handle traffic. Counters are per document so tests can measure a command
/// without depending on other windows or parallel tests.
#[derive(Debug, uniffi::Record)]
pub struct AppDocumentBridgeTraffic {
    pub document_reads: u64,
    pub draft_writes: u64,
}
#[uniffi::remote(Record)]
pub struct SessionHead {
    status: u16,
    version: String,
    headers: Vec<ResponseHeaderValue>,
    time_to_headers_ns: u64,
}

#[uniffi::remote(Record)]
pub struct SessionCompletion {
    bytes_received: u64,
    total_time_ns: u64,
}

#[uniffi::remote(Record)]
pub struct SessionFailure {
    kind: wirebolt_app::response::ResponseFailureKind,
    issues: Vec<FailureIssue>,
}

#[uniffi::remote(Record)]
pub struct SessionPreparedHeader {
    name: String,
    value: String,
    redacted: bool,
}

#[uniffi::remote(Record)]
pub struct SessionPreparedBody {
    byte_count: u64,
    content_type: Option<String>,
    text_preview: Option<String>,
    redacted: bool,
}

#[uniffi::remote(Record)]
pub struct SessionPrepared {
    proxy: Option<ResolvedDomainProxy>,
    method: String,
    url: String,
    headers: Vec<SessionPreparedHeader>,
    body: SessionPreparedBody,
    transport: DomainTransport,
}

#[uniffi::remote(Enum)]
pub enum SessionRunEvent {
    Prepared { snapshot: SessionPrepared },
    Head { head: SessionHead },
    Complete { completion: SessionCompletion },
}

#[uniffi::remote(Record)]
pub struct SessionRunSnapshot {
    head: Option<SessionHead>,
    prepared: Option<SessionPrepared>,
    text: String,
    data: Vec<u8>,
    bytes: u64,
    truncated: bool,
    completion: Option<SessionCompletion>,
    failure: Option<SessionFailure>,
    active_run_id: Option<String>,
    started_at: Option<f64>,
    awaiting_head: bool,
    cookies: Vec<StoredCookie>,
    body_path: Option<String>,
}

#[derive(Debug, uniffi::Object)]
pub struct AppDocumentSession {
    pub(super) document: Arc<Mutex<SessionDocument>>,
    pub(super) run: Arc<Mutex<DocumentRun>>,
    document_reads: AtomicU64,
    draft_writes: AtomicU64,
}

#[uniffi::export]
impl AppDocumentSession {
    #[uniffi::constructor]
    pub fn new(
        id: String,
        kind: SessionKind,
        collection_id: Option<String>,
        request_id: Option<String>,
        draft: DomainRequestDraft,
        saved_draft: Option<DomainRequestDraft>,
        note: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            document: Arc::new(Mutex::new(SessionDocument::new(
                id,
                kind,
                collection_id,
                request_id,
                draft,
                saved_draft,
                &note,
            ))),
            run: Arc::new(Mutex::new(DocumentRun::default())),
            document_reads: AtomicU64::new(0),
            draft_writes: AtomicU64::new(0),
        })
    }
    pub fn document(&self) -> SessionDocument {
        self.document_reads.fetch_add(1, Ordering::Relaxed);
        lock(&self.document).clone()
    }
    pub fn bridge_traffic(&self) -> AppDocumentBridgeTraffic {
        AppDocumentBridgeTraffic {
            document_reads: self.document_reads.load(Ordering::Relaxed),
            draft_writes: self.draft_writes.load(Ordering::Relaxed),
        }
    }
    pub fn edit(&self, draft: DomainRequestDraft) {
        self.draft_writes.fetch_add(1, Ordering::Relaxed);
        lock(&self.document).edit(draft);
    }
    pub fn relocate(&self, collection_id: String) {
        lock(&self.document).collection_id = Some(collection_id);
    }
    pub fn rename(&self, name: String) {
        lock(&self.document).rename(&name);
    }
    pub fn save(&self, draft: DomainRequestDraft, replace: bool) {
        self.draft_writes.fetch_add(1, Ordering::Relaxed);
        let mut document = lock(&self.document);
        if replace {
            document.mark_saved(draft);
        } else {
            document.saved_draft = Some(draft);
        }
    }
    pub fn snapshot(&self) -> SessionRunSnapshot {
        lock(&self.run).snapshot()
    }
    pub fn body(&self) -> Option<Arc<AppResponseBody>> {
        lock(&self.run)
            .body()
            .map(|inner| Arc::new(AppResponseBody { inner }))
    }
    pub fn begin(&self, run_id: String, directory: String) {
        lock(&self.document).touched = true;
        lock(&self.run).begin(&run_id, Path::new(&directory));
    }
    pub fn consume(&self, run_id: String, event: SessionRunEvent) {
        lock(&self.run).consume(&run_id, event);
    }
    pub fn append_batch(&self, run_id: String, chunks: Vec<Vec<u8>>) -> bool {
        lock(&self.run)
            .append_batch(&run_id, &chunks, Instant::now())
            .is_some()
    }
    pub fn publish(&self, run_id: String) {
        let mut run = lock(&self.run);
        if run.accepts(&run_id) {
            run.publish();
        }
    }
    pub fn finish(&self, run_id: String, failure: Option<SessionFailure>) {
        lock(&self.run).finish(&run_id, failure);
    }
    pub fn cancel(&self, run_id: String) {
        lock(&self.run).cancel(&run_id);
    }
    pub fn reset(&self) {
        lock(&self.run).reset();
    }
    pub fn copy_completed(&self, source: Arc<Self>) {
        if std::ptr::eq(self, source.as_ref()) {
            return;
        }
        let saved = lock(&source.document).saved_draft.clone();
        let run = lock(&source.run).clone();
        lock(&self.document).saved_draft = saved;
        lock(&self.run).copy_completed(&run);
    }
    pub fn cookies(&self, cookies: Vec<StoredCookie>, append: bool) {
        if append {
            lock(&self.run).append_cookies(cookies);
        } else {
            lock(&self.run).set_cookies(cookies);
        }
    }
    pub fn restore(&self, snapshot: SessionRunSnapshot, path: String) {
        lock(&self.run).restore(snapshot, PathBuf::from(path));
    }
}

#[uniffi::export]
pub fn session_publication_delay_ms() -> u64 {
    u64::try_from(wirebolt_app::session::run::PUBLICATION_INTERVAL.as_millis()).unwrap_or(16)
}

#[uniffi::export]
impl AppDocumentSession {
    pub fn was_touched(&self) -> bool {
        lock(&self.document).touched
    }
}
