# Rust application core

Status: in progress on `refactor/rust-app-core` (one pull request).

## Goal

Move every piece of application state and logic that does not need an Apple API out of Swift and into Rust, so that the macOS app becomes a rendering layer plus a few platform adapters. A future Linux or Windows interface would then be only an interface.

Scope is **macOS only**. No other platform is built or packaged here.

## Non-goals

- No user-visible behavior, layout or styling changes. Every screen, shortcut, message and file format stays the same.
- No new features, no redesign, no dependency upgrades beyond what the move needs.
- No Linux or Windows code paths, interface or packaging.

## Target architecture

```text
crates/
├── wirebolt-core/   # transport, storage, imports/exports, proxy, git (unchanged role)
├── wirebolt-app/    # NEW: application core — state + commands + events, no UI, no Apple APIs
│   ├── domain/      #   requests, workspace drafts, environments, variables, cURL, proxy policy,
│   │                #   sidebar snapshot, labels, theme tokens and contrast, OAuth 2 logic
│   ├── response/    #   body store + viewport, line/text indexes, search, folding, wrapping,
│   │                #   JSON document/tree, formatting, markdown parsing, history, cookies
│   └── session/     #   documents, editor groups, tabs, preview tabs, back/forward, layout,
│                    #   runs and streaming preview, WebSocket sessions, workspace orchestration
│                    #   (load, mutations, undo, send/cancel, environments, secrets, import/export, git)
└── wirebolt-ffi/    # UniFFI bridge; `app_domain.rs`, `app_response.rs`, `app_session.rs` expose wirebolt-app
apple/
├── Sources/WireboltKit/  # shrinks to: thin @Observable facades over Rust state, platform adapters
└── WireboltApp/          # SwiftUI/AppKit views (unchanged except call sites)
```

Rules:

1. **Rust owns state and logic.** Swift keeps only what needs SwiftUI, AppKit or another Apple framework: views, the native text editor, file panels, `NSWorkspace`, WebKit preview, and adapters for platform services.
2. **Platform services are Rust traits** (UniFFI foreign traits / callback interfaces) implemented in Swift: for example the OAuth browser session (`ASWebAuthenticationSession`), Keychain access that is not already in `wirebolt-core`, clipboard if needed. `wirebolt-app` must not depend on any Apple API; `cfg(target_vendor = "apple")` stays out of it.
3. **Swift facades keep observation fine-grained.** SwiftUI renders from `@Observable` objects. Facades mirror Rust state through events or snapshots and update only the properties that changed, so views re-render exactly as today. Keep the existing Swift type and member names where views use them, so view code changes as little as possible.
4. **Rust calls the core directly.** Where Swift used to round-trip through FFI adapters (`RustRequestRunner`, `RustWebSocketRunner`, `RustWorkspacePersistence`), the application core now calls `wirebolt-core` itself; the adapters disappear or shrink to wiring.
5. **One domain model.** Request, workspace and environment types exist once, in Rust. Swift uses UniFFI records/enums (or thin wrappers) instead of re-declaring them and decoding JSON. Prefer typed UniFFI records over JSON strings for new surfaces; existing JSON endpoints may be replaced.
6. **Hot paths stay cheap.** Viewport reads, line lookups while scrolling or typing, tab switches and streaming chunks must not add per-item FFI round-trips; batch across the bridge. The native performance contract (`./scripts/check.sh` workloads: tab switch, request click, sidebar filter, editor, response, markdown, interface) must keep passing within noise.

## Work split

One pull request, built by three agents in two waves on separate branches, integrated by the orchestrator.

| Wave | Agent | Branch | Owns (Swift → Rust) |
| --- | --- | --- | --- |
| 1 | A — response engine | `refactor/rust-app-core-response` | `ResponseStorage` (body store, history, cookies), `ResponseIndexCache`, `ResponseTextIndex`, `TextLineIndex`, `TextSearch`, `CodeFolding`, `CodeTextWrapping`, `JSONNode`, `JSONResponseDocument`, `ResponseFormatting`, `MarkdownPreview` (parsing) → `wirebolt-app::response`, `app_response.rs` |
| 1 | B — domain | `refactor/rust-app-core-domain` | `Models`, `VariableTemplates`, `CurlExport`, `ProxySettings`, `ProxyConnectionTest`, `SidebarSnapshot`, `WorkspaceChrome`, `CollectionImportSources`, `ThemeDefinition`, `SemanticPalette`, `OAuth2Service` (logic; browser via trait) → `wirebolt-app::domain`, `app_domain.rs` |
| 2 | C — sessions & orchestration | `refactor/rust-app-core-session` | `DocumentSessions`, `WireboltModel`, `WebSocket`, and the non-view logic in `WorkspaceUIState`, `WorkspaceActions`, `RustRequestRunner`, `RustWebSocketRunner`, `RustWorkspacePersistence` → `wirebolt-app::session`, `app_session.rs` |

Wave 2 starts from the integrated result of wave 1. Agents edit app-target (`apple/WireboltApp`) files only where a call site must change, and keep those edits minimal.

## Testing and acceptance

- Port every Swift test that exercises moved logic to Rust with the same cases (Swift Testing → `#[test]` / `#[tokio::test]`). Keep or adapt Swift tests that cover facades and adapters. Coverage must not drop.
- Keep the bridge smoke (`apple/Tests/BridgeSmoke.swift`, `StreamBridgeSmoke.swift`) passing; extend it for new surfaces where useful.
- Build lists: `scripts/app-sources.sh`, `scripts/test.sh` (bridge smoke), `scripts/editor-performance.sh` and the native workload build compile explicit Swift file lists. Add, remove or rename entries with every file change.
- `./scripts/check.sh` (lint, Rust and Swift tests, bridge smoke, performance smoke, editor and native workloads, app build, launch smoke) passes locally.
- `docs/development.md` (code map) and `CONTEXT.md` (if vocabulary changes) describe the new layout.
- This document ends with a table **What remains in Swift and why**, one row per remaining `WireboltKit` file and per app-target file that still holds non-view logic.

## Review

Each agent reviews its own branch with the process in `/Users/chris96u/.claude/skills/code-review/SKILL.md` (two axes: **Standards** — `CONTRIBUTING.md`, `docs/development.md`, the smell baseline in that skill; **Spec** — this document), against the branch's starting commit, then fixes every hard finding. The orchestrator then reviews the integrated branch the same way before opening the pull request.

Commit subjects are single-line and imperative; no `Co-authored-by` or tool attribution lines (`CONTRIBUTING.md`).

## What remains in Swift and why

Wave 1 domain files (the response/session owners add their remaining files during integration):

| File | What remains and why |
| --- | --- |
| `WireboltKit/Models.swift` | Thin projections of typed Rust request, workspace, environment and run-input records; Codable compatibility for existing adapters; runtime response/Git event envelopes consumed by the wave 2 orchestration. Child arrays and URL-edit display results are cached native projections for cheap view reads; the URL bridge carries no request body. |
| `WireboltKit/VariableTemplates.swift` | Native UTF-16/NSRange projections, cached completion entries and a batched Foundation locale-comparison foreign trait; all parsing, merging, masking and matching are Rust. |
| `WireboltKit/CurlExport.swift` | Typed export facade and Keychain-service identity; expansion, quoting and argument construction are Rust. |
| `WireboltKit/ProxySettings.swift` | Observable UserDefaults preferences, native UUID/error projections and typed proxy-form facades. |
| `WireboltKit/ProxyConnectionTest.swift` | Main-actor observation and task/event relay to the existing RequestRunner integration seam; Rust owns probe state, stale-event checks, draft construction, validation and results. The runner is the wave 2 transport adapter. |
| `WireboltKit/SidebarSnapshot.swift` | Shared native row projections and expansion presentation state, with one batched Rust index/filter call; rendering reads cached metadata. |
| `WireboltKit/WorkspaceChrome.swift` | Typed label facades and Apple locale-aware numeric formatting of Rust-selected timeout units. |
| `WireboltKit/CollectionImportSources.swift` | URL/error projections and a native package-folder foreign trait; recognition, filesystem traversal, limits and bundle construction are Rust. |
| `WireboltKit/ThemeDefinition.swift` | Cached token projections and the observable ThemeStore; all presets, density, geometry, typography and contrast resolution are Rust. |
| `WireboltKit/SemanticPalette.swift` | Cached native color projections for renderers; color selection, luminance and contrast are Rust. |
| `WireboltKit/OAuth2Service.swift` | ASWebAuthenticationSession presentation and Keychain access implement the Rust foreign trait; native error localization preserves network diagnostics; protocol requests, PKCE and token parsing/exchange are Rust. |
| `WireboltApp/RustWorkspacePersistence.swift` | Existing wave 2 persistence adapter; load uses a typed snapshot, while existing command/import/Git adapters retain their compatibility encoding. |
