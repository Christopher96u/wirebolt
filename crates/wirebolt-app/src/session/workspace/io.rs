//! Typed persistence seam. Production calls the core directly; foreign implementations
//! keep test stores and platform adapters usable without moving policy back to Swift.
use super::SessionWorkspaceCommand;
use crate::{
    domain::DomainWorkspace,
    session::persistence::{SessionCommandBatchResult, SessionPersistence},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFailure {
    pub kind: String,
    pub reason: String,
    pub localized: bool,
}
impl WorkspaceFailure {
    #[must_use]
    pub fn workspace() -> Self {
        Self {
            kind: "workspace".into(),
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceGitChange {
    pub path: String,
    pub previous_path: Option<String>,
    pub staged: String,
    pub unstaged: String,
    pub conflicted: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceGitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub changes: Vec<WorkspaceGitChange>,
    #[serde(default)]
    pub merging: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceGitOperation {
    pub outcome: String,
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
    result: Result<T, super::super::persistence::WorkspaceBridgeError>,
    map: impl FnOnce(T) -> WorkspaceReply,
) -> WorkspaceReply {
    match result {
        Ok(value) => map(value),
        Err(error) => WorkspaceReply::Failed {
            failure: WorkspaceFailure {
                kind: "workspace".into(),
                reason: error.to_string(),
                localized: true,
            },
        },
    }
}
fn decode_git<T: serde::de::DeserializeOwned>(
    result: Result<String, super::super::persistence::GitBridgeError>,
    map: impl FnOnce(T) -> WorkspaceReply,
) -> WorkspaceReply {
    match result {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(value) => map(value),
            Err(_) => WorkspaceReply::Failed {
                failure: WorkspaceFailure {
                    kind: "invalid_bridge_response".into(),
                    reason: "Git returned an invalid response.".into(),
                    localized: true,
                },
            },
        },
        Err(super::super::persistence::GitBridgeError::OperationFailed { kind, reason }) => {
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
                    || self.commit_import(&format, &source),
                    |name| self.commit_import_file(&format, &source, &name),
                );
                workspace_result(result, |json| {
                    #[derive(Deserialize)]
                    struct Commit {
                        summary: WorkspaceImportSummary,
                    }
                    match serde_json::from_str::<Commit>(&json) {
                        Ok(value) => WorkspaceReply::Imported {
                            summary: value.summary,
                        },
                        Err(_) => WorkspaceReply::Failed {
                            failure: WorkspaceFailure::workspace(),
                        },
                    }
                })
            }
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
                WorkspaceGitAction::Status => decode_git(self.git_status_json(), |status| {
                    WorkspaceReply::GitStatus { status }
                }),
                WorkspaceGitAction::Initialize => {
                    decode_git(self.git_initialize_json(), |status| {
                        WorkspaceReply::GitStatus { status }
                    })
                }
                action => decode_git(
                    match action {
                        WorkspaceGitAction::Pull => self.git_pull_json(),
                        WorkspaceGitAction::Commit { message } => self.git_commit_json(&message),
                        WorkspaceGitAction::Push => self.git_push_json(),
                        WorkspaceGitAction::Abort => self.git_abort_merge_json(),
                        WorkspaceGitAction::Status | WorkspaceGitAction::Initialize => {
                            unreachable!()
                        }
                    },
                    |operation| WorkspaceReply::GitOperation { operation },
                ),
            },
        }
    }
}
