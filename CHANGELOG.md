# Changelog

Changes under **Unreleased** are on `main` but not in a published build yet. Downloads are listed on the [releases page](https://github.com/Christopher96u/wirebolt/releases). Earlier 0.1.0 betas were distributed through a separate tap repository; this file does not reconstruct their history.

## Unreleased

### Added

- **QUERY** method (RFC 10008) in the method menu, with its own color.
- **Clear All History…** in Settings → Privacy removes all saved history.
- Custom themes from two colors and VS Code theme imports, with a live preview, checked contrast, and rename/delete controls (Settings → Appearance).
- Black, Paper and Ember themes with checked contrast, Compact and Comfortable density, and Increase Contrast support (Settings → Appearance).
- Settings open in the workspace window (**⌘,**); Esc or **⌘W** closes them.
- Streaming responses show a bounded partial preview while they arrive.
- **Save and Close** when closing a tab with unsaved edits.

### Changed

- Older Wirebolt workspaces, history and preferences are not migrated; the legacy Wirebolt v1 import and export are removed.

- Environments save as you edit, with undo and redo for names, variables and secrets. Globals appears once, and long names and values stay on one line. Browsing the inspector leaves the active environment unchanged; **Use for Requests** switches it explicitly.
- Find in large responses is faster and uses less memory; closing tabs no longer copies every open request.
- Sidebar requests show URL paths, folders are quieter, activity columns align, and History shows the active request's entry count.
- Redesigned sidebar, request composer and response bar. Environments are edited in a side inspector instead of a sheet.
- **⌘N** focuses the new request's URL.
- The response renderer **Raw** is now **Text**, and the **Raw** response section is now **Raw HTTP**.
- Tabs fit their titles and show the request method. The toolbar shows the workspace name instead of repeating the active request.
- Controls are monochrome: color now marks only methods, statuses and variables, and settings switches follow the design.
- Variables in the URL show as chips with the variable name; editing shows the raw `{{name}}`.
- Params and Headers read as a list: no header row, hairline separators, a remove button on hover, dimmed disabled rows and variable chips in cells.
- Section tab counts are plain numbers and count only enabled, named rows. Before the first send, the response area shows a one-line hint.
- The composer's method and authentication are menus. **Edit Long URL…** and **History** moved to its **⋯** menu, and the proxy shows only when the workspace or the request overrides the app default.

### Fixed

- Large multipart file uploads stream from disk instead of loading the complete upload into memory before Send. Bodies up to 64 KiB can still be resent on redirects. A user-set multipart **Content-Length** is replaced with the computed length.
- Bulk Edit accepts pasted CRLF and CR line endings in headers, query parameters and form fields without adding carriage returns to values.
- XML and HTML payloads stay unchanged by Format, which is now disabled for these types because inserted whitespace can change their content.
- OAuth sign-in preserves literal plus signs and valueless URL parameters, and uses form encoding for authorization parameters and callbacks (spaces encode as `+`, as required by RFC 6749).
- **Copy cURL** now encodes query parameters the same way as Send, including literal plus signs, punctuation and Unicode.
- Query and form fields marked sensitive are redacted in Sent Request and History while their values are still sent.
- Cookie expiry now gives valid `Max-Age` precedence over `Expires`, regardless of attribute order, and ignores invalid expiry attributes.
- Cookies keep directory scope and encoded path separators when choosing which requests receive them.
- Saving while deleting or moving a request could recreate stale files. Save now runs in order with workspace edits and refuses a target moved or deleted inside the app; saving recreates a request file removed externally.
- Malformed, newer-version and oversized workspace files disappeared from the sidebar without explanation. Open now keeps readable requests and shows a dismissible notice naming skipped files.
- Requests with the same ID shared history across collections. History now follows collection and request IDs through moves, saves and workspace folder renames or relocation.
- A damaged history index is reported once, then recording and clearing history continue with a fresh index.
- Clearing history or evicting old entries made a restored response unreadable. Displayed responses now keep their own temporary body until the last viewer releases it, using APFS cloning where available; crash leftovers are cleaned on the next launch.
- Moving an imported request could overwrite another request. New imports have unique IDs; moves with a ID collision are refused so both requests and their undo state survive.
- Deleting a collection also deleted attachments and other files that Undo could not restore. It now removes only readable Wirebolt documents and leaves other files in place.
- Malformed Insomnia folder IDs could crash import. Invalid parent links, cycles and hierarchies deeper than 128 levels now report an error.
- A failed response export deleted the previous destination. Export now replaces it only after the body has been copied successfully, respects normal file permissions and retains the destination's permissions and supported extended attributes.
- Saving a request larger than the 16 MiB document limit made it disappear on reopen. Save now rejects oversized documents and keeps the previous file.
- A response body write failure left the download running in the background. It now cancels the transfer and keeps the Storage failure visible.
- The response preview stayed blank when its temporary file could not be created. It now keeps a bounded preview in memory and reports the storage failure.
- Undoing and redoing an environment edit lost its previous Keychain values and changed the active environment.
- The composer showed the request's own timeout when the request used the workspace's, and **0 s** for no limit.
- With **Follow redirects**, a 301, 302, 307 or 308 that must resend the request content (for example a QUERY or PUT) was returned instead of followed. Content up to 64 KiB is now sent again.
- Variables in query parameters showed percent-encoded in the URL, and typing `{{` in a URL whose query had encoded values encoded them twice.
- Cookies: `Secure` cookies were dropped for `HTTPS://` URLs written in upper case, a blank first `Set-Cookie` segment was read as the cookie, `=secure` counted as the `Secure` attribute, and blank trailing segments such as `a=b; ;` could crash.

## 1.0.0-rc.1

First release candidate for Wirebolt 1.0. It collects everything since 0.1.0-beta.2 and is the first build published from this repository.

### Breaking changes

- The bundle identifier is now `io.github.christopher96u.wirebolt`.
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
