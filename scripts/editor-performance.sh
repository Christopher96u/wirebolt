#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
mkdir -p build

swiftc \
  -O \
  -parse-as-library \
  -swift-version 6 \
  -target arm64-apple-macosx15.0 \
  -o build/editor-performance \
  apple/Sources/WireboltKit/ProxySettings.swift \
  apple/Sources/WireboltKit/Models.swift \
  apple/Sources/WireboltKit/CodeFolding.swift \
  apple/Sources/WireboltKit/TextLineIndex.swift \
  apple/Sources/WireboltKit/TextSearch.swift \
  apple/Sources/WireboltKit/CodeTextWrapping.swift \
  apple/Sources/WireboltKit/ResponseTextIndex.swift \
  apple/Sources/WireboltKit/ResponseIndexCache.swift \
  apple/WireboltApp/FieldTextInput.swift \
  apple/WireboltApp/NativeCodeEditor.swift \
  apple/WireboltApp/IndexedTextLine.swift \
  apple/WireboltApp/EditorFind.swift \
  apple/WireboltApp/WireboltTheme.swift \
  apple/Benchmarks/EditorWorkloads.swift \
  -framework AppKit \
  -framework SwiftUI

build/editor-performance
