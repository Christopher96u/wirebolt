# Proxy and network settings

[Documentation](README.md)

## Choose the scope

| Scope | Where | Persist changes |
| --- | --- | --- |
| App default on this Mac | **Wirebolt → Settings → Network** | **Save** |
| Workspace override | **Workspace Settings** gear or Workspace menu | **Save** (proxy and transport settings); **Cancel** discards |
| Request override | Request **Settings** tab | **Apply to request**, then **⌘S** |

Resolution follows **request → workspace → app default**. A new installation uses the system proxy. The indicator beside Send shows the effective policy and its source.

![Workspace proxy settings](assets/screenshots/proxy.png)

## Modes

| Mode | Behavior |
| --- | --- |
| Inherit | Use the parent policy; available at workspace and request scope |
| System | Explicitly use macOS proxy settings |
| Direct — No proxy | Connect directly, overriding parent proxies |
| Manual | Use configured HTTP, HTTPS or SOCKS proxy routes |

Manual routes can target HTTP destinations, HTTPS destinations or all destinations. These describe the **destination URL scheme**, not simply the protocol used to connect to the proxy. Enter the proxy endpoint with its scheme and port.

An unmatched destination connects directly. A failed proxy connection does **not** trigger a direct retry. Host exclusions are not exposed in the UI yet.

For example, set a company proxy at workspace scope, then select Direct on a local development request. The workspace remains shared while that request bypasses the proxy.

## Test and verify

**Test connection** sends a cancellable HEAD probe to the URL you provide. It does not include the request's auth, headers, body or cookies. A server that rejects HEAD may fail this probe while a normal GET still succeeds.

Send the actual request to verify its complete configuration. The response's **Request** panel records the policy captured for that execution. HTTP, WebSocket and Copy cURL use the same policy inheritance. Reconnect a WebSocket after changing settings.

## Credentials and sharing

App defaults remain local to the Mac. Workspace and request proxy definitions can be shared as files; credential material stays in Keychain, with references in those definitions. Teammates must provision credentials locally.

## Timeouts, redirects and TLS

Expand **Timeouts, redirects & TLS** in workspace settings. Workspace transport edits are saved together with the proxy policy when you click **Save**. In a request, disable **Inherit workspace transport settings** to override them, then save the request.

- **Total timeout** limits the total execution; **Read timeout** controls the read deadline. Values are milliseconds; `0` disables that deadline.
- **Follow redirects** enables redirect following up to **Maximum redirects**.
- **Validate TLS certificates** controls certificate validation. Keep it enabled for normal use; disabling it removes server-certificate verification for that configuration.

In the workspace settings sheet, **Save** (Return) applies every edit and **Cancel** (Esc) discards them after confirmation.
