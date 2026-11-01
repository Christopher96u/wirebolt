//! Editor groups, preview replacement, navigation and compatible layout persistence.
use crate::domain::{DomainRequestDraft, identity::unique_id, url::separate_url_query};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKind {
    Http,
    WebSocket,
}
impl SessionKind {
    #[must_use]
    pub fn for_draft(draft: &DomainRequestDraft, fallback: Self) -> Self {
        if draft.web_socket || draft.url.starts_with("ws://") || draft.url.starts_with("wss://") {
            Self::WebSocket
        } else {
            fallback
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionDocument {
    pub id: String,
    pub kind: SessionKind,
    pub collection_id: Option<String>,
    pub request_id: String,
    pub draft: DomainRequestDraft,
    pub saved_draft: Option<DomainRequestDraft>,
    pub touched: bool,
}
impl SessionDocument {
    #[must_use]
    pub fn new(
        id: String,
        kind: SessionKind,
        collection_id: Option<String>,
        request_id: Option<String>,
        draft: DomainRequestDraft,
        saved_draft: Option<DomainRequestDraft>,
        note: &str,
    ) -> Self {
        let kind = SessionKind::for_draft(&draft, kind);
        let request_id = request_id.unwrap_or_else(|| draft.id.clone());
        let same = saved_draft.as_ref() == Some(&draft);
        let mut draft = separate_url_query(draft);
        draft.web_socket = kind == SessionKind::WebSocket;
        if !note.is_empty() {
            note.clone_into(&mut draft.note);
        }
        let saved_draft = if same {
            Some(draft.clone())
        } else {
            saved_draft.map(separate_url_query)
        };
        Self {
            id,
            kind,
            collection_id,
            request_id,
            draft,
            saved_draft,
            touched: false,
        }
    }
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.saved_draft.as_ref() != Some(&self.draft)
    }
    pub fn edit(&mut self, draft: DomainRequestDraft) {
        self.draft = draft;
        self.touched |= self.dirty();
    }
    pub fn mark_saved(&mut self, draft: DomainRequestDraft) {
        self.edit(draft.clone());
        self.saved_draft = Some(draft);
    }
    pub fn rename(&mut self, name: &str) {
        name.clone_into(&mut self.draft.name);
        if let Some(saved) = &mut self.saved_draft {
            name.clone_into(&mut saved.name);
        }
        self.touched |= self.dirty();
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTabDocument {
    pub id: String,
    pub kind: SessionKind,
    pub collection_id: Option<String>,
    pub request_id: String,
    pub dirty: bool,
    pub touched: bool,
}
impl SessionTabDocument {
    #[must_use]
    pub fn from_document(document: &SessionDocument) -> Self {
        Self {
            id: document.id.clone(),
            kind: document.kind.clone(),
            collection_id: document.collection_id.clone(),
            request_id: document.request_id.clone(),
            dirty: document.dirty(),
            touched: document.touched,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEditorGroup {
    pub id: String,
    #[serde(rename = "tabIDs")]
    pub tab_ids: Vec<String>,
    #[serde(rename = "selectedTabID")]
    pub selected_tab_id: Option<String>,
    #[serde(rename = "backwardTabIDs")]
    pub backward_tab_ids: Vec<String>,
    #[serde(rename = "forwardTabIDs")]
    pub forward_tab_ids: Vec<String>,
    #[serde(rename = "previewTabID")]
    pub preview_tab_id: Option<String>,
}
impl Default for SessionEditorGroup {
    fn default() -> Self {
        Self {
            id: unique_id(),
            tab_ids: vec![],
            selected_tab_id: None,
            backward_tab_ids: vec![],
            forward_tab_ids: vec![],
            preview_tab_id: None,
        }
    }
}
impl SessionEditorGroup {
    fn select(&mut self, id: &str) {
        if self.selected_tab_id.as_deref() == Some(id) {
            return;
        }
        if let Some(current) = self.selected_tab_id.take() {
            self.backward_tab_ids.push(current);
        }
        self.selected_tab_id = Some(id.into());
        self.forward_tab_ids.clear();
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLayoutTab {
    #[serde(rename = "collectionID")]
    pub collection_id: String,
    #[serde(rename = "requestID")]
    pub request_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLayoutGroup {
    pub tabs: Vec<SessionLayoutTab>,
    pub selected_index: Option<i64>,
    #[serde(default)]
    pub preview_index: Option<i64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLayout {
    pub groups: Vec<SessionLayoutGroup>,
    pub active_group_index: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionCloseScope {
    One { id: String },
    Others { id: String },
    RightOf { id: String },
    All,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTabSnapshot {
    pub groups: Vec<SessionEditorGroup>,
    pub active_group_id: String,
    pub session_ids: Vec<String>,
}
#[derive(Debug)]
pub struct SessionTabs {
    pub groups: Vec<SessionEditorGroup>,
    pub active_group_id: String,
}
impl Default for SessionTabs {
    fn default() -> Self {
        let group = SessionEditorGroup::default();
        Self {
            active_group_id: group.id.clone(),
            groups: vec![group],
        }
    }
}
impl SessionTabs {
    #[must_use]
    pub fn snapshot(&self, documents: &HashMap<String, SessionTabDocument>) -> SessionTabSnapshot {
        SessionTabSnapshot {
            groups: self.groups.clone(),
            active_group_id: self.active_group_id.clone(),
            session_ids: documents.keys().cloned().collect(),
        }
    }
    pub fn select(&mut self, id: &str, group: Option<&str>) {
        let group = group.unwrap_or(&self.active_group_id).to_owned();
        if let Some(target) = self
            .groups
            .iter_mut()
            .find(|g| g.id == group && g.tab_ids.iter().any(|tab| tab == id))
        {
            target.select(id);
            self.active_group_id = group;
        }
    }
    pub fn navigate(&mut self, forward: bool) {
        if let Some(group) = self
            .groups
            .iter_mut()
            .find(|g| g.id == self.active_group_id)
        {
            let (source, destination) = if forward {
                (&mut group.forward_tab_ids, &mut group.backward_tab_ids)
            } else {
                (&mut group.backward_tab_ids, &mut group.forward_tab_ids)
            };
            if let Some(id) = source.pop()
                && let Some(selected) = group.selected_tab_id.replace(id)
            {
                destination.push(selected);
            }
        }
    }
    pub fn pin(&mut self, id: &str) {
        if let Some(group) = self
            .groups
            .iter_mut()
            .find(|g| g.preview_tab_id.as_deref() == Some(id))
        {
            group.preview_tab_id = None;
        }
    }
    #[must_use]
    pub fn preview(&self, id: &str, documents: &HashMap<String, SessionTabDocument>) -> bool {
        documents.get(id).is_some_and(|d| !d.touched)
            && self
                .groups
                .iter()
                .any(|g| g.preview_tab_id.as_deref() == Some(id))
    }
    /// Returns the opened document and any replaced preview id.
    pub fn open(
        &mut self,
        document: SessionTabDocument,
        documents: &mut HashMap<String, SessionTabDocument>,
        group: Option<&str>,
        force: bool,
        preview: bool,
    ) -> (String, Option<String>) {
        let group = group.unwrap_or(&self.active_group_id).to_owned();
        let target = self.groups.iter().position(|g| g.id == group);
        if !force && let Some(index) = target {
            let existing = self.groups[index]
                .tab_ids
                .iter()
                .find(|id| {
                    documents.get(*id).is_some_and(|d| {
                        d.request_id == document.request_id
                            && d.collection_id == document.collection_id
                            && d.kind == document.kind
                    })
                })
                .cloned();
            if let Some(id) = existing {
                self.select(&id, Some(&group));
                return (id, None);
            }
        }
        let replaced = target
            .and_then(|index| self.groups[index].preview_tab_id.clone())
            .filter(|id| preview && self.preview(id, documents));
        let id = document.id.clone();
        documents.insert(id.clone(), document);
        if let Some(index) = target {
            let target = &mut self.groups[index];
            if let Some(replaced) = &replaced {
                if let Some(index) = target.tab_ids.iter().position(|tab| tab == replaced) {
                    target.tab_ids[index].clone_from(&id);
                }
                target.backward_tab_ids.retain(|tab| tab != replaced);
                target.forward_tab_ids.retain(|tab| tab != replaced);
                if target.selected_tab_id.as_ref() == Some(replaced) {
                    target.selected_tab_id = Some(id.clone());
                } else {
                    target.select(&id);
                }
            } else {
                target.tab_ids.push(id.clone());
                target.select(&id);
            }
            if preview {
                target.preview_tab_id = Some(id.clone());
            }
        }
        self.active_group_id = group;
        if let Some(replaced) = &replaced {
            self.remove_unreferenced(documents, std::slice::from_ref(replaced));
        }
        (id, replaced)
    }
    pub fn reopen(
        &mut self,
        document: SessionTabDocument,
        documents: &mut HashMap<String, SessionTabDocument>,
    ) {
        let id = document.id.clone();
        documents.insert(id.clone(), document);
        if let Some(group) = self
            .groups
            .iter_mut()
            .find(|g| g.id == self.active_group_id)
        {
            if !group.tab_ids.contains(&id) {
                group.tab_ids.push(id.clone());
            }
            group.select(&id);
        }
    }
    pub fn close(
        &mut self,
        scope: &SessionCloseScope,
        group: Option<&str>,
        allow_dirty: bool,
        documents: &mut HashMap<String, SessionTabDocument>,
    ) -> Vec<String> {
        let group = group.unwrap_or(&self.active_group_id);
        let Some(index) = self.groups.iter().position(|g| g.id == group) else {
            return vec![];
        };
        let target = &self.groups[index];
        let ids: Vec<String> = match scope {
            SessionCloseScope::One { id } => vec![id.clone()],
            SessionCloseScope::Others { id } => target
                .tab_ids
                .iter()
                .filter(|t| *t != id)
                .cloned()
                .collect(),
            SessionCloseScope::RightOf { id } => target
                .tab_ids
                .iter()
                .position(|t| t == id)
                .map(|i| target.tab_ids[i + 1..].to_vec())
                .unwrap_or_default(),
            SessionCloseScope::All => target.tab_ids.clone(),
        };
        let blocked: Vec<_> = ids
            .iter()
            .filter(|id| documents.get(*id).is_some_and(|d| d.dirty))
            .cloned()
            .collect();
        if !allow_dirty && !blocked.is_empty() {
            return blocked;
        }
        let target = &mut self.groups[index];
        target.tab_ids.retain(|id| !ids.contains(id));
        target.backward_tab_ids.retain(|id| !ids.contains(id));
        target.forward_tab_ids.retain(|id| !ids.contains(id));
        if target
            .preview_tab_id
            .as_ref()
            .is_some_and(|id| ids.contains(id))
        {
            target.preview_tab_id = None;
        }
        if target
            .selected_tab_id
            .as_ref()
            .is_some_and(|id| ids.contains(id))
        {
            target.selected_tab_id = target.tab_ids.last().cloned();
        }
        self.remove_unreferenced(documents, &ids);
        self.groups
            .retain(|g| g.id == self.active_group_id || !g.tab_ids.is_empty());
        if self.groups.len() > 1
            && self
                .groups
                .iter()
                .any(|g| g.id == self.active_group_id && g.tab_ids.is_empty())
        {
            self.groups.retain(|g| g.id != self.active_group_id);
            self.active_group_id = self.groups[0].id.clone();
        }
        vec![]
    }
    fn remove_unreferenced(
        &self,
        documents: &mut HashMap<String, SessionTabDocument>,
        ids: &[String],
    ) {
        let referenced: HashSet<_> = self.groups.iter().flat_map(|g| &g.tab_ids).collect();
        for id in ids {
            if !referenced.contains(id) {
                documents.remove(id);
            }
        }
    }
    #[must_use]
    pub fn layout(&self, documents: &HashMap<String, SessionTabDocument>) -> SessionLayout {
        let groups: Vec<_> = self
            .groups
            .iter()
            .filter_map(|group| {
                let tabs: Vec<_> = group
                    .tab_ids
                    .iter()
                    .filter_map(|id| {
                        let document = documents.get(id)?;
                        Some((
                            id,
                            SessionLayoutTab {
                                collection_id: document.collection_id.clone()?,
                                request_id: document.request_id.clone(),
                            },
                        ))
                    })
                    .collect();
                if tabs.is_empty() {
                    return None;
                }
                Some((
                    &group.id,
                    SessionLayoutGroup {
                        selected_index: tabs
                            .iter()
                            .position(|(id, _)| Some(*id) == group.selected_tab_id.as_ref())
                            .map(|i| i64::try_from(i).unwrap_or(i64::MAX)),
                        preview_index: tabs
                            .iter()
                            .position(|(id, _)| self.preview(id, documents))
                            .map(|i| i64::try_from(i).unwrap_or(i64::MAX)),
                        tabs: tabs.into_iter().map(|(_, tab)| tab).collect(),
                    },
                ))
            })
            .collect();
        SessionLayout {
            active_group_index: groups
                .iter()
                .position(|(id, _)| *id == &self.active_group_id)
                .map_or(0, |i| i64::try_from(i).unwrap_or(i64::MAX)),
            groups: groups.into_iter().map(|(_, g)| g).collect(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRestoreResult {
    pub documents: Vec<SessionDocument>,
    pub snapshot: SessionTabSnapshot,
}
impl SessionTabs {
    pub fn restore(
        &mut self,
        layout: &SessionLayout,
        resolved: Vec<Option<crate::domain::DomainLocation>>,
        documents: &mut HashMap<String, SessionTabDocument>,
    ) -> Vec<SessionDocument> {
        if !documents.is_empty() {
            return vec![];
        }
        let mut locations = resolved.into_iter();
        let mut groups = vec![];
        let mut restored = vec![];
        let mut active = None;
        for (group_index, saved) in layout.groups.iter().enumerate() {
            let mut group = SessionEditorGroup::default();
            for (tab_index, _) in saved.tabs.iter().enumerate() {
                let Some(Some(location)) = locations.next() else {
                    continue;
                };
                let draft = location.request;
                let document = SessionDocument::new(
                    unique_id(),
                    SessionKind::Http,
                    Some(location.collection_id),
                    None,
                    draft.clone(),
                    Some(draft),
                    "",
                );
                group.tab_ids.push(document.id.clone());
                if saved.selected_index == Some(i64::try_from(tab_index).unwrap_or(i64::MAX)) {
                    group.selected_tab_id = Some(document.id.clone());
                }
                if saved.preview_index == Some(i64::try_from(tab_index).unwrap_or(i64::MAX)) {
                    group.preview_tab_id = Some(document.id.clone());
                }
                documents.insert(
                    document.id.clone(),
                    SessionTabDocument::from_document(&document),
                );
                restored.push(document);
            }
            if group.tab_ids.is_empty() {
                continue;
            }
            if group.selected_tab_id.is_none() {
                group.selected_tab_id = group.tab_ids.last().cloned();
            }
            if layout.active_group_index == i64::try_from(group_index).unwrap_or(i64::MAX) {
                active = Some(group.id.clone());
            }
            groups.push(group);
        }
        if let Some(first) = groups.first() {
            self.active_group_id = active.unwrap_or_else(|| first.id.clone());
            self.groups = groups;
        }
        restored
    }
}
/// # Errors
/// Returns a session-layout decoding diagnostic for invalid preference bytes.
pub fn decode_layout(data: &[u8]) -> Result<SessionLayout, super::persistence::WorkspaceError> {
    super::serialization::decode(data, "session layout")
}
/// # Errors
/// Returns a session-layout encoding diagnostic if serialization fails.
pub fn encode_layout(
    layout: &SessionLayout,
) -> Result<Vec<u8>, super::persistence::WorkspaceError> {
    super::serialization::encode(layout, "session layout")
}
#[must_use]
pub fn existing_workspace_paths(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .filter(|path| std::path::Path::new(path).exists())
        .collect()
}
