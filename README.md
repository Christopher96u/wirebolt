# Wirebolt

A blazing-fast, native HTTP client for macOS: offline-first, Git-friendly, and free of telemetry.

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
