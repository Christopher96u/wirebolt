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

## Portability

API key and OAuth definitions use typed `wirebolt.authentication` metadata. Wirebolt reimports this configuration, including reference names, through **Wirebolt / Legacy Collection v1 JSON**. Other clients may not understand the extensions; this is not a promise of full Postman export compatibility.

After importing on another Mac, configure its environment, local upload paths and credentials before sending. Review URLs, literals and headers before sharing an exported file.
