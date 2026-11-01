//! Native editors stage keystrokes locally. This platform seam captures each draft
//! at its save boundary and acknowledges its projection before the next save starts.
use super::io::WorkspacePlatform;
use super::operations::{WorkspaceOperations, WorkspaceSave};
use crate::{
    domain::DomainAuthentication,
    session::{
        secrets::{SessionSecretPlatform, SessionSecrets},
        tabs::SessionDocument,
    },
};
use async_trait::async_trait;
#[async_trait]
pub trait WorkspaceDocuments: Send + Sync {
    async fn read(&self, id: String) -> Option<SessionDocument>;
    async fn saved(&self, result: WorkspaceSave);
}
impl WorkspaceOperations {
    pub async fn save_all_live(
        &self,
        ids: Vec<String>,
        platform: &dyn WorkspacePlatform,
        documents: &dyn WorkspaceDocuments,
        secrets: &SessionSecrets,
        secret_platform: &dyn SessionSecretPlatform,
    ) -> bool {
        for id in ids {
            let Some(document) = documents.read(id).await else {
                continue;
            };
            let fallback = self.first_collection_id();
            let result = self
                .save(document, fallback, platform, secrets, secret_platform)
                .await;
            let succeeded = result.succeeded;
            documents.saved(result).await;
            if !succeeded {
                return false;
            }
        }
        true
    }
}
/// A saved snapshot updates the saved revision, never the live editor contents.
/// Apply a generated credential migration only if authentication is still unchanged.
#[must_use]
pub fn saved_authentication(
    current: DomainAuthentication,
    original: &DomainAuthentication,
    saved: &DomainAuthentication,
) -> DomainAuthentication {
    if &current == original {
        saved.clone()
    } else {
        current
    }
}
