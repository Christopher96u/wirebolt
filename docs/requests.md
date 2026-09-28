# Requests and responses

[Documentation](README.md)

## Compose a request

Create an HTTP request from the **+** menu or **Request → New Request → HTTP**. Select a method and enter an `http://` or `https://` URL. Save with **⌘S** to persist the draft in a collection.

| Panel | Use it for |
| --- | --- |
| Params | Query parameter rows; unchecked rows are omitted |
| Headers | Request headers; unchecked rows are omitted |
| Body | Empty, text, JSON, XML, HTML, raw, URL-encoded form, multipart or file content |
| Auth | Basic, Bearer, API key or OAuth configuration |
| Note | Markdown documentation saved with the request |
| Settings | Request proxy policy and transport overrides |

For multipart uploads, choose text or file parts and select the local files. File bodies refer to local paths: sharing a workspace does not upload the file or make that path exist on another Mac.

## Send and cancel

Click **Send** or press **⌘Return**. While the request runs, use **Cancel** to stop that execution. A response is associated with the tab that sent it. Editing another request or switching tabs does not retarget an in-flight response.

Sending and saving are separate operations. **Send** uses the current draft; **⌘S** persists its definition. Export and Git use saved definitions.

## Inspect the response

![Native JSON response view](assets/screenshots/overview.png)

The response area shows status, timing and size. Use its panels to inspect the body, headers, cookies and executed request. The **Request** panel records the prepared execution, including the proxy policy used for that send.

Use the renderer menu for JSON, JSON tree, raw text, XML, HTML, Webview, image or hex views as appropriate for the response. A renderer cannot turn incompatible bytes into valid JSON or an image; use Raw or Hex to inspect unexpected content.

The HTML source view and Webview serve different purposes. Open a Webview only for content you want to render; Markdown notes use a separate local preview.

## Tabs, split editors and history

Open multiple requests in tabs. **Navigate → Split Right** creates a second editor group for comparison. Drafts and responses are independent between document sessions. Save each edited draft you want to retain.

Use **Request History** beside the request controls to inspect available prior runs. History is local runtime data, subject to storage limits, and is not included in workspace exports or Git commits.

Use **Request → Copy cURL** to copy a command representation. Review the clipboard content before sharing: URLs, bodies and manually entered values can contain sensitive information even when credential fields are redacted.
