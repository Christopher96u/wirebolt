# Workspaces and organization

[Documentation](README.md)

A workspace is a folder containing request definitions, collections and environments. It is the unit you can version with Git or share with another person.

## Create or open

- **File → New Workspace…** creates a new workspace folder. The workspace takes the folder's name.
- **File → Open Workspace…** (**⌘O**) opens a folder containing `wirebolt.toml`.
- **Workspace → Rename Workspace…** changes the name shown in the window title. The name is saved in `wirebolt.toml`; the folder keeps its name.
- **File → New Collection…** (**⇧⌘N**) creates a top-level collection; **File → New Folder** (**⌥⌘N**) creates a folder.
- The **+** menu also exposes workspace and collection actions.

Switching workspaces closes the old tabs and cancels active requests. If there are unsaved request edits, save them first or explicitly discard them in the prompt. A failed open preserves the current workspace.

## Collections, folders and ordering

A new workspace starts empty: use **New Request** (**⌘N**), **Import**, or drop a cURL, HAR, Postman or Wirebolt file on the window. Create requests and folders from the **+** menu or the relevant context menu; an empty collection shows its own **New Request** row. Right-click a sidebar item for its available actions.

Drag a request or subfolder to the **upper or lower edge** of a sibling row to reorder it. The insertion line shows the destination. Requests and folders can be mixed. Drop in the **center of a folder** to move an item into it. Without dragging, use **Move Up** / **Move Down** (**⌥⌘↑** / **⌥⌘↓**) or the context menu's **Move To** submenu.

Moving an open request keeps its document session associated with its new collection. Deleting a folder deletes its descendants and closes their tabs. Creating, renaming, moving, reordering and deleting items can be undone with **Edit → Undo** (**⌘Z**) and redone with **⇧⌘Z**; undo restores the files on disk with the same IDs and order. Wirebolt asks before a deletion only when it also removes other items or discards unsaved edits in open tabs, which undo can't bring back.

### Keyboard

When the sidebar has focus, its selection is drawn in the accent color:

- **↑ / ↓** move through every visible row, including collections and folders. Selecting a request opens it.
- **→** expands a collection or folder, then moves to its first item; **←** collapses it, or moves to the enclosing folder.
- Type the first letters of a name to jump to it.
- **Return** renames the selected item; **⌘↓** moves focus to the selected request's URL.
- **Delete** deletes the selected item.

## Files on disk

```text
workspace/
├── wirebolt.toml
├── collections/
│   └── <collection>/
│       ├── collection.toml
│       └── requests/<request>.toml
└── environments/<environment>.toml
```

Files use versioned TOML. Stable IDs identify documents; ordering is stored explicitly. Prefer app commands for ordinary edits. If you edit files outside the app, save or close your drafts first and reopen the workspace to load the changes.

Response bodies, local history, cookies and Keychain credentials are runtime data rather than workspace documents. Literal text you enter in a URL, body or variable **is** part of the saved definition. [Understand what is shared](environments-and-secrets.md#what-is-shared).

## Cookies

Each workspace has its own cookie jar, stored in `~/Library/Application Support/Wirebolt/Cookies` rather than in the workspace folder. Cookies set by a response are sent with later requests that match their domain, path and Secure attribute. Cookies with an expiry date are kept across launches; session cookies (no Expires or Max-Age) last until Wirebolt quits. A request that sets its own `Cookie` header does not receive jar cookies.

**Workspace → Cookies…** lists the open workspace's cookies with their domain, path, expiry and flags. Values are hidden until you choose **Show Values**. Select cookies and click **Delete** (or press Delete), or use **Clear All…** to empty the jar. Other workspaces keep their own cookies.

## Collaborate

Make the workspace folder the root of a Git repository (**Workspace → Git Collaboration…** offers **Initialize Git Repository**) and configure a remote, then use [Git Collaboration](git-collaboration.md). Export is another option when you want a [portable JSON document](import-export.md).
