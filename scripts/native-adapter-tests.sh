#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
# shellcheck source=scripts/app-sources.sh
source scripts/app-sources.sh
probe_dir="$(mktemp -d "${TMPDIR:-/tmp}/wirebolt-draft-probe.XXXXXX")"
trap 'rm -rf "$probe_dir"' EXIT
python3 scripts/draft-work-probe.py "$probe_dir"
for index in "${!wirebolt_app_inputs[@]}"; do
  case "${wirebolt_app_inputs[$index]}" in
    apple/Generated/wirebolt_ffi.swift) wirebolt_app_inputs[index]="$probe_dir/wirebolt_ffi.swift" ;;
    apple/Sources/WireboltKit/Models.swift) wirebolt_app_inputs[index]="$probe_dir/Models.swift" ;;
  esac
done
mkdir -p build
swiftc -O -whole-module-optimization -parse-as-library -swift-version 6 \
  -target arm64-apple-macosx15.0 -o build/native-adapter-tests \
  apple/Tests/NativeAdapterTests.swift "${wirebolt_app_inputs[@]}"
python3 scripts/swift-tests.py --log build/native-adapter-tests.log build/native-adapter-tests
