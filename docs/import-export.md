# Import and export

[Documentation](README.md)

## Import

Open the **+** menu and choose **Import**, or drop a file on the window or open it from Finder. Wirebolt recognizes the format from the file's contents; the format chosen in the menu is used only when the contents don't say. Supported formats are:

| Format | Intended input |
| --- | --- |
| cURL | One or more commands, pasted or saved in a text file |
| HAR | An HTTP archive saved from a browser or proxy |
| Postman Collection v2 | A Postman Collection v2.0 or v2.1 export |
| Wirebolt / Legacy Collection v1 JSON | A Wirebolt export or supported legacy document |

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
