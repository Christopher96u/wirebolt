# Development

[Documentation](README.md) · [Contributing](../CONTRIBUTING.md)

## Requirements

Use an Apple Silicon Mac with the Xcode toolchain selected by `xcode-select`. The repository pins Rust **1.98.0**. The existing development baseline is Xcode **26.6**; Xcode is not pinned by mise. Install `mise` if you use the repository's tool manager setup, plus `shellcheck` and `shfmt` for shell linting. Python 3 is used by documentation examples and release packaging.

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

The full gate runs linting, Rust/Swift tests, bridge smoke checks, performance smoke checks, editor workloads, the app build, native performance workloads and launch smoke validation.

In GitHub Actions (or with `WIREBOLT_SKIP_BENCHMARKS=1`) the gate skips the editor and native performance workloads, because GitHub's virtualized macOS runners are several times slower than the Apple Silicon these budgets target. Timing benchmarks therefore run only locally; run the full `./scripts/check.sh` before requesting review of an app change and before a release.

For a focused change:

```sh
./scripts/lint.sh
./scripts/test.sh
```

Generated bindings live under `apple/Generated` and are produced by the scripts. Do not hand-edit them. The Swift package compiles the generated bindings into `WireboltKit` and links the Rust bridge: run `./scripts/generate-bindings.sh` before invoking `swift test --package-path apple` directly. `./scripts/test.sh` does this automatically. Response search accepts a Rust regex-engine trait: `wirebolt-app` provides a portable default, and the Apple-only `wirebolt-ffi` adapter injects system ICU (`-licucore`) to preserve the native dialect; cookie dates parse in pure Rust to Unix seconds, with native `Date` conversion in Swift.

The Rust session suite covers storage conversions, typed secret/Git operations,
presentation migrations and cancellation. Bridge smoke checks exercise typed
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

## Performance

Run on an idle Apple Silicon Mac:

```sh
./scripts/performance.sh check
./scripts/native-performance.sh
./scripts/http-benchmark.sh
```

The contract distinguishes the response engine's cold first viewport (50 ms budget) from a complete native window's first content (300 ms budget, including native view construction and JSON formatting when applicable). Budgets are test targets, not universal timings promised for every Mac or workload.

Native workloads cover request and tab selection, large responses, resizing, renderer changes, scrolling, notes and editor behavior. Large response viewers index files in the background and draw bounded viewports.

## Code map

| Location | Responsibility |
| --- | --- |
| `crates/wirebolt-core` | Workspace persistence, import/export, request preparation and transport |
| `crates/wirebolt-app/src/domain` | Typed editable records and request/credential editing (`request`), stable IDs (`identity`), URL/query editing (`url`), window/tab/timeout labels (`chrome`), import recognition and summaries (`imports`), Git presentation (`git`), variables, cURL exports, proxy forms and probes, sidebar/drop/menu ordering, typed theme tokens, editor syntax/indentation, bounded PEM input and OAuth 2 protocol logic |
| `crates/wirebolt-app/src/response` | File-backed response bodies, sparse indexes, search, folding, wrapping policy, JSON parsing, Markdown run assembly/cache policy over platform parsing, history and cookies |
| `crates/wirebolt-app/src/session` | Document/run state, editor groups and previews, layout, WebSocket lifecycle, reversible workspace plans, credential lifecycle, typed history, workspace persistence/import/export/Git and transport execution; `session/workspace/` owns action construction, undo payloads, save/load transitions, sidebar/close workflows and typed persistence seams; `session/runtime` owns send/cancel, per-run cookie scoping, history/activity, environment resolution, secret commands, settings writes, OAuth acquisition and cURL coordination |
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

## Documentation changes

Use relative links, current UI labels and runnable examples. Keep screenshots in `docs/assets/screenshots` with descriptive alternative text. See [asset provenance](assets/README.md) before refreshing images. Check that docs describe the target revision, and update the changelog when documenting behavior that is not in a published release yet.
