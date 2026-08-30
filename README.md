# Wirebolt

A blazing-fast, native HTTP client for macOS: offline-first, Git-friendly, and free of telemetry.

Wirebolt currently includes a native request composer, incremental response viewer, versioned
file-based workspaces, environment variables, Keychain-backed secrets, and layered proxy policy.
Workspaces live locally and remain readable, deterministic TOML suitable for optional Git sync.

Keyboard shortcuts:

- `⌘↩` sends the current request.
- `⌘S` saves it to the workspace.
- `⌘N` creates a new request draft.

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
