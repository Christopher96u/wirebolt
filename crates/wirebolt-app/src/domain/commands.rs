//! Shared menu/palette command identity, labels, shortcuts and domain eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent menu eligibility facts, not mutually exclusive states."
)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutModifier {
    Control,
    Option,
    Shift,
    Command,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortcutKey {
    Character { value: String },
    LeftArrow,
    RightArrow,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandShortcut {
    pub key: ShortcutKey,
    pub modifiers: Vec<ShortcutModifier>,
    pub label: String,
}
impl CommandShortcut {
    fn new(key: ShortcutKey, modifiers: &[ShortcutModifier]) -> Self {
        use ShortcutModifier as M;
        let mut label = String::new();
        for (modifier, glyph) in [
            (M::Control, "⌃"),
            (M::Option, "⌥"),
            (M::Shift, "⇧"),
            (M::Command, "⌘"),
        ] {
            if modifiers.contains(&modifier) {
                label.push_str(glyph);
            }
        }
        label.push_str(&match &key {
            ShortcutKey::Character { value } => value.to_uppercase(),
            ShortcutKey::LeftArrow => "←".into(),
            ShortcutKey::RightArrow => "→".into(),
        });
        Self {
            key,
            modifiers: modifiers.to_vec(),
            label,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandDescriptor {
    pub command: WorkspaceCommand,
    pub title: String,
    pub shortcut: Option<CommandShortcut>,
    pub synonyms: String,
    pub unavailable_reason: Option<String>,
}

impl WorkspaceCommand {
    pub const ALL: [Self; 31] = [
        Self::CommandPalette,
        Self::NewHttp,
        Self::NewWebSocket,
        Self::Save,
        Self::FilterRequests,
        Self::ToggleSidebar,
        Self::EditUrl,
        Self::FocusResponse,
        Self::FocusSidebar,
        Self::Back,
        Self::Forward,
        Self::NextTab,
        Self::PreviousTab,
        Self::SplitRight,
        Self::RequestParams,
        Self::RequestHeaders,
        Self::RequestBody,
        Self::RequestAuth,
        Self::RequestNote,
        Self::RequestSettings,
        Self::ResponseHeaders,
        Self::ResponseBody,
        Self::ResponseCookies,
        Self::ResponseRaw,
        Self::ResponseRequest,
        Self::Settings,
        Self::Help,
        Self::WorkspaceSettings,
        Self::EnvironmentInspector,
        Self::SwitchEnvironment,
        Self::ChangeTheme,
    ];
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::CommandPalette => "command-palette",
            Self::NewHttp => "new-http",
            Self::NewWebSocket => "new-web-socket",
            Self::Save => "save",
            Self::FilterRequests => "filter-requests",
            Self::ToggleSidebar => "toggle-sidebar",
            Self::EditUrl => "edit-url",
            Self::FocusResponse => "focus-response",
            Self::FocusSidebar => "focus-sidebar",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::NextTab => "next-tab",
            Self::PreviousTab => "previous-tab",
            Self::SplitRight => "split-right",
            Self::RequestParams => "request-params",
            Self::RequestHeaders => "request-headers",
            Self::RequestBody => "request-body",
            Self::RequestAuth => "request-auth",
            Self::RequestNote => "request-note",
            Self::RequestSettings => "request-settings",
            Self::ResponseHeaders => "response-headers",
            Self::ResponseBody => "response-body",
            Self::ResponseCookies => "response-cookies",
            Self::ResponseRaw => "response-raw",
            Self::ResponseRequest => "response-request",
            Self::Settings => "settings",
            Self::Help => "help",
            Self::WorkspaceSettings => "workspace-settings",
            Self::EnvironmentInspector => "environment-inspector",
            Self::SwitchEnvironment => "switch-environment",
            Self::ChangeTheme => "change-theme",
        }
    }
    #[must_use]
    pub fn descriptor(self, context: &CommandContext) -> CommandDescriptor {
        let (title, synonyms) = self.labels(context);
        CommandDescriptor {
            command: self,
            title: title.into(),
            shortcut: self.shortcut(),
            synonyms: synonyms.into(),
            unavailable_reason: self.unavailable_reason(context).map(str::to_owned),
        }
    }
    fn labels(self, context: &CommandContext) -> (&'static str, &'static str) {
        use WorkspaceCommand as C;
        match self {
            C::CommandPalette => ("Command Palette…", ""),
            C::NewHttp => ("New Request", "create http request"),
            C::NewWebSocket => ("New WebSocket Request", "create socket ws"),
            C::Save => ("Save", "save request"),
            C::FilterRequests => ("Filter Requests", "search sidebar"),
            C::ToggleSidebar => (
                if context.sidebar_hidden {
                    "Show Sidebar"
                } else {
                    "Hide Sidebar"
                },
                "toggle sidebar",
            ),
            C::EditUrl => ("Edit URL", "focus address"),
            C::FocusResponse => ("Focus Response", "response viewer"),
            C::FocusSidebar => ("Focus Sidebar", "collections folders"),
            C::Back => ("Go Back", "navigation previous"),
            C::Forward => ("Go Forward", "navigation next"),
            C::NextTab => ("Show Next Tab", "next tab"),
            C::PreviousTab => ("Show Previous Tab", "previous tab"),
            C::SplitRight => ("Split Right", "split editor group"),
            C::RequestParams => ("Params", "request section query parameters"),
            C::RequestHeaders => ("Headers", "request section headers"),
            C::RequestBody => (
                if context.web_socket {
                    "Message"
                } else {
                    "Body"
                },
                "request section body message",
            ),
            C::RequestAuth => ("Auth", "request section authentication"),
            C::RequestNote => ("Note", "request section notes"),
            C::RequestSettings => ("Settings", "request section network proxy tls timeout"),
            C::ResponseHeaders => ("Headers", "response section headers"),
            C::ResponseBody => ("Body", "response section body"),
            C::ResponseCookies => ("Cookies", "response section cookies"),
            C::ResponseRaw => ("Raw HTTP", "response section raw http"),
            C::ResponseRequest => ("Request", "response section prepared request"),
            C::Settings => ("Settings…", "preferences appearance keyboard"),
            C::Help => ("Wirebolt Help", "help shortcuts documentation"),
            C::WorkspaceSettings => ("Workspace Settings…", "workspace configuration"),
            C::EnvironmentInspector => (
                "Open Environment Inspector",
                "environment globals variables inspector",
            ),
            C::SwitchEnvironment => ("Switch environment…", "environment globals"),
            C::ChangeTheme => ("Change theme…", "appearance theme black paper ember system"),
        }
    }
    fn shortcut(self) -> Option<CommandShortcut> {
        use ShortcutModifier::{Command, Control, Option as OptionKey, Shift};
        use WorkspaceCommand as C;
        let character = |value: &str| ShortcutKey::Character {
            value: value.into(),
        };
        let key = match self {
            C::CommandPalette => character("k"),
            C::NewHttp => character("n"),
            C::Save | C::ToggleSidebar => character("s"),
            C::FilterRequests => character("f"),
            C::EditUrl => character("l"),
            C::FocusResponse | C::FocusSidebar => character("0"),
            C::Back => ShortcutKey::LeftArrow,
            C::Forward => ShortcutKey::RightArrow,
            C::NextTab => character("}"),
            C::PreviousTab => character("{"),
            C::SplitRight => character("d"),
            C::RequestParams | C::ResponseHeaders => character("1"),
            C::RequestHeaders | C::ResponseBody => character("2"),
            C::RequestBody | C::ResponseCookies => character("3"),
            C::RequestAuth | C::ResponseRaw => character("4"),
            C::RequestNote | C::ResponseRequest => character("5"),
            C::RequestSettings => character("6"),
            C::Settings => character(","),
            C::Help => character("?"),
            _ => return None,
        };
        let modifiers = match self {
            C::FilterRequests | C::SplitRight => &[Shift, Command][..],
            C::FocusResponse
            | C::RequestParams
            | C::RequestHeaders
            | C::RequestBody
            | C::RequestAuth
            | C::RequestNote
            | C::RequestSettings => &[OptionKey, Command][..],
            C::ToggleSidebar
            | C::Back
            | C::Forward
            | C::ResponseHeaders
            | C::ResponseBody
            | C::ResponseCookies
            | C::ResponseRaw
            | C::ResponseRequest => &[Control, Command][..],
            _ => &[Command][..],
        };
        Some(CommandShortcut::new(key, modifiers))
    }
    fn unavailable_reason(self, context: &CommandContext) -> Option<&'static str> {
        use WorkspaceCommand as C;
        match self {
            C::CommandPalette if !context.can_open_palette => {
                Some("Open an available workspace window first.")
            }
            C::Save if !context.has_session => Some("Open a request to save."),
            C::Save if !context.has_name => Some("Give the request a name first."),
            C::Save if !context.has_collections => Some("Create a collection first."),
            C::Back if !context.can_go_back => Some("No previous location in this editor group."),
            C::Forward if !context.can_go_forward => Some("No next location in this editor group."),
            C::NextTab | C::PreviousTab if context.tab_count < 2 => {
                Some("Open another tab in this editor group.")
            }
            C::ResponseHeaders
            | C::ResponseBody
            | C::ResponseCookies
            | C::ResponseRaw
            | C::ResponseRequest
                if !context.has_session || context.web_socket =>
            {
                Some("Open an HTTP request first.")
            }
            C::EditUrl
            | C::FocusResponse
            | C::SplitRight
            | C::RequestParams
            | C::RequestHeaders
            | C::RequestBody
            | C::RequestAuth
            | C::RequestNote
            | C::RequestSettings
                if !context.has_session =>
            {
                Some("Open a request first.")
            }
            _ => None,
        }
    }
}
