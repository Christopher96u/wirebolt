# Rust application core

Status: Implemented

## Goal

Move every piece of application state and logic that does not need an Apple API out of Swift and into Rust, so that the macOS app becomes a rendering layer plus a few platform adapters. A future Linux or Windows interface would then be only an interface.

Scope is **macOS only**. No other platform is built or packaged here.

## Non-goals

- No user-visible behavior, layout or styling changes. Every screen, shortcut, message and file format stays the same.
- No new features, no redesign, no dependency upgrades beyond what the move needs.
- No Linux or Windows code paths, interface or packaging.

## Implemented architecture

```text
crates/
├── wirebolt-core/   # transport, storage, imports/exports, proxy, git (unchanged role)
├── wirebolt-app/    # application core — state + commands + events, no UI, no Apple APIs
│   ├── domain/      #   requests, workspace drafts, environments, variables, cURL, proxy policy,
│   │                #   sidebar snapshot, labels, theme tokens and contrast, OAuth 2 logic
│   ├── response/    #   body store + viewport, line/text indexes, search, folding, wrapping,
│   │                #   JSON document/tree, formatting, Markdown run assembly/cache, history, cookies
│   └── session/     #   documents, editor groups, tabs, preview tabs, back/forward, layout,
│                    #   runs and streaming preview, WebSocket sessions, workspace orchestration
│                    #   (load, mutations, undo, send/cancel, environments, secrets, import/export, git)
└── wirebolt-ffi/    # UniFFI bridge; `app_domain.rs`, `domain_syntax.rs`, `app_response.rs`, `app_session/` expose wirebolt-app
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

## Testing and acceptance

- Port every Swift test that exercises moved logic to Rust with the same cases (Swift Testing → `#[test]` / `#[tokio::test]`). Keep or adapt Swift tests that cover facades and adapters. Coverage must not drop.
- Keep the bridge smoke (`apple/Tests/BridgeSmoke.swift`, `StreamBridgeSmoke.swift`) passing; extend it for new surfaces where useful.
- Build lists: `scripts/app-sources.sh`, `scripts/test.sh` (bridge smoke), `scripts/editor-performance.sh` and the native workload build compile explicit Swift file lists. Add, remove or rename entries with every file change.
- `./scripts/check.sh` (lint, Rust and Swift tests, bridge smoke, performance smoke, editor and native workloads, app build, launch smoke) passes locally.
- `docs/development.md` (code map) and `CONTEXT.md` (if vocabulary changes) describe the new layout.
- This document ends with a table **What remains in Swift and why**, one row per remaining `WireboltKit` file and per app-target file that still holds non-view logic.

## What remains in Swift and why

Each row identifies the native responsibility retained by that source. Rust owns
application state, workspace and transport operations, serialization, presentation
migrations and diagnostics. Typed callbacks carry isolated probe/socket events;
application HTTP response bytes remain in Rust. The C header only exposes runtime
warmup and the core version handshake (ABI revision 5).

| File | What remains and why |
| --- | --- |
| `WireboltApp/EditorFind.swift` | Native search-field focus, keyboard handling and editor selection presentation over Rust search results. |
| `WireboltApp/EnvironmentEditor.swift` | SwiftUI form buffers, focus and save/dismiss presentation; credential storage policy is Rust. |
| `WireboltApp/FieldTextInput.swift` | Native text editing, selection, focus, accessibility and UTF-16 completion presentation. |
| `WireboltApp/IndexedResponseEditor.swift` | Native viewport drawing, selection and clipboard integration; worker cancellation forwards to Rust through the response task adapter. |
| `WireboltApp/IndexedTextLine.swift` | Native attributed-text layout, selection and drawing. |
| `WireboltApp/NativeCodeEditor.swift` | AppKit/TextKit editing, layout, applying syntax attributes and ruler drawing require native APIs; Rust returns token ranges, kinds, whole-line chunks and indent columns in one batch per highlight pass, alongside batched response coordinates/breaks. |
| `WireboltApp/NetworkSettingsView.swift` | Native settings form state, file panels and localized error presentation; Rust reads bounded PEM files, checks CA markers, parses identities and generates credential names. |
| `WireboltApp/NotesEditor.swift` | Native editor/Markdown view composition and focus. |
| `WireboltApp/PerformanceProbe.swift` | OS signposts, process memory, AppKit event and native frame instrumentation. |
| `WireboltApp/ResponseHexView.swift` | Native byte-grid layout, drawing and selection presentation. |
| `WireboltApp/ResponseViewer.swift` | Native renderer composition and AppKit/WebKit adapters; JSON file parsing is requested from Rust by path. |
| `WireboltApp/ResponseViewport.swift` | Native geometry, scrolling and bounded viewport presentation. |
| `WireboltApp/RustRequestRunner.swift` | SystemConfiguration proxy notifications and a typed async-stream/cancellation relay for isolated HTTP probes. Native runtime transport performs preparation and HTTP execution in Rust; no JSON event envelopes cross Swift. |
| `WireboltApp/RustWebSocketRunner.swift` | Typed connection wiring, async-consumer handoff and cancellation. Rust prepares and executes connections and supplies typed handshake headers and message events. |
| `WireboltApp/RustWorkspacePersistence.swift` | Native URL/default-folder selection and detached-task wiring to typed Rust workspace commands, snapshots and Git operations. No JSON import or Git decoding remains. |
| `WireboltApp/WireboltApp.swift` | SwiftUI scene/command declarations, AppKit application lifecycle, native window and menu integration. |
| `WireboltApp/WireboltTheme.swift` | SwiftUI/AppKit Color, Font and environment projections of Rust theme tokens. |
| `WireboltApp/WorkspaceActions.swift` | AppKit file panels, alerts, application/window registry, NSWorkspace actions and UserDefaults storage. Rust selects import routes and messages, recent-workspace membership/order, external-file queue contents and unsaved-document prompt text. |
| `WireboltApp/WorkspaceUIState.swift` | Native focus, editor-storage and presentation identity, geometry, window closing and UserDefaults access. Rust owns sidebar coordination, saved-preference migrations, default request panels and the sole pending dirty-close request; Swift observes a revision and projects that request. |
| `WireboltKit/CodeFolding.swift` | Projection facade with a generated syntax enum, incremental edit ingress, and batched editor coordinate conversion; native text snapshots are never echoed across the bridge on keystrokes. |
| `WireboltKit/CodeTextWrapping.swift` | CoreText font measurement, native shaping adapter, direct lookup of Rust-produced break snapshots and native attributed-string invisible-space rendering. |
| `WireboltKit/CollectionImportSources.swift` | URL/error projections and a native package-folder foreign trait; recognition, filesystem traversal, limits and bundle construction are Rust. Collection recognition and Bruno bundles cross the bridge as generated records, with generated error aliases. |
| `WireboltKit/CurlExport.swift` | Typed export facade and Keychain-service identity; expansion, quoting and argument construction are Rust. |
| `WireboltKit/DocumentSessions.swift` | Main-actor observation, native editor projections, object identity and UserDefaults storage of Rust-encoded layouts, with corrupt native preferences treated as absent. Run heads/completions/failures use generated records directly; prepared runs retain only native transport/proxy naming projections. Rust owns run transitions, body ownership, publication deadlines, groups, previews, navigation, close scopes and layout serialization/policy. Drafts synchronize at commands; response ingress is batched. |
| `WireboltKit/JSONNode.swift` | Projects Rust-produced tree records and their typed JsonNodeKind directly into the native tree view. |
| `WireboltKit/JSONResponseDocument.swift` | Retains the Rust-owned temporary document and mirrors its URL and bounded preview. |
| `WireboltKit/MarkdownPreview.swift` | Foundation full-Markdown semantic parsing implements a platform trait to preserve native syntax/intents; Rust assembles separators/list prefixes and owns cache policy. Swift applies AppKit fonts, colors and paragraph styles to Rust runs and mirrors retention for native object identity. |
| `WireboltKit/Models.swift` | Generated value/body/authentication-kind/OAuth/field/multipart/proxy and run records are used directly, with native naming/default conveniences. Foundation supplies numeric/case-insensitive JSON key collation through a platform trait, once per format command. Request/workspace/environment and prepared-run facades project Rust records; cached child arrays and URL-edit results keep view reads cheap. Serialization and workspace policy live in Rust. Git change/status/operation snapshots and delta/outcome values use generated records/enums; the existing typed workspace ingress API remains stable. |
| `WireboltKit/OAuth2Service.swift` | ASWebAuthenticationSession presentation, Keychain access and CFNetwork system-proxy/PAC resolution implement the Rust foreign trait; native error localization preserves network diagnostics; protocol requests, PKCE and token parsing/exchange are Rust. |
| `WireboltKit/ProxyConnectionTest.swift` | Main-actor observation and task lifetime for the isolated probe. Rust owns validation, probe state, draft construction, stale-event checks and results; the native runner uses typed Rust runtime transport. |
| `WireboltKit/ProxySettings.swift` | Observable UserDefaults preferences, native UUID/error projections and typed proxy-form facades; effective proxy/source, scopes, typed destinations, form modes and proxy documents use generated records/enums directly; PEM errors keep native spellings through generated aliases. Invalid bridge UUIDs are repaired once per draft and retained for stable native identity. |
| `WireboltKit/ResponseFormatting.swift` | Localized Foundation byte-count formatting and native presentation conveniences on the generated outcome badge. Duration, status, failure and outcome policy live in Rust. |
| `WireboltKit/ResponseIndexCache.swift` | Detached-task scheduling and Swift task cancellation forwarded to Rust; the retained cache and shared scans live in Rust. |
| `WireboltKit/ResponseStorage.swift` | Actor facades, Foundation URL/date cookie/history projections and shared native object identity. History crosses as typed session records; Rust owns serialization, storage formats and bounded body ingestion. |
| `WireboltKit/RuntimeAdapters.swift` | Native task/main-actor callbacks for injected runners, app-default preferences, persistence commands, OAuth presentation and fine-grained invalidations; Rust routes settings by scope. Production sends call Rust transport; the editor callback captures the latest native draft after credential flushing. |
| `WireboltKit/ResponseTextIndex.swift` | Immutable viewport metadata and batched row/search coordinate conversion for native views. |
| `WireboltKit/SemanticPalette.swift` | Cached native color projections and a generated status-class alias for renderers; color selection, luminance and contrast are Rust. |
| `WireboltKit/SidebarSnapshot.swift` | Shared native row projections and expansion presentation state, with one batched Rust index/filter call; rendering reads cached metadata. Navigation outcomes use the generated enum. Drop eligibility, move-menu collection order and environment-picker order use Rust policy over bounded metadata. |
| `WireboltKit/TextLineIndex.swift` | Value facade applying Rust-produced edit deltas to a native line-start mirror; keystrokes never transfer the complete index. Invalid edit ranges leave the index unchanged. |
| `WireboltKit/TextSearch.swift` | Query facade, UTF-16 `NSRange` adaptation and Swift cancellation/error adaptation. |
| `WireboltKit/ThemeDefinition.swift` | Cached token projections and the observable ThemeStore; presets use a generated enum with Rust-supplied persisted preference strings. Density, geometry, typography, typed method-chip tokens and contrast resolution are Rust. |
| `WireboltKit/VariableTemplates.swift` | Native UTF-16/NSRange projections, cached completion entries and a batched Foundation locale-comparison foreign trait; all parsing, merging, masking and matching are Rust. |
| `WireboltKit/WebSocket.swift` | Main-actor observation, native callback/async-sequence handoff, cached native UUID/date projections, and Foundation localization of Cocoa diagnostics. Rust owns lifecycle generations, message retention, body preparation and binary decoding. |
| `WireboltKit/WireboltModel.swift` | Observation, native UndoManager token registration and platform-effect relay. Rust owns workspace action construction, reversible edits and undo payloads, load/switch/rename transitions, save/discard/close policy, import/export and Git coordination (`session/workspace`), and send/cancel, history/activity, per-run cookies, variable selection and resolution, secret editing and release, proxy/transport writes, OAuth acquisition and cURL coordination (`session/runtime`). Native caches update only changed properties. |
| `WireboltKit/WorkspaceOperations.swift` | Typed callbacks for injected stores and native editor capture/save acknowledgements. Production operations call Rust directly. Git records retain generated delta/outcome enums without parsing strings. |
| `WireboltKit/WorkspaceChrome.swift` | Typed label facades and Apple locale-aware numeric formatting of Rust-selected timeout units. |
| `WireboltApp/ContentView.swift` | SwiftUI request/response composition, bindings, file panels, keyboard/focus actions and native drag hit-testing. Credential edit names, folder-drop eligibility, move-menu collections and environment ordering are Rust (`domain/request`, `domain/sidebar`). Body kinds, transitions and JSON/XML/HTML formatting are Rust (`domain/body`); the view passes edited text and applies the result. |
| `WireboltApp/GitCollaborationView.swift` | SwiftUI sheet/form state, native symbol/color styling and Finder reveal actions. Rust supplies porcelain diagnostics, operation labels and push help. |
| `WireboltApp/AppearanceSettingsView.swift` | SwiftUI appearance form composition, local presentation state and native theme previews over Rust theme tokens. |
| `WireboltApp/CookiesView.swift` | SwiftUI cookie table, selection, native dates and deletion controls over the Rust-owned cookie jar. |
| `WireboltApp/ImportSummaryView.swift` | SwiftUI summary presentation and native dismiss/focus behavior over the typed import summary. |
| `WireboltApp/SettingsModal.swift` | SwiftUI modal navigation, sizing and native presentation; native layout is committed before restoring the captured AppKit responder and selection. |
| `WireboltApp/SettingsPageComponents.swift` | Reusable SwiftUI form components, native layout and focus/accessibility presentation. |
| `WireboltApp/SettingsPages.swift` | SwiftUI settings-page composition and native platform settings links. |
| `WireboltApp/WireboltSettingsView.swift` | SwiftUI settings-window composition and native selection/presentation state. |

| Integration area | Final ownership |
| --- | --- |
| `wirebolt-app::response` Markdown | A platform parser trait supplies semantic runs. macOS uses Foundation's full Markdown interpretation; Rust assembles block separators/list prefixes and owns retention/accounting. Native differential tests compare exact attributed text and unfinished-code-block newlines with Foundation rendering. `wirebolt-app` has no Foundation dependency. |
| `wirebolt-app::response` regex and cookie dates | Search goes through a `RegexEngine` trait with a portable Rust default; on macOS the bridge (`wirebolt-ffi`, Apple-only) injects the ICU engine so the existing dialect is unchanged. Cookie dates are parsed in Rust using the acceptance rules of Foundation's `en_US_POSIX` formatter patterns `EEE, dd MMM yyyy HH:mm:ss zzz` and `EEEE, dd-MMM-yy HH:mm:ss zzz`, in that order, including their punctuation, whitespace, year and timezone leniency. Results are Unix seconds; Swift converts to `Date` at the boundary. `wirebolt-app` itself has no Apple dependency. |

App views and platform adapters retain their established formatting and native
responsibilities. Rust owns portable application policy; Swift retains native
presentation, observation and platform integration.
