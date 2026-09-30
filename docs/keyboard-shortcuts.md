# Keyboard shortcuts

[Documentation](README.md)

`⌘` = Command, `⇧` = Shift, `⌥` = Option, `⌃` = Control. Shortcuts apply to the workspace window and are disabled while an action is unavailable. The menu bar shows each menu shortcut next to its command; ⌃Tab / ⌃⇧Tab and the shortcuts under [In context](#in-context) work without a menu item.

## File

| Action | Shortcut |
| --- | --- |
| New request | ⌘N |
| New tab (untitled request) | ⌘T |
| New folder | ⌥⌘N |
| New collection | ⇧⌘N |
| Open workspace | ⌘O |
| Close tab | ⌘W (closes the window when no tab is open) |
| Close window | ⇧⌘W |
| Save request | ⌘S |

**File → New WebSocket Request** has no shortcut.

## Edit and View

| Action | Shortcut |
| --- | --- |
| Undo / redo sidebar changes | ⌘Z / ⇧⌘Z |
| Paste and match style | ⌥⇧⌘V |
| Show or hide the toolbar | ⌥⌘T |
| Filter requests | ⇧⌘F |
| Show or hide the sidebar | ⌃⌘S |

## Request

| Action | Shortcut |
| --- | --- |
| Send HTTP request | ⌘Return |
| Cancel request | ⌘. |
| Connect / disconnect WebSocket | ⌃⌘Return |
| Move sidebar item up / down | ⌥⌘↑ / ⌥⌘↓ |
| Copy cURL | ⌥⇧⌘C |
| Edit URL | ⌘L |
| Add key (switches to Params when the section has no key-value table) | ⇧⌘K |
| Bulk edit | ⌘B |

## Navigate

| Action | Shortcut |
| --- | --- |
| Go back / forward | ⌃⌘← / ⌃⌘→ |
| Split right | ⇧⌘D |
| Focus sidebar | ⌘0 |
| Focus response | ⌥⌘0 |
| Request section: Params, Headers, Body, Auth, Note, Settings | ⌥⌘1 – ⌥⌘6 |
| Response section: Headers, Body, Cookies, Raw, Request | ⌃⌘1 – ⌃⌘5 |
| Show next tab | ⌘} (⇧⌘] on a U.S. keyboard) or ⌃Tab |
| Show previous tab | ⌘{ (⇧⌘[ on a U.S. keyboard) or ⌃⇧Tab |
| Select tab 1–9 | ⌘1 – ⌘9 |

For a WebSocket request, ⌥⌘3 opens **Message**. Response sections apply to HTTP requests.

## Workspace and Help

| Action | Shortcut |
| --- | --- |
| Git collaboration | ⌃⌘G |
| Wirebolt Help | ⌘? |
| Settings | ⌘, |

## In context

- **Environment editor:** ⇧⌘K adds a variable entry; Return saves and Esc cancels.
- **Find in a response or editor:** ⌘F opens the find bar; Return and ⇧Return go to the next and previous match; ⌥⌘C, ⌥⌘W and ⌥⌘R toggle Match Case, Whole Word and Regular Expression; ⌥⌘L finds in the selection; Esc closes the bar.
- **Focused sidebar:** arrow keys move through rows (**←** / **→** collapse and expand), typing a name jumps to it, **Return** renames, **⌘↓** moves to the request's URL and **Delete** deletes. See [Workspaces](workspaces.md#keyboard).
