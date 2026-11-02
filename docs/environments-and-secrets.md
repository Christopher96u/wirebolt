# Environments and secrets

[Documentation](README.md)

## Create variables

1. Open the environment menu in the toolbar and choose **Configure Environments…**.
2. Select **Globals**, or create a named environment with **New Environment**.
3. Click **New Entry**, then edit the variable name and value. Keep the row checked to enable it. Click the lock button to make the value a secret.
4. Finish a field with Return, Tab or by moving focus. Each change saves immediately and can be undone with **⌘Z** or redone with **⇧⌘Z**. Closing the inspector also saves the field you are editing. Invalid names stay visible with an inline explanation; correct them before closing or switching environments.
5. Choose **Use for Requests** for the environment you are editing, or select it in the composer's environment picker. Browsing the inspector, including Globals, never changes the active environment. The active environment is marked **Active**.

Use variables in request fields with double braces:

```text
{{base_url}}/products?category={{category}}
```

Outside editing, the URL shows each reference as a chip with just the variable name, red when no active environment defines it; click the URL to edit the raw `{{name}}` text. Global variables apply across environments. The selected environment overrides matching Global keys. A disabled row is omitted from resolution. If a variable fails to resolve, check spelling, enabled state and the selected environment before sending again.

Selecting an environment in the inspector finishes the current edit and chooses what you edit next. Use **Use for Requests** or the composer's environment picker to change the active environment. Globals always appears once, at the top. Use an environment's context menu to rename or delete it. Long names and values use one line; hover or focus a field to read the full text.

## Credentials and secret references

A literal value is stored in the workspace. A secret reference stores a name whose value resolves from this Mac's Keychain. A teammate receives the reference name, not its credential value.

### Secret variables

In **Configure Environments…**, click a variable's lock button (or choose **Make Secret** from its context menu) to store its value in Keychain:

- The value field is masked; use the eye button to show it.
- Finishing an edit writes the value to a fresh Keychain item and saves only its reference in the environment file. Undo and redo restore the previous value without storing credential material in workspace files or undo steps.
- Requests use a secret variable like any other: `{{token}}`. Headers and bodies that use it are redacted in the sent-request view.
- Click the lock again (**Store in Workspace File**) to turn it back into a literal; the value is then saved in the environment file.

Each secret variable has its own Keychain item, so renaming the key keeps its value. A teammate who pulls the workspace sees the variable with an empty value and enters their own. Removing a secret variable or its environment deletes the Keychain item when you open another workspace, a pull reloads the workspace, or you quit; references you typed yourself, such as proxy credentials, are never deleted, and neither are items another workspace in **File → Open Recent** still references.

Basic, Bearer and API Key authentication fields and OAuth client secrets also store their values in Keychain, and OAuth stores acquired tokens there. Proxy credentials are also local. See [authentication](authentication.md) for exact controls.

Items use the Keychain service `io.github.christopher96u.wirebolt`. Approve access for Wirebolt when macOS asks.

## What is shared

| Data | Workspace / export |
| --- | --- |
| Saved URLs, parameters, headers, bodies, notes | Included |
| Literal environment values | Included |
| Secret-reference names | Included |
| Referenced Keychain credential values, including secret variables | Not resolved into the export |
| Response history and cookies | Not included in the workspace document tree |
| Paths to upload files | Can be included; file bytes are not bundled |

Review literal fields before committing or exporting. Keychain storage cannot protect a credential you put directly in an ordinary URL, note or JSON body.
