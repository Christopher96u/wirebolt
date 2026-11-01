//! Typed persistence seam. Production calls the core directly; foreign implementations
//! serve test stores and platform adapters.
use super::SessionWorkspaceCommand;
use crate::{
    domain::DomainWorkspace,
    session::persistence::{SessionCommandBatchResult, SessionPersistence},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "String", into = "String")]
pub enum WorkspaceFailureKind {
    Workspace,
    Bridge,
    InvalidBridgeResponse,
    UnsavedRequest,
    Unsupported,
    WorkspaceReload,
    NotConfigured,
    GitUnavailable,
    NotRepository,
    WorkspaceNotRepositoryRoot,
    CommandFailed,
    InvalidOutput,
    InvalidCommitMessage,
    IdentityMissing,
    AuthenticationRequired,
    DirtyWorkspace,
    MissingUpstream,
    MissingRemote,
    DetachedHead,
    TimedOut,
    NoMergeInProgress,
    Other { code: String },
}
impl WorkspaceFailureKind {
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::Workspace => "workspace",
            Self::Bridge => "bridge",
            Self::InvalidBridgeResponse => "invalid_bridge_response",
            Self::UnsavedRequest => "unsaved_request",
            Self::Unsupported => "unsupported",
            Self::WorkspaceReload => "workspace_reload",
            Self::NotConfigured => "not_configured",
            Self::GitUnavailable => "git_unavailable",
            Self::NotRepository => "not_repository",
            Self::WorkspaceNotRepositoryRoot => "workspace_not_repository_root",
            Self::CommandFailed => "command_failed",
            Self::InvalidOutput => "invalid_output",
            Self::InvalidCommitMessage => "invalid_commit_message",
            Self::IdentityMissing => "identity_missing",
            Self::AuthenticationRequired => "authentication_required",
            Self::DirtyWorkspace => "dirty_workspace",
            Self::MissingUpstream => "missing_upstream",
            Self::MissingRemote => "missing_remote",
            Self::DetachedHead => "detached_head",
            Self::TimedOut => "timed_out",
            Self::NoMergeInProgress => "no_merge_in_progress",
            Self::Other { code } => code,
        }
    }
}
impl From<String> for WorkspaceFailureKind {
    fn from(code: String) -> Self {
        match code.as_str() {
            "workspace" => Self::Workspace,
            "bridge" => Self::Bridge,
            "invalid_bridge_response" => Self::InvalidBridgeResponse,
            "unsaved_request" => Self::UnsavedRequest,
            "unsupported" => Self::Unsupported,
            "workspace_reload" => Self::WorkspaceReload,
            "not_configured" => Self::NotConfigured,
            "git_unavailable" => Self::GitUnavailable,
            "not_repository" => Self::NotRepository,
            "workspace_not_repository_root" => Self::WorkspaceNotRepositoryRoot,
            "command_failed" => Self::CommandFailed,
            "invalid_output" => Self::InvalidOutput,
            "invalid_commit_message" => Self::InvalidCommitMessage,
            "identity_missing" => Self::IdentityMissing,
            "authentication_required" => Self::AuthenticationRequired,
            "dirty_workspace" => Self::DirtyWorkspace,
            "missing_upstream" => Self::MissingUpstream,
            "missing_remote" => Self::MissingRemote,
            "detached_head" => Self::DetachedHead,
            "timed_out" => Self::TimedOut,
            "no_merge_in_progress" => Self::NoMergeInProgress,
            _ => Self::Other { code },
        }
    }
}
impl From<WorkspaceFailureKind> for String {
    fn from(kind: WorkspaceFailureKind) -> Self {
        kind.code().to_owned()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFailure {
    pub kind: WorkspaceFailureKind,
    pub reason: String,
    pub localized: bool,
}
impl WorkspaceFailure {
    #[must_use]
    pub fn workspace() -> Self {
        Self {
            kind: WorkspaceFailureKind::Workspace,
            reason: String::new(),
            localized: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceImportedEnvironment {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceImportSummary {
    pub collection_names: Vec<String>,
    pub request_count: u64,
    pub group_count: u64,
    #[serde(default)]
    pub environments: Vec<WorkspaceImportedEnvironment>,
    #[serde(default)]
    pub warnings: Vec<String>,
}
pub use wirebolt_core::GitDelta;
pub use wirebolt_core::GitOperationOutcome as WorkspaceGitOutcome;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceGitChange {
    pub path: String,
    pub previous_path: Option<String>,
    pub staged: GitDelta,
    pub unstaged: GitDelta,
    pub conflicted: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceGitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub changes: Vec<WorkspaceGitChange>,
    pub merging: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceGitOperation {
    pub outcome: WorkspaceGitOutcome,
    pub revision: Option<String>,
    pub status: WorkspaceGitStatus,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceGitAction {
    Status,
    Pull,
    Commit { message: String },
    Push,
    Abort,
    Initialize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceExport {
    Workspace,
    Collection { id: String },
    Request { collection_id: String, id: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceIO {
    Load,
    Commands {
        commands: Vec<SessionWorkspaceCommand>,
    },
    Import {
        format: wirebolt_core::ImportFormat,
        source: String,
        name: Option<String>,
    },
    ImportBrunoFolder {
        bundle: wirebolt_core::BrunoFolderBundle,
        name: Option<String>,
    },
    Export {
        target: WorkspaceExport,
    },
    Git {
        action: WorkspaceGitAction,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "One typed reply per coarse workspace operation."
)]
pub enum WorkspaceReply {
    Loaded { workspace: DomainWorkspace },
    Commands { result: SessionCommandBatchResult },
    Imported { summary: WorkspaceImportSummary },
    Exported { content: String },
    GitStatus { status: WorkspaceGitStatus },
    GitOperation { operation: WorkspaceGitOperation },
    Failed { failure: WorkspaceFailure },
}
#[async_trait]
pub trait WorkspacePlatform: Send + Sync {
    async fn execute(&self, request: WorkspaceIO) -> WorkspaceReply;
}
fn workspace_result<T>(
    result: Result<T, super::super::persistence::WorkspaceError>,
    map: impl FnOnce(T) -> WorkspaceReply,
) -> WorkspaceReply {
    match result {
        Ok(value) => map(value),
        Err(error) => WorkspaceReply::Failed {
            failure: WorkspaceFailure {
                kind: WorkspaceFailureKind::Workspace,
                reason: error.to_string(),
                localized: true,
            },
        },
    }
}
fn git_result<T>(
    result: Result<T, super::super::persistence::WorkspaceGitError>,
    map: impl FnOnce(T) -> WorkspaceReply,
) -> WorkspaceReply {
    match result {
        Ok(value) => map(value),
        Err(super::super::persistence::WorkspaceGitError::OperationFailed { kind, reason }) => {
            WorkspaceReply::Failed {
                failure: WorkspaceFailure {
                    kind,
                    reason,
                    localized: true,
                },
            }
        }
    }
}
impl SessionPersistence {
    /// Runs on a worker in the bridge, keeping filesystem/Git work off the UI thread.
    #[must_use]
    pub fn workspace_io(&self, request: WorkspaceIO) -> WorkspaceReply {
        match request {
            WorkspaceIO::Load => workspace_result(self.domain_snapshot(), |workspace| {
                WorkspaceReply::Loaded { workspace }
            }),
            WorkspaceIO::Commands { commands } => WorkspaceReply::Commands {
                result: self.execute_session_commands(commands),
            },
            WorkspaceIO::Import {
                format,
                source,
                name,
            } => {
                let result = name.map_or_else(
                    || self.commit_import(format, &source),
                    |name| self.commit_import_file(format, &source, &name),
                );
                workspace_result(result, |commit| WorkspaceReply::Imported {
                    summary: commit.summary,
                })
            }
            WorkspaceIO::ImportBrunoFolder { bundle, name } => workspace_result(
                self.commit_bruno_folder(&bundle, name.as_deref()),
                |commit| WorkspaceReply::Imported {
                    summary: commit.summary,
                },
            ),
            WorkspaceIO::Export { target } => workspace_result(
                match target {
                    WorkspaceExport::Workspace => self.export_workspace_json(),
                    WorkspaceExport::Collection { id } => self.export_collection_json(&id),
                    WorkspaceExport::Request { collection_id, id } => {
                        self.export_request_json(&collection_id, &id)
                    }
                },
                |content| WorkspaceReply::Exported { content },
            ),
            WorkspaceIO::Git { action } => match action {
                WorkspaceGitAction::Status => git_result(self.git_status(), |status| {
                    WorkspaceReply::GitStatus { status }
                }),
                WorkspaceGitAction::Initialize => git_result(self.git_initialize(), |status| {
                    WorkspaceReply::GitStatus { status }
                }),
                WorkspaceGitAction::Pull => git_result(self.git_pull(), |operation| {
                    WorkspaceReply::GitOperation { operation }
                }),
                WorkspaceGitAction::Commit { message } => {
                    git_result(self.git_commit(&message), |operation| {
                        WorkspaceReply::GitOperation { operation }
                    })
                }
                WorkspaceGitAction::Push => git_result(self.git_push(), |operation| {
                    WorkspaceReply::GitOperation { operation }
                }),
                WorkspaceGitAction::Abort => git_result(self.git_abort_merge(), |operation| {
                    WorkspaceReply::GitOperation { operation }
                }),
            },
        }
    }
}
