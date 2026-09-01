# Wirebolt

A blazing-fast, native HTTP client for macOS: offline-first, Git-friendly, and free of telemetry.

Wirebolt currently includes a native request composer, incremental response viewer, versioned
file-based workspaces, environment variables, Keychain-backed secrets, layered proxy policy, and
explicit Git collaboration. Workspaces live locally and remain readable, deterministic TOML.
Git status, pull, commit, and push run only when requested; conflicts are surfaced without automatic
resolution, and commits created by Wirebolt include only its managed workspace documents.

Keyboard shortcuts:

- `⌘↩` sends the current request.
- `⌘S` saves it to the workspace.
- `⇧⌘N` creates a new request draft (`⌘N` keeps the native New Window behavior).
- `⇧⌘G` opens Git collaboration.

## Development

Requirements: macOS, `mise`, Rust 1.98.0, and Xcode 26.6.

```sh
./scripts/check.sh
./scripts/build-app.sh
open build/Wirebolt.app
```

Measure the performance contract on an idle Apple Silicon Mac:

```sh
./scripts/performance.sh check
```

Benchmark the Rust HTTP engine over loopback:

```sh
./scripts/http-benchmark.sh
```
