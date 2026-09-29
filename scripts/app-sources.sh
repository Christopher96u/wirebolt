#!/usr/bin/env bash
# Shared source and link inputs for the app and its native workload runner.
# shellcheck disable=SC2034
wirebolt_app_inputs=(
  apple/Generated/wirebolt_ffi.swift
  apple/Sources/WireboltKit/ProxyConnectionTest.swift
  apple/Sources/WireboltKit/ProxySettings.swift
  apple/Sources/WireboltKit/MarkdownPreview.swift
  apple/Sources/WireboltKit/SemanticPalette.swift
  apple/Sources/WireboltKit/Models.swift
  apple/Sources/WireboltKit/CurlExport.swift
  apple/Sources/WireboltKit/WebSocket.swift
  apple/Sources/WireboltKit/JSONNode.swift
  apple/Sources/WireboltKit/CodeFolding.swift
  apple/Sources/WireboltKit/TextLineIndex.swift
  apple/Sources/WireboltKit/TextSearch.swift
  apple/Sources/WireboltKit/CodeTextWrapping.swift
  apple/Sources/WireboltKit/ResponseTextIndex.swift
  apple/Sources/WireboltKit/ResponseIndexCache.swift
  apple/Sources/WireboltKit/DocumentSessions.swift
  apple/Sources/WireboltKit/ResponseStorage.swift
  apple/Sources/WireboltKit/JSONResponseDocument.swift
  apple/Sources/WireboltKit/OAuth2Service.swift
  apple/Sources/WireboltKit/WireboltModel.swift
  apple/Sources/WireboltKit/SidebarSnapshot.swift
  apple/WireboltApp/WorkspaceUIState.swift
  apple/WireboltApp/ContentView.swift
  apple/WireboltApp/WorkspaceActions.swift
  apple/WireboltApp/NetworkSettingsView.swift
  apple/WireboltApp/NotesEditor.swift
  apple/WireboltApp/EnvironmentEditor.swift
  apple/WireboltApp/GitCollaborationView.swift
  apple/WireboltApp/PerformanceProbe.swift
  apple/WireboltApp/ResponseViewer.swift
  apple/WireboltApp/ResponseHexView.swift
  apple/WireboltApp/FieldTextInput.swift
  apple/WireboltApp/NativeCodeEditor.swift
  apple/WireboltApp/IndexedTextLine.swift
  apple/WireboltApp/EditorFind.swift
  apple/WireboltApp/IndexedResponseEditor.swift
  apple/WireboltApp/WireboltTheme.swift
  apple/WireboltApp/ResponseViewport.swift
  apple/WireboltApp/RustCore.swift
  apple/WireboltApp/RustRequestRunner.swift
  apple/WireboltApp/RustWebSocketRunner.swift
  apple/WireboltApp/RustWorkspacePersistence.swift
  -Xcc -fmodule-map-file=apple/Generated/wirebolt_ffiFFI.modulemap
  -Xcc -fmodule-map-file=crates/wirebolt-ffi/include/module.modulemap
  target/release/libwirebolt_ffi.a
  -framework AppKit
  -framework AuthenticationServices
  -framework CryptoKit
  -framework Security
  -framework SystemConfiguration
  -framework SwiftUI
  -framework WebKit
  -Xlinker -dead_strip
)
