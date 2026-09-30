# Import and export

[Documentation](README.md)

## Import

Open the **+** menu and choose an import action. Supported formats are:

| Format | Intended input |
| --- | --- |
| cURL | A request command |
| HAR | An HTTP archive |
| Postman v2 | A collection document |
| Insomnia (v4 JSON or v5 YAML) | An Insomnia collection export |
| Bruno Collection (Folder or JSON) | A Bruno collection folder, a single `.bru` file or a Bruno JSON export |
| Wirebolt / Legacy Collection v1 JSON | A Wirebolt export or supported legacy document |

You can also open or drop these files on the window; Wirebolt detects the format from the contents. Drop a Bruno collection folder to import it.

Review the import preview and diagnostics before confirming. Unsupported fields or malformed input should not be assumed to transfer merely because the original client supports them. Wirebolt does not execute imported Postman scripts as a scripting runtime.

### Insomnia

Export from Insomnia with **Export Data** (Insomnia v4, JSON or YAML) or export a collection as YAML (Insomnia v5, from Insomnia 10 onward). Wirebolt imports:

- folders and their order, requests, WebSocket requests, query parameters, headers and path parameters, including disabled rows;
- JSON, form URL-encoded, multipart (text and file parts), GraphQL (as a JSON body), file, XML, HTML and other text bodies;
- Basic, Bearer, API Key (header or query) and OAuth 2.0 (authorization code with PKCE, client credentials) authentication, including authentication and headers inherited from folders;
- request descriptions as notes;
- the base environment and its sub environments. Each sub environment becomes one Wirebolt environment that contains the base variables plus its own; nested values become dotted names such as `api.host`.

Variable tags such as `{{ _.base_url }}` become `{{base_url}}`. Other template tags (response references, `{% uuid %}` and other functions, filters) stay as written and are listed after the import so you can replace them.

### Bruno

In the **+** menu choose **Import → Bruno Collection (Folder or JSON)** and select the collection folder (the one with `bruno.json`) or a JSON file from Bruno's **Export Collection**. Wirebolt reads `bruno.json`, `collection.bru`, `folder.bru`, request `.bru` files and `environments/*.bru`; other files are not read. It imports:

- folders and requests in Bruno's sequence order, query and path parameters, headers, disabled (`~`) rows and request docs as notes;
- JSON, text, XML, SPARQL, form URL-encoded, multipart, file and GraphQL bodies; upload paths are resolved against the collection folder;
- Basic, Bearer, API Key and OAuth 2.0 (authorization code, client credentials) authentication, with collection and folder authentication and headers applied to requests that inherit them;
- environments, layered over collection variables. Secret variables become Keychain references: Bruno does not store their values in the collection, so set them in **Configure Environments** before sending.

Bruno's `{{variable}}` syntax is used unchanged.

### What is not imported

Neither client's scripts, tests or assertions run in Wirebolt, so they are not imported. The same applies to cookies, gRPC and Socket.IO requests, saved WebSocket messages, folder-level variables, unsupported authentication types (for example Digest, NTLM, AWS Signature) and OAuth 2.0 client secrets, which you enter again in the **Auth** tab. Imported credentials are moved to the Keychain; values that reference a variable, such as `{{token}}`, stay variable references.

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
