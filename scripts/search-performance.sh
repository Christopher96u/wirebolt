#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
scripts/generate-bindings.sh
mkdir -p build/search-performance
swiftc -O -parse-as-library -swift-version 6 -target arm64-apple-macosx15.0 \
  apple/Generated/wirebolt_ffi.swift apple/Benchmarks/SearchWorkloads.swift \
  -Xcc -fmodule-map-file=apple/Generated/wirebolt_ffiFFI.modulemap \
  target/release/libwirebolt_ffi.a -licucore -framework Security -framework SystemConfiguration \
  -o build/search-performance/workloads
if [[ "${1:-}" == build ]]; then exit 0; fi
python3 scripts/search-performance.py build/search-performance/workloads 10 build/search-performance/10m.json --check
python3 scripts/search-performance.py build/search-performance/workloads 100 build/search-performance/100m.json --check
