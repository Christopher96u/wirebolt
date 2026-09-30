use std::collections::BTreeMap;

use super::ImportedCollection;
use crate::{EnvironmentVariable, ProxyMode, Request, TransportSettings};

/// A parsed import that may create several collections and environments.
///
/// Most formats describe one collection; a Wirebolt workspace export also
/// carries its other collections, its environments and per-request transport.
#[derive(Clone, Debug)]
pub struct ImportedWorkspace {
    pub collections: Vec<ImportedCollection>,
    pub environments: Vec<ImportedEnvironment>,
    /// Proxy and transport settings keyed by [`super::ImportedRequest::source_id`].
    /// Requests without an entry keep the defaults of a new request.
    pub request_settings: BTreeMap<String, ImportedRequestSettings>,
}

impl From<ImportedCollection> for ImportedWorkspace {
    fn from(collection: ImportedCollection) -> Self {
        Self {
            collections: vec![collection],
            environments: Vec::new(),
            request_settings: BTreeMap::new(),
        }
    }
}

/// An environment to create on import. Values are literals or secret
/// reference names; secret material is never part of an import.
#[derive(Clone, Debug)]
pub struct ImportedEnvironment {
    pub name: String,
    /// Marks the environment whose variables apply to every request.
    pub global: bool,
    pub variables: Vec<EnvironmentVariable>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedRequestSettings {
    pub proxy_override: Option<ProxyMode>,
    pub transport: TransportSettings,
    pub inherits_workspace_transport: bool,
}

impl ImportedRequestSettings {
    pub fn apply(self, request: &mut Request) {
        request.proxy_override = self.proxy_override;
        request.transport = self.transport;
        request.inherits_workspace_transport = self.inherits_workspace_transport;
    }
}
