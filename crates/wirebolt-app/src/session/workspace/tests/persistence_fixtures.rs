use super::*;
use crate::session::persistence::SessionPersistence;

pub(super) fn persistence() -> (tempfile::TempDir, Arc<SessionPersistence>) {
    let root = tempfile::tempdir().unwrap();
    let store = SessionPersistence::open_or_create(
        root.path().to_string_lossy().into_owned(),
        "Storage".into(),
        Arc::new(super::files::NoSecrets),
    )
    .unwrap();
    (root, store)
}

pub(super) fn imported_pair(store: &SessionPersistence) -> DomainWorkspace {
    for url in ["http://localhost/one", "http://localhost/two"] {
        store
            .commit_import(wirebolt_core::ImportFormat::Curl, &format!("curl {url}"))
            .unwrap();
    }
    store.domain_snapshot().unwrap()
}

pub(super) fn apply_plan(
    store: &SessionPersistence,
    session: &WorkspaceSession,
    edit: SessionWorkspaceEdit,
) -> SessionWorkspacePlan {
    let plan = session.plan(None, edit).unwrap();
    store.apply_session_commands(plan.commands.clone()).unwrap();
    session.commit(plan.clone());
    plan
}

pub(super) struct SaveGate {
    pub(super) store: Arc<SessionPersistence>,
    pub(super) ready: tokio::sync::Notify,
    pub(super) release: tokio::sync::Notify,
    pub(super) pause: std::sync::atomic::AtomicBool,
}
#[async_trait]
impl WorkspacePlatform for SaveGate {
    async fn execute(&self, request: WorkspaceIO) -> WorkspaceReply {
        if matches!(&request, WorkspaceIO::Commands {commands} if commands.iter().any(|c| matches!(c, SessionWorkspaceCommand::SaveRequest {..})))
            && self.pause.swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.ready.notify_one();
            self.release.notified().await;
        }
        self.store.workspace_io(request)
    }
}
