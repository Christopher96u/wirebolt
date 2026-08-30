#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

scripts/cargo.sh test --workspace --all-features
scripts/generate-bindings.sh

mkdir -p build

swiftc \
  -O \
  -parse-as-library \
  -swift-version 6 \
  -target arm64-apple-macosx15.0 \
  -o build/bridge-smoke \
  apple/Generated/wirebolt_ffi.swift \
  apple/Tests/BridgeSmoke.swift \
  -Xcc -fmodule-map-file=apple/Generated/wirebolt_ffiFFI.modulemap \
  -Xcc -fmodule-map-file=crates/wirebolt-ffi/include/module.modulemap \
  target/release/libwirebolt_ffi.a

build/bridge-smoke
