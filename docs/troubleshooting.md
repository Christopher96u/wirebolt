# Troubleshooting

[Documentation](README.md)

| Symptom | Check |
| --- | --- |
| macOS blocks launching | Follow [first-launch instructions](installation.md#first-launch); use an Apple Silicon Mac with macOS 15+ |
| A feature in the docs is missing | Compare your version with [Unreleased changes](../CHANGELOG.md); docs follow current source |
| Workspace will not open | Choose the folder containing `wirebolt.toml`; check permissions and preserve a backup before manual edits |
| Variables are unresolved | Check exact key spelling, row enabled state, Global values and toolbar environment selection |
| Missing credential | The reference must exist in this Mac's Keychain; importing a workspace does not import its credential material |
| API key fails | **Value Secret** is a reference name, not a token-entry field; follow [secret provisioning](environments-and-secrets.md#credentials-and-secret-references) |
| Request cannot connect | Check the URL, server availability and the effective proxy policy beside Send |
| Local demo cannot connect | Start the demo server; confirm port 18990 is free and try Direct at request scope |
| Proxy test fails but GET works | The probe uses HEAD without request auth, headers, body or cookies |
| WebSocket changes do not take effect | Disconnect and reconnect with the new configuration |
| Export misses an edit | Save the request before exporting |
| Git actions are unavailable | Configure a workspace repository and remote; load status, enter a commit message or establish an upstream as needed |
| Markdown image does not render | Preview deliberately displays alternative text without fetching remote resources |

## Known boundaries

There is no general secret-management screen, no exposed proxy host-exclusion editor, and no promise that other clients understand Wirebolt's advanced-auth export extensions. OAuth compatibility depends on provider configuration. Published packages are ad-hoc signed and currently Apple Silicon only.

These guides document supported behavior; they are not a claim that every external proxy, identity provider or server configuration has been tested.

## Report a reproducible issue

Use the repository's [bug form](https://github.com/Christopher96u/wirebolt/issues/new/choose). Include:

- App version or source commit, macOS version and chip.
- Exact steps, expected result and actual result.
- A minimal request against a local or public non-sensitive endpoint.
- A screenshot or redacted error, if useful.
- Proxy scope/mode and selected environment when relevant.

For performance issues, include response size, tab/request count, what feels slow and whether the problem reproduces after restarting. Avoid attaching credentials, cookies, private response bodies or an entire real workspace. Follow [SECURITY.md](../SECURITY.md) for suspected vulnerabilities.
