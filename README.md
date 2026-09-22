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

Performance budgets distinguish the response engine's cold first viewport (50 ms)
from a complete native window's first content (300 ms, including AppKit/SwiftUI
construction and, for JSON responses, formatting). Native workloads in
`scripts/check.sh` enforce every reported budget, including initial display and
resizing, cold renderer switches, and scroll over 100,000-row documents. They also
check narrow/split-panel controls and preserve the reading position across wrapping
changes. Large response viewers draw a bounded first viewport while completing
the file index in the background. Narrow editor groups place the response below
the request to keep all controls reachable.

Benchmark the Rust HTTP engine over loopback:

```sh
./scripts/http-benchmark.sh
```

## Beta releases

On an Apple Silicon Mac with Python 3 and the authenticated GitHub CLI:

```sh
./scripts/package-beta.sh 0.1.0-beta.2
./scripts/publish-beta.sh 0.1.0-beta.2
```

Use a new version for each build. Packages target Apple Silicon and macOS 15 or later.
The package contains the app and dependency notices. The publisher uploads only the ZIP
and Cask to `Christopher96u/homebrew-tap`; the source repository remains private.
Release notes and the distribution repository description stay empty. An interrupted
publication can be retried with the same package without replacing an existing asset.
These betas use ad-hoc signing, so macOS requires approval on first launch.
