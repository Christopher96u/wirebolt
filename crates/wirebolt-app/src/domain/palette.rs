//! Ephemeral, metadata-only search. Inputs cannot carry request content or credentials.
use super::{
    commands::{CommandContext, WorkspaceCommand},
    models::DomainGroup,
    sidebar::{matches, normalize},
};
use std::collections::{HashMap, HashSet};

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
    pub group_name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteChoice {
    pub id: String,
    pub name: String,
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
        id: Option<String>,
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
    pub shortcut: String,
    pub unavailable_reason: Option<String>,
    pub current: bool,
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
}
#[derive(Clone, Debug)]
struct Entry {
    result: PaletteResult,
    title: String,
    secondary: String,
    corpus: String,
}
impl Entry {
    fn new(result: PaletteResult, synonyms: &str) -> Self {
        let title = normalize(&result.title);
        let secondary = normalize(&result.subtitle);
        let corpus = format!(
            "{title} {secondary} {} {}",
            normalize(&result.method),
            normalize(synonyms)
        );
        Self {
            result,
            title,
            secondary,
            corpus,
        }
    }
}
#[derive(Debug, Default)]
pub struct CommandPalette {
    requests: Vec<Entry>,
    tabs: Vec<Entry>,
    tab_metadata: Vec<PaletteTab>,
    environments: Vec<Entry>,
    themes: Vec<Entry>,
    context: CommandContext,
    active_group: String,
    selected_environment: Option<String>,
    selected_theme: String,
    state: PaletteSnapshot,
    parent: Option<PaletteSnapshot>,
}
impl CommandPalette {
    pub fn replace_workspace(&mut self, collections: Vec<PaletteCollection>) {
        self.requests.clear();
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
                self.requests.push(Entry::new(
                    PaletteResult {
                        id: format!("request:{}:{}", collection.id, request.id),
                        destination: PaletteDestination::Request {
                            collection_id: collection.id.clone(),
                            request_id: request.id,
                        },
                        title: request.name,
                        subtitle: names.join(" / "),
                        method: if request.web_socket {
                            "WS".into()
                        } else {
                            request.method
                        },
                        shortcut: String::new(),
                        unavailable_reason: None,
                        current: false,
                    },
                    "",
                ));
            }
        }
        self.state.revision += 1;
    }
    /// Only new/changed bounded tab metadata is normalized; no draft synchronization.
    pub fn update_tabs(&mut self, tabs: Vec<PaletteTab>, active_group: String) {
        if tabs == self.tab_metadata && active_group == self.active_group {
            return;
        }
        let old: HashMap<_, _> = self
            .tabs
            .drain(..)
            .map(|e| (e.result.id.clone(), e))
            .collect();
        self.tabs = tabs
            .iter()
            .map(|tab| {
                let id = format!("tab:{}:{}", tab.group_id, tab.tab_id);
                if let Some(entry) = old
                    .get(&id)
                    .filter(|_| self.tab_metadata.iter().any(|t| t == tab))
                {
                    return entry.clone();
                }
                Entry::new(
                    PaletteResult {
                        id,
                        destination: PaletteDestination::Tab {
                            group_id: tab.group_id.clone(),
                            tab_id: tab.tab_id.clone(),
                        },
                        title: tab.title.clone(),
                        subtitle: tab.group_name.clone(),
                        method: if tab.web_socket {
                            "WS".into()
                        } else {
                            tab.method.clone()
                        },
                        shortcut: String::new(),
                        unavailable_reason: None,
                        current: false,
                    },
                    "",
                )
            })
            .collect();
        self.tab_metadata = tabs;
        self.active_group = active_group;
        self.state.revision += 1;
    }
    pub fn update_choices(
        &mut self,
        environments: Vec<PaletteChoice>,
        themes: Vec<PaletteChoice>,
        selected_environment: Option<String>,
        selected_theme: String,
    ) {
        self.environments = std::iter::once(PaletteChoice {
            id: String::new(),
            name: "Globals only".into(),
        })
        .chain(environments)
        .map(|choice| {
            let id = (!choice.id.is_empty()).then_some(choice.id.clone());
            Entry::new(
                PaletteResult {
                    id: format!("environment:{}", choice.id),
                    destination: PaletteDestination::Environment { id: id.clone() },
                    title: choice.name,
                    subtitle: "Environment for requests".into(),
                    method: String::new(),
                    shortcut: String::new(),
                    unavailable_reason: None,
                    current: id == selected_environment,
                },
                "",
            )
        })
        .collect();
        self.themes = themes
            .into_iter()
            .map(|choice| {
                Entry::new(
                    PaletteResult {
                        id: format!("theme:{}", choice.id),
                        destination: PaletteDestination::Theme {
                            id: choice.id.clone(),
                        },
                        title: choice.name,
                        subtitle: "Appearance".into(),
                        method: String::new(),
                        shortcut: String::new(),
                        unavailable_reason: None,
                        current: choice.id == selected_theme,
                    },
                    "",
                )
            })
            .collect();
        self.selected_environment = selected_environment;
        self.selected_theme = selected_theme;
        self.state.revision += 1;
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
    pub fn resolve(&mut self, id: &str) -> Option<PaletteResult> {
        self.search();
        self.state.results.iter().find(|r| r.id == id).cloned()
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
    /// Finds a saved request's open destination in the active group first.
    #[must_use]
    pub fn request_tab(&self, collection: &str, request: &str) -> Option<PaletteDestination> {
        self.tab_metadata
            .iter()
            .filter(|t| t.collection_id.as_deref() == Some(collection) && t.request_id == request)
            .min_by_key(|t| t.group_id != self.active_group)
            .map(|t| PaletteDestination::Tab {
                group_id: t.group_id.clone(),
                tab_id: t.tab_id.clone(),
            })
    }
    fn commands(&self) -> Vec<Entry> {
        WorkspaceCommand::ALL
            .iter()
            .map(|command| {
                let descriptor = command.descriptor(&self.context);
                let subtitle = match command {
                    WorkspaceCommand::RequestParams
                    | WorkspaceCommand::RequestHeaders
                    | WorkspaceCommand::RequestBody
                    | WorkspaceCommand::RequestAuth
                    | WorkspaceCommand::RequestNote
                    | WorkspaceCommand::RequestSettings => "Request Section",
                    WorkspaceCommand::ResponseHeaders
                    | WorkspaceCommand::ResponseBody
                    | WorkspaceCommand::ResponseCookies
                    | WorkspaceCommand::ResponseRaw
                    | WorkspaceCommand::ResponseRequest => "Response Section",
                    _ => "Action",
                };
                let shortcut = format!(
                    "{}{}{}{}{}",
                    if descriptor.modifiers & 8 != 0 {
                        "⌃"
                    } else {
                        ""
                    },
                    if descriptor.modifiers & 4 != 0 {
                        "⌥"
                    } else {
                        ""
                    },
                    if descriptor.modifiers & 2 != 0 {
                        "⇧"
                    } else {
                        ""
                    },
                    if descriptor.modifiers & 1 != 0 {
                        "⌘"
                    } else {
                        ""
                    },
                    descriptor.key.to_uppercase()
                );
                Entry::new(
                    PaletteResult {
                        id: format!("command:{command:?}"),
                        destination: PaletteDestination::Command { command: *command },
                        title: descriptor.title,
                        subtitle: subtitle.into(),
                        method: String::new(),
                        shortcut,
                        unavailable_reason: descriptor.unavailable_reason,
                        current: false,
                    },
                    &descriptor.synonyms,
                )
            })
            .collect()
    }
    fn search(&mut self) -> PaletteSnapshot {
        let text = self.state.query.trim();
        let actions_only = text.starts_with('>') && self.state.page == PalettePage::Root;
        let query = normalize(if actions_only { text[1..].trim() } else { text });
        let words: Vec<_> = query.split_whitespace().collect();
        let commands = self.commands();
        let entries: Vec<&Entry> = match self.state.page {
            PalettePage::Environments => self.environments.iter().collect(),
            PalettePage::Themes => self.themes.iter().collect(),
            PalettePage::Root => {
                let filter = if actions_only {
                    PaletteFilter::Actions
                } else {
                    self.state.filter
                };
                let mut entries = vec![];
                if filter != PaletteFilter::Actions {
                    entries.extend(&self.tabs);
                    if !query.is_empty() {
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
        };
        let mut ranked: Vec<_> = entries
            .into_iter()
            .enumerate()
            .filter_map(|(order, entry)| {
                if !words.iter().all(|word| matches(&entry.corpus, word)) {
                    return None;
                }
                let rank = if query.is_empty() || entry.title == query {
                    0
                } else if entry.title.starts_with(&query) {
                    1
                } else if matches(&entry.title, &query) {
                    2
                } else {
                    3
                };
                let secondary = !query.is_empty() && !matches(&entry.secondary, &query);
                let open = match &entry.result.destination {
                    PaletteDestination::Tab { .. } => true,
                    PaletteDestination::Request {
                        collection_id,
                        request_id,
                    } => self.request_tab(collection_id, request_id).is_some(),
                    _ => false,
                };
                Some(((rank, secondary, !open, order), &entry.result))
            })
            .collect();
        if !query.is_empty() {
            ranked.sort_by_key(|(rank, _)| *rank);
        }
        self.state.total = u64::try_from(ranked.len()).unwrap_or(u64::MAX);
        self.state.results = ranked
            .into_iter()
            .take(50)
            .map(|(_, r)| r.clone())
            .collect();
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

#[cfg(test)]
mod tests;
