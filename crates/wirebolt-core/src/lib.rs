#![forbid(unsafe_code)]

mod http_engine;
mod request;
mod storage;

pub use http_engine::{
    HttpEngine, HttpEngineConfig, HttpVersion, HttpVersionPolicy, Run, RunCancellation, RunError,
    RunErrorKind, RunHeader, RunOptions, StreamControl,
};
pub use request::{
    HeaderField, PreparedRequest, RequestDraft, RequestPreparationError, prepare_request,
};
pub use storage::{
    CURRENT_SCHEMA_VERSION, Collection, CollectionSnapshot, DocumentId, Environment,
    IdentifierError, MigrationReport, Request, RequestBody, RequestHeader, SaveOutcome, SecretName,
    StorageError, ValueSource, Workspace, WorkspaceDocument, WorkspaceSnapshot, WorkspaceStore,
};

pub const STREAM_ABI_VERSION: u32 = 1;

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
                stream_abi_version: 1,
            }
        );
    }
}
