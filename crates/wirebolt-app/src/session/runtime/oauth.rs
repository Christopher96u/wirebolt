//! OAuth command state. Native interfaces only project receipts and localized failures.
use super::{
    RuntimeAuthorizer, RuntimeObserver, RuntimeOperationError, RuntimeTokenReceipt, acquire_token,
};
use crate::lock;
use crate::session::{
    secrets::{SessionSecretPlatform, SessionSecrets},
    tabs::SessionDocument,
};
use std::{collections::HashMap, sync::Mutex};
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeOAuthSnapshot {
    pub busy: bool,
    pub failure: Option<RuntimeOperationError>,
    pub receipts: HashMap<String, RuntimeTokenReceipt>,
}
#[derive(Debug, Default)]
pub struct RuntimeOAuth {
    state: Mutex<RuntimeOAuthSnapshot>,
}
impl RuntimeOAuth {
    #[must_use]
    pub fn snapshot(&self) -> RuntimeOAuthSnapshot {
        lock(&self.state).clone()
    }
    pub fn reset(&self) {
        lock(&self.state).receipts.clear();
    }
    pub async fn acquire(
        &self,
        secrets: &SessionSecrets,
        document: &Mutex<SessionDocument>,
        platform: &dyn SessionSecretPlatform,
        authorizer: &dyn RuntimeAuthorizer,
        observer: &dyn RuntimeObserver,
    ) {
        {
            let mut state = lock(&self.state);
            state.busy = true;
            state.failure = None;
        }
        observer.changed();
        let result = acquire_token(secrets, document, platform, authorizer, Some(observer)).await;
        let id = lock(document).id.clone();
        {
            let mut state = lock(&self.state);
            state.busy = false;
            match result {
                Ok(Some(receipt)) => {
                    state.receipts.insert(id, receipt);
                }
                Ok(None) => {}
                Err(error) => state.failure = Some(error),
            }
        }
        observer.changed();
    }
}
