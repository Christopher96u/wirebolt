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

swiftc \
  -O \
  -whole-module-optimization \
  -parse-as-library \
  -swift-version 6 \
  -target arm64-apple-macosx15.0 \
  -o "$executable_dir/Wirebolt" \
  apple/Generated/wirebolt_ffi.swift \
  apple/Sources/WireboltKit/Models.swift \
  apple/Sources/WireboltKit/CurlExport.swift \
  apple/Sources/WireboltKit/WebSocket.swift \
  apple/Sources/WireboltKit/JSONNode.swift \
  apple/Sources/WireboltKit/CodeFolding.swift \
  apple/Sources/WireboltKit/TextSearch.swift \
  apple/Sources/WireboltKit/CodeTextWrapping.swift \
  apple/Sources/WireboltKit/ResponseTextIndex.swift \
  apple/Sources/WireboltKit/DocumentSessions.swift \
  apple/Sources/WireboltKit/ResponseStorage.swift \
  apple/Sources/WireboltKit/OAuth2Service.swift \
  apple/Sources/WireboltKit/WireboltModel.swift \
  apple/WireboltApp/WireboltApp.swift \
  apple/WireboltApp/WorkspaceUIState.swift \
  apple/WireboltApp/ContentView.swift \
  apple/WireboltApp/EnvironmentEditor.swift \
  apple/WireboltApp/GitCollaborationView.swift \
  apple/WireboltApp/PerformanceProbe.swift \
  apple/WireboltApp/ResponseViewer.swift \
  apple/WireboltApp/ResponseHexView.swift \
  apple/WireboltApp/FieldTextInput.swift \
  apple/WireboltApp/NativeCodeEditor.swift \
  apple/WireboltApp/EditorFind.swift \
  apple/WireboltApp/IndexedResponseEditor.swift \
  apple/WireboltApp/WireboltTheme.swift \
  apple/WireboltApp/ResponseViewport.swift \
  apple/WireboltApp/RustCore.swift \
  apple/WireboltApp/RustRequestRunner.swift \
  apple/WireboltApp/RustWebSocketRunner.swift \
  apple/WireboltApp/RustWorkspacePersistence.swift \
  -Xcc -fmodule-map-file=apple/Generated/wirebolt_ffiFFI.modulemap \
  -Xcc -fmodule-map-file=crates/wirebolt-ffi/include/module.modulemap \
  target/release/libwirebolt_ffi.a \
  -framework AppKit \
  -framework AuthenticationServices \
  -framework CryptoKit \
  -framework Security \
  -framework SystemConfiguration \
  -framework SwiftUI \
  -framework WebKit \
  -Xlinker -dead_strip

codesign --force --sign - --timestamp=none "$app_dir"
