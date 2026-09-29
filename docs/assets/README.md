# Documentation assets

[Documentation](../README.md)

`icon.png` is exported from the app's existing `AppIcon.icns`.

Screenshots show Wirebolt at source revision `0378531`, using a disposable Catalog API workspace and local fixtures. Overview, notes, environments, WebSocket and Git images were captured from the running app window. The proxy image was rendered from the same native settings view and omits the surrounding window. These are actual app views, not design mockups or generated artwork.

| Image | Demonstrates |
| --- | --- |
| `screenshots/overview.png` | Real HTTP response from the documented local demo server |
| `screenshots/environments.png` | Literal local-development variables |
| `screenshots/proxy.png` | Manual workspace proxy configuration; the displayed endpoint is illustrative, not a running proxy |
| `screenshots/notes.png` | Native Markdown preview beside a real response |
| `screenshots/websocket.png` | Text exchange with a local WebSocket echo fixture |
| `screenshots/git.png` | Status of an isolated demo Git repository; no hosted remote |

To refresh: build current source, create a disposable workspace with demo data, run the local HTTP fixture, and capture the corresponding native views or visible app window. Use a WebSocket echo fixture for the socket view; the documented HTTP server does not implement that protocol. Check every image for credentials and private data, preserve legible text, and update this provenance and the relevant guide together. Do not use audit failure screenshots as product screenshots.
