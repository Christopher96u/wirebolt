//! Document save/discard/restore policy. Native facades retain editor/response identity.
use crate::{
    domain::{DomainRequestDraft, DomainWorkspace},
    session::tabs::{
        SessionDocument, SessionEditorGroup, SessionLayout, SessionRestoreResult,
        SessionTabDocument, SessionTabs,
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
#[must_use]
pub fn close(
    tabs: &mut SessionTabs,
    documents: Vec<SessionDocument>,
    scope: &crate::session::tabs::SessionCloseScope,
    group: Option<&str>,
    allow_dirty: bool,
) -> WorkspaceClose {
    let mut projections: HashMap<_, _> = documents
        .iter()
        .map(|d| (d.id.clone(), SessionTabDocument::from_document(d)))
        .collect();
    let blocked = tabs.close(scope, group, allow_dirty, &mut projections);
    let removed = if blocked.is_empty() {
        documents
            .into_iter()
            .filter(|d| !projections.contains_key(&d.id))
            .map(|d| WorkspaceDiscard {
                id: d.id,
                saved: if allow_dirty { d.saved_draft } else { None },
            })
            .collect()
    } else {
        vec![]
    };
    WorkspaceClose {
        blocked,
        removed,
        snapshot: tabs.snapshot(&projections),
    }
}
