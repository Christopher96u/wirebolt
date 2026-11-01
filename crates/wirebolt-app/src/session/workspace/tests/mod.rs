use super::{actions::WorkspaceAction as Action, io::*, operations::*, *};
use crate::session::run::lock;
use crate::{
    domain::*,
    session::{
        persistence::SessionCommandBatchResult,
        secrets::{SessionSecretPlatform, SessionSecrets},
        tabs::*,
    },
};
use async_trait::async_trait;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
mod files;
mod imports_git;
mod interface;
mod lifecycle;
mod mutations;
mod undo;
#[derive(Debug, Default)]
struct Store {
    commands: Mutex<Vec<SessionWorkspaceCommand>>,
    replies: Mutex<VecDeque<WorkspaceReply>>,
    fail_at: Mutex<Option<usize>>,
}
#[async_trait]
impl WorkspacePlatform for Store {
    async fn execute(&self, request: WorkspaceIO) -> WorkspaceReply {
        if let WorkspaceIO::Commands { commands } = request {
            let mut saved = lock(&self.commands);
            let mut completed = 0;
            for command in commands {
                if lock(&self.fail_at).is_some_and(|i| i == saved.len()) {
                    return WorkspaceReply::Commands {
                        result: SessionCommandBatchResult {
                            completed,
                            failure_reason: Some("failed".into()),
                        },
                    };
                }
                saved.push(command);
                completed += 1;
            }
            WorkspaceReply::Commands {
                result: SessionCommandBatchResult {
                    completed,
                    failure_reason: None,
                },
            }
        } else {
            lock(&self.replies).pop_front().unwrap_or_else(failure)
        }
    }
}
fn failure() -> WorkspaceReply {
    WorkspaceReply::Failed {
        failure: WorkspaceFailure {
            kind: crate::session::workspace::io::WorkspaceFailureKind::Unsupported,
            reason: "failed".into(),
            localized: false,
        },
    }
}
#[derive(Debug, Default)]
struct Secrets;
#[async_trait]
impl SessionSecretPlatform for Secrets {
    async fn read(&self, _: String) -> Result<Option<String>, ()> {
        Ok(None)
    }
    async fn save(&self, _: String, _: String) -> Result<(), ()> {
        Ok(())
    }
    async fn remove(&self, _: String) -> Result<(), ()> {
        Ok(())
    }
}
fn location(id: &str, group: Option<&str>, order: i64) -> DomainLocation {
    DomainLocation {
        collection_id: "api".into(),
        group_id: group.map(str::to_owned),
        order,
        request: DomainRequestDraft {
            id: id.into(),
            name: match id {
                "deep" => "Deep",
                "shallow" => "Shallow",
                _ => "Root",
            }
            .into(),
            ..Default::default()
        },
    }
}
fn nested() -> DomainWorkspace {
    DomainWorkspace {
        name: "Undo".into(),
        collections: vec![DomainCollection {
            id: "api".into(),
            name: "API".into(),
            order: 3,
            groups: vec![
                DomainGroup {
                    id: "child".into(),
                    name: "Child".into(),
                    parent_id: Some("parent".into()),
                    order: 1,
                },
                DomainGroup {
                    id: "parent".into(),
                    name: "Parent".into(),
                    parent_id: None,
                    order: 0,
                },
                DomainGroup {
                    id: "grandchild".into(),
                    name: "Grandchild".into(),
                    parent_id: Some("child".into()),
                    order: 0,
                },
            ],
            requests: vec![
                location("deep", Some("grandchild"), 4),
                location("shallow", Some("parent"), 7),
                location("root", None, 2),
            ],
        }],
        ..Default::default()
    }
}
struct Harness {
    workspace: Arc<WorkspaceSession>,
    ops: WorkspaceOperations,
    store: Store,
    secrets: SessionSecrets,
}
impl Harness {
    fn new(workspace: DomainWorkspace) -> Self {
        let session = Arc::new(WorkspaceSession::default());
        session.replace(workspace);
        Self {
            ops: WorkspaceOperations::new(session.clone()),
            workspace: session,
            store: Store::default(),
            secrets: SessionSecrets::default(),
        }
    }
    async fn apply(&self, action: Action) -> WorkspaceMutation {
        self.ops
            .mutate(action, None, &self.store, &self.secrets, &Secrets)
            .await
    }
    async fn undo(&self, token: WorkspaceUndoToken) -> (WorkspaceMutation, WorkspaceUndoToken) {
        let reverse = self.ops.reverse_token(token.token).unwrap();
        let result = self
            .ops
            .undo(
                token.token,
                reverse.token,
                None,
                &self.store,
                &self.secrets,
                &Secrets,
            )
            .await;
        (result, reverse)
    }
}
fn sorted(mut w: DomainWorkspace) -> DomainWorkspace {
    for c in &mut w.collections {
        c.groups.sort_by(|a, b| a.id.cmp(&b.id));
        c.requests.sort_by(|a, b| a.request.id.cmp(&b.request.id));
    }
    w
}
fn document(id: &str, collection: Option<&str>, saved: bool) -> SessionDocument {
    let draft = DomainRequestDraft {
        id: id.into(),
        name: id.to_uppercase(),
        ..Default::default()
    };
    SessionDocument::new(
        id.into(),
        SessionKind::Http,
        collection.map(str::to_owned),
        None,
        draft.clone(),
        saved.then_some(draft),
        "",
    )
}
#[derive(Debug)]
struct NativeDocuments {
    values: Vec<SessionDocument>,
    saved: Mutex<Vec<WorkspaceSave>>,
}
#[async_trait]
impl super::documents::WorkspaceDocuments for NativeDocuments {
    async fn read(&self, id: String) -> Option<SessionDocument> {
        self.values.iter().find(|d| d.id == id).cloned()
    }
    async fn saved(&self, result: WorkspaceSave) {
        lock(&self.saved).push(result);
    }
}
impl WorkspaceOperations {
    async fn save_all(
        &self,
        values: Vec<SessionDocument>,
        _fallback: Option<String>,
        platform: &dyn WorkspacePlatform,
        secrets: &SessionSecrets,
        secret_platform: &dyn SessionSecretPlatform,
    ) -> Vec<WorkspaceSave> {
        let ids = values.iter().map(|d| d.id.clone()).collect();
        let documents = NativeDocuments {
            values,
            saved: Mutex::default(),
        };
        self.save_all_live(ids, platform, &documents, secrets, secret_platform)
            .await;
        lock(&documents.saved).clone()
    }
}
