# Command Palette

Navigate → Command Palette… (⌘K) opens a lazy overlay in the primary workspace
window on macOS 15 or later. It requires a loaded workspace without Settings,
sheets, alerts, rename/close prompts, imports, or load/Git/OAuth transitions.
⌘K is registered once; ⇧⌘K remains Add Key.

## Scope

Search includes every saved request, independently of sidebar filtering and
folder expansion, and open tabs in every editor group, including unsaved drafts.
A saved request open in any group has one row, with its method chip, title,
collection/folder path and an Open marker. If it is only open in a different group,
the marker is Open in split N. Confirmation prefers an existing tab in the active
group. Its current native draft title and method remain searchable. Unsaved tabs
have separate exact group/tab destinations, marked Unsaved; only multiple editor
groups add a split hint. All method chips reuse the tabs' themed colors.
An empty query lists open requests and unsaved tabs in group/tab order, then New
Request, Filter Requests, Settings, Switch environment and Change theme. All / Requests / Actions
filters are visible; a leading `>` selects actions. Results stop at 50 and show a
refinement hint when more match.

The shared Rust command catalog supplies menu and palette identities, labels,
shortcuts and domain eligibility. Native dispatch adds window and modal guards.
Actions cover HTTP/WebSocket creation, Save, request filtering, sidebar visibility,
URL/response/sidebar focus, Back/Forward, next/previous tab, Split Right,
request/response sections, Settings, Help, Workspace Settings and the environment
inspector. Send/cancel/connect, OAuth, proxy tests, network Git operations and
destructive actions are excluded.

Switch environment offers Globals only (nil) and selectable environments; Change
theme offers Follow System, Black, Paper, Ember and installed custom themes.
Current choices have checkmarks. Submenus receive their own Rust page title,
search placeholder and accessibility label (Search environments… / Search themes…).
Environment rows have no filler subtitle; theme rows say Built-in or Custom.
Command rows show their shortcut on the right without subtitles. Presentation-only
markers and choice categories do not expand the search corpus. Unavailable
actions retain an inline reason. Globals only is a typed selection, separate from
environment IDs. Stable result IDs are independent of display titles and Debug output.
Shortcuts carry typed keys and modifier sets, with labels rendered once in Rust.

## Privacy

The index accepts only request names, methods/kinds, collection/folder names;
current tab titles, methods/kinds and editor-group names; command titles and fixed
synonyms; environment and theme names. Stable IDs identify destinations.

URLs, bodies, notes, headers, query parameters, auth, variable names/values,
secret references/material, cookies, responses, history, WebSocket messages and
file paths never enter search or result labels. Search never resolves variables,
reads Keychain, prepares a run or sends traffic. Queries are ephemeral, without
persistent history, telemetry or logs. User-provided names remain private data.
Native title/method updates carry bounded metadata, without synchronizing drafts.

## State transitions

- Closed → opening: capture weak responder/window identity, selection and workspace
  epoch before ending native editing. Marked text or refused focus transfer blocks
  opening. Flush inspector editing; failure hands focus back to its error.
- Opening → presented: build/update the metadata index, mount the overlay, isolate
  background hit testing/accessibility and focus the native search field.
- Query/filter/highlight: Rust updates results and selected stable ID, without
  document navigation or preference changes. A synchronous query cannot publish
  a stale asynchronous result. Existing selection survives if still present.
- Environment/theme command → submenu: retain the parent query, filter and
  highlight. Esc or Backspace on an empty query restores that parent.
- Esc at root, ⌘K, ⌘W or scrim/close button → closed: remove isolation, then restore
  the original responder and bounded UTF-16 selection after layout, provided the
  workspace, active group/tab and responder are still valid. Otherwise, within
  the same workspace, focus the current editor or sidebar.
- Confirm: refresh metadata and eligibility; resolve stable identity once. Missing
  or unavailable destinations keep the palette open with a reason. A request
  selects its existing tab in the active group first, then another group, or opens
  a regular tab; request confirmations pin previews. A tab result selects exactly
  its group/tab and preserves drafts, preview, response and run.
- Environment confirmation flushes inspector editing before setting the selected
  environment once; failure preserves the old selection and hands off to the
  inspector. While confirmation executes, Esc, Backspace and the back button cannot
  navigate. Closing the palette before the flush completes invalidates the pending
  activation; the edit can finish without changing the selected environment.
  After the flush, Rust revalidates the typed choice, independently of the current
  query or page. Theme confirmation writes `interfaceAppearance` once.
- Destination handoff removes isolation before dispatch. Settings inherits the
  captured origin; no queued palette restoration steals destination focus. Sidebar
  visibility and tab/history navigation restore the origin if it remains the right
  target, otherwise focus the active editor. Section commands focus the selected
  request or response content.
- Workspace replacement, external modal presentation or window close invalidates
  pending work and dismisses without restoring a stale responder.

Return/Esc/arrows belong to native marked-text composition first. Document
commands, focused sidebar/inspector actions and ⌃Tab are blocked at dispatch while
open; ⌘W dismisses only the palette. ⇧⌘W closes the window through the existing
inspector/unsaved-edit guard and dismisses the palette as part of that path. Quit
retains its existing unsaved-edit guard. Settings and Help keep their previous
app-wide availability, including Help while Settings is open.

## Ranking and performance

Rust reuses the sidebar's Unicode case/diacritic normalization, with a separate
allowlisted corpus. All query words must match. Ranking is exact title, prefix,
substring, then other metadata; collection/folder matches are secondary, open
results break ties, and stable source order resolves remaining ties.

The metadata index is built lazily on first presentation. Collection revision
changes update the request projection; environment-variable edits leave it intact.
Rust reuses unchanged normalized request entries, so a request rename normalizes
only the changed entry. Tabs, environments and themes compare their own bounded
metadata independently; an unrelated change cannot rebuild the request index. Query
updates stay in the palette's observation subtree. The native `command-palette` workload uses 10,000
requests, 20 tabs and a 1 MiB draft; it measures cold/warm opening, warm opening
immediately after an environment-variable edit and after a request rename,
dense/absent queries through layout/display, native typing and cancellation, and
asserts zero full-draft reads/writes. M5 Pro gates are all warm opens (including
both after-edit cases) p95 ≤50 ms, query to visible results ≤25 ms, and typing feedback ≤16 ms. These are calibrated test budgets,
not guarantees for other hardware or physical display latency.
