#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

scripts/generate-bindings.sh

app_dir="$repo_dir/build/Wirebolt.app"
executable_dir="$app_dir/Contents/MacOS"
resources_dir="$app_dir/Contents/Resources"

mkdir -p "$executable_dir" "$resources_dir"
cp apple/WireboltApp/Info.plist "$app_dir/Contents/Info.plist"

swiftc \
  -O \
  -whole-module-optimization \
  -parse-as-library \
  -swift-version 6 \
  -target arm64-apple-macosx15.0 \
  -o "$executable_dir/Wirebolt" \
  apple/Generated/wirebolt_ffi.swift \
  apple/Sources/WireboltKit/Models.swift \
  apple/Sources/WireboltKit/WireboltModel.swift \
  apple/WireboltApp/WireboltApp.swift \
  apple/WireboltApp/ContentView.swift \
  apple/WireboltApp/EnvironmentEditor.swift \
  apple/WireboltApp/PerformanceProbe.swift \
  apple/WireboltApp/ResponseViewer.swift \
  apple/WireboltApp/ResponseViewport.swift \
  apple/WireboltApp/RustCore.swift \
  apple/WireboltApp/RustRequestRunner.swift \
  apple/WireboltApp/RustWorkspacePersistence.swift \
  -Xcc -fmodule-map-file=apple/Generated/wirebolt_ffiFFI.modulemap \
  -Xcc -fmodule-map-file=crates/wirebolt-ffi/include/module.modulemap \
  target/release/libwirebolt_ffi.a \
  -framework AppKit \
  -framework Security \
  -framework SystemConfiguration \
  -framework SwiftUI \
  -Xlinker -dead_strip

codesign --force --sign - --timestamp=none "$app_dir"
