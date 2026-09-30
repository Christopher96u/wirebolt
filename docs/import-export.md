# Import and export

[Documentation](README.md)

## Import

Open the **+** menu and choose **Import**, or drop a file on the window or open it from Finder. Wirebolt recognizes the format from the file's contents; the format chosen in the menu is used only when the contents don't say. Supported formats are:

| Format | Intended input |
| --- | --- |
| cURL | One or more commands, pasted or saved in a text file |
| HAR | An HTTP archive saved from a browser or proxy |
| Postman Collection v2 | A Postman Collection v2.0 or v2.1 export |
| Insomnia (v4 JSON or v5 YAML) | An Insomnia collection export |
| Bruno Collection (Folder or JSON) | A Bruno collection folder, a single `.bru` file or a Bruno JSON export |
| Wirebolt / Legacy Collection v1 JSON | A Wirebolt export or supported legacy document |

You can also open or drop these files on the window; Wirebolt detects the format from the contents. Drop a Bruno collection folder to import it.

After an import, Wirebolt opens the first imported request and summarizes what it created: requests, folders and environments. When something couldn't be carried over exactly, the summary lists it so you can review it before sending. When an import fails, the message says why, for example that the file is an OpenAPI document, which isn't supported yet, or a Postman Collection v1, which must be exported again as v2.1.

Imported credentials — Basic passwords, bearer tokens, API keys and OAuth 2.0 client secrets — are stored in this Mac's Keychain; the workspace keeps only references. Values that are `{{variable}}` references stay as they are.

### Postman

- Folders, request order, methods, headers, query parameters and bodies are imported. Disabled headers, query parameters, form fields and variables stay disabled.
- Authentication is imported for Basic, Bearer, API Key and OAuth 2.0 (client credentials and authorization code). Requests that inherit authentication get their folder's or collection's configuration. Authorization code grants use PKCE and the redirect URI `wirebolt://oauth/callback`, which must be registered with the provider. Other types, such as Digest or AWS Signature, are listed in the summary.
- Raw bodies keep their language: JSON (even while `{{variables}}` make it invalid JSON), XML, HTML or text. GraphQL bodies become JSON with `query` and `variables`. Binary bodies become file bodies that point at the original path.
- Collection and folder variables become an environment named after the collection. Secret-type variables and variables whose names look like credentials, such as `accessToken`, are stored in Keychain.
- Path variables such as `:id` are replaced with their values.
- Request descriptions become the request's Notes. Collection and folder descriptions, pre-request and test scripts, and saved example responses aren't imported; the summary lists them. Wirebolt doesn't run Postman scripts.
- `protocolProfileBehavior` settings for SSL certificate validation and redirects become the request's transport settings.

### HAR

- Every HTTP and WebSocket entry becomes a request named after its method and path, in a collection named after the file.
- HTTP/2 pseudo-headers such as `:authority`, and connection headers such as `Host`, `Content-Length` and `Connection`, are dropped because the transport sets them.
- URL-encoded and multipart form bodies become form bodies. HAR files don't contain uploaded files, so file fields must be chosen again before sending.
- An `Authorization: Bearer` or `Basic` header becomes the request's authentication.

### cURL

- Multi-line commands, including a browser's **Copy as cURL**, and several commands in one paste or file are imported; each `curl` command becomes a request.
- `-X`, `-H`, `-d`/`--data`/`--data-raw`/`--data-binary`, `--data-urlencode`, `--json`, `-F`, `-u`, `-b`, `-A`, `-e`, `-I`, `-G`, `-T` and `--url` are mapped. Several `-d` values are joined with `&`; `-d` data without a `Content-Type` is sent as a URL-encoded form, and `key=value` data becomes form fields. `@file` data becomes a file body.
- `-k`, `-L`, `--max-redirs`, `-m` and `--cacert` become the request's transport settings. Options that only affect output, such as `-s`, `-o` or `--compressed`, are ignored; other unsupported options, such as `--proxy`, are listed in the summary.

### Insomnia

Export from Insomnia with **Export Data** (Insomnia v4, JSON or YAML) or export a collection as YAML (Insomnia v5, from Insomnia 10 onward). Wirebolt imports:

- folders and their order, requests, WebSocket requests, query parameters, headers and path parameters, including disabled rows;
- JSON, form URL-encoded, multipart (text and file parts), GraphQL (as a JSON body), file, XML, HTML and other text bodies;
- Basic, Bearer, API Key (header or query) and OAuth 2.0 (authorization code with PKCE, client credentials) authentication, including authentication and headers inherited from folders;
- request descriptions as notes;
- the base environment and its sub environments. Each sub environment becomes one Wirebolt environment that contains the base variables plus its own; nested values become dotted names such as `api.host`.

Variable tags such as `{{ _.base_url }}` become `{{base_url}}`. Other template tags (response references, `{% uuid %}` and other functions, filters) stay as written and are listed in the summary so you can replace them. Variables whose names look like credentials are stored in Keychain, as for Postman.

### Bruno

In the **+** menu choose **Import → Bruno Collection (Folder or JSON)** and select the collection folder (the one with `bruno.json`) or a JSON file from Bruno's **Export Collection**. Wirebolt reads `bruno.json`, `collection.bru`, `folder.bru`, request `.bru` files and `environments/*.bru`; other files are not read. It imports:

- folders and requests in Bruno's sequence order, query and path parameters, headers, disabled (`~`) rows and request docs as notes;
- JSON, text, XML, SPARQL, form URL-encoded, multipart, file and GraphQL bodies; upload paths are resolved against the collection folder;
- Basic, Bearer, API Key and OAuth 2.0 (authorization code, client credentials) authentication, with collection and folder authentication and headers applied to requests that inherit them;
- environments, layered over collection variables. Secret variables become Keychain references: Bruno doesn't store their values in the collection or its exports, so set them in **Configure Environments** before sending.

Bruno's `{{variable}}` syntax is used unchanged.

### What isn't imported from Insomnia and Bruno

Neither client's scripts, tests or assertions run in Wirebolt, so they aren't imported. The same applies to cookies, gRPC and Socket.IO requests, saved WebSocket messages, folder-level variables and unsupported authentication types (for example Digest, NTLM, AWS Signature). The summary lists each of them.

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
