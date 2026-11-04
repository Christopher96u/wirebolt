//! Shared menu/palette command identity, labels, shortcuts and domain eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkspaceCommand {
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandDescriptor {
    pub command: WorkspaceCommand,
    pub title: String,
    pub key: String,
    /// AppKit-compatible modifier bits: command=1, shift=2, option=4, control=8.
    pub modifiers: u8,
    pub synonyms: String,
    pub unavailable_reason: Option<String>,
}

impl WorkspaceCommand {
    pub const ALL: [Self; 30] = [
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
    pub fn descriptor(self, context: &CommandContext) -> CommandDescriptor {
        use WorkspaceCommand as C;
        let (title, key, modifiers, synonyms) = match self {
            C::NewHttp => ("New Request", "n", 1, "create http request"),
            C::NewWebSocket => ("New WebSocket Request", "", 0, "create socket ws"),
            C::Save => ("Save", "s", 1, "save request"),
            C::FilterRequests => ("Filter Requests", "f", 3, "search sidebar"),
            C::ToggleSidebar => (
                if context.sidebar_hidden {
                    "Show Sidebar"
                } else {
                    "Hide Sidebar"
                },
                "s",
                9,
                "toggle sidebar",
            ),
            C::EditUrl => ("Edit URL", "l", 1, "focus address"),
            C::FocusResponse => ("Focus Response", "0", 5, "response viewer"),
            C::FocusSidebar => ("Focus Sidebar", "0", 1, "collections folders"),
            C::Back => ("Go Back", "\u{f702}", 9, "navigation previous"),
            C::Forward => ("Go Forward", "\u{f703}", 9, "navigation next"),
            C::NextTab => ("Show Next Tab", "}", 1, "next tab"),
            C::PreviousTab => ("Show Previous Tab", "{", 1, "previous tab"),
            C::SplitRight => ("Split Right", "d", 3, "split editor group"),
            C::RequestParams => ("Params", "1", 5, "request section query parameters"),
            C::RequestHeaders => ("Headers", "2", 5, "request section headers"),
            C::RequestBody => (
                if context.web_socket {
                    "Message"
                } else {
                    "Body"
                },
                "3",
                5,
                "request section body message",
            ),
            C::RequestAuth => ("Auth", "4", 5, "request section authentication"),
            C::RequestNote => ("Note", "5", 5, "request section notes"),
            C::RequestSettings => (
                "Settings",
                "6",
                5,
                "request section network proxy tls timeout",
            ),
            C::ResponseHeaders => ("Headers", "1", 9, "response section headers"),
            C::ResponseBody => ("Body", "2", 9, "response section body"),
            C::ResponseCookies => ("Cookies", "3", 9, "response section cookies"),
            C::ResponseRaw => ("Raw HTTP", "4", 9, "response section raw http"),
            C::ResponseRequest => ("Request", "5", 9, "response section prepared request"),
            C::Settings => ("Settings…", ",", 1, "preferences appearance keyboard"),
            C::Help => ("Wirebolt Help", "?", 1, "help shortcuts documentation"),
            C::WorkspaceSettings => ("Workspace Settings…", "", 0, "workspace configuration"),
            C::EnvironmentInspector => (
                "Open Environment Inspector",
                "",
                0,
                "environment globals variables inspector",
            ),
            C::SwitchEnvironment => ("Switch environment…", "", 0, "environment globals"),
            C::ChangeTheme => (
                "Change theme…",
                "",
                0,
                "appearance theme black paper ember system",
            ),
        };
        let reason = self.unavailable_reason(context);
        CommandDescriptor {
            command: self,
            title: title.into(),
            key: key.into(),
            modifiers,
            synonyms: synonyms.into(),
            unavailable_reason: reason.map(str::to_owned),
        }
    }
    fn unavailable_reason(self, context: &CommandContext) -> Option<&'static str> {
        use WorkspaceCommand as C;
        match self {
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
