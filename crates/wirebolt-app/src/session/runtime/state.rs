use crate::lock;
use crate::{
    domain::{DomainEnvironment, DomainValueSource},
    response::{ResponseCookieJar, RunHistory, StoredCookie, cookie_storage_path, reference_time},
    session::{
        history::{SessionHistoryEntry, SessionRequestIdentity},
        run::{DocumentRun, SessionRunSnapshot},
    },
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
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
struct HistoryContext {
    requests: Option<HashSet<SessionRequestIdentity>>,
    moves: HashMap<SessionRequestIdentity, SessionRequestIdentity>,
}
#[derive(Debug)]
pub struct RuntimeState {
    pub history: RuntimeHistory,
    default_cookies: RuntimeCookies,
    cookies: Mutex<RuntimeCookies>,
    workspace_cookies: Mutex<HashMap<std::path::PathBuf, RuntimeCookies>>,
    snapshot: Mutex<RuntimeSnapshot>,
    selected: Mutex<Option<String>>,
    generation: AtomicU64,
    history_context: Mutex<HistoryContext>,
    storage_problem: Mutex<Option<String>>,
}
impl RuntimeState {
    #[must_use]
    pub fn new(history: RuntimeHistory, cookies: RuntimeCookies) -> Self {
        Self {
            history,
            default_cookies: cookies.clone(),
            cookies: Mutex::new(cookies),
            workspace_cookies: Mutex::default(),
            snapshot: Mutex::default(),
            selected: Mutex::default(),
            generation: AtomicU64::new(0),
            storage_problem: Mutex::default(),
            history_context: Mutex::new(HistoryContext {
                requests: None,
                moves: HashMap::new(),
            }),
        }
    }
    /// # Errors
    /// Reports damaged data once, or a failed read.
    pub fn load_workspace_history(
        &self,
        workspace: &crate::domain::DomainWorkspace,
    ) -> crate::response::ResponseResult<()> {
        let mut history = lock(&self.history);
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.reconcile_history(workspace);
        {
            let mut state = lock(&self.snapshot);
            state.history_entries.clear();
            state.request_activity.clear();
            state.history_revision += 1;
        }
        let entries = history.workspace_entries(workspace)?;
        let mut state = lock(&self.snapshot);
        state.history_entries.clear();
        state.request_activity.clear();
        for entry in entries {
            let identity = entry.identity;
            let activity = RequestActivity {
                date: entry.created_at,
                status: entry.response_head.map(|head| head.status),
            };
            let previous = state
                .request_activity
                .entry(identity.activity_key())
                .or_insert(activity.clone());
            if previous.date < activity.date {
                *previous = activity;
            }
        }
        state.history_revision += 1;
        history.take_load_problem()
    }
    #[must_use]
    pub fn request_identity(
        &self,
        collection: Option<&str>,
        request: &str,
    ) -> SessionRequestIdentity {
        SessionRequestIdentity {
            collection_id: collection.unwrap_or("").into(),
            request_id: request.into(),
        }
    }
    #[must_use]
    pub fn document_identity(
        &self,
        document: &crate::session::tabs::SessionDocument,
    ) -> SessionRequestIdentity {
        self.request_identity(document.collection_id.as_deref(), &document.request_id)
    }
    pub fn reconcile_history(&self, workspace: &crate::domain::DomainWorkspace) {
        let mut context = lock(&self.history_context);
        context.requests = Some(
            workspace
                .collections
                .iter()
                .flat_map(|collection| {
                    collection
                        .requests
                        .iter()
                        .map(|request| SessionRequestIdentity {
                            collection_id: collection.id.clone(),
                            request_id: request.request.id.clone(),
                        })
                })
                .collect(),
        );
        let requests = context.requests.clone().unwrap_or_default();
        context.moves.retain(|from, _| !requests.contains(from));
    }
    /// # Errors
    /// Returns an I/O error if the history index cannot be saved.
    pub fn apply_workspace_effects(
        &self,
        effects: &[crate::session::workspace::SessionDocumentEffect],
    ) -> crate::response::ResponseResult<()> {
        use crate::session::workspace::SessionDocumentEffect;
        for effect in effects {
            if let SessionDocumentEffect::Relocate {
                collection_id,
                request_id,
                destination,
            } = effect
            {
                let from = self.request_identity(Some(collection_id), request_id);
                let to = self.request_identity(Some(destination), request_id);
                let mut history = lock(&self.history);
                let mut context = lock(&self.history_context);
                // Redirect in-flight completions too; undo reverses the mapping.
                context.moves.remove(&to);
                for target in context.moves.values_mut() {
                    if *target == from {
                        target.clone_from(&to);
                    }
                }
                context.moves.insert(from.clone(), to.clone());
                if let Some(requests) = &mut context.requests {
                    requests.remove(&from);
                    requests.insert(to.clone());
                }
                drop(context);
                history.relocate(&from, &to)?;
                self.collect_history_problem(&mut history);
                let entries = history.list(&to)?;
                let context = lock(&self.history_context);
                if context
                    .requests
                    .as_ref()
                    .is_some_and(|requests| !requests.contains(&to))
                {
                    continue;
                }
                let mut snapshot = lock(&self.snapshot);
                if let Some(activity) = snapshot.request_activity.remove(&from.activity_key()) {
                    snapshot
                        .request_activity
                        .insert(to.activity_key(), activity);
                }
                snapshot.history_entries = entries;
                snapshot.history_revision += 1;
            }
        }
        Ok(())
    }
    /// Reads only retained notices; this does not load files.
    pub fn take_storage_problem(&self) -> Option<String> {
        lock(&self.storage_problem)
            .take()
            .or_else(|| lock(&self.cookie_jar()).take_storage_problem())
    }
    fn collect_history_problem(&self, history: &mut RunHistory) {
        if let Err(error) = history.take_load_problem() {
            *lock(&self.storage_problem) = Some(error.to_string());
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
            lock(&self.workspace_cookies)
                .entry(storage.clone())
                .or_insert_with(|| Arc::new(Mutex::new(ResponseCookieJar::new(Some(storage)))))
                .clone()
        } else {
            self.default_cookies.clone()
        };
        lock(&self.snapshot).cookie_revision += 1;
    }
    #[must_use]
    pub fn cookies(&self) -> Vec<StoredCookie> {
        let mut cookies = lock(&self.cookie_jar()).all(crate::response::unix_time());
        cookies.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
        cookies
    }
    /// # Errors
    /// Returns a storage error without removing the cookie on failure.
    pub fn delete_cookies(&self, ids: &[String]) -> crate::response::ResponseResult<()> {
        lock(&self.cookie_jar()).delete(ids)?;
        lock(&self.snapshot).cookie_revision += 1;
        Ok(())
    }
    /// # Errors
    /// Returns a storage error without clearing cookies on failure.
    pub fn clear_cookies(&self) -> crate::response::ResponseResult<()> {
        lock(&self.cookie_jar()).remove_all()?;
        lock(&self.snapshot).cookie_revision += 1;
        Ok(())
    }
    pub fn received_cookies(&self, cookies: &[StoredCookie]) {
        if !cookies.is_empty() {
            lock(&self.snapshot).cookie_revision += 1;
        }
    }
    /// # Errors
    /// Returns a history storage error.
    pub fn history_entries(
        &self,
        id: &SessionRequestIdentity,
    ) -> crate::response::ResponseResult<Vec<SessionHistoryEntry>> {
        let mut history = lock(&self.history);
        let entries = history.list(id)?;
        self.collect_history_problem(&mut history);
        Ok(entries)
    }
    /// # Errors
    /// Returns a history storage error.
    pub fn history_count(
        &self,
        id: &SessionRequestIdentity,
    ) -> crate::response::ResponseResult<u64> {
        let mut history = lock(&self.history);
        let count = history.count(id)?;
        self.collect_history_problem(&mut history);
        Ok(count)
    }
    /// # Errors
    /// Returns a history storage error.
    pub fn load_history(&self, id: &SessionRequestIdentity) -> crate::response::ResponseResult<()> {
        let entries = self.history_entries(id)?;
        lock(&self.snapshot).history_entries = entries;
        Ok(())
    }
    /// # Errors
    /// Returns an error if the history index cannot be updated.
    pub fn clear_history(
        &self,
        id: &SessionRequestIdentity,
    ) -> crate::response::ResponseResult<()> {
        let mut history = lock(&self.history);
        history.clear(id)?;
        self.collect_history_problem(&mut history);
        let mut state = lock(&self.snapshot);
        state.history_entries.clear();
        state.request_activity.remove(&id.activity_key());
        state.history_revision += 1;
        Ok(())
    }
    /// # Errors
    /// Returns a storage error if reading, backing up or writing the index fails.
    pub fn clear_all_history(&self) -> crate::response::ResponseResult<()> {
        let mut history = lock(&self.history);
        history.clear_all()?;
        self.collect_history_problem(&mut history);
        let mut state = lock(&self.snapshot);
        state.history_entries.clear();
        state.request_activity.clear();
        state.history_revision += 1;
        Ok(())
    }
    pub fn record(
        &self,
        id: &SessionRequestIdentity,
        run_id: &str,
        snapshot: SessionRunSnapshot,
        body: &crate::session::run::SharedBody,
    ) {
        let Some(prepared) = snapshot.prepared else {
            return;
        };
        let mut history = lock(&self.history);
        let id = {
            let context = lock(&self.history_context);
            context.moves.get(id).unwrap_or(id).clone()
        };
        let entry = SessionHistoryEntry {
            id: run_id.into(),
            identity: id.clone(),
            created_at: reference_time(),
            prepared,
            response_head: snapshot.head.clone(),
            completion: snapshot.completion,
            failure: snapshot.failure,
            body_path: String::new(),
        };
        if history.record(entry, &lock(body)).is_err() {
            return;
        }
        self.collect_history_problem(&mut history);
        let Ok(entries) = history.list(&id) else {
            return;
        };
        let context = lock(&self.history_context);
        if !id.collection_id.is_empty()
            && context
                .requests
                .as_ref()
                .is_some_and(|requests| !requests.contains(&id))
        {
            return;
        }
        let mut state = lock(&self.snapshot);
        state.history_entries = entries;
        state.request_activity.insert(
            id.activity_key(),
            RequestActivity {
                date: reference_time(),
                status: snapshot.head.map(|h| h.status),
            },
        );
        state.history_revision += 1;
    }
    pub fn reset_workspace(&self) {
        self.select_environment(None);
        lock(&self.history_context).requests = Some(HashSet::new());
        let mut state = lock(&self.snapshot);
        self.generation.fetch_add(1, Ordering::AcqRel);
        state.history_entries.clear();
        state.request_activity.clear();
    }
    pub fn restore(&self, entry: SessionHistoryEntry, run: &Mutex<DocumentRun>) {
        let (snapshot, body) = history_snapshot(entry);
        lock(run).restore(snapshot, body);
    }
    pub async fn restore_latest(
        self: Arc<Self>,
        id: SessionRequestIdentity,
        run: Arc<Mutex<DocumentRun>>,
        observer: &dyn super::RuntimeObserver,
    ) {
        let generation = self.generation.load(Ordering::Acquire);
        let state = self.clone();
        let restored = tokio::task::spawn_blocking(move || {
            let entry = state.history_entries(&id).ok()?.into_iter().next()?;
            let activity = RequestActivity {
                date: entry.created_at,
                status: entry.response_head.as_ref().map(|head| head.status),
            };
            let id = entry.identity.activity_key();
            let (snapshot, body) = history_snapshot(entry);
            Some((id, activity, snapshot, body))
        })
        .await
        .ok()
        .flatten();
        let Some((id, activity, snapshot, body)) = restored else {
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
        run.restore(snapshot, body);
        state.request_activity.insert(id, activity);
    }
}
fn history_snapshot(
    entry: SessionHistoryEntry,
) -> (SessionRunSnapshot, Option<crate::response::BodyStore>) {
    let body = crate::response::BodyStore::retained(Path::new(&entry.body_path)).ok();
    let viewport = body
        .as_ref()
        .and_then(|body| {
            body.viewport(0, crate::response::VIEWPORT_BYTES as u64)
                .ok()
        })
        .unwrap_or_default();
    let bytes = entry
        .completion
        .as_ref()
        .map_or(viewport.len() as u64, |c| c.bytes_received);
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
        body,
    )
}
