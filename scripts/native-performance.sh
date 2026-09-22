#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
# shellcheck source=scripts/app-sources.sh
source scripts/app-sources.sh
scripts/generate-bindings.sh
mkdir -p build/native-performance
swiftc -O -whole-module-optimization -g -parse-as-library -swift-version 6 -emit-executable \
  -emit-module-path build/native-performance/workloads.swiftmodule \
  -target arm64-apple-macosx15.0 -o build/native-performance/workloads \
  apple/Benchmarks/NativeWorkloads.swift "${wirebolt_app_inputs[@]}"
if [[ "${1:-}" == build ]]; then exit 0; fi
build/native-performance/workloads interface 1 build/native-performance/interface.json
build/native-performance/workloads request-click 20 build/native-performance/request-click.json
build/native-performance/workloads editor 10000 build/native-performance/editor.json
build/native-performance/workloads editor 100000 build/native-performance/editor-large.json
build/native-performance/workloads workspace 1000 build/native-performance/workspace.json
build/native-performance/workloads workspace 10000 build/native-performance/workspace-large.json
build/native-performance/workloads response 2000 build/native-performance/response.json
build/native-performance/workloads response 10000 build/native-performance/response-large.json
build/native-performance/workloads response 100000 build/native-performance/response-100k.json
WIREBOLT_BENCH_WRAP=off build/native-performance/workloads response 10000 build/native-performance/response-nowrap.json
