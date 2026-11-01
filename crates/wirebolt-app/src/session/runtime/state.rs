use crate::{
    domain::{DomainEnvironment, DomainValueSource},
    response::{ResponseCookieJar, RunHistory, StoredCookie, cookie_storage_path, reference_time},
    session::{
        history::{self, SessionHistoryEntry},
        run::{DocumentRun, SessionRunSnapshot, lock},
    },
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub type RuntimeHistory = Arc<Mutex<RunHistory>>;
pub type RuntimeCookies = Arc<Mutex<ResponseCookieJar>>;
#[derive(Clone, Debug, PartialEq)]
pub struct RequestActivity {
    pub date: f64,
    pub status: Option<u16>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeSnapshot {
    pub history_entries: Vec<SessionHistoryEntry>,
    pub history_revision: u64,
    pub request_activity: HashMap<String, RequestActivity>,
    pub cookie_revision: u64,
}
#[derive(Debug)]
pub struct RuntimeState {
    pub history: RuntimeHistory,
    default_cookies: RuntimeCookies,
    cookies: Mutex<RuntimeCookies>,
    snapshot: Mutex<RuntimeSnapshot>,
    selected: Mutex<Option<String>>,
    generation: AtomicU64,
}
impl RuntimeState {
    #[must_use]
    pub fn new(history: RuntimeHistory, cookies: RuntimeCookies) -> Self {
        Self {
            history,
            default_cookies: cookies.clone(),
            cookies: Mutex::new(cookies),
            snapshot: Mutex::default(),
            selected: Mutex::default(),
            generation: AtomicU64::new(0),
        }
    }
    pub fn select_environment(&self, id: Option<String>) {
        *lock(&self.selected) = id;
    }
    #[must_use]
    pub fn active_variables(
        &self,
        environments: &[DomainEnvironment],
    ) -> HashMap<String, DomainValueSource> {
        super::active_variables(environments, lock(&self.selected).as_deref())
    }
    #[must_use]
    pub fn cookie_revision(&self) -> u64 {
        lock(&self.snapshot).cookie_revision
    }
    #[must_use]
    pub fn snapshot(&self) -> RuntimeSnapshot {
        lock(&self.snapshot).clone()
    }
    #[must_use]
    pub fn cookie_jar(&self) -> RuntimeCookies {
        lock(&self.cookies).clone()
    }
    pub fn configure_cookies(&self, workspace: Option<&str>, directory: Option<&str>) {
        *lock(&self.cookies) = if let (Some(workspace), Some(directory)) = (workspace, directory) {
            let storage = cookie_storage_path(Path::new(workspace), Path::new(directory));
            let _ = std::fs::remove_file(Path::new(directory).join("cookies.json"));
            Arc::new(Mutex::new(ResponseCookieJar::new(Some(storage))))
        } else {
            self.default_cookies.clone()
        };
        lock(&self.snapshot).cookie_revision += 1;
    }
    #[must_use]
    pub fn cookies(&self) -> Vec<StoredCookie> {
        let mut cookies = lock(&self.cookie_jar()).all(reference_time());
        cookies.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
        cookies
    }
    pub fn delete_cookie(&self, id: &str) {
        lock(&self.cookie_jar()).delete(id);
        lock(&self.snapshot).cookie_revision += 1;
    }
    pub fn clear_cookies(&self) {
        lock(&self.cookie_jar()).remove_all();
        lock(&self.snapshot).cookie_revision += 1;
    }
    pub fn received_cookies(&self, cookies: &[StoredCookie]) {
        if !cookies.is_empty() {
            lock(&self.snapshot).cookie_revision += 1;
        }
    }
    #[must_use]
    pub fn history_entries(&self, id: &str) -> Vec<SessionHistoryEntry> {
        history::list(&mut lock(&self.history), id)
    }
    #[must_use]
    pub fn history_count(&self, id: &str) -> u64 {
        lock(&self.history).count(id)
    }
    pub fn load_history(&self, id: &str) {
        let entries = self.history_entries(id);
        lock(&self.snapshot).history_entries = entries;
    }
    pub fn clear_history(&self, id: &str) {
        let _ = lock(&self.history).clear(id);
        let mut state = lock(&self.snapshot);
        state.history_entries.clear();
        state.request_activity.remove(id);
        state.history_revision += 1;
    }
    pub fn record(
        &self,
        id: &str,
        run_id: &str,
        snapshot: SessionRunSnapshot,
        body: &crate::session::run::SharedBody,
    ) {
        let Some(prepared) = snapshot.prepared else {
            return;
        };
        let entry = SessionHistoryEntry {
            id: run_id.into(),
            request_id: id.into(),
            created_at: reference_time(),
            prepared,
            response_head: snapshot.head.clone(),
            completion: snapshot.completion,
            failure: snapshot.failure,
            body_path: String::new(),
        };
        let _ = history::record(&mut lock(&self.history), entry, &lock(body));
        let entries = self.history_entries(id);
        let mut state = lock(&self.snapshot);
        state.history_entries = entries;
        state.request_activity.insert(
            id.into(),
            RequestActivity {
                date: reference_time(),
                status: snapshot.head.map(|h| h.status),
            },
        );
        state.history_revision += 1;
    }
    pub fn reset_workspace(&self) {
        self.select_environment(None);
        let mut state = lock(&self.snapshot);
        self.generation.fetch_add(1, Ordering::AcqRel);
        state.history_entries.clear();
        state.request_activity.clear();
    }
    pub fn restore(&self, entry: SessionHistoryEntry, run: &Mutex<DocumentRun>) {
        let (snapshot, path) = history_snapshot(entry);
        lock(run).restore(snapshot, path);
    }
    pub async fn restore_latest(
        self: Arc<Self>,
        id: String,
        run: Arc<Mutex<DocumentRun>>,
        observer: &dyn super::RuntimeObserver,
    ) {
        let generation = self.generation.load(Ordering::Acquire);
        let state = self.clone();
        let restored = tokio::task::spawn_blocking(move || {
            let entry = state.history_entries(&id).into_iter().next()?;
            let activity = RequestActivity {
                date: entry.created_at,
                status: entry.response_head.as_ref().map(|head| head.status),
            };
            let id = entry.request_id.clone();
            let (snapshot, path) = history_snapshot(entry);
            Some((id, activity, snapshot, path))
        })
        .await
        .ok()
        .flatten();
        let Some((id, activity, snapshot, path)) = restored else {
            return;
        };
        // The native identity check occurs after all filesystem work, just as in the original facade.
        if observer.prepare().await.is_err() {
            return;
        }
        let mut run = lock(&run);
        let mut state = lock(&self.snapshot);
        let current = run.snapshot();
        if self.generation.load(Ordering::Acquire) != generation
            || current.active_run_id.is_some()
            || current.head.is_some()
            || current.failure.is_some()
        {
            return;
        }
        run.restore(snapshot, path);
        state.request_activity.insert(id, activity);
    }
}
fn history_snapshot(entry: SessionHistoryEntry) -> (SessionRunSnapshot, PathBuf) {
    let viewport = history::viewport(&entry.body_path);
    let bytes = entry
        .completion
        .as_ref()
        .map_or(viewport.len() as u64, |c| c.bytes_received);
    let path = PathBuf::from(&entry.body_path);
    (
        SessionRunSnapshot {
            head: entry.response_head,
            prepared: Some(entry.prepared),
            text: String::from_utf8_lossy(&viewport).into_owned(),
            truncated: bytes > viewport.len() as u64,
            data: viewport,
            bytes,
            completion: entry.completion,
            failure: entry.failure,
            body_path: Some(entry.body_path),
            ..SessionRunSnapshot::default()
        },
        path,
    )
}
