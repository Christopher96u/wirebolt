//! Typed workspace io application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{
    Arc, DomainWorkspace, Mutex, PushAvailability, SessionCommandBatchResult,
    SessionWorkspaceCommand, WorkspaceBridge, WorkspaceExport, WorkspaceFailure,
    WorkspaceGitAction, WorkspaceGitChange, WorkspaceGitDelta, WorkspaceGitOperation,
    WorkspaceGitOutcome, WorkspaceGitStatus, WorkspaceIO, WorkspaceImportSummary,
    WorkspaceImportedEnvironment, WorkspaceReply, lock,
};
#[uniffi::remote(Record)]
pub struct WorkspaceFailure {
    pub kind: String,
    pub reason: String,
    pub localized: bool,
}

#[uniffi::remote(Record)]
pub struct WorkspaceImportedEnvironment {
    pub id: String,
    pub name: String,
}

#[uniffi::remote(Record)]
pub struct WorkspaceImportSummary {
    pub collection_names: Vec<String>,
    pub request_count: u64,
    pub group_count: u64,
    pub environments: Vec<WorkspaceImportedEnvironment>,
    pub warnings: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct WorkspaceGitChange {
    pub path: String,
    pub previous_path: Option<String>,
    pub staged: WorkspaceGitDelta,
    pub unstaged: WorkspaceGitDelta,
    pub conflicted: bool,
}

#[uniffi::remote(Record)]
pub struct WorkspaceGitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub changes: Vec<WorkspaceGitChange>,
    pub merging: bool,
}

#[uniffi::remote(Record)]
pub struct WorkspaceGitOperation {
    pub outcome: WorkspaceGitOutcome,
    pub revision: Option<String>,
    pub status: WorkspaceGitStatus,
}

#[uniffi::remote(Enum)]
pub enum WorkspaceGitAction {
    Status,
    Pull,
    Commit { message: String },
    Push,
    Abort,
    Initialize,
}

#[uniffi::remote(Enum)]
pub enum WorkspaceExport {
    Workspace,
    Collection { id: String },
    Request { collection_id: String, id: String },
}

#[uniffi::remote(Enum)]
pub enum WorkspaceIO {
    Load,
    Commands {
        commands: Vec<SessionWorkspaceCommand>,
    },
    Import {
        format: String,
        source: String,
        name: Option<String>,
    },
    Export {
        target: WorkspaceExport,
    },
    Git {
        action: WorkspaceGitAction,
    },
}

#[uniffi::remote(Enum)]
pub enum WorkspaceReply {
    Loaded { workspace: DomainWorkspace },
    Commands { result: SessionCommandBatchResult },
    Imported { summary: WorkspaceImportSummary },
    Exported { content: String },
    GitStatus { status: WorkspaceGitStatus },
    GitOperation { operation: WorkspaceGitOperation },
    Failed { failure: WorkspaceFailure },
}

#[derive(Debug, uniffi::Object)]
pub struct WorkspaceIOReply {
    sender: Mutex<Option<tokio::sync::oneshot::Sender<WorkspaceReply>>>,
}

#[uniffi::export]
impl WorkspaceIOReply {
    pub fn complete(&self, reply: WorkspaceReply) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(reply);
        }
    }
}

#[uniffi::export(with_foreign)]
pub trait WorkspaceIOPlatform: Send + Sync {
    fn execute(&self, request: WorkspaceIO, reply: Arc<WorkspaceIOReply>);
}

#[derive(uniffi::Object)]
pub struct AppWorkspacePlatform {
    pub(super) native: Option<Arc<wirebolt_app::session::persistence::SessionPersistence>>,
    pub(super) foreign: Option<Arc<dyn WorkspaceIOPlatform>>,
}

impl std::fmt::Debug for AppWorkspacePlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppWorkspacePlatform")
            .finish_non_exhaustive()
    }
}

#[uniffi::export]
impl AppWorkspacePlatform {
    #[uniffi::constructor]
    pub fn workspace(bridge: Arc<WorkspaceBridge>) -> Arc<Self> {
        Arc::new(Self {
            native: Some(bridge.inner.clone()),
            foreign: None,
        })
    }
    #[uniffi::constructor]
    pub fn foreign(platform: Arc<dyn WorkspaceIOPlatform>) -> Arc<Self> {
        Arc::new(Self {
            native: None,
            foreign: Some(platform),
        })
    }
}

#[async_trait::async_trait]
impl wirebolt_app::session::workspace::io::WorkspacePlatform for AppWorkspacePlatform {
    async fn execute(&self, request: WorkspaceIO) -> WorkspaceReply {
        if let Some(native) = &self.native {
            let native = native.clone();
            return tokio::task::spawn_blocking(move || native.workspace_io(request))
                .await
                .unwrap_or_else(|_| WorkspaceReply::Failed {
                    failure: WorkspaceFailure::workspace(),
                });
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        if let Some(platform) = &self.foreign {
            platform.execute(
                request,
                Arc::new(WorkspaceIOReply {
                    sender: Mutex::new(Some(sender)),
                }),
            );
        }
        receiver.await.unwrap_or_else(|_| WorkspaceReply::Failed {
            failure: WorkspaceFailure::workspace(),
        })
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AppWorkspacePlatform {
    pub async fn execute(&self, request: WorkspaceIO) -> WorkspaceReply {
        wirebolt_app::session::workspace::io::WorkspacePlatform::execute(self, request).await
    }
}

#[uniffi::export]
pub fn session_git_porcelain(
    staged: WorkspaceGitDelta,
    unstaged: WorkspaceGitDelta,
    conflicted: bool,
) -> String {
    wirebolt_app::session::workspace::presentation::git_porcelain(staged, unstaged, conflicted)
}
#[uniffi::export]
pub fn session_git_change_kind(
    staged: WorkspaceGitDelta,
    unstaged: WorkspaceGitDelta,
    conflicted: bool,
) -> String {
    wirebolt_app::session::workspace::presentation::git_change_kind(staged, unstaged, conflicted)
}
#[uniffi::export]
pub fn session_git_operation_label(outcome: WorkspaceGitOutcome) -> String {
    wirebolt_app::session::workspace::presentation::git_operation_label(outcome)
}
#[uniffi::export]
pub fn session_git_push_help(availability: PushAvailability) -> String {
    wirebolt_app::session::workspace::presentation::git_push_help(availability)
}

#[uniffi::remote(Enum)]
pub enum WorkspaceGitDelta {
    None,
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Unmerged,
}
#[uniffi::remote(Enum)]
pub enum WorkspaceGitOutcome {
    NothingToCommit,
    Committed,
    Updated,
    UpToDate,
    Pushed,
    Conflicted,
    MergeAborted,
}
