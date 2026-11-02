//! One indexed outline per workspace revision; filtering returns row positions in one batch.
use super::models::{DomainCollection, DomainEnvironment, DomainGroup};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet, VecDeque},
};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use unicode_segmentation::UnicodeSegmentation;

#[must_use]
pub fn normalize(text: &str) -> String {
    if text.is_ascii() {
        return text.to_ascii_lowercase();
    }
    text.case_fold()
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .nfc()
        .collect()
}
#[must_use]
pub fn natural_compare(a: &str, b: &str) -> Ordering {
    let a = normalize(a);
    let b = normalize(b);
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek(), b.peek()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut x = String::new();
                let mut y = String::new();
                while a.peek().is_some_and(char::is_ascii_digit) {
                    x.push(a.next().unwrap_or_default());
                }
                while b.peek().is_some_and(char::is_ascii_digit) {
                    y.push(b.next().unwrap_or_default());
                }
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let cmp = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if cmp != Ordering::Equal {
                    return cmp;
                }
            }
            _ => {
                let cmp = a.next().cmp(&b.next());
                if cmp != Ordering::Equal {
                    return cmp;
                }
            }
        }
    }
}
#[must_use]
pub fn request_path(url: &str) -> String {
    let text = url.trim();
    if text.is_empty() {
        return String::new();
    }
    let path = text.split(['?', '#']).next().unwrap_or_default();
    let path = if let Some(rest) = path
        .strip_prefix("{{")
        .and_then(|s| s.find("}}").map(|i| &s[i + 2..]))
    {
        rest
    } else {
        let authority = if let Some((scheme, rest)) = path.split_once("://") {
            if scheme.is_empty() {
                return String::new();
            }
            rest
        } else if let Some(rest) = path.strip_prefix("//") {
            rest
        } else {
            return if path.starts_with('/') {
                path.into()
            } else {
                String::new()
            };
        };
        if authority.is_empty() || authority.starts_with('/') {
            return String::new();
        }
        authority.find('/').map_or("", |i| &authority[i..])
    };
    if path.is_empty() {
        "/".into()
    } else {
        path.into()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarRow {
    pub id: String,
    pub depth: u64,
    pub parent_id: Option<String>,
    pub collection_index: u64,
    pub group_index: Option<u64>,
    pub request_index: Option<u64>,
    pub collection_id: String,
    pub title: String,
    pub move_identifier: Option<String>,
    pub request_path: String,
    pub ancestors: Vec<u64>,
    pub search: String,
}
#[derive(Debug)]
pub struct SidebarIndex {
    pub rows: Vec<SidebarRow>,
}
impl SidebarIndex {
    #[must_use]
    pub fn new(collections: &[DomainCollection]) -> Self {
        Self::from_sources(
            &collections
                .iter()
                .map(DomainSidebarCollection::from)
                .collect::<Vec<_>>(),
        )
    }
    #[must_use]
    pub fn from_sources(collections: &[DomainSidebarCollection]) -> Self {
        let mut order: Vec<_> = (0..collections.len()).collect();
        order.sort_by(|&a, &b| {
            let (a, b) = (&collections[a], &collections[b]);
            a.order
                .cmp(&b.order)
                .then_with(|| canonical_compare(&a.name, &b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut rows = vec![];
        for ci in order {
            append_collection(&mut rows, &collections[ci], ci);
        }
        Self { rows }
    }
    #[must_use]
    pub fn visible(
        &self,
        query: &str,
        collapsed: &HashSet<String>,
        expanded: &HashSet<String>,
    ) -> Vec<u64> {
        let query = normalize(query.trim());
        let mut included = vec![false; self.rows.len()];
        let mut matching = vec![false; self.rows.len()];
        let mut result = vec![];
        for (i, row) in self.rows.iter().enumerate() {
            if query.is_empty() {
                if row
                    .ancestors
                    .iter()
                    .any(|&a| matching[usize::try_from(a).unwrap_or(usize::MAX)])
                {
                    continue;
                }
                result.push(i as u64);
                matching[i] = if row.group_index.is_some() {
                    !expanded.contains(&row.id.replacen("group:", "", 1))
                } else {
                    row.request_index.is_none() && collapsed.contains(&row.collection_id)
                };
            } else if matches(&row.search, &query)
                || row
                    .ancestors
                    .iter()
                    .any(|&a| matching[usize::try_from(a).unwrap_or(usize::MAX)])
            {
                matching[i] = true;
                included[i] = true;
                for &a in &row.ancestors {
                    included[usize::try_from(a).unwrap_or(usize::MAX)] = true;
                }
            }
        }
        if query.is_empty() {
            result
        } else {
            included
                .into_iter()
                .enumerate()
                .filter_map(|(i, yes)| yes.then_some(i as u64))
                .collect()
        }
    }
    #[must_use]
    pub fn sibling(&self, id: &str, delta: i64) -> Option<u64> {
        if delta != 1 && delta != -1 {
            return None;
        }
        let i = self.rows.iter().position(|r| r.id == id)?;
        let row = &self.rows[i];
        let mut p = if delta > 0 {
            i.checked_add(1)?
        } else {
            i.checked_sub(1)?
        };
        while let Some(candidate) = self.rows.get(p) {
            if candidate.depth < row.depth {
                return None;
            }
            if candidate.depth == row.depth {
                return (candidate.parent_id == row.parent_id
                    && candidate.collection_id == row.collection_id
                    && (candidate.group_index.is_some() || candidate.request_index.is_some()))
                .then_some(p as u64);
            }
            p = if delta > 0 {
                p.checked_add(1)?
            } else {
                p.checked_sub(1)?
            };
        }
        None
    }
}
#[must_use]
pub fn ordered_children(c: &DomainCollection, parent: Option<&str>) -> Vec<String> {
    let mut rows: Vec<_> = c
        .groups
        .iter()
        .filter(|g| g.parent_id.as_deref() == parent)
        .map(|g| (g.order, 0, &g.name, &g.id, format!("group:{}", g.id)))
        .chain(
            c.requests
                .iter()
                .filter(|r| r.group_id.as_deref() == parent)
                .map(|r| {
                    (
                        r.order,
                        1,
                        &r.request.name,
                        &r.request.id,
                        format!("request:{}", r.request.id),
                    )
                }),
        )
        .collect();
    rows.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then_with(|| canonical_compare(a.2, b.2))
            .then_with(|| a.3.cmp(b.3))
    });
    rows.into_iter().map(|r| r.4).collect()
}
#[must_use]
pub fn subtree(c: &DomainCollection, root: Option<&str>) -> Vec<u64> {
    group_subtree(&c.groups, root)
}
fn group_subtree(groups: &[DomainGroup], root: Option<&str>) -> Vec<u64> {
    let mut result: Vec<_> = root
        .and_then(|id| groups.iter().position(|g| g.id == id))
        .into_iter()
        .map(|i| i as u64)
        .collect();
    let mut visited: HashSet<_> = result
        .iter()
        .map(|&i| groups[usize::try_from(i).unwrap_or(usize::MAX)].id.as_str())
        .collect();
    let mut queue = VecDeque::from([root]);
    while let Some(parent) = queue.pop_front() {
        for (i, g) in groups.iter().enumerate() {
            if g.parent_id.as_deref() == parent && visited.insert(&g.id) {
                result.push(i as u64);
                queue.push_back(Some(&g.id));
            }
        }
    }
    result
}
#[must_use]
pub fn folder_outline(c: &DomainCollection) -> Vec<Vec<u64>> {
    let index = SidebarIndex::new(std::slice::from_ref(c));
    index
        .rows
        .into_iter()
        .filter_map(|r| {
            r.group_index
                .map(|i| vec![i, r.depth - u64::from(c.id != "workspace-root")])
        })
        .collect()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarOutcome {
    Select { id: String },
    Expand { id: String },
    Collapse { id: String },
    None,
}
#[must_use]
pub fn step(rows: &[SidebarRow], current: Option<&str>, delta: i64) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let index = current.and_then(|id| rows.iter().position(|r| r.id == id));
    let i = index.map_or_else(
        || if delta > 0 { 0 } else { rows.len() - 1 },
        |i| {
            let offset = usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX);
            if delta < 0 {
                i.saturating_sub(offset)
            } else {
                i.saturating_add(offset).min(rows.len() - 1)
            }
        },
    );
    Some(rows[i].id.clone())
}
#[must_use]
pub fn arrow(
    rows: &[SidebarRow],
    current: Option<&str>,
    right: bool,
    expanded: bool,
) -> SidebarOutcome {
    let Some(i) = current.and_then(|id| rows.iter().position(|r| r.id == id)) else {
        return SidebarOutcome::None;
    };
    let row = &rows[i];
    if right {
        if row.request_index.is_some() {
            return SidebarOutcome::None;
        }
        if !expanded {
            return SidebarOutcome::Expand { id: row.id.clone() };
        }
        if let Some(next) = rows
            .get(i + 1)
            .filter(|r| r.parent_id.as_ref() == Some(&row.id))
        {
            return SidebarOutcome::Select {
                id: next.id.clone(),
            };
        }
    } else {
        if row.request_index.is_none() && expanded {
            return SidebarOutcome::Collapse { id: row.id.clone() };
        }
        if let Some(parent) = row
            .parent_id
            .as_ref()
            .filter(|id| rows.iter().any(|r| r.id == **id))
        {
            return SidebarOutcome::Select { id: parent.clone() };
        }
    }
    SidebarOutcome::None
}
#[must_use]
pub fn type_select(rows: &[SidebarRow], current: Option<&str>, prefix: &str) -> Option<String> {
    let prefix = normalize(prefix);
    if prefix.is_empty() || rows.is_empty() {
        return None;
    }
    let index = current.and_then(|id| rows.iter().position(|r| r.id == id));
    let start = index.map_or(0, |i| i + usize::from(prefix.chars().count() == 1));
    (0..rows.len())
        .map(|offset| &rows[(start + offset) % rows.len()])
        .find(|r| normalize(&r.title).starts_with(&prefix))
        .map(|r| r.id.clone())
}

fn append_collection(rows: &mut Vec<SidebarRow>, c: &DomainSidebarCollection, ci: usize) {
    let mut ancestors = vec![];
    if c.id != "workspace-root" {
        ancestors.push(rows.len() as u64);
        rows.push(SidebarRow {
            id: format!("collection:{}", c.id),
            depth: 0,
            parent_id: None,
            collection_index: ci as u64,
            group_index: None,
            request_index: None,
            collection_id: c.id.clone(),
            title: c.name.clone(),
            move_identifier: None,
            request_path: String::new(),
            ancestors: vec![],
            search: normalize(&c.name),
        });
    }
    let mut children: HashMap<&str, Vec<ChildIndex<'_>>> = HashMap::new();
    for (i, g) in c.groups.iter().enumerate() {
        children
            .entry(g.parent_id.as_deref().unwrap_or_default())
            .or_default()
            .push((g.order, 0, &g.name, &g.id, i));
    }
    for (i, r) in c.requests.iter().enumerate() {
        children
            .entry(r.group_id.as_deref().unwrap_or_default())
            .or_default()
            .push((r.order, 1, &r.name, &r.id, i));
    }
    for list in children.values_mut() {
        list.sort_unstable_by(compare_child);
    }
    // An explicit stack keeps malformed/deep input from overflowing the process stack.
    let mut stack: Vec<_> = children
        .get("")
        .into_iter()
        .flatten()
        .rev()
        .map(|child| (*child, ancestors.clone()))
        .collect();
    let mut visited = HashSet::new();
    while let Some(((_, kind, _, _, i), ancestors)) = stack.pop() {
        let (id, title, search, path, movement) = if kind == 0 {
            let g = &c.groups[i];
            if !visited.insert(&g.id) {
                continue;
            }
            (
                format!("group:{}:{}", c.id, g.id),
                g.name.clone(),
                g.name.clone(),
                String::new(),
                format!("group|{}|{}", c.id, g.id),
            )
        } else {
            let r = &c.requests[i];
            (
                format!("request:{}/{}", r.collection_id, r.id),
                r.name.clone(),
                format!("{}\0{}\0{}", r.name, r.method, r.url),
                request_path(&r.url),
                format!("request|{}|{}", r.collection_id, r.id),
            )
        };
        let position = rows.len() as u64;
        let parent_id = ancestors
            .last()
            .map(|&a| rows[usize::try_from(a).unwrap_or(usize::MAX)].id.clone());
        rows.push(SidebarRow {
            id,
            depth: ancestors.len() as u64,
            parent_id,
            collection_index: ci as u64,
            group_index: (kind == 0).then_some(i as u64),
            request_index: (kind == 1).then_some(i as u64),
            collection_id: c.id.clone(),
            title,
            move_identifier: Some(movement),
            request_path: path,
            ancestors: ancestors.clone(),
            search: normalize(&search),
        });
        if kind == 0 {
            let mut ancestors = ancestors;
            ancestors.push(position);
            stack.extend(
                children
                    .get(c.groups[i].id.as_str())
                    .into_iter()
                    .flatten()
                    .rev()
                    .map(|child| (*child, ancestors.clone())),
            );
        }
    }
}

type ChildIndex<'a> = (i64, u8, &'a str, &'a str, usize);

/// Matches complete extended grapheme clusters, as Swift String.contains does.
#[must_use]
pub fn matches(text: &str, query: &str) -> bool {
    if text.is_ascii() {
        return text.contains(query);
    }
    let mut boundaries: Vec<_> = text.grapheme_indices(true).map(|(i, _)| i).collect();
    boundaries.push(text.len());
    text.match_indices(query).any(|(i, _)| {
        boundaries.binary_search(&i).is_ok() && boundaries.binary_search(&(i + query.len())).is_ok()
    })
}

fn canonical_compare(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        a.cmp(b)
    } else {
        a.nfc().cmp(b.nfc())
    }
}

fn compare_child<T>(a: &(i64, u8, &str, &str, T), b: &(i64, u8, &str, &str, T)) -> Ordering {
    a.0.cmp(&b.0)
        .then(a.1.cmp(&b.1))
        .then_with(|| canonical_compare(a.2, b.2))
        .then_with(|| a.3.cmp(b.3))
}

/// A sidebar input intentionally contains no body, authentication, headers or transport data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainSidebarRequest {
    pub id: String,
    pub collection_id: String,
    pub group_id: Option<String>,
    pub order: i64,
    pub name: String,
    pub method: String,
    pub url: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainSidebarCollection {
    pub id: String,
    pub name: String,
    pub order: i64,
    pub groups: Vec<DomainGroup>,
    pub requests: Vec<DomainSidebarRequest>,
}
impl From<&DomainCollection> for DomainSidebarCollection {
    fn from(c: &DomainCollection) -> Self {
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            order: c.order,
            groups: c.groups.clone(),
            requests: c
                .requests
                .iter()
                .map(|r| DomainSidebarRequest {
                    id: r.request.id.clone(),
                    collection_id: r.collection_id.clone(),
                    group_id: r.group_id.clone(),
                    order: r.order,
                    name: r.request.name.clone(),
                    method: r.request.method.clone(),
                    url: r.request.url.clone(),
                })
                .collect(),
        }
    }
}

/// Folder-center drops allow requests across collections and folders within their own tree.
#[must_use]
pub fn can_drop_inside(
    collection: &DomainSidebarCollection,
    folder: &str,
    identifier: &str,
) -> bool {
    let parts: Vec<_> = identifier.split('|').collect();
    match parts.as_slice() {
        ["request", _, _] => true,
        ["group", source, group] if *source == collection.id => {
            !group_subtree(&collection.groups, Some(group))
                .into_iter()
                .any(|i| collection.groups[usize::try_from(i).unwrap_or(usize::MAX)].id == folder)
        }
        _ => false,
    }
}

/// Move menus retain workspace order, with canonical name and ID tie breakers.
#[must_use]
pub fn move_collections(collections: &[DomainSidebarCollection], identifier: &str) -> Vec<u64> {
    let folder = identifier.starts_with("group|");
    let source = identifier.split('|').nth(1);
    let mut indices: Vec<_> = (0..collections.len())
        .filter(|&i| !folder || Some(collections[i].id.as_str()) == source)
        .collect();
    indices.sort_by(|&a, &b| {
        let (a, b) = (&collections[a], &collections[b]);
        a.order
            .cmp(&b.order)
            .then_with(|| canonical_compare(&a.name, &b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
    indices.into_iter().map(|i| i as u64).collect()
}

/// The environment picker excludes globals and sorts names using canonical comparison.
#[must_use]
pub fn selectable_environments(environments: &[DomainEnvironment]) -> Vec<u64> {
    let mut indices: Vec<_> = (0..environments.len())
        .filter(|&i| environments[i].id != "global")
        .collect();
    indices.sort_by(|&a, &b| canonical_compare(&environments[a].name, &environments[b].name));
    indices.into_iter().map(|i| i as u64).collect()
}
