#![forbid(unsafe_code)]

mod body_decoder;
mod export_engine;
pub use export_engine::{
    ExportError, GLOBAL_ENVIRONMENT_ID, export_legacy_v1_collection, export_legacy_v1_request,
    export_legacy_v1_workspace,
};
mod git_collaboration;
mod http_engine;
mod import_engine;
mod proxy;
mod request;
mod request_pipeline;
mod storage;
mod websocket;
pub use websocket::{WebSocketConnection, WebSocketError, WebSocketFrame};

pub use body_decoder::ContentEncoding;
pub use git_collaboration::{
    GitChange, GitDelta, GitError, GitErrorKind, GitOperation, GitOperationOutcome, GitStatus,
    GitWorkspace,
};
pub use http_engine::{
    DEFAULT_USER_AGENT, HttpEngine, HttpEngineConfig, HttpVersion, HttpVersionPolicy, Run,
    RunCancellation, RunError, RunErrorKind, RunHead, RunOptions, StreamControl,
    TlsConfigurationError,
};
pub use import_engine::{
    ImportEngine, ImportError, ImportFormat, ImportedCollection, ImportedEnvironment,
    ImportedGroup, ImportedRequest, ImportedRequestSettings, ImportedSecret, ImportedWorkspace,
    ParsedImport,
};
#[cfg(target_vendor = "apple")]
pub use proxy::{KeychainSecretResolver, KeychainSecretStore};
pub use proxy::{
    ManualProxy, NoSecrets, ProxyConfigurationError, ProxyConfigurationErrorKind, ProxyCredentials,
    ProxyDestination, ProxyDiagnostic, ProxyEndpoint, ProxyMode, ProxyModeKind, ProxyPolicy,
    ProxyProtocol, ProxyRoute, ProxyRouteDiagnostic, ProxySource, ResolvedProxy, ResolvedSecret,
    SecretResolutionError, SecretResolutionErrorKind, SecretResolver,
};
pub use request::{
    HeaderField, PreparedRequest, RequestDraft, RequestPreparationError, prepare_request,
};
pub use request_pipeline::{RequestIssue, RequestIssueKind, RequestPipeline, RequestPipelineError};
pub use storage::{
    ApiKeyPlacement, CURRENT_SCHEMA_VERSION, Collection, CollectionSnapshot, DocumentId,
    DocumentProblem, DocumentProblemKind, Environment, EnvironmentVariable, Group, IdentifierError,
    MigrationReport, MultipartPart, MultipartPartKind, Oauth2Configuration, Oauth2Grant, Request,
    RequestAuthentication, RequestBody, RequestHeader, RequestValueField, SaveOutcome, SecretName,
    StorageError, TransportSettings, ValueSource, Workspace, WorkspaceDocument, WorkspaceSnapshot,
    WorkspaceStore,
};

pub const STREAM_ABI_VERSION: u32 = 4;

#[derive(Debug, Eq, PartialEq)]
pub struct CoreHandshake {
    pub product: &'static str,
    pub core_version: &'static str,
    pub stream_abi_version: u32,
}

#[must_use]
pub const fn handshake() -> CoreHandshake {
    CoreHandshake {
        product: "Wirebolt",
        core_version: env!("CARGO_PKG_VERSION"),
        stream_abi_version: STREAM_ABI_VERSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_describes_the_core_contract() {
        assert_eq!(
            handshake(),
            CoreHandshake {
                product: "Wirebolt",
                core_version: "0.1.0",
                stream_abi_version: 4,
            }
        );
    }
}
