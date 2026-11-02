# Documentation assets

[Documentation](../README.md)

`icon.png` is exported from the app's existing `AppIcon.icns`.

The environment inspector image was refreshed for the pre-rc2 integration on November 2, 2026, using the `environment-screenshots` native workload and `NativeTestSupport.capture`. It renders the current inspector in process with literal fixture variables, disposable preferences and isolated history/cookies; it does not open a real workspace or access Keychain.

The other screenshots show Wirebolt at source revision `324d11a`, using a disposable Catalog API workspace and local fixtures. Overview, notes, WebSocket and Git images were captured from the running app window. The proxy image was rendered from the same native settings view and omits the surrounding window. These are actual app views, not design mockups or generated artwork.

| Image | Demonstrates |
| --- | --- |
| `screenshots/overview.png` | Real HTTP response from the documented local demo server |
| `screenshots/environments.png` | Current environment inspector, rendered in process with literal demo variables and disposable preferences |
| `screenshots/proxy.png` | Manual workspace proxy configuration; the displayed endpoint is illustrative, not a running proxy |
| `screenshots/notes.png` | Native Markdown preview beside a real response |
| `screenshots/websocket.png` | Text exchange with a local WebSocket echo fixture |
| `screenshots/git.png` | Status of an isolated demo Git repository with a local bare remote; no hosted service |

To refresh: build current source, create a disposable workspace with demo data, run the local HTTP fixture, and capture the corresponding native views or visible app window. Use a WebSocket echo fixture for the socket view; the documented HTTP server does not implement that protocol. Check every image for credentials and private data, preserve legible text, and update this provenance and the relevant guide together. Do not use audit failure screenshots as product screenshots.
