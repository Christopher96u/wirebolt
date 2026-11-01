//! Document run state. Transport ingress and presentation publication are separate so
//! a burst never requires a foreign call for each chunk.
use crate::lock;
use crate::{
    domain::{DomainTransport, proxy::ResolvedDomainProxy},
    response::{
        BodyPreview, BodyProgress, BodyStore, FailureIssue, ResponseHeaderValue, StoredCookie,
        reference_time,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub type SharedBody = Arc<Mutex<BodyStore>>;
pub const PUBLICATION_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionHead {
    pub status: u16,
    pub version: String,
    pub headers: Vec<ResponseHeaderValue>,
    pub time_to_headers_ns: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCompletion {
    pub bytes_received: u64,
    pub total_time_ns: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFailure {
    pub kind: crate::response::ResponseFailureKind,
    pub issues: Vec<FailureIssue>,
}
impl SessionFailure {
    #[must_use]
    pub fn new(kind: crate::response::ResponseFailureKind) -> Self {
        Self {
            kind,
            issues: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPreparedHeader {
    pub name: String,
    pub value: String,
    pub redacted: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPreparedBody {
    pub byte_count: u64,
    pub content_type: Option<String>,
    pub text_preview: Option<String>,
    pub redacted: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPrepared {
    pub proxy: Option<ResolvedDomainProxy>,
    pub method: String,
    pub url: String,
    pub headers: Vec<SessionPreparedHeader>,
    pub body: SessionPreparedBody,
    pub transport: DomainTransport,
}
#[derive(Clone, Debug, PartialEq)]
pub enum SessionRunEvent {
    Prepared { snapshot: SessionPrepared },
    Head { head: SessionHead },
    Complete { completion: SessionCompletion },
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionRunSnapshot {
    pub head: Option<SessionHead>,
    pub prepared: Option<SessionPrepared>,
    pub text: String,
    pub data: Vec<u8>,
    pub bytes: u64,
    pub truncated: bool,
    pub completion: Option<SessionCompletion>,
    pub failure: Option<SessionFailure>,
    pub active_run_id: Option<String>,
    pub started_at: Option<f64>,
    pub awaiting_head: bool,
    pub cookies: Vec<StoredCookie>,
    pub body_path: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct DocumentRun {
    snapshot: SessionRunSnapshot,
    body: Option<SharedBody>,
    pending_body: Option<SharedBody>,
    pending_prepared: Option<SessionPrepared>,
    pending_cookies: Vec<StoredCookie>,
    publication: Option<Instant>,
    fallback: Option<BodyPreview>,
}
impl DocumentRun {
    #[must_use]
    pub fn snapshot(&self) -> SessionRunSnapshot {
        self.snapshot.clone()
    }
    #[must_use]
    pub fn body(&self) -> Option<SharedBody> {
        self.body.clone()
    }
    #[must_use]
    pub fn accepts(&self, id: &str) -> bool {
        self.snapshot.active_run_id.as_deref() == Some(id)
    }
    pub fn begin(&mut self, id: &str, directory: &Path) {
        self.publication = None;
        self.snapshot.failure = None;
        self.pending_body = BodyStore::new(id, directory)
            .ok()
            .map(|body| Arc::new(Mutex::new(body)));
        self.pending_prepared = None;
        self.pending_cookies.clear();
        self.snapshot.awaiting_head = true;
        self.snapshot.started_at = Some(reference_time());
        self.snapshot.active_run_id = Some(id.into());
    }
    fn present_pending(&mut self) {
        if !self.snapshot.awaiting_head {
            return;
        }
        let active = self.snapshot.active_run_id.take();
        let started = self.snapshot.started_at;
        self.reset();
        self.snapshot.active_run_id = active;
        self.snapshot.started_at = started;
        self.body = self.pending_body.take();
        self.fallback = self.body.is_none().then(BodyPreview::default);
        self.snapshot.body_path = self
            .body
            .as_ref()
            .map(|b| lock(b).path().to_string_lossy().into_owned());
        self.snapshot.prepared = self.pending_prepared.take();
        self.snapshot.cookies = std::mem::take(&mut self.pending_cookies);
        self.snapshot.awaiting_head = false;
    }
    pub fn consume(&mut self, id: &str, event: SessionRunEvent) {
        if !self.accepts(id) {
            return;
        }
        match event {
            SessionRunEvent::Prepared { snapshot } => {
                if self.snapshot.awaiting_head {
                    self.pending_prepared = Some(snapshot);
                } else {
                    self.snapshot.prepared = Some(snapshot);
                }
            }
            SessionRunEvent::Head { head } => {
                self.present_pending();
                self.snapshot.head = Some(head);
            }
            SessionRunEvent::Complete { completion } => {
                self.finish(id, None);
                if self.snapshot.failure.is_none() {
                    self.snapshot.bytes = completion.bytes_received;
                    self.snapshot.completion = Some(completion);
                }
            }
        }
    }
    /// Returns the delay to the next publication, if any. The caller schedules at
    /// most one wakeup; the due time belongs to this run and is cleared at termination.
    pub fn append_batch(&mut self, id: &str, chunks: &[Vec<u8>], now: Instant) -> Option<Duration> {
        if !self.accepts(id) {
            return None;
        }
        self.present_pending();
        if let Some(body) = &self.body {
            let failed = {
                let mut body = lock(body);
                chunks
                    .iter()
                    .try_for_each(|chunk| body.append(chunk))
                    .is_err()
            };
            if failed {
                self.finish(
                    id,
                    Some(SessionFailure::new(
                        crate::response::ResponseFailureKind::Storage,
                    )),
                );
                return None;
            }
        } else if let Some(preview) = &mut self.fallback {
            for chunk in chunks {
                preview.append(chunk);
            }
        }
        let due = self.publication.get_or_insert(now + PUBLICATION_INTERVAL);
        Some(due.saturating_duration_since(now))
    }
    pub fn publish_due(&mut self, id: &str, now: Instant) {
        if self.accepts(id) && self.publication.is_some_and(|due| now >= due) {
            self.publish();
        }
    }
    pub fn publish(&mut self) {
        self.publication = None;
        let progress = self.body.as_ref().map_or_else(
            || self.fallback.as_mut().map(BodyPreview::progress),
            |body| Some(lock(body).progress()),
        );
        if let Some(BodyProgress {
            bytes,
            data,
            text,
            truncated,
        }) = progress
        {
            self.snapshot.bytes = bytes;
            self.snapshot.data = data;
            self.snapshot.text = text;
            self.snapshot.truncated = truncated;
        }
    }
    pub fn finish(&mut self, id: &str, failure: Option<SessionFailure>) {
        if !self.accepts(id) {
            return;
        }
        self.present_pending();
        if let Some(body) = &self.body {
            let _ = lock(body).finish();
        }
        self.publish();
        self.snapshot.failure = failure.or_else(|| {
            self.fallback
                .as_ref()
                .map(|_| SessionFailure::new(crate::response::ResponseFailureKind::Storage))
        });
        self.snapshot.active_run_id = None;
        self.snapshot.started_at = None;
    }
    pub fn cancel(&mut self, id: &str) {
        self.finish(
            id,
            Some(SessionFailure::new(
                crate::response::ResponseFailureKind::Cancelled,
            )),
        );
    }
    pub fn reset(&mut self) {
        let active = self.snapshot.active_run_id.take();
        let started = self.snapshot.started_at;
        let awaiting = self.snapshot.awaiting_head;
        self.snapshot = SessionRunSnapshot {
            active_run_id: active,
            started_at: started,
            awaiting_head: awaiting,
            ..SessionRunSnapshot::default()
        };
        self.body = None;
        self.fallback = None;
    }
    pub fn append_cookies(&mut self, cookies: Vec<StoredCookie>) {
        if self.snapshot.awaiting_head {
            self.pending_cookies.extend(cookies);
        } else {
            self.snapshot.cookies.extend(cookies);
        }
    }
    pub fn set_cookies(&mut self, cookies: Vec<StoredCookie>) {
        self.snapshot.cookies = cookies;
    }
    pub fn copy_completed(&mut self, source: &Self) {
        if source.snapshot.active_run_id.is_some() {
            return;
        }
        self.snapshot = source.snapshot.clone();
        self.body.clone_from(&source.body);
        self.fallback.clone_from(&source.fallback);
    }
    pub fn restore(&mut self, mut snapshot: SessionRunSnapshot, path: PathBuf) {
        snapshot.active_run_id = None;
        snapshot.started_at = None;
        snapshot.awaiting_head = false;
        self.body = BodyStore::existing(path)
            .ok()
            .map(|body| Arc::new(Mutex::new(body)));
        snapshot.body_path = self
            .body
            .as_ref()
            .map(|b| lock(b).path().to_string_lossy().into_owned());
        self.snapshot = snapshot;
        self.pending_body = None;
        self.fallback = None;
        self.pending_prepared = None;
        self.pending_cookies.clear();
        self.publication = None;
    }
}

impl From<&super::transport::PreparedRunSnapshotDocument> for SessionPrepared {
    fn from(snapshot: &super::transport::PreparedRunSnapshotDocument) -> Self {
        Self {
            proxy: Some(snapshot.proxy.clone()),
            method: snapshot.method.clone(),
            url: snapshot.url.clone(),
            headers: snapshot
                .headers
                .iter()
                .map(|h| SessionPreparedHeader {
                    name: h.name.clone(),
                    value: h.value.clone(),
                    redacted: h.redacted,
                })
                .collect(),
            body: SessionPreparedBody {
                byte_count: snapshot.body.byte_count,
                content_type: snapshot.body.content_type.clone(),
                text_preview: snapshot.body.text_preview.clone(),
                redacted: snapshot.body.redacted,
            },
            transport: super::conversions::transport_from_core(snapshot.transport.clone()),
        }
    }
}
impl From<&wirebolt_core::RunHead> for SessionHead {
    fn from(head: &wirebolt_core::RunHead) -> Self {
        Self {
            status: head.status,
            version: super::transport::http_version(head.version).into(),
            headers: head
                .headers
                .iter()
                .map(|(name, value)| ResponseHeaderValue {
                    name: name.to_string(),
                    value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
                })
                .collect(),
            time_to_headers_ns: super::transport::duration_ns(head.time_to_headers),
        }
    }
}
impl From<&super::transport::RunFailureDocument> for SessionFailure {
    fn from(failure: &super::transport::RunFailureDocument) -> Self {
        Self {
            kind: failure.kind.clone(),
            issues: failure.issues.clone(),
        }
    }
}
impl SessionCompletion {
    #[must_use]
    pub fn from_transport(bytes_received: u64, total_time: Duration) -> Self {
        Self {
            bytes_received,
            total_time_ns: super::transport::duration_ns(total_time),
        }
    }
}
