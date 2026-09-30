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
- **Client certificate** sends a certificate to servers that require mutual TLS (mTLS). Click **Choose…** and select one PEM file that contains the certificate and its private key, or select the certificate and key files together. Include intermediate certificates after the leaf certificate if the server needs them. The private key must be unencrypted; convert DER or PKCS #12 (`.p12`) files to PEM first, for example `openssl pkcs12 -in client.p12 -out client.pem -nodes`. Wirebolt stores the certificate and key in Keychain; workspace files keep only a reference, so each teammate chooses their own certificate. **Remove** stops sending it.
- **Custom CA** also trusts the certificate authorities in a PEM bundle, such as a private or development CA. Wirebolt reads the file when it sends a request and stores its absolute path in the workspace, so the file must exist at the same path on each Mac that uses it.

If the certificate or the CA file can't be loaded, the request fails with a TLS configuration error before it connects. **Copy cURL** includes `--cacert` with the CA path, and `--cert` and `--key` that read the certificate and key from Keychain with `security find-generic-password` when the command runs. The copied command never contains the key, and it works only on a Mac whose Keychain holds that item.

In the workspace settings sheet, **Save** (Return) applies every edit and **Cancel** (Esc) discards them after confirmation.
