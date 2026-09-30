# Changelog

Changes under **Unreleased** describe the source branch and may not be included in downloadable betas. Published binaries are listed in the [distribution releases](https://github.com/Christopher96u/homebrew-tap/releases). This file does not reconstruct unverified release history.

## Unreleased

### Breaking changes

- The bundle identifier is now `io.github.christopher96u.wirebolt`. Preferences, the remembered workspace and Keychain items stored under `local.wirebolt.app` are not migrated: reopen the workspace and re-enter stored credentials.
- Wirebolt runs one workspace window. **⌘N** creates a request; File → New Window is gone.
- Deleting a request or an empty folder happens immediately and can be undone with **⌘Z**. Wirebolt asks first only when a deletion also removes other items or discards unsaved edits.
- Environment and workspace settings sheets use **Cancel** and **Save** (Return). Esc cancels and asks before discarding edits; closing no longer saves.
- Shortcuts: **⇧⌘N** is New Collection (it was New HTTP Request), next and previous tab are also **⇧⌘]** / **⇧⌘[**, Copy cURL is **⌥⇧⌘C** and Git Collaboration is **⌃⌘G** (the old shortcuts clashed with macOS). See [keyboard shortcuts](docs/keyboard-shortcuts.md).

### Added

- Undo and redo for creating, renaming, moving, reordering and deleting requests, folders and collections, including the files on disk.
- Preview tabs: a single click in the sidebar reuses one italic tab until you edit, send or double-click it.
- Unsaved-edit dots on tabs and the window close button; quitting or closing the window asks to save, discard or cancel.
- Tabs, the active tab and each request's latest response are restored at launch.
- `{{variable}}` highlighting, value tooltips and completion in the URL and key-value fields.
- Keyboard access: full sidebar navigation (arrows, type-select, Return to rename), Move Up/Down (**⌥⌘↑** / **⌥⌘↓**) and Move To, Cancel Request (**⌘.**), request and response section commands (**⌥⌘1–6**, **⌃⌘1–5**), Focus Sidebar (**⌘0**) and Focus Response (**⌥⌘0**).
- A welcome state with New Request, Import and Open Workspace for empty workspaces, and a New Request row in empty collections.
- Open or drop cURL, HAR, Postman and Wirebolt JSON files on the app to import them. File → Open Recent lists workspaces.
- API Key and OAuth 2.0 can be chosen in the Auth tab; every request keeps its own Keychain credentials, including duplicates.
- Secret environment variables stored in Keychain, a Cookies window per workspace (delete one or Clear All), Rename Workspace, and client certificate / custom CA settings.
- Git Collaboration can initialize a repository and abort a conflicted merge.
- An import summary lists what was imported, the environment created and anything skipped.
- Insomnia (v4 JSON, v5 YAML) and Bruno (collection folder, `.bru` file or JSON export) import, including folders, bodies, authentication and environments.
- Response pane: elapsed time and Cancel while sending, the previous response stays visible until the new one arrives, and failures explain the cause with Retry, Network Settings and View Request.
- VoiceOver announcements for finished, failed and cancelled requests; clearer labels for credential, multipart and key-value fields, checkboxes and sidebar rows; help tags on icon-only buttons.
- Local Markdown Edit/Preview for request notes, Git Collaboration commands, in-app help in its own window, user documentation and a local quick-start API example.

### Changed

- Postman import keeps authentication (with inheritance), variables, path variables, disabled rows, GraphQL and binary bodies, body languages and descriptions. HAR import drops HTTP/2 pseudo-headers and maps form bodies. cURL import handles multi-line commands and the common options (`-u`, `-F`, `-b`, `--data-urlencode`, `-k`, `-L`, …). Unsupported files (OpenAPI, Postman v1) explain why.
- Wirebolt JSON export round-trips secret references, request proxy and TLS settings, content types, multipart files, environments and separate collections.
- Copy cURL reads secrets from Keychain when the command runs instead of placing them on the clipboard.
- New workspaces are named after their folder. Cookies are kept per workspace and session cookies are no longer saved.

- Native Send, Cancel, Connect and Disconnect buttons in title case; an unavailable Send or Connect looks disabled.
- The URL bar keeps a fixed-width status that shows the response status, **Failed** or **Cancelled**, and collapses to icons in narrow split editors so the URL stays readable.
- Method and status colors are adaptive and meet 4.5:1 contrast in light and dark appearances; each method has its own color.
- Durations, sizes and status lines read naturally (“<0.1 ms”, “1.23 s”, “18.6 KB”, “302 Found”).
- Invisible characters are hidden by default. JSON can decode Unicode escapes for display.
- The Light, Dark and System appearance applies to every window, including Settings.
- The Git sheet shows readable file statuses. Without an upstream, Push becomes **Publish** (pushes the branch to `origin` and tracks it); Push is disabled when there is nothing to push.
- Save and open panels attach to the window as sheets; export failures explain the reason.

### Fixed

- Quitting or closing the window no longer discards unsaved request edits silently.
- Toolbar actions stay at the trailing edge, and the tab strip marks tabs cut off at its edges.
- The unfocused sidebar selection stays visible in Dark Mode.
- Faster launch and tab switching: history, cookies and the proxy monitor load off the launch path; image responses decode off the main thread.
- The JSON tree keeps document key order; resizing large responses no longer re-wraps on every pixel.
- Saving an open request after moving it across collections targets the new location.
- Deleting a folder closes descendant request sessions.
- Pending workspace transport edits persist after settings closes.
- Wirebolt JSON export preserves API key and OAuth configuration through native metadata.

### Clarified

- Privacy settings describe behaviors instead of presenting inactive switches.
- Advanced authentication export portability and local credential provisioning have explicit limits.
