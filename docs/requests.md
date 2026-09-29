# Requests and responses

[Documentation](README.md)

## Compose a request

Create an HTTP request with **File → New Request** (**⌘N**), the toolbar **+** menu or a collection's context menu. Select a method and enter an `http://` or `https://` URL. Save with **⌘S** to persist the draft in a collection.

| Panel | Use it for |
| --- | --- |
| Params | Query parameter rows; unchecked rows are omitted |
| Headers | Request headers; unchecked rows are omitted |
| Body | Empty, text, JSON, XML, HTML, raw, URL-encoded form, multipart or file content |
| Auth | Basic, Bearer, API key or OAuth configuration |
| Note | Markdown documentation saved with the request |
| Settings | Request proxy policy and transport overrides |

Switch panels with **⌥⌘1** – **⌥⌘6** (**Navigate → Request Section**). **Request → Add Key** (**⇧⌘K**) adds a Params or Headers row; from any other panel it switches to Params first.

For multipart uploads, choose text or file parts and select the local files. File bodies refer to local paths: sharing a workspace does not upload the file or make that path exist on another Mac.

## Send and cancel

Click **Send** or press **⌘Return**. Send is unavailable until the request has a URL. While the request runs, **Cancel** replaces Send; press **⌘.** or use the **Cancel** button beside the elapsed time in the response area. A response is associated with the tab that sent it. Editing another request or switching tabs does not retarget an in-flight response.

The status beside the URL shows the latest outcome: the response status (for example **200 OK**), **Failed** with the reason in its help tag, or **Cancelled**. In a narrow split editor it shows only the icon.

Sending and saving are separate operations. **Send** uses the current draft; **⌘S** persists its definition. Export and Git use saved definitions.

## Inspect the response

![Native JSON response view](assets/screenshots/overview.png)

The response area shows status, timing and size. Use its panels to inspect the body, headers, cookies and executed request, or switch with **⌃⌘1** – **⌃⌘5** (**Navigate → Response Section**). **Navigate → Focus Response** (**⌥⌘0**) moves the keyboard focus into the response. The **Request** panel records the prepared execution, including the proxy policy used for that send.

While a request is resent, the previous response stays visible, dimmed, until the new one arrives. When a request fails, the response area explains the failure and offers **Retry**, **Network Settings** and, when available, **View Request**.

Use the renderer menu for JSON, JSON tree, raw text, XML, HTML, Webview, image or hex views as appropriate for the response. A renderer cannot turn incompatible bytes into valid JSON or an image; use Raw or Hex to inspect unexpected content.

The HTML source view and Webview serve different purposes. Open a Webview only for content you want to render; Markdown notes use a separate local preview.

## Tabs, split editors and history

Clicking a request in the sidebar opens it in a *preview* tab (italic title) that the next click reuses. Editing or sending the request, double-clicking the tab or ⌘-clicking the sidebar row keeps it open. A dot replaces the close button on tabs with unsaved edits. Switch tabs with **⇧⌘]** / **⇧⌘[**, **⌃Tab** / **⌃⇧Tab** or **⌘1** – **⌘9**.

Open multiple requests in tabs. **Navigate → Split Right** (**⇧⌘D**) creates a second editor group for comparison. Drafts and responses are independent between document sessions. Save each edited draft you want to retain. Wirebolt reopens your tabs and the latest response of each request at the next launch.

Use **Request History** beside the request controls to inspect available prior runs. History is local runtime data, subject to storage limits, and is not included in workspace exports or Git commits.

Use **Request → Copy cURL** to copy a command representation. Review the clipboard content before sharing: URLs, bodies and manually entered values can contain sensitive information even when credential fields are redacted.
