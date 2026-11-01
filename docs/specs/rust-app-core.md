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
