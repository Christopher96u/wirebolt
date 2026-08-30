#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

mode="${1:-measure}"
if [[ "$mode" != "measure" && "$mode" != "check" && "$mode" != "smoke" ]]; then
  echo "usage: scripts/performance.sh [measure|check|smoke]" >&2
  exit 1
fi

if [[ "$mode" == "smoke" ]]; then
  scripts/generate-bindings.sh
else
  scripts/build-app.sh
fi

mkdir -p build

swiftc \
  -O \
  -parse-as-library \
  -swift-version 6 \
  -target arm64-apple-macosx15.0 \
  -o build/performance-contract \
  apple/Generated/wirebolt_ffi.swift \
  apple/WireboltApp/ResponseViewport.swift \
  apple/Benchmarks/PerformanceContract.swift \
  -Xcc -fmodule-map-file=apple/Generated/wirebolt_ffiFFI.modulemap \
  -Xcc -fmodule-map-file=crates/wirebolt-ffi/include/module.modulemap \
  target/release/libwirebolt_ffi.a \
  -framework AppKit \
  -framework Security \
  -framework SystemConfiguration

if [[ "$mode" == "smoke" ]]; then
  exec build/performance-contract --smoke
fi

arguments=(--app build/Wirebolt.app/Contents/MacOS/Wirebolt)
if [[ "$mode" == "check" ]]; then
  arguments+=(--budgets performance/budgets.json)
fi

build/performance-contract "${arguments[@]}" | tee build/performance-results.json
