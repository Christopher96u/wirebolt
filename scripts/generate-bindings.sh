#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

mkdir -p apple/Generated

scripts/cargo.sh build --release -p wirebolt-ffi
scripts/cargo.sh run --release -p uniffi-bindgen -- \
  generate \
  --library target/release/libwirebolt_ffi.dylib \
  --language swift \
  --out-dir apple/Generated
