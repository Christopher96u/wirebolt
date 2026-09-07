#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

scripts/cargo.sh test --workspace --all-features
swift test --package-path apple
scripts/generate-bindings.sh

mkdir -p build

swiftc \
  -O \
  -parse-as-library \
  -swift-version 6 \
  -target arm64-apple-macosx15.0 \
  -o build/bridge-smoke \
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
  apple/Tests/BridgeSmoke.swift \
  apple/WireboltApp/PerformanceProbe.swift \
  apple/WireboltApp/RustRequestRunner.swift \
  apple/WireboltApp/RustWorkspacePersistence.swift \
  -Xcc -fmodule-map-file=apple/Generated/wirebolt_ffiFFI.modulemap \
  -Xcc -fmodule-map-file=crates/wirebolt-ffi/include/module.modulemap \
  target/release/libwirebolt_ffi.a \
  -framework AppKit \
  -framework AuthenticationServices \
  -framework CryptoKit \
  -framework Security \
  -framework SystemConfiguration

build/bridge-smoke
