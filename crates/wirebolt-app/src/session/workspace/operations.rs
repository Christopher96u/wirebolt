//! Application orchestration. Undo payloads and busy/error transitions live here;
//! the native `UndoManager` holds only opaque tokens and action names.
use super::actions::{self, NamedEdit, WorkspaceAction};
use super::io::{
    WorkspaceExport, WorkspaceFailure, WorkspaceGitAction, WorkspaceGitOperation,
    WorkspaceGitStatus, WorkspaceIO, WorkspaceImportSummary, WorkspacePlatform, WorkspaceReply,
};
use super::{
    SessionWorkspaceCommand, SessionWorkspaceEdit, SessionWorkspacePlan, WorkspaceSession,
};
use crate::lock;
use crate::{
    domain::{
        DomainLocation, DomainRequestDraft, DomainWorkspace,
        request::{own_auth, request_references},
    },
    session::secrets::{SessionSecretPlatform, SessionSecrets},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent operation availability is mirrored into fine-grained native observation."
)]
pub struct WorkspacePresentation {
    pub loading: bool,
    pub loaded: bool,
    pub git_busy: bool,
    pub importing: bool,
    pub git_status: Option<WorkspaceGitStatus>,
    pub git_operation: Option<WorkspaceGitOperation>,
    pub git_failure: Option<WorkspaceFailure>,
    pub import_failure: Option<String>,
    pub export_failure: Option<String>,
    pub import_summary: Option<WorkspaceImportSummary>,
    pub operation_failure: Option<crate::response::ResponseFailureKind>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceImport {
    pub source: String,
    pub format: wirebolt_core::ImportFormat,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub selected: Option<String>,
    pub pasted: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceUndoToken {
    pub token: u64,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct WorkspaceMutation {
    pub succeeded: bool,
    pub plan: Option<SessionWorkspacePlan>,
    pub undo: Option<WorkspaceUndoToken>,
    pub created_id: Option<String>,
    pub root: Option<crate::domain::DomainCollection>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceLoad {
    pub workspace: DomainWorkspace,
    pub selected_environment_id: Option<String>,
    pub select: Option<DomainLocation>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSave {
    pub original_authentication: crate::domain::DomainAuthentication,
    pub document_id: String,
    pub draft: DomainRequestDraft,
    pub location: Option<DomainLocation>,
    pub succeeded: bool,
}
#[derive(Debug, Default)]
struct State {
    presentation: WorkspacePresentation,
    sequence: u64,
    undo: HashMap<u64, (Option<SessionWorkspaceEdit>, String)>,
    search: HashMap<(String, String), (String, String)>,
    redo_tokens: HashSet<u64>,
}
#[derive(Debug)]
pub struct WorkspaceOperations {
    workspace: Arc<WorkspaceSession>,
    state: Mutex<State>,
    edits: tokio::sync::Mutex<()>,
    reorders: tokio::sync::Mutex<()>,
}
#[allow(
    clippy::needless_pass_by_value,
    reason = "Commands own lifted inputs and can be queued across asynchronous execution."
)]
impl WorkspaceOperations {
    #[must_use]
    pub fn new(workspace: Arc<WorkspaceSession>) -> Self {
        Self {
            workspace,
            state: Mutex::default(),
            edits: tokio::sync::Mutex::new(()),
            reorders: tokio::sync::Mutex::new(()),
        }
    }
    #[must_use]
    pub fn presentation(&self) -> WorkspacePresentation {
        lock(&self.state).presentation.clone()
    }
    pub fn clear_undo(&self) {
        let mut state = lock(&self.state);
        state.undo.clear();
        state.redo_tokens.clear();
    }
    fn fail(&self, kind: crate::response::ResponseFailureKind) {
        lock(&self.state).presentation.operation_failure = Some(kind);
    }
    #[must_use]
    pub fn begin_load(&self, configured: bool) -> bool {
        let mut state = lock(&self.state);
        if !configured {
            state.presentation.loaded = true;
            return false;
        }
        state.presentation.loading = true;
        true
    }
    #[must_use]
    pub fn loaded(&self, workspace: DomainWorkspace, selected: Option<String>) -> WorkspaceLoad {
        let selected_environment_id = selected
            .filter(|id| workspace.environments.iter().any(|e| &e.id == id))
            .or_else(|| {
                workspace
                    .environments
                    .iter()
                    .find(|e| e.id != "global")
                    .map(|e| e.id.clone())
            });
        let select = workspace
            .collections
            .iter()
            .flat_map(|c| &c.requests)
            .next()
            .cloned();
        self.workspace.replace(workspace.clone());
        WorkspaceLoad {
            workspace,
            selected_environment_id,
            select,
        }
    }
    pub async fn load(
        &self,
        platform: &dyn WorkspacePlatform,
        switching: bool,
    ) -> Option<WorkspaceLoad> {
        let result = platform.execute(WorkspaceIO::Load).await;
        let mut state = lock(&self.state);
        state.presentation.loading = false;
        if !switching {
            state.presentation.loaded = true;
        }
        if let WorkspaceReply::Loaded { workspace } = result {
            if switching {
                state.presentation = WorkspacePresentation {
                    loaded: true,
                    ..Default::default()
                };
                state.undo.clear();
            }
            drop(state);
            Some(self.loaded(workspace, None))
        } else {
            state.presentation.operation_failure =
                Some(crate::response::ResponseFailureKind::Workspace);
            None
        }
    }
    pub async fn rename(
        &self,
        proposed: String,
        platform: &dyn WorkspacePlatform,
    ) -> Option<String> {
        let name = proposed.trim().to_owned();
        if name.is_empty() {
            return None;
        }
        if lock(&self.workspace.workspace).name == name {
            return Some(name);
        }
        if commands(
            platform,
            vec![SessionWorkspaceCommand::RenameWorkspace { name: name.clone() }],
        )
        .await
        .failure_reason
        .is_some()
        {
            self.fail(crate::response::ResponseFailureKind::Workspace);
            return None;
        }
        lock(&self.workspace.workspace).name.clone_from(&name);
        Some(name)
    }
    fn register(&self, edit: Option<SessionWorkspaceEdit>, name: String) -> WorkspaceUndoToken {
        let mut state = lock(&self.state);
        state.sequence += 1;
        let token = state.sequence;
        state.undo.insert(token, (edit, name.clone()));
        WorkspaceUndoToken { token, name }
    }
    #[must_use]
    pub fn reverse_token(&self, token: u64) -> Option<WorkspaceUndoToken> {
        let name = lock(&self.state).undo.get(&token)?.1.clone();
        let reverse = self.register(None, name);
        let mut state = lock(&self.state);
        if !state.redo_tokens.contains(&token) {
            state.redo_tokens.insert(reverse.token);
        }
        Some(reverse)
    }
    /// Serializes edits and undo, including root creation, across asynchronous stores.
    pub async fn mutate(
        &self,
        command: WorkspaceAction,
        selected: Option<String>,
        platform: &dyn WorkspacePlatform,
        secrets: &SessionSecrets,
        secret_platform: &dyn SessionSecretPlatform,
    ) -> WorkspaceMutation {
        let _reorder = if matches!(
            command,
            WorkspaceAction::Reorder { .. } | WorkspaceAction::MoveBy { .. }
        ) {
            let Ok(guard) = self.reorders.try_lock() else {
                return WorkspaceMutation::default();
            };
            Some(guard)
        } else {
            None
        };
        let _guard = self.edits.lock().await;
        let needs_root = matches!(&command, WorkspaceAction::CreateRequest { collection_id, .. } if collection_id.as_deref().unwrap_or(actions::ROOT_COLLECTION) == actions::ROOT_COLLECTION)
            || matches!(&command, WorkspaceAction::CreateGroup { collection_id, .. } if collection_id == actions::ROOT_COLLECTION);
        let mut root = None;
        if needs_root
            && !lock(&self.workspace.workspace)
                .collections
                .iter()
                .any(|c| c.id == actions::ROOT_COLLECTION)
        {
            let collection = actions::root_collection();
            if commands(
                platform,
                vec![SessionWorkspaceCommand::CreateCollection {
                    collection: collection.clone(),
                }],
            )
            .await
            .failure_reason
            .is_some()
            {
                self.fail(crate::response::ResponseFailureKind::Workspace);
                return WorkspaceMutation::default();
            }
            lock(&self.workspace.workspace)
                .collections
                .insert(0, collection.clone());
            root = Some(collection);
        }
        let Some(edit) = actions::action(&lock(&self.workspace.workspace), command) else {
            return WorkspaceMutation {
                root,
                ..Default::default()
            };
        };
        let mut result = self
            .apply_named(edit, selected, platform, secrets, secret_platform)
            .await;
        result.root = root;
        if result.succeeded {
            let mut state = lock(&self.state);
            let redo = std::mem::take(&mut state.redo_tokens);
            state.undo.retain(|token, _| !redo.contains(token));
        }
        result
    }
    pub async fn undo(
        &self,
        token: u64,
        reverse: u64,
        selected: Option<String>,
        platform: &dyn WorkspacePlatform,
        secrets: &SessionSecrets,
        secret_platform: &dyn SessionSecretPlatform,
    ) -> WorkspaceMutation {
        let _guard = self.edits.lock().await;
        let entry = {
            let mut state = lock(&self.state);
            state.redo_tokens.remove(&token);
            state.undo.remove(&token)
        };
        let Some((Some(edit), name)) = entry else {
            self.clear_undo();
            return WorkspaceMutation::default();
        };
        let mut result = self
            .apply_named(
                NamedEdit {
                    edit,
                    name,
                    created_id: None,
                },
                selected,
                platform,
                secrets,
                secret_platform,
            )
            .await;
        let inverse = result
            .undo
            .take()
            .and_then(|u| lock(&self.state).undo.remove(&u.token))
            .and_then(|e| e.0);
        if result.succeeded {
            if let Some(entry) = lock(&self.state).undo.get_mut(&reverse) {
                entry.0 = inverse;
            }
        } else {
            self.clear_undo();
        }
        result
    }
    async fn apply_named(
        &self,
        named: NamedEdit,
        selected: Option<String>,
        platform: &dyn WorkspacePlatform,
        secrets: &SessionSecrets,
        secret_platform: &dyn SessionSecretPlatform,
    ) -> WorkspaceMutation {
        let Some(mut plan) = self.workspace.plan(selected, named.edit.clone()) else {
            return WorkspaceMutation::default();
        };
        // Duplicate credentials before writing a copy; never share generated keychain names.
        let mut duplicate = None;
        if let SessionWorkspaceEdit::DuplicateRequest {
            collection_id,
            id,
            new_id,
            ..
        } = &named.edit
        {
            let source =
                super::location(&lock(&self.workspace.workspace), collection_id, id).cloned();
            if let Some(source) = source {
                let (auth, renames) = own_auth(source.request.authentication, false);
                if secrets.flush(secret_platform).await.is_err()
                    || secrets
                        .copy(renames.clone(), secret_platform)
                        .await
                        .is_err()
                {
                    self.fail(crate::response::ResponseFailureKind::Workspace);
                    return WorkspaceMutation::default();
                }
                if !renames.is_empty() {
                    duplicate = Some((
                        collection_id.clone(),
                        new_id.clone(),
                        source.order.saturating_add(1),
                        auth,
                    ));
                }
            }
        }
        secrets.observe(plan.observed_secrets.clone());
        let outcome = commands(platform, plan.commands.clone()).await;
        let failed = outcome.failure_reason.is_some();
        if failed {
            let Some(partial) = self.workspace.partial(plan, outcome.completed) else {
                self.fail(crate::response::ResponseFailureKind::Workspace);
                return WorkspaceMutation::default();
            };
            plan = partial;
        }
        if let Some((collection_id, id, order, auth)) = duplicate
            && let Some(copy) = plan
                .collections
                .iter_mut()
                .find(|c| c.id == collection_id)
                .and_then(|c| c.requests.iter_mut().find(|r| r.request.id == id))
        {
            copy.request.authentication = auth;
            let mut persisted = copy.clone();
            persisted.order = order;
            if commands(
                platform,
                vec![SessionWorkspaceCommand::SaveRequest {
                    location: persisted,
                }],
            )
            .await
            .failure_reason
            .is_some()
            {
                self.fail(crate::response::ResponseFailureKind::Workspace);
                return WorkspaceMutation::default();
            }
        }
        self.workspace.commit(plan.clone());
        secrets.observe(plan.observed_secrets.clone());
        if failed {
            self.fail(crate::response::ResponseFailureKind::Workspace);
        }
        let undo = (!failed).then(|| self.register(Some(plan.inverse.clone()), named.name));
        WorkspaceMutation {
            succeeded: !failed,
            plan: Some(plan),
            undo,
            created_id: named.created_id,
            root: None,
        }
    }
    #[must_use]
    pub fn can_reorder(
        &self,
        identifier: String,
        target: String,
        collection_id: String,
        parent: Option<String>,
    ) -> bool {
        actions::can_reorder(
            &lock(&self.workspace.workspace),
            &identifier,
            &target,
            &collection_id,
            parent.as_deref(),
        )
    }
    #[must_use]
    pub fn can_move_to(
        &self,
        identifier: String,
        collection_id: String,
        parent: Option<String>,
    ) -> bool {
        actions::can_move_to(
            &lock(&self.workspace.workspace),
            &identifier,
            &collection_id,
            parent.as_deref(),
        )
    }
    #[must_use]
    pub fn next_order(&self, collection_id: String, parent: Option<String>) -> i64 {
        actions::next_order(
            &lock(&self.workspace.workspace),
            &collection_id,
            parent.as_deref(),
        )
    }
    #[must_use]
    pub fn new_request(
        &self,
        web_socket: bool,
        collection_id: String,
        group_id: Option<String>,
    ) -> DomainLocation {
        let location = actions::new_request(
            &lock(&self.workspace.workspace),
            web_socket,
            collection_id,
            group_id,
        );
        self.saved_request(&location);
        location
    }
    fn saved_request(&self, location: &DomainLocation) {
        if let Some(c) = lock(&self.workspace.workspace)
            .collections
            .iter_mut()
            .find(|c| c.id == location.collection_id)
        {
            if let Some(r) = c
                .requests
                .iter_mut()
                .find(|r| r.request.id == location.request.id)
            {
                *r = location.clone();
            } else {
                c.requests.push(location.clone());
            }
        }
    }
    pub async fn save(
        &self,
        document: crate::session::tabs::SessionDocument,
        fallback: Option<String>,
        platform: &dyn WorkspacePlatform,
        secrets: &SessionSecrets,
        secret_platform: &dyn SessionSecretPlatform,
    ) -> WorkspaceSave {
        let mut result = WorkspaceSave {
            original_authentication: document.draft.authentication.clone(),
            document_id: document.id,
            draft: document.draft,
            location: None,
            succeeded: false,
        };
        let Some(collection_id) = document.collection_id.or(fallback) else {
            self.fail(crate::response::ResponseFailureKind::Workspace);
            return result;
        };
        if secrets.flush(secret_platform).await.is_err() {
            self.fail(crate::response::ResponseFailureKind::Workspace);
            return result;
        }
        let (authentication, renames) = own_auth(result.draft.authentication.clone(), true);
        if secrets.copy(renames, secret_platform).await.is_err() {
            self.fail(crate::response::ResponseFailureKind::Workspace);
            return result;
        }
        result.draft.authentication = authentication;
        secrets.observe(request_references(&result.draft).into_iter().collect());
        let saved = super::location(
            &lock(&self.workspace.workspace),
            &collection_id,
            &result.draft.id,
        )
        .cloned();
        let location = DomainLocation {
            collection_id,
            group_id: saved.as_ref().and_then(|r| r.group_id.clone()),
            order: saved.map_or(0, |r| r.order),
            request: result.draft.clone(),
        };
        if commands(
            platform,
            vec![SessionWorkspaceCommand::SaveRequest {
                location: location.clone(),
            }],
        )
        .await
        .failure_reason
        .is_some()
        {
            self.fail(crate::response::ResponseFailureKind::Workspace);
            return result;
        }
        self.saved_request(&location);
        result.location = Some(location);
        result.succeeded = true;
        result
    }
}
async fn commands(
    platform: &dyn WorkspacePlatform,
    commands: Vec<SessionWorkspaceCommand>,
) -> crate::session::persistence::SessionCommandBatchResult {
    match platform.execute(WorkspaceIO::Commands { commands }).await {
        WorkspaceReply::Commands { result } => result,
        _ => crate::session::persistence::SessionCommandBatchResult {
            completed: 0,
            failure_reason: Some("workspace".into()),
        },
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "Commands own lifted inputs and can be queued across asynchronous execution."
)]
impl WorkspaceOperations {
    #[must_use]
    pub fn begin_git(&self, configured: bool, action: WorkspaceGitAction, dirty: bool) -> bool {
        let mut s = lock(&self.state);
        if action == WorkspaceGitAction::Pull && dirty {
            s.presentation.git_failure = Some(WorkspaceFailure {
                kind: crate::session::workspace::io::WorkspaceFailureKind::UnsavedRequest,
                reason: "Save or discard request edits before pulling.".into(),
                localized: true,
            });
            return false;
        }
        if s.presentation.git_busy {
            return false;
        }
        if !configured {
            s.presentation.git_failure = Some(WorkspaceFailure {
                kind: super::io::WorkspaceFailureKind::NotConfigured,
                reason: "Open a workspace before using Git collaboration.".into(),
                localized: true,
            });
            return false;
        }
        s.presentation.git_failure = None;
        s.presentation.git_busy = true;
        true
    }
    pub async fn git(
        &self,
        action: WorkspaceGitAction,
        platform: &dyn WorkspacePlatform,
        persistence: Option<&dyn WorkspacePlatform>,
    ) -> Option<WorkspaceLoad> {
        let reply = platform
            .execute(WorkspaceIO::Git {
                action: action.clone(),
            })
            .await;
        let reload = match reply {
            WorkspaceReply::GitStatus { status } => {
                let mut s = lock(&self.state);
                if action == WorkspaceGitAction::Initialize {
                    s.presentation.git_operation = None;
                }
                s.presentation.git_status = Some(status);
                false
            }
            WorkspaceReply::GitOperation { operation } => {
                let reload = (action == WorkspaceGitAction::Pull
                    && operation.outcome == super::io::WorkspaceGitOutcome::Updated)
                    || action == WorkspaceGitAction::Abort;
                let mut s = lock(&self.state);
                s.presentation.git_status = Some(operation.status.clone());
                s.presentation.git_operation = Some(operation);
                reload
            }
            reply => {
                lock(&self.state).presentation.git_failure = Some(match reply {
                    WorkspaceReply::Failed { failure }
                        if failure.kind
                            != crate::session::workspace::io::WorkspaceFailureKind::Bridge
                            || failure.localized =>
                    {
                        failure
                    }
                    _ => WorkspaceFailure {
                        kind: crate::session::workspace::io::WorkspaceFailureKind::Bridge,
                        reason: "Git operation failed.".into(),
                        localized: true,
                    },
                });
                false
            }
        };
        let mut loaded = None;
        if reload && let Some(persistence) = persistence {
            match persistence.execute(WorkspaceIO::Load).await {
                WorkspaceReply::Loaded { workspace } => {
                    loaded = Some(self.loaded(workspace, None));
                    self.clear_undo();
                }
                _ => {
                    lock(&self.state).presentation.git_failure = Some(WorkspaceFailure {
                        kind: crate::session::workspace::io::WorkspaceFailureKind::WorkspaceReload,
                        reason: "Git updated the workspace, but Wirebolt could not reload it."
                            .into(),
                        localized: true,
                    });
                }
            }
        }
        lock(&self.state).presentation.git_busy = false;
        loaded
    }
    #[must_use]
    pub fn begin_import(&self, configured: bool, pasted: bool) -> bool {
        let mut s = lock(&self.state);
        if s.presentation.importing || !configured {
            return false;
        }
        s.presentation.importing = true;
        if !pasted {
            s.presentation.import_failure = None;
        }
        true
    }
    pub fn finish_import_failure(&self, message: String, pasted: bool) -> String {
        let mut s = lock(&self.state);
        s.presentation.importing = false;
        s.presentation.import_failure = Some(message.clone());
        let _ = pasted;
        message
    }
    pub async fn import(
        &self,
        request: WorkspaceImport,
        platform: &dyn WorkspacePlatform,
    ) -> Option<WorkspaceLoad> {
        let WorkspaceImport {
            source,
            format,
            name,
            display_name,
            selected,
            pasted,
        } = request;
        let previous: std::collections::HashSet<_> = lock(&self.workspace.workspace)
            .collections
            .iter()
            .map(|c| c.id.clone())
            .collect();
        let outcome = platform
            .execute(WorkspaceIO::Import {
                format,
                source,
                name,
            })
            .await;
        if let WorkspaceReply::Imported { ref summary } = outcome
            && let WorkspaceReply::Loaded { workspace } = platform.execute(WorkspaceIO::Load).await
        {
            let environment = selected
                .filter(|id| workspace.environments.iter().any(|e| &e.id == id))
                .or_else(|| {
                    summary
                        .environments
                        .iter()
                        .find(|e| workspace.environments.iter().any(|v| v.id == e.id))
                        .map(|e| e.id.clone())
                });
            let mut loaded = self.loaded(workspace, environment);
            loaded.select = loaded
                .workspace
                .collections
                .iter()
                .find(|c| !previous.contains(&c.id))
                .and_then(|c| c.requests.first())
                .cloned();
            let mut s = lock(&self.state);
            s.presentation.import_summary = Some(summary.clone());
            s.presentation.importing = false;
            s.presentation.import_failure = None;
            return Some(loaded);
        }
        let reason = match outcome {
            WorkspaceReply::Failed { failure } if failure.localized => Some(failure.reason),
            _ => None,
        };
        self.finish_import_failure(
            import_failure_message(reason.as_deref(), display_name.as_deref()),
            pasted,
        );
        None
    }
    pub async fn export(
        &self,
        target: WorkspaceExport,
        platform: &dyn WorkspacePlatform,
    ) -> Option<String> {
        match platform.execute(WorkspaceIO::Export { target }).await {
            WorkspaceReply::Exported { content } => Some(content),
            reply => {
                let mut s = lock(&self.state);
                s.presentation.export_failure = Some(match reply {
                    WorkspaceReply::Failed { failure } => failure.reason,
                    _ => String::new(),
                });
                s.presentation.operation_failure =
                    Some(crate::response::ResponseFailureKind::Export);
                None
            }
        }
    }
}
#[must_use]
pub fn import_failure_message(reason: Option<&str>, name: Option<&str>) -> String {
    let subject = name.map_or_else(
        || "The import failed.".into(),
        |n| format!("“{n}” couldn’t be imported."),
    );
    let Some(reason) = reason.filter(|s| !s.is_empty()) else {
        return format!("{subject} Check its format and contents.");
    };
    let mut text = reason.trim().to_owned();
    if let Some(first) = text.chars().next().filter(|c| c.is_lowercase()) {
        text = format!("{}{}", first.to_uppercase(), &text[first.len_utf8()..]);
    }
    if text.chars().last().is_some_and(|c| !".!?".contains(c)) {
        text.push('.');
    }
    format!("{subject} {text}")
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "Commands own lifted inputs and can be queued across asynchronous execution."
)]
impl WorkspaceOperations {
    pub fn restore_tabs(
        &self,
        tabs: &mut crate::session::tabs::SessionTabs,
        documents: &mut HashMap<String, crate::session::tabs::SessionTabDocument>,
        layout: Option<crate::session::tabs::SessionLayout>,
    ) -> super::lifecycle::WorkspaceRestore {
        super::lifecycle::restore(&lock(&self.workspace.workspace), tabs, documents, layout)
    }
}
impl WorkspaceSession {
    #[must_use]
    pub fn snapshot(&self) -> DomainWorkspace {
        lock(&self.workspace).clone()
    }
}

impl WorkspaceOperations {
    pub fn take_failure(&self) -> Option<crate::response::ResponseFailureKind> {
        lock(&self.state).presentation.operation_failure.take()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceRepositoryProblem {
    NotRepository,
    NotRepositoryRoot,
    GitUnavailable,
}
#[must_use]
pub fn repository_problem(
    kind: &super::io::WorkspaceFailureKind,
) -> Option<WorkspaceRepositoryProblem> {
    match kind {
        super::io::WorkspaceFailureKind::NotRepository => {
            Some(WorkspaceRepositoryProblem::NotRepository)
        }
        super::io::WorkspaceFailureKind::WorkspaceNotRepositoryRoot => {
            Some(WorkspaceRepositoryProblem::NotRepositoryRoot)
        }
        super::io::WorkspaceFailureKind::GitUnavailable => {
            Some(WorkspaceRepositoryProblem::GitUnavailable)
        }
        _ => None,
    }
}

impl WorkspaceOperations {
    #[must_use]
    pub fn environment_unchanged(&self, environment: &crate::domain::DomainEnvironment) -> bool {
        lock(&self.workspace.workspace)
            .environments
            .iter()
            .find(|e| e.id == environment.id)
            == Some(environment)
    }
}

impl WorkspaceOperations {
    #[must_use]
    pub fn request_matches(
        &self,
        request: crate::domain::sidebar::DomainSidebarRequest,
        normalized: &str,
    ) -> bool {
        let key = (request.collection_id, request.id);
        let source = format!("{}\0{}\0{}", request.name, request.method, request.url);
        let mut state = lock(&self.state);
        let indexed = state
            .search
            .entry(key)
            .or_insert_with(|| (String::new(), String::new()));
        if indexed.0 != source {
            indexed.1 = crate::domain::sidebar::normalize(&source);
            indexed.0 = source;
        }
        indexed.1.contains(normalized)
    }
}

impl WorkspaceOperations {
    #[must_use]
    pub fn first_collection_id(&self) -> Option<String> {
        lock(&self.workspace.workspace)
            .collections
            .first()
            .map(|c| c.id.clone())
    }
}
