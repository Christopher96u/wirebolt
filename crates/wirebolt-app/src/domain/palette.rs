//! Ephemeral, metadata-only search. Inputs cannot carry request content or credentials.
use super::{
    commands::{CommandContext, WorkspaceCommand},
    models::DomainGroup,
    sidebar::{matches, normalize},
};
use std::collections::{HashMap, HashSet};

const RESULT_LIMIT: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRequest {
    pub id: String,
    pub name: String,
    pub method: String,
    pub web_socket: bool,
    pub group_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteCollection {
    pub id: String,
    pub name: String,
    pub groups: Vec<DomainGroup>,
    pub requests: Vec<PaletteRequest>,
}
impl From<&super::models::DomainCollection> for PaletteCollection {
    fn from(collection: &super::models::DomainCollection) -> Self {
        Self {
            id: collection.id.clone(),
            name: collection.name.clone(),
            groups: collection.groups.clone(),
            requests: collection
                .requests
                .iter()
                .map(|location| PaletteRequest {
                    id: location.request.id.clone(),
                    name: location.request.name.clone(),
                    method: location.request.method.clone(),
                    web_socket: location.request.web_socket,
                    group_id: location.group_id.clone(),
                })
                .collect(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteTab {
    pub group_id: String,
    pub tab_id: String,
    pub collection_id: Option<String>,
    pub request_id: String,
    pub title: String,
    pub method: String,
    pub web_socket: bool,
    pub group_index: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteChoice {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteThemeKind {
    BuiltIn,
    Custom,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteTheme {
    pub id: String,
    pub name: String,
    pub kind: PaletteThemeKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteEnvironmentSelection {
    Globals,
    Environment { id: String },
}
impl PaletteEnvironmentSelection {
    fn id(&self) -> String {
        match self {
            Self::Globals => "choice:globals-only".into(),
            Self::Environment { id } => format!("environment:{id}"),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteTabTarget {
    pub group_id: String,
    pub tab_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteFailure {
    DestinationRemoved,
    RequestRemoved,
    TabClosed,
    EnvironmentRemoved,
    ThemeRemoved,
    CommandUnavailable { reason: String },
    CreateFailed,
    SaveFailed,
}
impl PaletteFailure {
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::DestinationRemoved => "This destination is no longer available.",
            Self::RequestRemoved => "This request is no longer available.",
            Self::TabClosed => "This tab is no longer open.",
            Self::EnvironmentRemoved => "This environment is no longer available.",
            Self::ThemeRemoved => "This theme is no longer available.",
            Self::CommandUnavailable { reason } => reason,
            Self::CreateFailed => {
                "The request could not be created. Check the workspace and try again."
            }
            Self::SaveFailed => {
                "The request could not be saved. Check the workspace and try again."
            }
        }
        .into()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "UniFFI carries one bounded result record by value."
)]
pub enum PaletteResolution {
    Ready { result: PaletteResult },
    Failed { reason: PaletteFailure },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteDestination {
    Request {
        collection_id: String,
        request_id: String,
    },
    Tab {
        group_id: String,
        tab_id: String,
    },
    Command {
        command: WorkspaceCommand,
    },
    Environment {
        selection: PaletteEnvironmentSelection,
    },
    Theme {
        id: String,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaletteFilter {
    #[default]
    All,
    Requests,
    Actions,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PalettePage {
    #[default]
    Root,
    Environments,
    Themes,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteResult {
    pub id: String,
    pub destination: PaletteDestination,
    pub title: String,
    pub subtitle: String,
    pub method: String,
    pub web_socket: bool,
    pub shortcut: String,
    pub unavailable_reason: Option<String>,
    pub current: bool,
    pub marker: String,
    pub open_tab: Option<PaletteTabTarget>,
    pub accessibility_label: String,
}
impl PaletteResult {
    fn new(id: String, destination: PaletteDestination, title: String) -> Self {
        Self {
            id,
            destination,
            title,
            subtitle: String::new(),
            method: String::new(),
            web_socket: false,
            shortcut: String::new(),
            unavailable_reason: None,
            current: false,
            marker: String::new(),
            open_tab: None,
            accessibility_label: String::new(),
        }
    }
    fn accessible(mut self) -> Self {
        self.accessibility_label = [
            self.method.as_str(),
            &self.title,
            &self.subtitle,
            &self.marker,
            if self.current { "Current" } else { "" },
            self.unavailable_reason.as_deref().unwrap_or(""),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
        self
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaletteSnapshot {
    pub revision: u64,
    pub page: PalettePage,
    pub filter: PaletteFilter,
    pub query: String,
    pub selected_id: Option<String>,
    pub results: Vec<PaletteResult>,
    pub total: u64,
    pub effective_filter: PaletteFilter,
    pub page_title: String,
    pub search_placeholder: String,
    pub search_label: String,
    pub result_summary: String,
}
#[derive(Clone, Debug)]
struct Entry {
    result: PaletteResult,
    title: String,
    alternate_titles: Vec<String>,
    secondary: String,
    corpus: String,
}
impl Entry {
    fn add_title(&mut self, title: String) {
        if title != self.title && !self.alternate_titles.contains(&title) {
            self.corpus.push(' ');
            self.corpus.push_str(&title);
            self.alternate_titles.push(title);
        }
    }
    fn new(result: PaletteResult, secondary: &str, synonyms: &str) -> Self {
        let title = normalize(&result.title);
        let secondary = normalize(secondary);
        let corpus = format!(
            "{title} {secondary} {} {}",
            normalize(&result.method),
            normalize(synonyms)
        );
        Self {
            result,
            title,
            alternate_titles: vec![],
            secondary,
            corpus,
        }
    }
}
#[derive(Debug, Default)]
pub struct CommandPalette {
    saved: Vec<Entry>,
    requests: Vec<Entry>,
    tabs: Vec<Entry>,
    tab_metadata: Vec<PaletteTab>,
    environments: Vec<Entry>,
    themes: Vec<Entry>,
    context: CommandContext,
    active_group: String,
    group_count: u64,
    environment_metadata: Option<(Vec<PaletteChoice>, PaletteEnvironmentSelection)>,
    theme_metadata: Option<(Vec<PaletteTheme>, String)>,
    normalized_requests: u64,
    state: PaletteSnapshot,
    parent: Option<PaletteSnapshot>,
}
impl CommandPalette {
    /// Reuses normalized request entries; a rename only normalizes its changed entry.
    pub fn replace_workspace(&mut self, collections: Vec<PaletteCollection>) {
        let old: HashMap<_, _> = self
            .saved
            .drain(..)
            .map(|e| (e.result.id.clone(), e))
            .collect();
        for collection in collections {
            let folders: HashMap<_, _> = collection
                .groups
                .iter()
                .map(|g| (g.id.as_str(), g))
                .collect();
            for request in collection.requests {
                let mut names = vec![];
                let mut parent = request.group_id.as_deref();
                let mut seen = HashSet::new();
                while let Some(group) = parent.and_then(|id| folders.get(id)) {
                    if !seen.insert(&group.id) {
                        break;
                    }
                    names.push(group.name.as_str());
                    parent = group.parent_id.as_deref();
                }
                names.reverse();
                names.insert(0, &collection.name);
                let mut result = PaletteResult::new(
                    format!("request:{}:{}", collection.id, request.id),
                    PaletteDestination::Request {
                        collection_id: collection.id.clone(),
                        request_id: request.id,
                    },
                    request.name,
                );
                result.subtitle = names.join(" / ");
                result.method = method_label(request.method, request.web_socket);
                result.web_socket = request.web_socket;
                result = result.accessible();
                if let Some(entry) = old.get(&result.id).filter(|e| e.result == result) {
                    self.saved.push(entry.clone());
                } else {
                    self.normalized_requests += 1;
                    self.saved
                        .push(Entry::new(result.clone(), &result.subtitle, ""));
                }
            }
        }
        self.merge_tabs();
        self.state.revision += 1;
    }
    #[must_use]
    pub const fn normalized_requests(&self) -> u64 {
        self.normalized_requests
    }

    /// Only changed bounded tab metadata is normalized; no draft synchronization.
    pub fn update_tabs(&mut self, tabs: Vec<PaletteTab>, active_group: String, group_count: u64) {
        if tabs == self.tab_metadata
            && active_group == self.active_group
            && group_count == self.group_count
        {
            return;
        }
        self.group_count = group_count;
        self.tab_metadata = tabs;
        self.active_group = active_group;
        self.merge_tabs();
        self.state.revision += 1;
    }
    fn merge_tabs(&mut self) {
        self.requests = self.saved.clone();
        self.tabs.clear();
        let mut saved: HashMap<_, _> = self
            .requests
            .iter_mut()
            .filter_map(|entry| match &entry.result.destination {
                PaletteDestination::Request {
                    collection_id,
                    request_id,
                } => Some(((collection_id.clone(), request_id.clone()), entry)),
                _ => None,
            })
            .collect();
        let multiple_groups = self.group_count > 1;
        for tab in &self.tab_metadata {
            if let Some(entry) = tab
                .collection_id
                .as_ref()
                .and_then(|id| saved.get_mut(&(id.clone(), tab.request_id.clone())))
            {
                if entry.result.open_tab.is_some()
                    && (tab.group_id != self.active_group
                        || entry
                            .result
                            .open_tab
                            .as_ref()
                            .is_some_and(|t| t.group_id == self.active_group))
                {
                    continue;
                }
                let mut result = entry.result.clone();
                result.title.clone_from(&tab.title);
                result.method = method_label(tab.method.clone(), tab.web_socket);
                result.web_socket = tab.web_socket;
                result.open_tab = Some(PaletteTabTarget {
                    group_id: tab.group_id.clone(),
                    tab_id: tab.tab_id.clone(),
                });
                result.marker = if tab.group_id == self.active_group {
                    "Open".into()
                } else {
                    format!("Open in split {}", tab.group_index + 1)
                };
                **entry = Entry::new(result.clone().accessible(), &result.subtitle, "");
            } else {
                self.tabs.push(Self::unsaved_tab(tab, multiple_groups));
            }
        }
        for tab in &self.tab_metadata {
            if let Some(entry) = tab
                .collection_id
                .as_ref()
                .and_then(|id| saved.get_mut(&(id.clone(), tab.request_id.clone())))
            {
                entry.add_title(normalize(&tab.title));
                entry.corpus.push(' ');
                entry.corpus.push_str(&normalize(&method_label(
                    tab.method.clone(),
                    tab.web_socket,
                )));
            }
        }
        for source in &self.saved {
            if let PaletteDestination::Request {
                collection_id,
                request_id,
            } = &source.result.destination
                && let Some(entry) = saved.get_mut(&(collection_id.clone(), request_id.clone()))
            {
                entry.add_title(source.title.clone());
                entry.corpus.push(' ');
                entry.corpus.push_str(&normalize(&source.result.method));
            }
        }
    }
    fn unsaved_tab(tab: &PaletteTab, multiple_groups: bool) -> Entry {
        let mut result = PaletteResult::new(
            format!("tab:{}:{}", tab.group_id, tab.tab_id),
            PaletteDestination::Tab {
                group_id: tab.group_id.clone(),
                tab_id: tab.tab_id.clone(),
            },
            tab.title.clone(),
        );
        result.method = method_label(tab.method.clone(), tab.web_socket);
        result.web_socket = tab.web_socket;
        result.subtitle = if multiple_groups {
            format!("Unsaved · Split {}", tab.group_index + 1)
        } else {
            "Unsaved".into()
        };
        Entry::new(result.accessible(), "", "")
    }
    pub fn update_choices(
        &mut self,
        environments: Vec<PaletteChoice>,
        themes: Vec<PaletteTheme>,
        selected_environment: PaletteEnvironmentSelection,
        selected_theme: String,
    ) {
        let environment_metadata = (environments, selected_environment);
        if self.environment_metadata.as_ref() != Some(&environment_metadata) {
            self.environments =
                std::iter::once((PaletteEnvironmentSelection::Globals, "Globals only".into()))
                    .chain(environment_metadata.0.iter().map(|e| {
                        (
                            PaletteEnvironmentSelection::Environment { id: e.id.clone() },
                            e.name.clone(),
                        )
                    }))
                    .map(|(selection, name)| {
                        let mut result = PaletteResult::new(
                            selection.id(),
                            PaletteDestination::Environment {
                                selection: selection.clone(),
                            },
                            name,
                        );
                        result.current = selection == environment_metadata.1;
                        Entry::new(result.accessible(), "", "")
                    })
                    .collect();
            self.environment_metadata = Some(environment_metadata);
            self.state.revision += 1;
        }
        let theme_metadata = (themes, selected_theme);
        if self.theme_metadata.as_ref() != Some(&theme_metadata) {
            self.themes = theme_metadata
                .0
                .iter()
                .map(|choice| {
                    let mut result = PaletteResult::new(
                        format!("theme:{}", choice.id),
                        PaletteDestination::Theme {
                            id: choice.id.clone(),
                        },
                        choice.name.clone(),
                    );
                    result.subtitle = match choice.kind {
                        PaletteThemeKind::BuiltIn => "Built-in",
                        PaletteThemeKind::Custom => "Custom",
                    }
                    .into();
                    result.current = choice.id == theme_metadata.1;
                    Entry::new(result.accessible(), "", "")
                })
                .collect();
            self.theme_metadata = Some(theme_metadata);
            self.state.revision += 1;
        }
    }
    pub fn set_context(&mut self, context: CommandContext) {
        self.context = context;
    }
    pub fn open(&mut self) -> PaletteSnapshot {
        self.parent = None;
        self.state.page = PalettePage::Root;
        self.state.query.clear();
        self.state.filter = PaletteFilter::All;
        self.state.selected_id = None;
        self.search()
    }
    pub fn query(&mut self, query: String, filter: PaletteFilter) -> PaletteSnapshot {
        self.state.query = query;
        self.state.filter = filter;
        self.search()
    }
    pub fn refresh(&mut self) -> PaletteSnapshot {
        self.search()
    }
    pub fn move_selection(&mut self, delta: i64) -> PaletteSnapshot {
        let len = self.state.results.len();
        if len > 0 {
            let index = self
                .state
                .results
                .iter()
                .position(|r| Some(&r.id) == self.state.selected_id.as_ref())
                .unwrap_or(0);
            let next = (i64::try_from(index).unwrap_or(0) + delta)
                .rem_euclid(i64::try_from(len).unwrap_or(1));
            self.state.selected_id = Some(
                self.state.results[usize::try_from(next).unwrap_or(0)]
                    .id
                    .clone(),
            );
        }
        self.state.clone()
    }
    pub fn select(&mut self, id: &str) -> PaletteSnapshot {
        if self.state.results.iter().any(|r| r.id == id) {
            self.state.selected_id = Some(id.into());
        }
        self.state.clone()
    }
    /// Re-resolves stable identity against current metadata, never a row offset.
    #[must_use]
    pub fn resolve(&self, id: &str) -> PaletteResolution {
        let commands = self.commands();
        let result = self
            .requests
            .iter()
            .chain(&self.tabs)
            .chain(&self.environments)
            .chain(&self.themes)
            .chain(&commands)
            .find(|e| e.result.id == id)
            .map(|e| e.result.clone());
        match result {
            Some(result) => match &result.unavailable_reason {
                Some(reason) => PaletteResolution::Failed {
                    reason: PaletteFailure::CommandUnavailable {
                        reason: reason.clone(),
                    },
                },
                None => PaletteResolution::Ready { result },
            },
            None => PaletteResolution::Failed {
                reason: if id.starts_with("request:") {
                    PaletteFailure::RequestRemoved
                } else if id.starts_with("tab:") {
                    PaletteFailure::TabClosed
                } else if id.starts_with("environment:") {
                    PaletteFailure::EnvironmentRemoved
                } else if id.starts_with("theme:") {
                    PaletteFailure::ThemeRemoved
                } else {
                    PaletteFailure::DestinationRemoved
                },
            },
        }
    }
    pub fn enter(&mut self, page: PalettePage) -> PaletteSnapshot {
        if self.state.page == PalettePage::Root && page != PalettePage::Root {
            self.parent = Some(self.state.clone());
            self.state.page = page;
            self.state.query.clear();
            self.state.selected_id = None;
            self.search();
            self.state.selected_id = self
                .state
                .results
                .iter()
                .find(|r| r.current)
                .map(|r| r.id.clone())
                .or(self.state.selected_id.take());
        }
        self.state.clone()
    }
    pub fn back(&mut self) -> Option<PaletteSnapshot> {
        let parent = self.parent.take()?;
        self.state = parent;
        Some(self.search())
    }
    fn commands(&self) -> Vec<Entry> {
        WorkspaceCommand::ALL
            .iter()
            .filter(|c| **c != WorkspaceCommand::CommandPalette)
            .map(|command| {
                let descriptor = command.descriptor(&self.context);
                let mut result = PaletteResult::new(
                    format!("command:{}", command.id()),
                    PaletteDestination::Command { command: *command },
                    descriptor.title,
                );
                result.shortcut = descriptor.shortcut.map_or_else(String::new, |s| s.label);
                result.unavailable_reason = descriptor.unavailable_reason;
                Entry::new(result.accessible(), "", &descriptor.synonyms)
            })
            .collect()
    }
    fn root_entries<'a>(&'a self, query: &str, commands: &'a [Entry]) -> Vec<&'a Entry> {
        let filter = self.state.effective_filter;
        let mut entries = vec![];
        if filter != PaletteFilter::Actions {
            if query.is_empty() {
                let mut seen = HashSet::new();
                for tab in &self.tab_metadata {
                    let entry = self.requests.iter().find(|e| e.result.open_tab.as_ref().is_some_and(|t| t.group_id == tab.group_id && t.tab_id == tab.tab_id))
                                .or_else(|| self.tabs.iter().find(|e| matches!(&e.result.destination, PaletteDestination::Tab { group_id, tab_id } if *group_id == tab.group_id && *tab_id == tab.tab_id)));
                    if let Some(entry) = entry.filter(|e| seen.insert(e.result.id.clone())) {
                        entries.push(entry);
                    }
                }
            } else {
                entries.extend(&self.tabs);
                entries.extend(&self.requests);
            }
        }
        if filter != PaletteFilter::Requests {
            entries.extend(commands.iter().filter(|entry| {
                !query.is_empty()
                    || matches!(
                        entry.result.destination,
                        PaletteDestination::Command {
                            command: WorkspaceCommand::NewHttp
                                | WorkspaceCommand::FilterRequests
                                | WorkspaceCommand::SwitchEnvironment
                                | WorkspaceCommand::ChangeTheme
                                | WorkspaceCommand::Settings
                        }
                    )
            }));
        }
        entries
    }
    fn search(&mut self) -> PaletteSnapshot {
        let text = self.state.query.trim();
        let actions_only = text.starts_with('>') && self.state.page == PalettePage::Root;
        let query = normalize(if actions_only { text[1..].trim() } else { text });
        let words: Vec<_> = query.split_whitespace().collect();
        self.state.effective_filter = if actions_only {
            PaletteFilter::Actions
        } else {
            self.state.filter
        };
        let (title, label) = match self.state.page {
            PalettePage::Root => ("Command Palette", "Search requests or actions"),
            PalettePage::Environments => ("Switch environment", "Search environments"),
            PalettePage::Themes => ("Change theme", "Search themes"),
        };
        self.state.page_title = title.into();
        self.state.search_label = label.into();
        self.state.search_placeholder = format!("{label}…");
        let commands = self.commands();
        let entries: Vec<&Entry> = match self.state.page {
            PalettePage::Environments => self.environments.iter().collect(),
            PalettePage::Themes => self.themes.iter().collect(),
            PalettePage::Root => self.root_entries(&query, &commands),
        };
        let mut ranked: Vec<_> = entries
            .into_iter()
            .enumerate()
            .filter_map(|(order, entry)| {
                if !words.iter().all(|word| matches(&entry.corpus, word)) {
                    return None;
                }
                let rank = std::iter::once(&entry.title)
                    .chain(&entry.alternate_titles)
                    .map(|title| {
                        if query.is_empty() || *title == query {
                            0
                        } else if title.starts_with(&query) {
                            1
                        } else if matches(title, &query) {
                            2
                        } else {
                            3
                        }
                    })
                    .min()
                    .unwrap_or(3);
                let secondary = !query.is_empty() && !matches(&entry.secondary, &query);
                let open = match &entry.result.destination {
                    PaletteDestination::Tab { .. } => true,
                    PaletteDestination::Request { .. } => entry.result.open_tab.is_some(),
                    _ => false,
                };
                Some(((rank, secondary, !open, order), &entry.result))
            })
            .collect();
        if !query.is_empty() {
            ranked.sort_by_key(|(rank, _)| *rank);
        }
        let total = u64::try_from(ranked.len()).unwrap_or(u64::MAX);
        let result_summary = if ranked.len() > RESULT_LIMIT {
            format!("{RESULT_LIMIT} of {} · Refine search", ranked.len())
        } else {
            format!(
                "{} {}",
                ranked.len(),
                if ranked.len() == 1 {
                    "result"
                } else {
                    "results"
                }
            )
        };
        let results = ranked
            .into_iter()
            .take(RESULT_LIMIT)
            .map(|(_, r)| r.clone())
            .collect();
        self.state.total = total;
        self.state.result_summary = result_summary;
        self.state.results = results;
        if !self
            .state
            .results
            .iter()
            .any(|r| Some(&r.id) == self.state.selected_id.as_ref())
        {
            self.state.selected_id = self.state.results.first().map(|r| r.id.clone());
        }
        self.state.clone()
    }
}

fn method_label(method: String, web_socket: bool) -> String {
    if web_socket { "WS".into() } else { method }
}
