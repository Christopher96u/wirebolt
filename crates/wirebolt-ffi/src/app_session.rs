//! Typed session bridge, organized by the application modules it exposes.
use crate::{WorkspaceBridge, app_response::AppResponseBody};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};
use wirebolt_app::{
    domain::{
        DomainAuthentication, DomainBody, DomainCollection, DomainEnvironment, DomainGroup,
        DomainLocation, DomainOAuthConfiguration, DomainProxy, DomainRequestDraft, DomainRunInput,
        DomainTransport, DomainValueSource, DomainWorkspace, request::PushAvailability,
    },
    response::{ResponseHeaderValue, StoredCookie},
    session::{
        history::SessionHistoryEntry,
        persistence::{SessionCommandBatchResult, SessionWorkspaceDelta, SessionWorkspaceNodeKind},
        run::{
            DocumentRun, SessionCompletion, SessionFailure, SessionHead, SessionIssue,
            SessionPrepared, SessionPreparedBody, SessionPreparedHeader, SessionProxy,
            SessionProxySource, SessionRunEvent, SessionRunSnapshot, lock,
        },
        runtime as app_runtime,
        runtime::{
            RequestActivity, RunObserver, RuntimeAuthorizer, RuntimeContext, RuntimeInput,
            RuntimeOAuthSnapshot, RuntimeObserver, RuntimeOperationError, RuntimeProxyScope,
            RuntimeRequestError, RuntimeRequestKey, RuntimeRuns, RuntimeSendAction,
            RuntimeSettingsPlatform, RuntimeSnapshot, RuntimeState, RuntimeTokenReceipt,
            RuntimeTransport, RuntimeTransportSettings, RuntimeTransportSnapshot,
        },
        tabs::{
            AppSessionLayout, AppSessionLayoutGroup, SessionCloseScope, SessionDocument,
            SessionEditorGroup, SessionKind, SessionLayoutTab, SessionRestoreResult,
            SessionTabDocument, SessionTabSnapshot, SessionTabs,
        },
        websocket::{
            SessionSocket, SessionSocketMessage, SessionSocketNotice, SessionSocketSnapshot,
            SessionSocketStatus, decode_binary, message_body,
        },
        workspace::{
            SessionDeleteTarget, SessionDocumentEffect, SessionWorkspaceCommand,
            SessionWorkspaceEdit, SessionWorkspacePlan,
            actions::WorkspaceAction,
            files::{WorkspaceImportFile, WorkspaceSelectionProblem},
            interface::{
                WorkspaceDirtyClose, WorkspaceSidebarAction, WorkspaceSidebarResult,
                WorkspaceSidebarState, WorkspaceUnsavedPrompt,
            },
            io::{
                WorkspaceExport, WorkspaceFailure, WorkspaceGitAction, WorkspaceGitChange,
                WorkspaceGitDelta, WorkspaceGitOperation, WorkspaceGitOutcome, WorkspaceGitStatus,
                WorkspaceIO, WorkspaceImportSummary, WorkspaceImportedEnvironment, WorkspaceReply,
            },
            lifecycle::{WorkspaceClose, WorkspaceDiscard, WorkspaceRestore},
            operations::{
                WorkspaceImport, WorkspaceLoad, WorkspaceMutation, WorkspacePresentation,
                WorkspaceRepositoryProblem, WorkspaceSave, WorkspaceUndoToken,
            },
        },
    },
};
mod run;
pub use run::*;
mod tabs;
pub use tabs::*;
mod secrets;
mod websocket;
mod workspace;
pub use secrets::*;
mod history;
mod workspace_io;
pub use workspace_io::*;
mod interface;
mod workspace_documents;
pub use workspace_documents::*;
mod runtime;
pub use runtime::*;
mod serialization;
