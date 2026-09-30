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

For a focused change:

```sh
./scripts/lint.sh
./scripts/test.sh
```

Generated bindings live under `apple/Generated` and are produced by the scripts. Do not hand-edit them.

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
| `crates/wirebolt-ffi` | Swift-facing bridge and streaming interface |
| `apple/Sources/WireboltKit` | Sessions, models, storage and platform coordination |
| `apple/WireboltApp` | Native interface and app adapters |
| `apple/Tests` | Swift tests and bridge smoke validation |
| `apple/Benchmarks` | Native performance workloads |
| `scripts` | Reproducible build, checks and release tooling |

Read [CONTEXT.md](../CONTEXT.md) for workspace vocabulary, ownership and invariants.

## Documentation changes

Use relative links, current UI labels and runnable examples. Keep screenshots in `docs/assets/screenshots` with descriptive alternative text. See [asset provenance](assets/README.md) before refreshing images. Check that docs describe the target revision, and update the changelog when documenting behavior that is not in a published release yet.
