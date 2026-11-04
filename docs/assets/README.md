# Documentation assets

[Documentation](../README.md)

`icon.png` is exported from the app's existing `AppIcon.icns`.

All twelve screenshots were refreshed on **November 5, 2026**, for the **1.0.0-rc.2** preparation, using native UI source at `f7a2871` (the merged command-palette implementation). They are captures of production SwiftUI/AppKit views hosted by the documentation runner, with a disposable Catalog API workspace, preferences, history, cookie jar and theme library. They omit the macOS menu bar and window frame. They are not design mockups or generated artwork.

Overview and Notes show real responses from the production Rust HTTP transport against the [documentation demo API](../examples/demo-server.py). The capture runner starts that API on a free loopback port; the normal quick-start command uses port 18990. WebSocket messages/handshake events and Git status are display fixtures injected into the production views. No WebSocket endpoint, remote Git repository, OAuth provider or proxy is contacted for those images. Proxy endpoints and OAuth URLs are illustrative. Environment values and cookies contain demonstration data only. The runner renders a fixture workspace, uses disposable app preferences/runtime storage and does not read or write Keychain credentials.

| Image | Demonstrates |
| --- | --- |
| `screenshots/overview.png` | JSON POST and successful live loopback response |
| `screenshots/notes.png` | Local Markdown preview beside that response |
| `screenshots/environments.png` | Globals, named environments, explicit active selection and literal variables in the inspector |
| `screenshots/proxy.png` | Manual workspace proxy route; the endpoint is illustrative |
| `screenshots/websocket.png` | Sent/received demonstration messages and selected-message contents |
| `screenshots/git.png` | Display-only Git status with demonstration workspace changes |
| `screenshots/command-palette.png` | Workspace request search and an existing-tab marker |
| `screenshots/command-palette-environments.png` | Environment choices and the active checkmark |
| `screenshots/command-palette-themes.png` | System, built-in and installed custom theme choices |
| `screenshots/appearance.png` | Appearance settings with a custom theme, density, font and motion preferences |
| `screenshots/authentication.png` | OAuth PKCE configuration with demonstration provider URLs |
| `screenshots/cookies.png` | Cookie metadata with values hidden |

## Refresh

On the project's supported Apple Silicon development setup:

```sh
./scripts/docs-screenshots.sh
```

The runner builds the current native views, starts the isolated HTTP fixture, checks GET/POST responses and captures the twelve PNGs under `build/docs-screenshots/`. It writes a manifest and source commit beside them, then stops its server and removes temporary fixture storage and preferences. Pass another output directory as the first argument when needed.

Review every image for layout, legibility and private content before copying the PNGs into `docs/assets/screenshots/`. Update this provenance and the relevant guide together. The manifest identifies the capture method; it does not establish manual VoiceOver coverage, packaged-app compatibility or performance measurements. Keep test failure images out of product documentation.
