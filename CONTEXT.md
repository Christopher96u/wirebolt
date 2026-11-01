# Wirebolt domain context

## Product boundary

Wirebolt is a native macOS HTTP workspace. A workspace contains the editable,
Git-friendly definition of requests and environments. Runtime artifacts such as
response bodies, history, cookies, OAuth tokens, and private keys are local and
never belong to the workspace document tree.

## Workspace vocabulary

- **Workspace**: the persistence and collaboration boundary.
- **Collection**: a top-level ordered container in a workspace.
- **Group**: an ordered, nestable container inside a collection. The UI calls
  this a **Folder** to match the macOS file-navigation metaphor.
- **Request**: a saved HTTP definition located in one collection and optionally
  one group. A request's stable ID and field-row IDs survive reloads.
- **Environment**: an ordered set of enabled or disabled variable rows.
- **Secret reference**: a name stored in the workspace that resolves material
  from Keychain. Secret material itself is not workspace data.

Collections, groups, requests, and their sibling order form one hierarchical
tree. Workspace mutations are expressed as commands and produce monotonically
versioned deltas; a routine mutation must not reload the entire tree.

## Editing and execution

- **Document session**: per-tab mutable UI state for one request draft,
  including dirty revision, selected panels, renderer, response, and active run.
- **Editor group**: one independently selected tab strip. Two groups form the
  optional split editor.
- **Run**: one execution identified by a `RunID` and owned by the document
  session that started it.
- **Prepared run snapshot**: the immutable, redacted description of exactly what
  the transport received after variable, authentication, proxy, and TLS
  resolution. Later draft edits cannot change it.

Preparing a run crosses the coarse workspace bridge once. Streaming response
events use the narrow C ABI and are routed by `RunID`; cancellation is scoped to
that run rather than to the active tab or the whole application.

## Runtime storage

Response bodies are file-backed. UI state retains bounded viewport data and
indexes, not an ever-growing body string. History uses a quota and LRU eviction.
The cookie jar applies domain, path, secure, expiry, and SameSite rules within
its workspace. Runtime storage remains outside Git-visible workspace files.

## Ownership

- Rust owns workspace persistence, import parsing, the editable request/workspace/
  environment records, variable editing and resolution, cURL exports, proxy policy, forms
  and probe state, sidebar indexes, chrome labels, theme/contrast computation,
  OAuth 2 protocol logic, immutable request preparation, HTTP transport, and TLS input.
- Rust also owns response body storage, indexing, search, folding, wrapping policy,
  JSON and Markdown parsing, run history, and cookie policy in `wirebolt-app::response`.
- Rust owns document runs, editor groups, preview replacement, navigation and layout,
  WebSocket lifecycle, reversible workspace edit plans, credential release policy,
  typed history orchestration and direct workspace/transport execution in
  `wirebolt-app::session`. Its runtime commands own send/cancel orchestration,
  per-run cookie scoping, history/activity revisions, environment selection, secret
  editing/release, proxy/transport writes, OAuth acquisition and cURL coordination.
- Swift projects typed Rust records and owns macOS interaction,
  window composition, native response rendering and font measurement, Keychain
  platform adapters, and observation facades. Remaining model orchestration is
  tracked in `docs/specs/rust-app-core.md`.
- `WireboltModel` publishes small presentation state and relays runtime commands;
  Rust requests the latest native editor projection after credential flushing.
  Views do not parse imports, resolve secrets, or assemble transport requests themselves.

## Invariants

1. No secret material is serialized to workspace, Git, history, diagnostics,
   raw-request views, or exports.
2. A run snapshot is immutable and attributable to exactly one `RunID`.
3. Tabs and editor groups never share mutable draft or response state.
4. Large uploads and downloads stay streaming and file-backed.
5. Enabled controls perform a real action; unavailable features are visibly
   disabled.
6. Performance budgets are release gates, not advisory measurements.
