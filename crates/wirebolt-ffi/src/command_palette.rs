//! Typed metadata-only command palette bridge.
#![allow(clippy::needless_pass_by_value, reason = "UniFFI lifts owned records.")]
use crate::lock;
use std::sync::{Arc, Mutex};
use wirebolt_app::domain::commands::{
    CommandContext, CommandDescriptor, CommandShortcut, ShortcutKey, ShortcutModifier,
    WorkspaceCommand,
};
use wirebolt_app::domain::models::DomainGroup;
use wirebolt_app::domain::palette::{
    CommandPalette, PaletteChoice, PaletteCollection, PaletteDestination,
    PaletteEnvironmentSelection, PaletteFailure, PaletteFilter, PalettePage, PaletteRequest,
    PaletteResolution, PaletteResult, PaletteSnapshot, PaletteTab, PaletteTabTarget, PaletteTheme,
    PaletteThemeKind, PaletteWorkspaceChange,
};

#[uniffi::remote(Enum)]
pub enum WorkspaceCommand {
    CommandPalette,
    NewHttp,
    NewWebSocket,
    Save,
    FilterRequests,
    ToggleSidebar,
    EditUrl,
    FocusResponse,
    FocusSidebar,
    Back,
    Forward,
    NextTab,
    PreviousTab,
    SplitRight,
    RequestParams,
    RequestHeaders,
    RequestBody,
    RequestAuth,
    RequestNote,
    RequestSettings,
    ResponseHeaders,
    ResponseBody,
    ResponseCookies,
    ResponseRaw,
    ResponseRequest,
    Settings,
    Help,
    WorkspaceSettings,
    EnvironmentInspector,
    SwitchEnvironment,
    ChangeTheme,
}

#[uniffi::remote(Record)]
pub struct CommandContext {
    pub has_session: bool,
    pub web_socket: bool,
    pub has_name: bool,
    pub has_collections: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub tab_count: u64,
    pub sidebar_hidden: bool,
    pub can_open_palette: bool,
}

#[uniffi::remote(Enum)]
pub enum ShortcutModifier {
    Control,
    Option,
    Shift,
    Command,
}

#[uniffi::remote(Enum)]
pub enum ShortcutKey {
    Character { value: String },
    LeftArrow,
    RightArrow,
}

#[uniffi::remote(Record)]
pub struct CommandShortcut {
    pub key: ShortcutKey,
    pub modifiers: Vec<ShortcutModifier>,
    pub label: String,
}

#[uniffi::remote(Record)]
pub struct CommandDescriptor {
    pub command: WorkspaceCommand,
    pub title: String,
    pub shortcut: Option<CommandShortcut>,
    pub synonyms: String,
    pub unavailable_reason: Option<String>,
}

#[uniffi::remote(Record)]
pub struct PaletteRequest {
    pub id: String,
    pub name: String,
    pub method: String,
    pub web_socket: bool,
    pub group_id: Option<String>,
}

#[uniffi::remote(Record)]
pub struct PaletteCollection {
    pub id: String,
    pub name: String,
    pub groups: Vec<DomainGroup>,
    pub requests: Vec<PaletteRequest>,
}

#[uniffi::remote(Record)]
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

#[uniffi::remote(Record)]
pub struct PaletteChoice {
    pub id: String,
    pub name: String,
}

#[uniffi::remote(Enum)]
pub enum PaletteThemeKind {
    BuiltIn,
    Custom,
}

#[uniffi::remote(Record)]
pub struct PaletteTheme {
    pub id: String,
    pub name: String,
    pub kind: PaletteThemeKind,
}

#[uniffi::remote(Enum)]
pub enum PaletteEnvironmentSelection {
    Globals,
    Environment { id: String },
}

#[uniffi::remote(Record)]
pub struct PaletteTabTarget {
    pub group_id: String,
    pub tab_id: String,
}

#[uniffi::remote(Enum)]
pub enum PaletteWorkspaceChange {
    Rebuild,
    RenameRequest {
        collection_id: String,
        request_id: String,
        name: String,
    },
}

#[uniffi::remote(Enum)]
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

#[uniffi::remote(Enum)]
pub enum PaletteResolution {
    Ready { result: PaletteResult },
    Failed { reason: PaletteFailure },
}

#[uniffi::remote(Enum)]
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

#[uniffi::remote(Enum)]
pub enum PaletteFilter {
    All,
    Requests,
    Actions,
}

#[uniffi::remote(Enum)]
pub enum PalettePage {
    Root,
    Environments,
    Themes,
}

#[uniffi::remote(Record)]
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

#[uniffi::remote(Record)]
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

#[uniffi::export]
pub fn domain_workspace_command_descriptor(
    command: WorkspaceCommand,
    context: CommandContext,
) -> CommandDescriptor {
    command.descriptor(&context)
}
#[uniffi::export]
pub fn domain_palette_failure_message(reason: PaletteFailure) -> String {
    reason.message()
}
#[derive(Debug, uniffi::Object, Default)]
pub struct AppCommandPalette {
    inner: Mutex<CommandPalette>,
}
#[uniffi::export]
impl AppCommandPalette {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    pub fn replace_workspace(&self, collections: Vec<PaletteCollection>) {
        lock(&self.inner).replace_workspace(collections);
    }
    pub fn apply_workspace_change(&self, change: PaletteWorkspaceChange) -> bool {
        lock(&self.inner).apply_workspace_change(change)
    }
    pub fn normalized_requests(&self) -> u64 {
        lock(&self.inner).normalized_requests()
    }
    pub fn update_tabs(&self, tabs: Vec<PaletteTab>, active_group: String, group_count: u64) {
        lock(&self.inner).update_tabs(tabs, active_group, group_count);
    }
    pub fn update_choices(
        &self,
        environments: Vec<PaletteChoice>,
        themes: Vec<PaletteTheme>,
        selected_environment: PaletteEnvironmentSelection,
        selected_theme: String,
    ) {
        lock(&self.inner).update_choices(
            environments,
            themes,
            selected_environment,
            selected_theme,
        );
    }
    pub fn set_context(&self, context: CommandContext) {
        lock(&self.inner).set_context(context);
    }
    pub fn open(&self) -> PaletteSnapshot {
        lock(&self.inner).open()
    }
    pub fn query(&self, query: String, filter: PaletteFilter) -> PaletteSnapshot {
        lock(&self.inner).query(query, filter)
    }
    pub fn refresh(&self) -> PaletteSnapshot {
        lock(&self.inner).refresh()
    }
    pub fn move_selection(&self, delta: i64) -> PaletteSnapshot {
        lock(&self.inner).move_selection(delta)
    }
    pub fn select(&self, id: String) -> PaletteSnapshot {
        lock(&self.inner).select(&id)
    }
    pub fn resolve(&self, id: String) -> PaletteResolution {
        lock(&self.inner).resolve(&id)
    }
    pub fn enter(&self, page: PalettePage) -> PaletteSnapshot {
        lock(&self.inner).enter(page)
    }
    pub fn back(&self) -> Option<PaletteSnapshot> {
        lock(&self.inner).back()
    }
}
