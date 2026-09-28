# Workspaces and organization

[Documentation](README.md)

A workspace is a folder containing request definitions, collections and environments. It is the unit you can version with Git or share with another person.

## Create or open

- **File → New Workspace…** creates a new workspace folder.
- **File → Open Workspace…** (**⌘O**) opens a folder containing `wirebolt.toml`.
- **File → New Collection…** creates a top-level collection.
- The **+** menu also exposes workspace and collection actions.

Switching workspaces closes the old tabs and cancels active requests. If there are unsaved request edits, save them first or explicitly discard them in the prompt. A failed open preserves the current workspace.

## Collections, folders and ordering

Create requests and folders from the **+** menu or the relevant context menu. Right-click a sidebar item for its available actions.

Drag a request or subfolder to the **upper or lower edge** of a sibling row to reorder it. The insertion line shows the destination. Requests and folders can be mixed. Drop in the **center of a folder** to move an item into it.

Moving an open request keeps its document session associated with its new collection. Deleting a folder deletes its descendants and closes their tabs; read the confirmation before discarding edited documents.

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

## Collaborate

Initialize a Git repository in the workspace folder and configure a remote, then use [Git Collaboration](git-collaboration.md). Export is another option when you want a [portable JSON document](import-export.md).
