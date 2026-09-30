# Import and export

[Documentation](README.md)

## Import

Open the **+** menu and choose an import action. Supported formats are:

| Format | Intended input |
| --- | --- |
| cURL | A request command |
| HAR | An HTTP archive |
| Postman v2 | A collection document |
| Wirebolt / Legacy Collection v1 JSON | A Wirebolt export or supported legacy document |

Review the import preview and diagnostics before confirming. Unsupported fields or malformed input should not be assumed to transfer merely because the original client supports them. Wirebolt does not execute imported Postman scripts as a scripting runtime.

For a quick local example, import:

```sh
curl -X POST http://127.0.0.1:18990/echo -H 'Content-Type: application/json' --data '{"name":"Desk lamp","quantity":2}'
```

Run the [demo API](quick-start.md) first if you want to send it.

## Export

Save edits with **⌘S**. Choose **Export Wirebolt JSON…** from the relevant request, collection or workspace actions and select a destination file.

Exports describe **saved definitions**, not unsaved tab drafts. They do not bundle response history, upload-file contents or resolved Keychain credentials.

| Export | Contents |
| --- | --- |
| Request | One request |
| Collection | The collection, its folders and requests |
| Workspace | Every collection, top-level requests and environments |

Each request keeps its headers, query parameters, body and content type, authentication, notes, proxy override and transport settings (TLS validation, redirects, timeouts, client certificate and custom CA). Secret values are never exported: header, query, form, multipart and authentication fields that use Keychain secrets keep only the secret's reference name, and sensitive fields stay marked sensitive. Environment variables export literal values as written and secret variables as reference names.

Upload files, for **File** bodies and multipart file parts, are exported as paths. If a path doesn't exist on the Mac that imports the file, Wirebolt imports the request and warns so you can choose the file again.

Importing a Wirebolt export with **Wirebolt / Legacy Collection v1 JSON** restores:

- a collection export as one collection with its original name;
- a workspace export as one collection per exported collection, a collection named after the file for top-level requests, and the exported environments. The exported global environment becomes this workspace's global environment only if it doesn't have one; otherwise it's added as a regular environment.

Imported collections, folders, requests and environments get new identifiers, so importing never replaces existing items.

## Portability

API key and OAuth definitions use typed `wirebolt.authentication` metadata. Wirebolt reimports this configuration, including reference names, through **Wirebolt / Legacy Collection v1 JSON**. Other clients may not understand the extensions; this is not a promise of full Postman export compatibility.

After importing on another Mac, configure its environment, local upload paths and credentials before sending. Review URLs, literals and headers before sharing an exported file.
