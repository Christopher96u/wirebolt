#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

scripts/generate-bindings.sh

app_dir="${WIREBOLT_APP_DIR:-$repo_dir/build/Wirebolt.app}"
executable_dir="$app_dir/Contents/MacOS"
resources_dir="$app_dir/Contents/Resources"

mkdir -p "$executable_dir" "$resources_dir"
cp apple/WireboltApp/Info.plist "$app_dir/Contents/Info.plist"
cp apple/WireboltApp/Resources/AppIcon.icns "$resources_dir/AppIcon.icns"

# shellcheck source=scripts/app-sources.sh
source scripts/app-sources.sh

swiftc \
  -O \
  -whole-module-optimization \
  -parse-as-library \
  -swift-version 6 \
  -target arm64-apple-macosx15.0 \
  -o "$executable_dir/Wirebolt" \
  apple/WireboltApp/WireboltApp.swift \
  "${wirebolt_app_inputs[@]}"

codesign --force --sign - --timestamp=none "$app_dir"
