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
    collections::HashMap,
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
    workspace: String,
    moves: HashMap<SessionRequestIdentity, SessionRequestIdentity>,
}
#[derive(Debug)]
pub struct RuntimeState {
    pub history: RuntimeHistory,
    default_cookies: RuntimeCookies,
    cookies: Mutex<RuntimeCookies>,
    snapshot: Mutex<RuntimeSnapshot>,
    selected: Mutex<Option<String>>,
    generation: AtomicU64,
    history_context: Mutex<HistoryContext>,
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
            history_context: Mutex::new(HistoryContext {
                workspace: super::super::history::UNSAVED_WORKSPACE_NAMESPACE.into(),
                moves: HashMap::new(),
            }),
        }
    }
    pub fn configure_history_workspace(&self, path: Option<&str>) {
        self.set_history_namespace(path.map_or_else(
            || super::super::history::UNSAVED_WORKSPACE_NAMESPACE.into(),
            |path| crate::response::workspace_namespace(Path::new(path)),
        ));
    }
    fn set_history_namespace(&self, namespace: String) {
        let mut context = lock(&self.history_context);
        if context.workspace != namespace {
            context.workspace = namespace;
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
    }
    /// # Errors
    /// Reports damaged indexes and failed legacy migrations without replacing them.
    pub fn load_workspace_history(
        &self,
        namespace: &str,
        workspace: &crate::domain::DomainWorkspace,
    ) -> crate::response::ResponseResult<()> {
        let mut history = lock(&self.history);
        self.set_history_namespace(namespace.to_owned());
        {
            let mut state = lock(&self.snapshot);
            state.history_entries.clear();
            state.request_activity.clear();
            state.history_revision += 1;
        }
        history.migrate_legacy(namespace, workspace)?;
        let entries = history.workspace_entries(namespace)?;
        let mut state = lock(&self.snapshot);
        state.history_entries.clear();
        state.request_activity.clear();
        for entry in entries {
            let Some(identity) = entry.identity else {
                continue;
            };
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
        Ok(())
    }
    #[must_use]
    pub fn request_identity(
        &self,
        collection: Option<&str>,
        request: &str,
    ) -> SessionRequestIdentity {
        SessionRequestIdentity {
            workspace: lock(&self.history_context).workspace.clone(),
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
        let namespace = context.workspace.clone();
        context.moves.retain(|from, _| {
            from.workspace != namespace
                || crate::session::workspace::location(
                    workspace,
                    &from.collection_id,
                    &from.request_id,
                )
                .is_none()
        });
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
                drop(context);
                history.relocate(&from, &to)?;
                let entries = history.list(&to)?;
                let context = lock(&self.history_context);
                if to.workspace != context.workspace {
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
    pub fn legacy_history(&self, id: &str) -> Vec<SessionHistoryEntry> {
        lock(&self.history).legacy_list(id).unwrap_or_default()
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
        let mut cookies = lock(&self.cookie_jar()).all(crate::response::unix_time());
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
    /// # Errors
    /// Returns a history storage error.
    pub fn history_entries(
        &self,
        id: &SessionRequestIdentity,
    ) -> crate::response::ResponseResult<Vec<SessionHistoryEntry>> {
        lock(&self.history).list(id)
    }
    /// # Errors
    /// Returns a history storage error.
    pub fn history_count(
        &self,
        id: &SessionRequestIdentity,
    ) -> crate::response::ResponseResult<u64> {
        lock(&self.history).count(id)
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
        lock(&self.history).clear(id)?;
        let mut state = lock(&self.snapshot);
        state.history_entries.clear();
        state.request_activity.remove(&id.activity_key());
        state.history_revision += 1;
        Ok(())
    }
    /// # Errors
    /// Returns a storage error without replacing a damaged index.
    pub fn clear_all_history(&self) -> crate::response::ResponseResult<()> {
        lock(&self.history).clear_all()?;
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
            request_id: id.request_id.clone(),
            identity: Some(id.clone()),
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
        let Ok(entries) = history.list(&id) else {
            return;
        };
        let context = lock(&self.history_context);
        if id.workspace != context.workspace {
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
            let id = entry.identity.as_ref()?.activity_key();
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
