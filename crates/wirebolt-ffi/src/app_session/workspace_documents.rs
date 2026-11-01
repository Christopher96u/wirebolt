//! Typed workspace documents application boundary.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned records at the boundary."
)]
use super::{Arc, Mutex, SessionDocument, WorkspaceSave};
use crate::lock;
#[derive(Debug, uniffi::Object)]
pub struct WorkspaceDocumentReply {
    sender: Mutex<Option<tokio::sync::oneshot::Sender<Option<SessionDocument>>>>,
}

#[uniffi::export]
impl WorkspaceDocumentReply {
    pub fn finish(&self, document: Option<SessionDocument>) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(document);
        }
    }
}

#[uniffi::export(with_foreign)]
pub trait WorkspaceDocumentPlatform: Send + Sync {
    fn read(&self, id: String, reply: Arc<WorkspaceDocumentReply>);
    fn saved(&self, result: WorkspaceSave, reply: Arc<WorkspaceDocumentReply>);
}

#[derive(uniffi::Object)]
pub struct AppWorkspaceDocuments {
    pub(super) inner: Arc<dyn WorkspaceDocumentPlatform>,
}

impl std::fmt::Debug for AppWorkspaceDocuments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppWorkspaceDocuments")
            .finish_non_exhaustive()
    }
}

#[uniffi::export]
impl AppWorkspaceDocuments {
    #[uniffi::constructor]
    pub fn foreign(platform: Arc<dyn WorkspaceDocumentPlatform>) -> Arc<Self> {
        Arc::new(Self { inner: platform })
    }
}

#[async_trait::async_trait]
impl wirebolt_app::session::workspace::documents::WorkspaceDocuments for AppWorkspaceDocuments {
    async fn read(&self, id: String) -> Option<SessionDocument> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.inner.read(
            id,
            Arc::new(WorkspaceDocumentReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        receiver.await.ok().flatten()
    }
    async fn saved(&self, result: WorkspaceSave) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.inner.saved(
            result,
            Arc::new(WorkspaceDocumentReply {
                sender: Mutex::new(Some(sender)),
            }),
        );
        let _ = receiver.await;
    }
}
