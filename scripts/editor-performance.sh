#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
scripts/generate-bindings.sh
mkdir -p build

swiftc \
  -O \
  -parse-as-library \
  -swift-version 6 \
  -target arm64-apple-macosx15.0 \
  -o build/editor-performance \
  apple/Generated/wirebolt_ffi.swift \
  apple/Sources/WireboltKit/ProxySettings.swift \
  apple/Sources/WireboltKit/Models.swift \
  apple/Sources/WireboltKit/CodeFolding.swift \
  apple/Sources/WireboltKit/TextLineIndex.swift \
  apple/Sources/WireboltKit/TextSearch.swift \
  apple/Sources/WireboltKit/CodeTextWrapping.swift \
  apple/Sources/WireboltKit/ResponseTextIndex.swift \
  apple/Sources/WireboltKit/ResponseIndexCache.swift \
  apple/Sources/WireboltKit/ThemeDefinition.swift \
  apple/Sources/WireboltKit/SemanticPalette.swift \
  apple/Sources/WireboltKit/VariableTemplates.swift \
  apple/WireboltApp/FieldTextInput.swift \
  apple/WireboltApp/NativeCodeEditor.swift \
  apple/WireboltApp/IndexedTextLine.swift \
  apple/WireboltApp/EditorFind.swift \
  apple/WireboltApp/WireboltTheme.swift \
  apple/Benchmarks/EditorWorkloads.swift \
  -Xcc -fmodule-map-file=apple/Generated/wirebolt_ffiFFI.modulemap \
  -licucore \
  target/release/libwirebolt_ffi.a \
  -framework Security \
  -framework SystemConfiguration \
  -framework AppKit \
  -framework SwiftUI

build/editor-performance
