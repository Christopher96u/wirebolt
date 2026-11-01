//! Document save/discard/restore policy. Native facades retain editor/response identity.
use crate::{
    domain::{DomainRequestDraft, DomainWorkspace},
    session::tabs::{
        SessionCloseTargets, SessionDocument, SessionEditorGroup, SessionLayout,
        SessionRestoreResult, SessionTabDocument, SessionTabs,
    },
};
use std::collections::{HashMap, HashSet};
#[must_use]
pub fn dirty_ids(groups: &[SessionEditorGroup], documents: &[SessionTabDocument]) -> Vec<String> {
    let dirty: HashSet<_> = documents
        .iter()
        .filter(|d| d.dirty)
        .map(|d| d.id.as_str())
        .collect();
    let mut seen = HashSet::new();
    groups
        .iter()
        .flat_map(|g| &g.tab_ids)
        .filter(|id| seen.insert(id.as_str()) && dirty.contains(id.as_str()))
        .cloned()
        .collect()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceDiscard {
    pub id: String,
    pub saved: Option<DomainRequestDraft>,
}
#[must_use]
pub fn discard(documents: Vec<SessionDocument>) -> Vec<WorkspaceDiscard> {
    documents
        .into_iter()
        .filter(SessionDocument::dirty)
        .map(|d| WorkspaceDiscard {
            id: d.id,
            saved: d.saved_draft,
        })
        .collect()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceRestore {
    pub result: SessionRestoreResult,
    pub restore_history: bool,
}
#[must_use]
#[allow(
    clippy::implicit_hasher,
    reason = "This map is the concrete retained SessionTabs document store."
)]
pub fn restore(
    workspace: &DomainWorkspace,
    tabs: &mut SessionTabs,
    documents: &mut HashMap<String, SessionTabDocument>,
    layout: Option<SessionLayout>,
) -> WorkspaceRestore {
    let active = tabs
        .snapshot(documents)
        .groups
        .iter()
        .find(|g| g.id == tabs.active_group_id)
        .and_then(|g| g.selected_tab_id.as_ref())
        .is_some();
    let mut restored = vec![];
    if !active {
        if let Some(layout) = layout.filter(|_| documents.is_empty()) {
            let resolved = layout
                .groups
                .iter()
                .flat_map(|g| &g.tabs)
                .map(|tab| super::location(workspace, &tab.collection_id, &tab.request_id).cloned())
                .collect();
            restored = tabs.restore(&layout, resolved, documents);
        }
        if restored.is_empty()
            && let Some(location) = workspace
                .collections
                .iter()
                .flat_map(|c| &c.requests)
                .next()
        {
            let draft = location.request.clone();
            let document = SessionDocument::new(
                crate::domain::identity::unique_id(),
                crate::session::tabs::SessionKind::Http,
                Some(location.collection_id.clone()),
                None,
                draft.clone(),
                Some(draft),
                "",
            );
            tabs.open(
                SessionTabDocument::from_document(&document),
                documents,
                None,
                false,
                false,
            );
            return WorkspaceRestore {
                result: SessionRestoreResult {
                    documents: vec![document],
                    snapshot: tabs.snapshot(documents),
                },
                restore_history: false,
            };
        }
    }
    let restore_history = !restored.is_empty();
    WorkspaceRestore {
        result: SessionRestoreResult {
            documents: restored,
            snapshot: tabs.snapshot(documents),
        },
        restore_history,
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceClose {
    pub blocked: Vec<String>,
    pub removed: Vec<WorkspaceDiscard>,
    pub snapshot: crate::session::tabs::SessionTabSnapshot,
}
/// Read target state without cloning a whole live document across the bridge.
pub trait CloseDocument {
    fn is_dirty(&self) -> bool;
    fn saved_draft(&self) -> Option<DomainRequestDraft>;
}
impl CloseDocument for SessionDocument {
    fn is_dirty(&self) -> bool {
        self.dirty()
    }
    fn saved_draft(&self) -> Option<DomainRequestDraft> {
        self.saved_draft.clone()
    }
}
#[must_use]
#[allow(
    clippy::implicit_hasher,
    reason = "This is the retained SessionTabs projection store."
)]
pub fn close(
    state: &mut (SessionTabs, HashMap<String, SessionTabDocument>),
    documents: Vec<(SessionTabDocument, impl CloseDocument)>,
    targets: &SessionCloseTargets,
    allow_dirty: bool,
) -> WorkspaceClose {
    let (tabs, projections) = state;
    let target_ids: HashSet<_> = targets.ids.iter().collect();
    let handles: HashMap<_, _> = documents
        .into_iter()
        .map(|(mut projection, document)| {
            if target_ids.contains(&projection.id) {
                projection.dirty = document.is_dirty();
            }
            let id = projection.id.clone();
            projections.insert(id.clone(), projection);
            (id, document)
        })
        .collect();
    projections.retain(|id, _| handles.contains_key(id));
    // Close policy needs only native editor metadata. Fetch saved payloads lazily,
    // after the entire scope has passed the dirty-close decision.
    let prior: Vec<_> = projections
        .values()
        .map(|d| (d.id.clone(), d.dirty))
        .collect();
    let blocked = tabs.close_resolved(targets, allow_dirty, projections);
    let removed = if blocked.is_empty() {
        prior
            .into_iter()
            .filter(|(id, _)| !projections.contains_key(id))
            .map(|(id, dirty)| WorkspaceDiscard {
                saved: if allow_dirty && dirty {
                    handles.get(&id).and_then(CloseDocument::saved_draft)
                } else {
                    None
                },
                id,
            })
            .collect()
    } else {
        vec![]
    };
    WorkspaceClose {
        blocked,
        removed,
        snapshot: tabs.snapshot(projections),
    }
}
