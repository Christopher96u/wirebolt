uniffi::setup_scaffolding!();

use std::{
    collections::BTreeMap,
    error::Error,
    ffi::c_void,
    fmt,
    path::PathBuf,
    ptr,
    sync::{Arc, LazyLock, Mutex, mpsc},
    time::Duration,
};

use serde::{Deserialize, Serialize};
#[cfg(not(target_vendor = "apple"))]
use wirebolt_core::NoSecrets;
use wirebolt_core::{
    Collection, DocumentId, DocumentProblem, DocumentProblemKind, Environment, GitChange, GitDelta,
    GitError, GitErrorKind, GitOperation, GitOperationOutcome, GitStatus, GitWorkspace, HttpEngine,
    HttpEngineConfig, HttpVersion, ProxyMode, ProxyPolicy, Request, RequestAuthentication,
    RequestBody, RequestHeader, RequestIssue, RequestIssueKind, RequestPipeline, RequestValueField,
    ResolvedProxy, RunCancellation, RunError, RunErrorKind, RunHead, RunOptions, SecretName,
    SecretResolver, StreamControl, ValueSource, Workspace, WorkspaceDocument, WorkspaceSnapshot,
    WorkspaceStore,
};

#[derive(Debug, Eq, PartialEq, uniffi::Record)]
pub struct CoreHandshake {
    pub product: String,
    pub core_version: String,
    pub stream_abi_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct HeaderField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct BridgeRequestDraft {
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderField>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct PreparedRequestSummary {
    pub method: String,
    pub url: String,
    pub header_count: u64,
    pub body_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Error)]
pub enum RequestPreparationError {
    InvalidRequest { reason: String },
}

impl fmt::Display for RequestPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { reason } => formatter.write_str(reason),
        }
    }
}

impl Error for RequestPreparationError {}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Error)]
pub enum WorkspaceBridgeError {
    OperationFailed { reason: String },
}

impl fmt::Display for WorkspaceBridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationFailed { reason } => formatter.write_str(reason),
        }
    }
}

impl Error for WorkspaceBridgeError {}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Error)]
pub enum GitBridgeError {
    OperationFailed { kind: String, reason: String },
}

impl fmt::Display for GitBridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationFailed { reason, .. } => formatter.write_str(reason),
        }
    }
}

impl Error for GitBridgeError {}

#[derive(Debug, uniffi::Object)]
pub struct WorkspaceBridge {
    store: WorkspaceStore,
}

#[derive(Debug, Serialize)]
struct WorkspaceSnapshotDocument<'a> {
    name: &'a str,
    proxy: &'a Option<ProxyMode>,
    collections: Vec<CollectionSnapshotDocument<'a>>,
    environments: Vec<EnvironmentSnapshotDocument<'a>>,
    problems: Vec<DocumentProblemDocument<'a>>,
}

#[derive(Debug, Serialize)]
struct CollectionSnapshotDocument<'a> {
    id: &'a str,
    name: &'a str,
    requests: Vec<RequestSnapshotDocument<'a>>,
}

#[derive(Debug, Serialize)]
struct RequestSnapshotDocument<'a> {
    id: &'a str,
    name: &'a str,
    method: &'a str,
    url: &'a str,
    query: &'a [RequestValueField],
    headers: &'a [RequestHeader],
    authentication: &'a RequestAuthentication,
    body: &'a RequestBody,
    proxy: &'a Option<ProxyMode>,
}

#[derive(Debug, Serialize)]
struct EnvironmentSnapshotDocument<'a> {
    id: &'a str,
    name: &'a str,
    variables: &'a BTreeMap<String, ValueSource>,
}

#[derive(Debug, Serialize)]
struct DocumentProblemDocument<'a> {
    path: String,
    kind: &'static str,
    reason: &'a str,
}

#[derive(Debug, Serialize)]
struct GitStatusDocument<'a> {
    branch: &'a Option<String>,
    upstream: &'a Option<String>,
    ahead: u64,
    behind: u64,
    revision: &'a Option<String>,
    changes: Vec<GitChangeDocument<'a>>,
}

#[derive(Debug, Serialize)]
struct GitChangeDocument<'a> {
    path: &'a str,
    previous_path: &'a Option<String>,
    staged: &'static str,
    unstaged: &'static str,
    conflicted: bool,
}

#[derive(Debug, Serialize)]
struct GitOperationDocument<'a> {
    outcome: &'static str,
    revision: &'a Option<String>,
    status: GitStatusDocument<'a>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedRequestDocument {
    id: String,
    name: String,
    method: String,
    url: String,
    #[serde(default)]
    query: Vec<RequestValueField>,
    #[serde(default)]
    headers: Vec<RequestHeader>,
    #[serde(default)]
    authentication: RequestAuthentication,
    #[serde(default = "empty_body")]
    body: RequestBody,
    #[serde(default)]
    proxy: Option<ProxyMode>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedEnvironmentDocument {
    id: String,
    name: String,
    #[serde(default)]
    variables: BTreeMap<String, ValueSource>,
}

#[uniffi::export]
impl WorkspaceBridge {
    #[uniffi::constructor]
    /// Opens an existing workspace or creates an empty one.
    ///
    /// Documents that cannot be migrated are skipped and reported through
    /// the snapshot's `problems`; they never prevent the workspace from
    /// opening.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] when storage cannot be initialized.
    pub fn open_or_create(path: String, name: String) -> Result<Arc<Self>, WorkspaceBridgeError> {
        let root = PathBuf::from(path);
        let store = if root.join("wirebolt.toml").is_file() {
            WorkspaceStore::open(root)
        } else {
            WorkspaceStore::create(root, &Workspace::new(name))
        }
        .map_err(|_| WorkspaceBridgeError::operation("workspace could not be opened"))?;
        store
            .migrate()
            .map_err(|_| WorkspaceBridgeError::operation("workspace could not be migrated"))?;
        Ok(Arc::new(Self { store }))
    }

    /// Returns one stable JSON snapshot without exposing filesystem layout.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] when the workspace cannot be loaded.
    pub fn snapshot_json(&self) -> Result<String, WorkspaceBridgeError> {
        let snapshot = self
            .store
            .load()
            .map_err(|_| WorkspaceBridgeError::operation("workspace could not be loaded"))?;
        serde_json::to_string(&WorkspaceSnapshotDocument::from(&snapshot))
            .map_err(|_| WorkspaceBridgeError::operation("workspace snapshot could not be encoded"))
    }

    /// Creates or updates a collection.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for invalid IDs or storage failures.
    pub fn save_collection(&self, id: String, name: String) -> Result<(), WorkspaceBridgeError> {
        let id = document_id(id)?;
        self.store
            .save(&WorkspaceDocument::Collection(Collection::new(id, name)))
            .map(|_| ())
            .map_err(|_| WorkspaceBridgeError::operation("collection could not be saved"))
    }

    /// Creates or updates one request from a bridge JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for invalid input or storage failures.
    pub fn save_request(
        &self,
        collection_id: String,
        request_json: &str,
    ) -> Result<(), WorkspaceBridgeError> {
        let collection_id = document_id(collection_id)?;
        let document: SavedRequestDocument = serde_json::from_str(request_json)
            .map_err(|_| WorkspaceBridgeError::operation("request document is invalid"))?;
        let mut request = Request::new(
            document_id(document.id)?,
            document.name,
            document.method,
            document.url,
        );
        request.query = document.query;
        request.headers = document.headers;
        request.authentication = document.authentication;
        request.body = document.body;
        request.proxy_override = document.proxy;
        self.store
            .save(&WorkspaceDocument::Request {
                collection_id,
                request,
            })
            .map(|_| ())
            .map_err(|_| WorkspaceBridgeError::operation("request could not be saved"))
    }

    /// Creates or updates one environment from a bridge JSON document.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for invalid input or storage failures.
    pub fn save_environment(&self, environment_json: &str) -> Result<(), WorkspaceBridgeError> {
        let document: SavedEnvironmentDocument = serde_json::from_str(environment_json)
            .map_err(|_| WorkspaceBridgeError::operation("environment document is invalid"))?;
        let environment =
            Environment::new(document_id(document.id)?, document.name, document.variables);
        self.store
            .save(&WorkspaceDocument::Environment(environment))
            .map(|_| ())
            .map_err(|_| WorkspaceBridgeError::operation("environment could not be saved"))
    }

    /// Stores secret material in Keychain, never in the workspace.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBridgeError`] for invalid names or Keychain failures.
    pub fn save_secret(&self, name: String, value: &str) -> Result<(), WorkspaceBridgeError> {
        let name = SecretName::new(name)
            .map_err(|_| WorkspaceBridgeError::operation("secret name is invalid"))?;
        #[cfg(target_vendor = "apple")]
        {
            wirebolt_core::KeychainSecretStore::default()
                .save(&name, value)
                .map_err(|_| WorkspaceBridgeError::operation("secret could not be saved"))?;
            clear_manual_http_engines();
            Ok(())
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (name, value);
            Err(WorkspaceBridgeError::operation(
                "secret storage is unavailable",
            ))
        }
    }

    /// Returns a read-only Git status snapshot without fetching.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] when this workspace is not a repository or
    /// Git cannot inspect it.
    pub fn git_status_json(&self) -> Result<String, GitBridgeError> {
        let status = self.git_workspace()?.status()?;
        encode_git_document(&GitStatusDocument::from(&status))
    }

    /// Pulls the configured upstream explicitly and leaves conflicts intact.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] when Git rejects or cannot run the pull.
    pub fn git_pull_json(&self) -> Result<String, GitBridgeError> {
        let operation = self.git_workspace()?.pull()?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Commits only Wirebolt-managed workspace documents.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] for invalid messages, missing Git identity,
    /// or another failed Git operation.
    pub fn git_commit_json(&self, message: &str) -> Result<String, GitBridgeError> {
        let operation = self.git_workspace()?.commit(message)?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }

    /// Pushes the current branch using the user's Git credentials.
    ///
    /// # Errors
    ///
    /// Returns [`GitBridgeError`] when the repository has no usable branch or
    /// remote, authentication fails, or the update is rejected.
    pub fn git_push_json(&self) -> Result<String, GitBridgeError> {
        let operation = self.git_workspace()?.push()?;
        encode_git_document(&GitOperationDocument::from(&operation))
    }
}

impl WorkspaceBridge {
    fn git_workspace(&self) -> Result<GitWorkspace, GitBridgeError> {
        GitWorkspace::open(self.store.root()).map_err(Into::into)
    }
}

impl WorkspaceBridgeError {
    fn operation(reason: &str) -> Self {
        Self::OperationFailed {
            reason: reason.to_owned(),
        }
    }
}

impl From<GitError> for GitBridgeError {
    fn from(error: GitError) -> Self {
        Self::OperationFailed {
            kind: git_error_kind(error.kind).to_owned(),
            reason: error.to_string(),
        }
    }
}

impl<'a> From<&'a GitStatus> for GitStatusDocument<'a> {
    fn from(status: &'a GitStatus) -> Self {
        Self {
            branch: &status.branch,
            upstream: &status.upstream,
            ahead: status.ahead,
            behind: status.behind,
            revision: &status.revision,
            changes: status.changes.iter().map(Into::into).collect(),
        }
    }
}

impl<'a> From<&'a GitChange> for GitChangeDocument<'a> {
    fn from(change: &'a GitChange) -> Self {
        Self {
            path: &change.path,
            previous_path: &change.previous_path,
            staged: git_delta(change.staged),
            unstaged: git_delta(change.unstaged),
            conflicted: change.conflicted,
        }
    }
}

impl<'a> From<&'a GitOperation> for GitOperationDocument<'a> {
    fn from(operation: &'a GitOperation) -> Self {
        Self {
            outcome: git_operation_outcome(operation.outcome),
            revision: &operation.revision,
            status: GitStatusDocument::from(&operation.status),
        }
    }
}

fn encode_git_document(document: &impl Serialize) -> Result<String, GitBridgeError> {
    serde_json::to_string(document).map_err(|_| GitBridgeError::OperationFailed {
        kind: "encoding".to_owned(),
        reason: "Git result could not be encoded".to_owned(),
    })
}

const fn git_delta(delta: GitDelta) -> &'static str {
    match delta {
        GitDelta::None => "none",
        GitDelta::Added => "added",
        GitDelta::Modified => "modified",
        GitDelta::Deleted => "deleted",
        GitDelta::Renamed => "renamed",
        GitDelta::Copied => "copied",
        GitDelta::TypeChanged => "type_changed",
        GitDelta::Untracked => "untracked",
        GitDelta::Unmerged => "unmerged",
    }
}

const fn git_operation_outcome(outcome: GitOperationOutcome) -> &'static str {
    match outcome {
        GitOperationOutcome::NothingToCommit => "nothing_to_commit",
        GitOperationOutcome::Committed => "committed",
        GitOperationOutcome::Updated => "updated",
        GitOperationOutcome::UpToDate => "up_to_date",
        GitOperationOutcome::Pushed => "pushed",
        GitOperationOutcome::Conflicted => "conflicted",
    }
}

const fn git_error_kind(kind: GitErrorKind) -> &'static str {
    match kind {
        GitErrorKind::GitUnavailable => "git_unavailable",
        GitErrorKind::NotRepository => "not_repository",
        GitErrorKind::WorkspaceNotRepositoryRoot => "workspace_not_repository_root",
        GitErrorKind::CommandFailed => "command_failed",
        GitErrorKind::InvalidOutput => "invalid_output",
        GitErrorKind::InvalidCommitMessage => "invalid_commit_message",
        GitErrorKind::IdentityMissing => "identity_missing",
        GitErrorKind::AuthenticationRequired => "authentication_required",
        GitErrorKind::DirtyWorkspace => "dirty_workspace",
        GitErrorKind::MissingUpstream => "missing_upstream",
        GitErrorKind::MissingRemote => "missing_remote",
        GitErrorKind::DetachedHead => "detached_head",
        GitErrorKind::TimedOut => "timed_out",
    }
}

const fn document_problem_kind(kind: DocumentProblemKind) -> &'static str {
    match kind {
        DocumentProblemKind::InvalidToml => "invalid_toml",
        DocumentProblemKind::InvalidDocument => "invalid_document",
        DocumentProblemKind::UnsupportedSchema => "unsupported_schema",
        DocumentProblemKind::DocumentTooLarge => "document_too_large",
        DocumentProblemKind::Io => "io",
    }
}

fn document_id(value: String) -> Result<DocumentId, WorkspaceBridgeError> {
    DocumentId::new(value).map_err(|_| WorkspaceBridgeError::operation("document ID is invalid"))
}

impl<'a> From<&'a WorkspaceSnapshot> for WorkspaceSnapshotDocument<'a> {
    fn from(snapshot: &'a WorkspaceSnapshot) -> Self {
        Self {
            name: &snapshot.workspace.name,
            proxy: &snapshot.workspace.proxy,
            collections: snapshot
                .collections
                .iter()
                .map(|snapshot| CollectionSnapshotDocument {
                    id: snapshot.collection.id.as_str(),
                    name: &snapshot.collection.name,
                    requests: snapshot
                        .requests
                        .iter()
                        .map(RequestSnapshotDocument::from)
                        .collect(),
                })
                .collect(),
            environments: snapshot
                .environments
                .iter()
                .map(|environment| EnvironmentSnapshotDocument {
                    id: environment.id.as_str(),
                    name: &environment.name,
                    variables: &environment.variables,
                })
                .collect(),
            problems: snapshot
                .problems
                .iter()
                .map(DocumentProblemDocument::from)
                .collect(),
        }
    }
}

impl<'a> From<&'a DocumentProblem> for DocumentProblemDocument<'a> {
    fn from(problem: &'a DocumentProblem) -> Self {
        Self {
            path: problem.path.to_string_lossy().into_owned(),
            kind: document_problem_kind(problem.kind),
            reason: &problem.reason,
        }
    }
}

impl<'a> From<&'a Request> for RequestSnapshotDocument<'a> {
    fn from(request: &'a Request) -> Self {
        Self {
            id: request.id.as_str(),
            name: &request.name,
            method: &request.method,
            url: &request.url,
            query: &request.query,
            headers: &request.headers,
            authentication: &request.authentication,
            body: &request.body,
            proxy: &request.proxy_override,
        }
    }
}

#[uniffi::export]
#[must_use]
pub fn core_handshake() -> CoreHandshake {
    wirebolt_core::handshake().into()
}

#[uniffi::export]
/// Prepares one coarse-grained request across the Swift–Rust boundary.
///
/// # Errors
///
/// Returns [`RequestPreparationError`] when the Rust core rejects any request
/// component.
pub fn prepare_request(
    draft: BridgeRequestDraft,
) -> Result<PreparedRequestSummary, RequestPreparationError> {
    let prepared = wirebolt_core::prepare_request(draft.into()).map_err(|error| {
        RequestPreparationError::InvalidRequest {
            reason: error.to_string(),
        }
    })?;

    Ok(PreparedRequestSummary {
        method: prepared.method().to_string(),
        url: prepared.url().to_string(),
        header_count: prepared.headers().len().try_into().unwrap_or(u64::MAX),
        body_bytes: prepared.body().len().try_into().unwrap_or(u64::MAX),
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn wirebolt_stream_abi_version() -> u32 {
    wirebolt_core::STREAM_ABI_VERSION
}

/// Starts the shared runtime and warms the direct and system HTTP engines in
/// the background, so the first request does not pay for TLS and proxy
/// discovery. Returns 1 when the runtime is available.
#[unsafe(no_mangle)]
pub extern "C" fn wirebolt_runtime_warmup() -> u8 {
    let Some(runtime) = SHARED_RUNTIME.as_ref() else {
        return 0;
    };
    runtime.spawn_blocking(|| {
        for mode in [ProxyMode::Direct, ProxyMode::System] {
            let proxy = ProxyPolicy::with_workspace(mode).resolve(None);
            let _ = shared_http_engine(&proxy, &wirebolt_core::NoSecrets);
        }
    });
    1
}

/// Drops every cached HTTP engine so the next run rebuilds its client. Call
/// this when the network configuration changes: the system proxy settings
/// are captured once per engine, and pooled connections may be dead.
#[unsafe(no_mangle)]
pub extern "C" fn wirebolt_reset_engines() {
    reset_http_engines();
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WireboltRunCallbacks {
    pub on_head: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
    pub on_chunk: Option<extern "C" fn(*mut c_void, *const u8, usize) -> u8>,
    pub on_complete: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
    pub on_error: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
}

#[derive(Debug)]
pub struct WireboltRunSession {
    cancellation: RunCancellation,
    completion: Mutex<Option<mpsc::Receiver<()>>>,
}

#[derive(Debug, Default)]
struct SharedHttpEngines {
    system: Mutex<Option<Arc<HttpEngine>>>,
    direct: Mutex<Option<Arc<HttpEngine>>>,
    manual: Mutex<Vec<(ProxyMode, Arc<HttpEngine>)>>,
}

static SHARED_HTTP_ENGINES: LazyLock<SharedHttpEngines> = LazyLock::new(SharedHttpEngines::default);
static SHARED_RUNTIME: LazyLock<Option<tokio::runtime::Runtime>> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("wirebolt-http")
        .enable_all()
        .build()
        .ok()
});

/// Timeouts of zero mean "no limit"; the engine still needs a finite
/// deadline, so a day stands in for infinity.
const UNLIMITED_TIMEOUT: Duration = Duration::from_hours(24);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunInput {
    method: String,
    url: String,
    #[serde(default)]
    query: Vec<RequestValueField>,
    #[serde(default)]
    headers: Vec<RequestHeader>,
    #[serde(default)]
    authentication: RequestAuthentication,
    #[serde(default = "empty_body")]
    body: RequestBody,
    #[serde(default)]
    variables: BTreeMap<String, ValueSource>,
    #[serde(default)]
    workspace_proxy: Option<ProxyMode>,
    #[serde(default)]
    request_proxy: Option<ProxyMode>,
    #[serde(default = "default_total_timeout_ms")]
    total_timeout_ms: u64,
    #[serde(default = "default_read_timeout_ms")]
    read_timeout_ms: u64,
    #[serde(default)]
    max_response_bytes: Option<u64>,
    #[serde(default = "default_decode_content")]
    decode_content: bool,
}

const fn empty_body() -> RequestBody {
    RequestBody::Empty
}

const fn default_total_timeout_ms() -> u64 {
    30_000
}

const fn default_read_timeout_ms() -> u64 {
    10_000
}

const fn default_decode_content() -> bool {
    true
}

fn timeout_from_millis(milliseconds: u64) -> Duration {
    if milliseconds == 0 {
        UNLIMITED_TIMEOUT
    } else {
        Duration::from_millis(milliseconds)
    }
}

#[derive(Debug, Serialize)]
struct ResponseHeadDocument<'a> {
    status: u16,
    version: &'static str,
    headers: Vec<ResponseHeaderDocument<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_encoding: Option<&'static str>,
    warnings: &'a [RequestIssueDocument],
    time_to_headers_ns: u64,
}

#[derive(Debug, Serialize)]
struct ResponseHeaderDocument<'a> {
    name: &'a str,
    value: std::borrow::Cow<'a, str>,
}

#[derive(Debug, Serialize)]
struct RunCompleteDocument {
    bytes_received: u64,
    bytes_decoded: u64,
    total_time_ns: u64,
}

#[derive(Debug, Serialize)]
struct RunFailureDocument {
    kind: &'static str,
    issues: Vec<RequestIssueDocument>,
}

#[derive(Debug, Serialize)]
struct RequestIssueDocument {
    path: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
}

impl From<RequestIssue> for RequestIssueDocument {
    fn from(issue: RequestIssue) -> Self {
        Self {
            path: issue.path,
            kind: request_issue_kind(issue.kind),
            reference: issue.reference,
        }
    }
}

/// Starts one asynchronous HTTP run.
///
/// The input bytes are copied before this function returns. The callback
/// context and callback functions must remain valid until a terminal callback
/// has returned. Callback byte buffers are borrowed only during the callback.
/// Do not call [`wirebolt_run_free`] from inside a callback.
///
/// # Safety
///
/// `input_json` must point to `input_length` readable bytes, unless the length
/// is zero. `context` must satisfy the lifetime contract above. The returned
/// pointer must be released exactly once with [`wirebolt_run_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_run_start(
    input_json: *const u8,
    input_length: usize,
    callbacks: WireboltRunCallbacks,
    context: *mut c_void,
) -> *mut WireboltRunSession {
    if callbacks.on_head.is_none()
        || callbacks.on_chunk.is_none()
        || callbacks.on_complete.is_none()
        || callbacks.on_error.is_none()
        || (input_json.is_null() && input_length != 0)
    {
        return ptr::null_mut();
    }

    let input = if input_length == 0 {
        Vec::new()
    } else {
        // SAFETY: The caller promises a readable buffer for `input_length` bytes.
        unsafe { std::slice::from_raw_parts(input_json, input_length) }.to_vec()
    };
    let Some(runtime) = SHARED_RUNTIME.as_ref() else {
        return ptr::null_mut();
    };
    let cancellation = RunCancellation::new();
    let worker_cancellation = cancellation.clone();
    let context_address = context as usize;
    let (completion_sender, completion) = mpsc::sync_channel(1);
    runtime.spawn(async move {
        // The run executes in its own task so that a panic anywhere inside
        // it still ends with a terminal callback instead of a silent hang.
        let worker = tokio::spawn(execute_run(
            input,
            worker_cancellation,
            callbacks,
            context_address,
        ));
        if worker.await.is_err() {
            emit_failure(
                &callbacks,
                context_address,
                &RunFailureDocument::new("internal"),
            );
        }
        let _ = completion_sender.send(());
    });

    Box::into_raw(Box::new(WireboltRunSession {
        cancellation,
        completion: Mutex::new(Some(completion)),
    }))
}

/// Requests cancellation of an active run.
///
/// # Safety
///
/// `session` must be null or a live pointer returned by
/// [`wirebolt_run_start`] that has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_run_cancel(session: *mut WireboltRunSession) {
    // SAFETY: The caller owns a live session for the duration of this call.
    if let Some(session) = unsafe { session.as_ref() } {
        session.cancellation.cancel();
    }
}

/// Cancels, joins, and releases one run session.
///
/// # Safety
///
/// `session` must be null or a live pointer returned by
/// [`wirebolt_run_start`]. It must be passed to this function at most once and
/// this function must not be called from a run callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_run_free(session: *mut WireboltRunSession) {
    if session.is_null() {
        return;
    }
    // SAFETY: Ownership is transferred back exactly once by the caller.
    let session = unsafe { Box::from_raw(session) };
    session.cancellation.cancel();
    let completion = session
        .completion
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(completion) = completion {
        let _ = completion.recv();
    }
}

async fn execute_run(
    input: Vec<u8>,
    cancellation: RunCancellation,
    callbacks: WireboltRunCallbacks,
    context: usize,
) {
    let parsed = serde_json::from_slice::<RunInput>(&input);
    let Ok(input) = parsed else {
        emit_failure(
            &callbacks,
            context,
            &RunFailureDocument::new("invalid_input"),
        );
        return;
    };

    #[cfg(target_vendor = "apple")]
    execute_run_with_secrets(
        input,
        cancellation,
        callbacks,
        context,
        wirebolt_core::KeychainSecretResolver::default(),
    )
    .await;
    #[cfg(not(target_vendor = "apple"))]
    execute_run_with_secrets(input, cancellation, callbacks, context, NoSecrets).await;
}

async fn execute_run_with_secrets<R>(
    input: RunInput,
    cancellation: RunCancellation,
    callbacks: WireboltRunCallbacks,
    context: usize,
    secrets: R,
) where
    R: SecretResolver + Clone + Send + 'static,
{
    let options = RunOptions {
        total_timeout: timeout_from_millis(input.total_timeout_ms),
        read_timeout: timeout_from_millis(input.read_timeout_ms),
        max_response_bytes: input.max_response_bytes,
        decode_content: input.decode_content,
    };
    // Secret lookups can block on Keychain (even prompting the user) and
    // building a client loads TLS and proxy state, so neither runs on the
    // two async workers that stream every other response.
    let prepared = tokio::task::spawn_blocking(move || prepare_run(input, &secrets)).await;
    let (prepared, engine) = match prepared {
        Ok(Ok(prepared)) => prepared,
        Ok(Err(failure)) => {
            emit_failure(&callbacks, context, &failure);
            return;
        }
        Err(_) => {
            emit_failure(&callbacks, context, &RunFailureDocument::new("internal"));
            return;
        }
    };
    let warnings: Vec<RequestIssueDocument> = prepared
        .warnings()
        .iter()
        .cloned()
        .map(Into::into)
        .collect();
    let result = engine
        .run_observed(
            prepared,
            options,
            &cancellation,
            |head| emit_head(&callbacks, context, head, &warnings),
            |chunk| {
                let should_continue = callbacks.on_chunk.map_or(0, |callback| {
                    callback(context as *mut c_void, chunk.as_ptr(), chunk.len())
                });
                if should_continue == 0 {
                    StreamControl::Stop
                } else {
                    StreamControl::Continue
                }
            },
        )
        .await;
    match result {
        Ok(run) => emit_json(
            callbacks.on_complete,
            context,
            &RunCompleteDocument {
                bytes_received: run.bytes_received,
                bytes_decoded: run.bytes_decoded,
                total_time_ns: duration_ns(run.total_time),
            },
        ),
        Err(error) => emit_failure(
            &callbacks,
            context,
            &RunFailureDocument::from_run_error(&error),
        ),
    }
}

type PreparedRun = (wirebolt_core::PreparedRequest, Arc<HttpEngine>);

fn prepare_run<R: SecretResolver + ?Sized>(
    input: RunInput,
    secrets: &R,
) -> Result<PreparedRun, RunFailureDocument> {
    let mut request = Request::new(
        DocumentId::new("draft").expect("static document ID"),
        "Draft",
        input.method,
        input.url,
    );
    request.query = input.query;
    request.headers = input.headers;
    request.authentication = input.authentication;
    request.body = input.body;
    request.proxy_override = input.request_proxy;
    let environment = Environment::new(
        DocumentId::new("active").expect("static document ID"),
        "Active".to_owned(),
        input.variables,
    );
    let prepared = RequestPipeline::new(Some(&environment), secrets)
        .prepare(&request)
        .map_err(|error| RunFailureDocument {
            kind: "invalid_request",
            issues: error.issues.into_iter().map(Into::into).collect(),
        })?;
    let proxy = ProxyPolicy::new(input.workspace_proxy).resolve(request.proxy_override.as_ref());
    let engine = shared_http_engine(&proxy, secrets)
        .map_err(|error| RunFailureDocument::from_run_error(&error))?;
    Ok((prepared, engine))
}

fn shared_http_engine<R: SecretResolver + ?Sized>(
    proxy: &ResolvedProxy,
    secrets: &R,
) -> Result<Arc<HttpEngine>, RunError> {
    let slot = match proxy.mode() {
        ProxyMode::System => &SHARED_HTTP_ENGINES.system,
        ProxyMode::Direct => &SHARED_HTTP_ENGINES.direct,
        ProxyMode::Manual(_) => return shared_manual_http_engine(proxy, secrets),
    };

    let mut slot = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(engine) = slot.as_ref() {
        return Ok(Arc::clone(engine));
    }
    let engine = Arc::new(HttpEngine::with_proxy(
        &HttpEngineConfig::default(),
        proxy,
        secrets,
    )?);
    *slot = Some(Arc::clone(&engine));
    Ok(engine)
}

/// Manual engines are keyed by proxy mode alone: the same configuration
/// reached through a workspace policy or a request override shares one pool.
fn shared_manual_http_engine<R: SecretResolver + ?Sized>(
    proxy: &ResolvedProxy,
    secrets: &R,
) -> Result<Arc<HttpEngine>, RunError> {
    const MAX_CACHED_MANUAL_ENGINES: usize = 8;

    let mut engines = SHARED_HTTP_ENGINES
        .manual
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, engine)) = engines
        .iter()
        .find(|(candidate, _)| candidate == proxy.mode())
    {
        return Ok(Arc::clone(engine));
    }
    let engine = Arc::new(HttpEngine::with_proxy(
        &HttpEngineConfig::default(),
        proxy,
        secrets,
    )?);
    if engines.len() == MAX_CACHED_MANUAL_ENGINES {
        engines.remove(0);
    }
    engines.push((proxy.mode().clone(), Arc::clone(&engine)));
    Ok(engine)
}

fn clear_manual_http_engines() {
    SHARED_HTTP_ENGINES
        .manual
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

fn reset_http_engines() {
    clear_manual_http_engines();
    for slot in [&SHARED_HTTP_ENGINES.system, &SHARED_HTTP_ENGINES.direct] {
        slot.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}

fn emit_head(
    callbacks: &WireboltRunCallbacks,
    context: usize,
    head: &RunHead,
    warnings: &[RequestIssueDocument],
) {
    emit_json(
        callbacks.on_head,
        context,
        &ResponseHeadDocument {
            status: head.status,
            version: http_version(head.version),
            headers: head
                .headers
                .iter()
                .map(|(name, value)| ResponseHeaderDocument {
                    name: name.as_str(),
                    value: String::from_utf8_lossy(value.as_bytes()),
                })
                .collect(),
            remote_addr: head.remote_addr.map(|address| address.to_string()),
            content_encoding: head
                .content_encoding
                .map(wirebolt_core::ContentEncoding::token),
            warnings,
            time_to_headers_ns: duration_ns(head.time_to_headers),
        },
    );
}

fn emit_failure(callbacks: &WireboltRunCallbacks, context: usize, failure: &RunFailureDocument) {
    emit_json(callbacks.on_error, context, failure);
}

fn emit_json<T: Serialize>(
    callback: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
    context: usize,
    value: &T,
) {
    let Some(callback) = callback else { return };
    let Ok(json) = serde_json::to_vec(value) else {
        return;
    };
    callback(context as *mut c_void, json.as_ptr(), json.len());
}

impl RunFailureDocument {
    const fn new(kind: &'static str) -> Self {
        Self {
            kind,
            issues: Vec::new(),
        }
    }

    fn from_run_error(error: &RunError) -> Self {
        Self::new(run_error_kind(error.kind()))
    }
}

const fn run_error_kind(kind: RunErrorKind) -> &'static str {
    match kind {
        RunErrorKind::Cancelled => "cancelled",
        RunErrorKind::TotalTimeout => "total_timeout",
        RunErrorKind::ReadTimeout => "read_timeout",
        RunErrorKind::ConnectTimeout => "connect_timeout",
        RunErrorKind::StreamStopped => "stream_stopped",
        RunErrorKind::ResponseTooLarge => "response_too_large",
        RunErrorKind::Connection => "connection",
        RunErrorKind::Request => "request",
        RunErrorKind::ResponseBody => "response_body",
        RunErrorKind::Proxy => "proxy",
        RunErrorKind::Transport => "transport",
    }
}

const fn request_issue_kind(kind: RequestIssueKind) -> &'static str {
    match kind {
        RequestIssueKind::InvalidMethod => "invalid_method",
        RequestIssueKind::InvalidUrl => "invalid_url",
        RequestIssueKind::InvalidHeaderName => "invalid_header_name",
        RequestIssueKind::InvalidHeaderValue => "invalid_header_value",
        RequestIssueKind::ConflictingHeader => "conflicting_header",
        RequestIssueKind::InvalidJson => "invalid_json",
        RequestIssueKind::InvalidTemplate => "invalid_template",
        RequestIssueKind::TemplateTooDeep => "template_too_deep",
        RequestIssueKind::ResolvedValueTooLarge => "resolved_value_too_large",
        RequestIssueKind::MissingVariable => "missing_variable",
        RequestIssueKind::CyclicVariable => "cyclic_variable",
        RequestIssueKind::MissingSecret => "missing_secret",
    }
}

const fn http_version(version: HttpVersion) -> &'static str {
    match version {
        HttpVersion::Http09 => "HTTP/0.9",
        HttpVersion::Http10 => "HTTP/1.0",
        HttpVersion::Http11 => "HTTP/1.1",
        HttpVersion::Http2 => "HTTP/2",
        HttpVersion::Http3 => "HTTP/3",
        HttpVersion::Other => "HTTP/?",
    }
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

impl From<wirebolt_core::CoreHandshake> for CoreHandshake {
    fn from(value: wirebolt_core::CoreHandshake) -> Self {
        Self {
            product: value.product.to_owned(),
            core_version: value.core_version.to_owned(),
            stream_abi_version: value.stream_abi_version,
        }
    }
}

impl From<BridgeRequestDraft> for wirebolt_core::RequestDraft {
    fn from(value: BridgeRequestDraft) -> Self {
        Self {
            method: value.method,
            url: value.url,
            headers: value.headers.into_iter().map(Into::into).collect(),
            body: value.body,
        }
    }
}

impl From<HeaderField> for wirebolt_core::HeaderField {
    fn from(value: HeaderField) -> Self {
        Self {
            name: value.name,
            value: value.value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::Command,
        sync::{Condvar, Mutex},
        thread,
        time::Duration,
    };

    #[test]
    fn both_bridge_surfaces_share_the_same_version() {
        let handshake = core_handshake();

        assert_eq!(handshake.product, "Wirebolt");
        assert_eq!(handshake.stream_abi_version, wirebolt_stream_abi_version());
    }

    #[test]
    fn prepares_a_request_through_the_coarse_bridge() {
        let summary = prepare_request(BridgeRequestDraft {
            method: "POST".to_owned(),
            url: "https://api.example.com/v1/items".to_owned(),
            headers: vec![HeaderField {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
            }],
            body: br#"{"fast":true}"#.to_vec(),
        })
        .expect("valid request");

        assert_eq!(summary.method, "POST");
        assert_eq!(summary.url, "https://api.example.com/v1/items");
        assert_eq!(summary.header_count, 1);
        assert_eq!(summary.body_bytes, 13);
    }

    #[test]
    fn stream_abi_delivers_head_chunks_and_completion_in_order() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback server");
        let address = listener.local_addr().expect("loopback address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept HTTP client");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read HTTP request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nX-Wirebolt: ffi\r\nConnection: close\r\n\r\npong",
                )
                .expect("write HTTP response");
        });
        let input = serde_json::json!({
            "method": "GET",
            "url": format!("http://{address}/ffi"),
            "workspace_proxy": { "mode": "direct" }
        })
        .to_string();
        let (events, state_pointer) = run_to_completion(&input);
        server.join().expect("HTTP server");
        assert_eq!(events, ["head", "chunk:pong", "complete"]);
        // SAFETY: The callback context is no longer used after session join.
        drop(unsafe { Box::from_raw(state_pointer) });
    }

    #[test]
    fn failure_documents_always_carry_an_issues_array() {
        let input = serde_json::json!({
            "method": "GET",
            "url": "http://127.0.0.1:1/refused",
            "workspace_proxy": { "mode": "direct" }
        })
        .to_string();
        let (events, state_pointer) = run_to_completion(&input);
        // SAFETY: The callback context is no longer used after session join.
        let state = unsafe { Box::from_raw(state_pointer) };

        assert_eq!(events, ["error"]);
        let failure = state
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("failure document");
        assert!(failure["issues"].is_array(), "{failure}");
        assert_eq!(failure["kind"], "connection");
    }

    #[test]
    fn workspace_bridge_round_trips_hierarchy_without_secret_values() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        let mut workspace = Workspace::new("Demo");
        workspace.proxy = Some(ProxyMode::Direct);
        WorkspaceStore::create(temporary.path(), &workspace).expect("create workspace");
        let bridge = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Demo".to_owned(),
        )
        .expect("workspace bridge");
        bridge
            .save_collection("api".to_owned(), "API".to_owned())
            .expect("save collection");
        bridge
            .save_request(
                "api".to_owned(),
                serde_json::json!({
                    "id": "health",
                    "name": "Health",
                    "method": "GET",
                    "url": "https://example.com/health",
                    "authentication": {
                        "kind": "bearer",
                        "token": { "secret": "api.token" }
                    },
                    "body": { "kind": "empty" }
                })
                .to_string()
                .as_str(),
            )
            .expect("save request");
        std::fs::write(
            temporary.path().join("environments/broken.toml"),
            "<<<<<<< HEAD\nid = \"broken\"\n=======\n",
        )
        .expect("write conflicted environment");

        let snapshot = bridge.snapshot_json().expect("workspace snapshot");
        let document: serde_json::Value = serde_json::from_str(&snapshot).expect("snapshot JSON");

        assert_eq!(document["name"], "Demo");
        assert_eq!(document["proxy"]["mode"], "direct");
        assert_eq!(document["collections"][0]["id"], "api");
        assert_eq!(document["collections"][0]["requests"][0]["id"], "health");
        assert_eq!(
            document["collections"][0]["requests"][0]["authentication"]["token"]["secret"],
            "api.token"
        );
        assert_eq!(document["problems"][0]["path"], "environments/broken.toml");
        assert_eq!(document["problems"][0]["kind"], "invalid_toml");
        assert!(!snapshot.contains("secret-value"));
    }

    #[test]
    fn workspace_bridge_exposes_git_operations_as_stable_json() {
        let temporary = tempfile::tempdir().expect("temporary workspace");
        WorkspaceStore::create(temporary.path(), &Workspace::new("Demo"))
            .expect("create workspace");
        let bridge = WorkspaceBridge::open_or_create(
            temporary.path().to_string_lossy().into_owned(),
            "Demo".to_owned(),
        )
        .expect("workspace bridge");
        for arguments in [
            &["init", "-b", "main"][..],
            &["config", "user.name", "Wirebolt Tests"][..],
            &["config", "user.email", "wirebolt@example.invalid"][..],
        ] {
            let output = Command::new("git")
                .arg("-C")
                .arg(temporary.path())
                .args(arguments)
                .output()
                .expect("run Git");
            assert!(output.status.success());
        }

        let before: serde_json::Value =
            serde_json::from_str(&bridge.git_status_json().expect("Git status JSON"))
                .expect("decode Git status");
        assert_eq!(before["branch"], "main");
        assert_eq!(before["revision"], serde_json::Value::Null);
        assert_eq!(before["changes"][0]["path"], "wirebolt.toml");
        assert_eq!(before["changes"][0]["unstaged"], "untracked");

        let operation: serde_json::Value = serde_json::from_str(
            &bridge
                .git_commit_json("initial workspace")
                .expect("Git commit JSON"),
        )
        .expect("decode Git operation");
        assert_eq!(operation["outcome"], "committed");
        assert!(operation["revision"].is_string());
        assert_eq!(operation["status"]["changes"], serde_json::json!([]));
    }

    #[test]
    fn shared_direct_engine_reuses_the_same_client_pool() {
        let proxy = ProxyPolicy::with_workspace(ProxyMode::Direct).resolve(None);

        let first = shared_http_engine(&proxy, &wirebolt_core::NoSecrets).expect("first engine");
        let second = shared_http_engine(&proxy, &wirebolt_core::NoSecrets).expect("second engine");

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn shared_manual_engine_is_keyed_by_mode_and_reset_on_demand() {
        let route = wirebolt_core::ProxyRoute::new(
            wirebolt_core::ProxyDestination::All,
            wirebolt_core::ProxyEndpoint::new("http://proxy.internal:8080")
                .expect("proxy endpoint"),
        );
        let manual =
            ProxyMode::Manual(wirebolt_core::ManualProxy::new(vec![route]).expect("manual proxy"));
        let from_workspace = ProxyPolicy::with_workspace(manual.clone()).resolve(None);
        let from_request = ProxyPolicy::default().resolve(Some(&manual));

        clear_manual_http_engines();
        let first =
            shared_http_engine(&from_workspace, &wirebolt_core::NoSecrets).expect("first engine");
        let second =
            shared_http_engine(&from_request, &wirebolt_core::NoSecrets).expect("second engine");
        assert!(Arc::ptr_eq(&first, &second));

        wirebolt_reset_engines();
        let replacement = shared_http_engine(&from_workspace, &wirebolt_core::NoSecrets)
            .expect("replacement engine");
        assert!(!Arc::ptr_eq(&first, &replacement));
    }

    #[test]
    fn zero_timeouts_mean_unlimited() {
        assert_eq!(timeout_from_millis(0), UNLIMITED_TIMEOUT);
        assert_eq!(timeout_from_millis(250), Duration::from_millis(250));
    }

    fn run_to_completion(input: &str) -> (Vec<String>, *mut CallbackState) {
        let state_pointer = Box::into_raw(Box::new(CallbackState::default()));
        let callbacks = WireboltRunCallbacks {
            on_head: Some(record_head),
            on_chunk: Some(record_chunk),
            on_complete: Some(record_complete),
            on_error: Some(record_error),
        };

        // SAFETY: The input and callback context outlive the run session.
        let session = unsafe {
            wirebolt_run_start(input.as_ptr(), input.len(), callbacks, state_pointer.cast())
        };
        assert!(!session.is_null());
        // SAFETY: `state_pointer` remains owned by the caller until after join.
        let state = unsafe { &*state_pointer };
        let events = state.wait_for_terminal();

        // SAFETY: The worker reached a terminal callback and this is the sole free.
        unsafe { wirebolt_run_free(session) };
        (events, state_pointer)
    }

    #[derive(Debug, Default)]
    struct CallbackState {
        events: Mutex<Vec<String>>,
        failure: Mutex<Option<serde_json::Value>>,
        terminal: Condvar,
    }

    impl CallbackState {
        fn wait_for_terminal(&self) -> Vec<String> {
            let events = self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (events, timeout) = self
                .terminal
                .wait_timeout_while(events, Duration::from_secs(5), |events| {
                    !events
                        .last()
                        .is_some_and(|event| matches!(event.as_str(), "complete" | "error"))
                })
                .expect("wait for terminal callback");
            assert!(!timeout.timed_out(), "run callback timed out");
            events.clone()
        }
    }

    extern "C" fn record_head(context: *mut c_void, json: *const u8, length: usize) {
        let state = callback_state(context);
        let document = callback_bytes(json, length);
        let head: serde_json::Value = serde_json::from_slice(document).expect("head JSON");
        assert_eq!(head["status"], 200);
        assert!(head["warnings"].is_array());
        state
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push("head".to_owned());
    }

    extern "C" fn record_chunk(context: *mut c_void, bytes: *const u8, length: usize) -> u8 {
        let state = callback_state(context);
        let body = String::from_utf8_lossy(callback_bytes(bytes, length));
        state
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(format!("chunk:{body}"));
        1
    }

    extern "C" fn record_complete(context: *mut c_void, json: *const u8, length: usize) {
        let state = callback_state(context);
        let document: serde_json::Value =
            serde_json::from_slice(callback_bytes(json, length)).expect("completion JSON");
        assert_eq!(document["bytes_received"], 4);
        assert_eq!(document["bytes_decoded"], 4);
        state
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push("complete".to_owned());
        state.terminal.notify_all();
    }

    extern "C" fn record_error(context: *mut c_void, json: *const u8, length: usize) {
        let state = callback_state(context);
        let document: serde_json::Value =
            serde_json::from_slice(callback_bytes(json, length)).expect("failure JSON");
        *state
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(document);
        state
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push("error".to_owned());
        state.terminal.notify_all();
    }

    fn callback_state(context: *mut c_void) -> &'static CallbackState {
        // SAFETY: Tests pass a live `CallbackState` for every callback.
        unsafe { &*context.cast::<CallbackState>() }
    }

    fn callback_bytes(pointer: *const u8, length: usize) -> &'static [u8] {
        // SAFETY: The ABI guarantees readable bytes for the callback duration.
        unsafe { std::slice::from_raw_parts(pointer, length) }
    }
}
