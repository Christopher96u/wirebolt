# Environments and secrets

[Documentation](README.md)

## Create variables

1. Open the environment menu in the toolbar and choose **Configure Environments…**.
2. Select **Global Environment**, or create a named environment with **New Environment**.
3. Add a key and value. Keep the row checked to enable it.
4. Click **Save** (or press Return). **Cancel** (Esc) discards your edits, including deleted environments, after confirmation.
5. Select the desired environment in the toolbar.

![Environment variables for the local demo API](assets/screenshots/environments.png)

Use variables in request fields with double braces:

```text
{{base_url}}/products?category={{category}}
```

Global variables apply across environments. The selected environment overrides matching Global keys. A disabled row is omitted from resolution. If a variable fails to resolve, check spelling, enabled state and the selected environment before sending again.

Selecting an environment in the editor chooses what you edit; selecting it in the main toolbar chooses what requests use.

## Credentials and secret references

A literal value is stored in the workspace. A secret reference stores a name whose value resolves from this Mac's Keychain. A teammate receives the reference name, not its credential value.

Basic and Bearer authentication fields integrate with Keychain. OAuth provides a dedicated client-secret storage action and stores acquired tokens in Keychain. Proxy credentials are also local. See [authentication](authentication.md) for exact controls.

The environment editor currently edits literal values and existing secret-reference names; it does not provide a general secret creation or literal-to-secret toggle. Do not paste a real credential into an ordinary variable expecting it to become secret automatically.

For a manually provisioned reference, use **Keychain Access → File → New Password Item** in the login keychain:

- **Keychain Item Name:** `io.github.christopher96u.wirebolt` (the service).
- **Account Name:** the exact reference, for example `demo.api-key`.
- **Password:** the credential value.

Approve access for Wirebolt when macOS asks. The API key **Value Secret** field expects that account/reference name. It does not store a pasted token as a new Keychain item.

## What is shared

| Data | Workspace / export |
| --- | --- |
| Saved URLs, parameters, headers, bodies, notes | Included |
| Literal environment values | Included |
| Secret-reference names | Included |
| Referenced Keychain credential values | Not resolved into the export |
| Response history and cookies | Not included in the workspace document tree |
| Paths to upload files | Can be included; file bytes are not bundled |

Review literal fields before committing or exporting. Keychain storage cannot protect a credential you put directly in an ordinary URL, note or JSON body.
