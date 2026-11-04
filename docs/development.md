# Development

[Documentation](README.md) · [Contributing](../CONTRIBUTING.md)

## Requirements

Use an Apple Silicon Mac with the Xcode toolchain selected by `xcode-select`. The repository pins Rust **1.98.0**. The existing development baseline is Xcode **26.6**; Xcode is not pinned by mise. Install `mise` if you use the repository's tool manager setup, plus `shellcheck` and `shfmt` for shell linting. Python 3 is required by the Swift test watchdog, documentation examples and release packaging.

Inspect `.mise.toml`, `rust-toolchain.toml` and `.github/workflows/ci.yml` when reproducing the CI environment; these are the source of truth for versions.

```sh
./scripts/build-app.sh
open build/Wirebolt.app
```

To open a specific workspace directly:

```sh
build/Wirebolt.app/Contents/MacOS/Wirebolt --workspace /absolute/path/to/workspace
```

An explicit `--workspace` overrides the remembered workspace path. Avoid launching multiple copies while testing interactions.

## Checks

```sh
./scripts/check.sh
```

The full gate runs linting, Rust/Swift tests, bridge smoke checks, performance smoke checks, search and editor workloads, the app build, native performance workloads and launch smoke validation. Native adapter guards run in CI as well as locally.

In GitHub Actions (or with `WIREBOLT_SKIP_BENCHMARKS=1`) the gate skips the search, editor and native performance workloads, because GitHub's virtualized macOS runners are several times slower than the Apple Silicon these budgets target. Timing benchmarks therefore run only locally; run the full `./scripts/check.sh` before requesting review of an app change and before a release.

For a focused change:

```sh
./scripts/lint.sh
./scripts/test.sh
```

`scripts/test.sh` builds the Swift tests, then runs them with
`LIBDISPATCH_COOPERATIVE_POOL_STRICT=1` locally and in CI. Blocking Swift's
cooperative pool therefore stalls the local suite too. `scripts/swift-tests.py`
stops the test runner and its full descendant process tree after 180 seconds,
including SwiftPM helpers in separate process groups. It prints tests that started
without finishing and keeps the output in `build/swift-tests.log`. The limit covers test
execution, not compilation; override it with
`WIREBOLT_SWIFT_TEST_TIMEOUT_SECONDS` when investigating a slow run. To repeat an
already built suite with the same guard, run `python3 scripts/swift-tests.py`.
The bridge smoke executable also uses the strict pool and watchdog, with its
output in `build/bridge-smoke.log`.

Generated bindings live under `apple/Generated` and are produced by the scripts. Do not hand-edit them. The Swift package compiles the generated bindings into `WireboltKit` and links the Rust bridge: run `./scripts/generate-bindings.sh` before invoking `swift test --package-path apple` directly. `./scripts/test.sh` does this automatically. Response search accepts a Rust regex-engine trait: `wirebolt-app` provides a portable default, and the Apple-only `wirebolt-ffi` adapter injects system ICU (`-licucore`) to preserve the native dialect; cookie dates parse in pure Rust to Unix seconds, with native `Date` conversion in Swift.

Only the current workspace schema is supported; other versions are rejected with
an unsupported-version diagnostic.

The Rust session suite covers storage conversions, typed secret/Git operations,
presentation policy and cancellation. Bridge smoke checks exercise typed
transport events and workspace commands. Swift adapter tests compare the Notes
preview with Foundation's full Markdown renderer, including complete attributed
text and cache costs, across headings, inline styles, code, nested/ordered/task
lists, quotes, links/autolinks, images, tables, HTML, breaks, rules, footnotes,
escapes and Unicode. Foundation's full Markdown interpretation is supplied through
a platform trait; Rust assembles visible runs and owns cache retention. Editor
syntax tests compare batched Rust token ranges, precedence, line boundaries and
indent columns with native syntax patterns, including CR/CRLF, VT/FF, NEL and
Unicode line/paragraph separators. The Markdown corpus compares every attributed
run and visible text, including unfinished fenced-code-block newlines and inserted
block separators.

The native adapter suite also covers sidebar search focus, Settings cleanup, custom method validation, bulk field reconciliation, multipart text/file value access, Find dismissal, WebSocket error placement and disconnect, atomic cookie deletion failure, and non-blocking orphaned inspector drafts. The isolated launch smoke also checks the native File menu. Set `WIREBOLT_NATIVE_CAPTURE_DIRECTORY` to capture the General, Appearance, Globals and orphaned-edit notice fixtures. Cookie jars remain scoped in memory across workspace switches; explicit deletions roll back on a failed atomic write. Staged TLS identity choices stay in a Rust secret editor until workspace Save.

## Workspace persistence

Read and write limits both apply to the complete serialized document (16 MiB),
including TOML metadata and escaping. A rejected save keeps the previous file.
Moves create a destination atomically without replacing any existing file, and roll
it back if the source cannot be removed. Collection deletion stages readable
managed documents and rolls them back on a staging failure; unmanaged files,
malformed documents and nonempty directories remain in place.

Production loads return `WorkspaceLoadResult` with typed per-file problems.
The bridge carries at most ten diagnostics (200 characters per relative path),
the number omitted and a Rust-composed notice. Reasons never include document
contents. The app presents a dismissible notice while keeping readable requests
available. Save and workspace replacement use the same guard as mutations and
Undo; a queued stale save of a request moved or deleted inside the app fails.
Saving a request file removed externally recreates it at its last known location.

## History storage

The local history `index.json` contains an `entries` array. Every entry requires
an identity with collection ID and request ID. Unsupported or malformed indexes
are reported once; recording and clearing then use a fresh index. Unreadable
indexes remain errors. Index writes use atomic replacement.

History follows collection and request IDs, independently of the workspace folder's
name or location. Re-cloning a workspace retains history; copies with the same IDs
share history. New imports receive unique request IDs. Requests without
a collection use an empty collection ID, stable across launches. Their first save,
moves and Undo relocate scoped entries within the serialized Rust workspace
operation and redirect in-flight completions to the saved collection. Replacement
loads reconcile redirects; late completions for requests absent from the open
workspace do not change its history presentation.

Restored responses own temporary body copies independently of history retention.
Copies are made on workers into paths that do not exist, allowing APFS cloning.
Releasing the last body handle removes the temporary file and directory. Process
leases protect another running instance's copies; the next history load cleans up
unleased directories left after a crash. Index updates finish before evicted bodies
are removed, so failed writes preserve existing responses.

## Performance

Run on an idle Apple Silicon Mac:

```sh
./scripts/performance.sh check
./scripts/native-performance.sh
./scripts/search-performance.sh
./scripts/http-benchmark.sh
```

The contract distinguishes the response engine's cold first viewport (50 ms budget) from a complete native window's first content (300 ms budget, including native view construction and JSON formatting when applicable). Budgets are test targets, not universal timings promised for every Mac or workload.

The `command-palette` native workload covers a 10,000-request fixture with warm
open (50 ms p95), query through visible results (25 ms p95), and native typing
(16 ms p95) gates calibrated on the M5 Pro. It asserts zero full-draft reads and
writes, including a 1 MiB open draft. See [the command palette spec](specs/command-palette.md).

Native workloads cover request and tab selection, large responses, resizing, renderer changes, scrolling, notes and editor behavior. Large response viewers index files in the background and draw bounded viewports.

The indexed search workload calls `AppTextIndex.search` through the production
UniFFI/ICU path on 10 and 100 MiB UTF-8 files. Five fresh processes per size each
run an absent query, a dense query capped at 20,000 matches, and a last-byte query.
Fixture creation and indexing are outside query timings. Results include every
sample, median, minimum and maximum; process peak RSS is captured after the
first two queries, before the last-byte query. Fresh processes keep allocator
retention from repeated queries out of this two-query measurement. Exact match
coordinates are asserted. With five samples, the maximum is conservatively checked
against each p95 timing budget. Results are in `build/search-performance/`.

Local search budgets in `performance/budgets.json` are: 100 MiB capped median
**150 ms**, capped p95 **200 ms**, absent p95 **350 ms**, and two-query peak RSS
**550 MiB**. These search thresholds and the 8 ms native tab-close threshold below were
calibrated on an Apple M5 Pro (15 cores, 24 GB), as recorded separately in
`performance/budgets.json`; the other budgets retain their M2 Pro reference. The synthetic byte scan
in `FeatureWorkloads` remains a separate component baseline.

The native `tab-close` workload closes one clean tab among twenty 1 MiB drafts
through `WorkspaceUIState.closeTab`, including command and native selection work,
with a p95 budget of **8 ms**. It reports measured full-document reads, draft
writes and saved-draft returns, and asserts zero on all three for clean close;
it does not estimate bridge bytes. It also verifies final-tab dirty confirmation
and reopening. The same flow runs in the CI native adapter guard. Run it alone with
`./scripts/native-performance.sh build`, then
`build/native-performance/workloads tab-close 20 build/native-performance/tab-close.json 5`.
Rust policy and native adapter tests cover close scopes at 2, 20 and 100 tabs,
blocked closes, split/preview tabs, cancellation, save/discard and undo.
Close resolves the Rust-owned target scope once and synchronizes only assigned
target drafts. Every native assignment sets a cheap synchronization flag, including
no-op and Unicode-normalization-only writes. Native dirty/touched presentation
keeps Swift equality semantics; Rust checks byte-exact dirty state at the close
command boundary and fetches saved drafts only for dirty removed targets.

`scripts/native-adapter-tests.sh` uses temporary instrumented compiler inputs to
assert that 60 edits/no-op writes to a clean 1 MiB body perform zero draft
encodings and synchronizations, with no extra full-draft comparisons over
`5511d4a`. Shipping types gain no counters for this guard. The native `body-typing`
workload measures the actual body-editor binding through insert/delete and
layout/display, with the existing 16 ms typing budget.

The full gate includes both workloads locally and skips their timings in CI.
Launch smoke still runs in CI: it copies and signs this checkout's app under a
unique bundle/defaults identifier, gives each launch temporary `HOME`,
`CFFIXED_USER_HOME`, temporary files and workspace roots, and probes Foundation's
resolved home/Application Support and bundle identity before starting the timer.
The timed child repeats these assertions before constructing the application.
On success or failure the harness unregisters the copied app with `lsregister -u`
before deleting it, because LaunchServices registration can live outside HOME.
It also compares read-only `defaults export io.github.christopher96u.wirebolt -`
snapshots before and after the run, including an absent domain. It never writes
to that real domain.

The native `editor_scroll` workload retains its 16 ms p95 budget. It measures main
thread CPU time (`CLOCK_THREAD_CPUTIME_ID`) for a clip-view scroll, scroller
reflection and one flush of pending layout/display. This counts viewport layout,
highlighting and drawing work without charging AppKit's display-frame waits to
the application; a wall-time measurement can sit at the display interval even
when the app has little work to do. The same samples also enforce an
`editor_scroll_wall` p95 gate of **33.3 ms** (1000/30 ms): two frames at 60 Hz.
This fixed budget tolerates display waits across variable refresh rates while
catching main-thread blocking in locks, synchronous FFI, file I/O or highlighting
waits that consume little CPU. Both metrics include the same scroll and viewport
flush; run-loop pacing stays outside the timed interval.
Scrolling already redraws the exposed viewport; forcing `NSTextView.display()`
after that flush requests a second draw and includes another display-frame wait
in the sample. The workload omits that redundant draw. These gates measure
application work and blocking through the native display flush, not physical frame presentation;
the other native editor workloads continue to measure wall time.

## Code map

| Location | Responsibility |
| --- | --- |
| `crates/wirebolt-core` | Workspace persistence, imports, request preparation and transport |
| `crates/wirebolt-app/src/domain` | Typed editable records and request/credential editing (`request`), stable IDs (`identity`), URL/query editing (`url`), window/tab/timeout labels (`chrome`), import recognition and summaries (`imports`), Git presentation (`git`), variables, cURL exports, proxy forms and probes, sidebar/drop/menu ordering, metadata-only command palette indexing/ranking and shared command eligibility, typed theme tokens, custom-theme derivation/JSONC import/library persistence, editor syntax/indentation, bounded PEM input and OAuth 2 protocol logic |
| `crates/wirebolt-app/src/response` | File-backed response bodies, sparse indexes, search, folding, wrapping policy, JSON parsing, Markdown run assembly/cache policy over platform parsing, history and cookies |
| `crates/wirebolt-app/src/session` | Document/run state, editor groups and previews, layout, WebSocket lifecycle, reversible workspace plans, credential lifecycle, typed history, workspace persistence/import/Git and transport execution; `session/workspace/` owns live environment field commits and validation, action construction, undo payloads, save/load transitions, sidebar/close workflows and typed persistence seams; `session/runtime` owns send/cancel, per-run cookie scoping, history/activity, environment resolution, secret commands, settings writes, OAuth acquisition and cURL coordination |
| `crates/wirebolt-ffi` | Typed domain, response and session objects; foreign platform traits and typed HTTP probe/WebSocket callbacks; `app_session/` mirrors run, tabs, secrets, workspace and runtime responsibilities |
| `apple/Sources/WireboltKit` | Native observation facades, cached editor projections and Apple platform adapters; Rust owns serialization and workspace/runtime orchestration |
| `apple/WireboltApp` | Native interface, file panels, lifecycle and typed Rust connection/persistence wiring |
| `apple/Tests` | Swift tests and bridge smoke validation |
| `apple/Benchmarks` | Native performance workloads |
| `scripts` | Reproducible build, checks and release tooling |

Bridge names follow their application responsibility: `domain_*` free functions
expose editable values and presentation policy, `app_*` functions expose response
operations and bounded platform-file input, `session_*` functions expose session
policy/codecs, and `workspace_*` functions expose workspace interface actions.
The editor syntax surface is `domain_syntax_highlight` (`domain_syntax.rs`).
API key placement, OAuth grants, multipart kinds, proxy destinations and proxy
sources reuse the core enums. Workspace open/create selection uses a typed mode;
Bruno folders cross as typed bundles through the import path. The C control
surface uses ABI revision 5.
Most bridge objects use an `App` prefix; session layout records use `SessionLayout`,
`SessionLayoutGroup` and `SessionLayoutTab` in Rust. The generated Swift layout
record is named `RustSessionLayout` to distinguish it from the native persistence
facade. Git delta values use the core `GitDelta` type throughout Rust and the bridge.
Swift aliases the generated domain enums and adds only display titles, case lists
and small `RawRepresentable` extensions for persisted preference strings, with theme
preset spellings supplied by Rust. Swift's own enums are view state: panel sections,
renderers, filters, drop positions and picker tags such as `ProxySelection`.

Read [CONTEXT.md](../CONTEXT.md) for workspace vocabulary, ownership and invariants.

## Custom themes

Settings → Appearance can derive a theme **From 2 colors** or import a VS Code
`.json` color theme through **Import…** or a file drop. Both paths show a preview
before saving. The preview lists contrast adjustments; mid-tone backgrounds may
move toward light or dark so all text remains readable. Increase Contrast also
applies to custom syntax, variable, status and method colors. Geometry uses the
same Rust-owned tokens as built-in themes.

VS Code import accepts UTF-8 JSONC (comments, trailing commas and an optional BOM),
up to **1 MiB**. It maps editor, sidebar, list, input, button, diagnostic and token
scope colors, composites alpha colors over the canvas, and derives missing
tokens. Parent `include` files are reported in the preview and are not resolved.

Rust persists up to **64** custom palettes in version 2 of
`~/Library/Application Support/Wirebolt/custom-themes.json`, outside the workspace.
The existing `interfaceAppearance` preference holds the selected built-in name or
custom ID. Saving uses an atomic replacement. A corrupt file reports a notice and uses built-in themes until a new theme is saved.
Unsupported versions stay unchanged and read-only.
Deleting the active custom theme returns to **Follow System**.

`custom_themes.rs` tests contrast, method separation, JSONC fixtures and storage
recovery. `CustomThemeTests` exercises the typed native facade under the strict
cooperative pool. `CustomThemeNativeTests` hosts the production Appearance page
with temporary storage and preferences, sends events only to its own window, and
checks picker selection, preview saving, deletion and visible import errors. Run
it with `scripts/native-adapter-tests.sh`. To capture fixture-only in-process PNGs:

```sh
mkdir -p build/review-screenshots
WIREBOLT_NATIVE_CAPTURE_DIRECTORY="$PWD/build/review-screenshots" scripts/native-adapter-tests.sh
```

This optional variable controls captures from both native suites. No real
workspace, preferences or Keychain is used. The benchmark-only
`environment-screenshots` mode writes `environment-inspector.png` to the same
capture directory when the variable is set, otherwise beside its output JSON;
it also uses disposable fixtures.

## Documentation changes

Use relative links, current UI labels and runnable examples. Keep screenshots in `docs/assets/screenshots` with descriptive alternative text. See [asset provenance](assets/README.md) before refreshing images. Check that docs describe the target revision, and update the changelog when documenting behavior that is not in a published release yet.
