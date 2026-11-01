//! Saved-request identity is separate from the native per-row observation flags.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeRequestKey {
    pub collection_id: String,
    pub request_id: String,
}
#[must_use]
pub fn active_request_key(
    collection_id: Option<String>,
    request_id: Option<String>,
) -> Option<RuntimeRequestKey> {
    Some(RuntimeRequestKey {
        collection_id: collection_id?,
        request_id: request_id?,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tabs::*;
    use std::collections::HashMap;
    fn key(
        tabs: &SessionTabs,
        documents: &HashMap<String, SessionTabDocument>,
    ) -> Option<RuntimeRequestKey> {
        let group = tabs.groups.iter().find(|g| g.id == tabs.active_group_id)?;
        let document = documents.get(group.selected_tab_id.as_ref()?)?;
        active_request_key(
            document.collection_id.clone(),
            Some(document.request_id.clone()),
        )
    }
    #[test]
    fn saved_requests_follow_selection_and_unsaved_documents_select_none() {
        let mut tabs = SessionTabs::default();
        let mut docs = HashMap::new();
        assert_eq!(key(&tabs, &docs), None);
        for id in ["a", "b"] {
            tabs.open(
                SessionTabDocument {
                    id: id.into(),
                    kind: SessionKind::Http,
                    collection_id: Some("api".into()),
                    request_id: id.into(),
                    dirty: false,
                    touched: false,
                },
                &mut docs,
                None,
                false,
                false,
            );
            assert_eq!(
                key(&tabs, &docs),
                active_request_key(Some("api".into()), Some(id.into()))
            );
        }
        tabs.select("a", None);
        assert_eq!(key(&tabs, &docs).unwrap().request_id, "a");
        tabs.open(
            SessionTabDocument {
                id: "scratch".into(),
                kind: SessionKind::Http,
                collection_id: None,
                request_id: "scratch".into(),
                dirty: false,
                touched: false,
            },
            &mut docs,
            None,
            false,
            false,
        );
        assert_eq!(key(&tabs, &docs), None);
        tabs.select("b", None);
        assert_eq!(key(&tabs, &docs).unwrap().request_id, "b");
    }
    #[test]
    fn moving_active_request_changes_only_its_collection_identity() {
        let original = active_request_key(Some("api".into()), Some("a".into())).unwrap();
        let moved = active_request_key(Some("moved".into()), Some("a".into())).unwrap();
        assert_ne!(original, moved);
        assert_eq!(original.request_id, moved.request_id);
        assert!(active_request_key(None, Some("a".into())).is_none());
        assert!(active_request_key(Some("api".into()), None).is_none());
    }
}
