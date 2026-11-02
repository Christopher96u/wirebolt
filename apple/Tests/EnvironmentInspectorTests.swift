import AppKit
import Foundation
import SwiftUI

private struct InspectorRunner: RequestRunner {
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> { AsyncThrowingStream { $0.finish() } }
    func cancel(runID: RunID) {}
}
private actor InspectorStore: WorkspacePersistence {
    func load() async throws -> WorkspaceDraft { WorkspaceDraft(name: "Fixture") }
    func save(request: RequestDraft, in collectionID: String) async throws {}
    func save(environment: EnvironmentDraft) async throws {}
    func saveSecret(name: String, value: String) async throws { preconditionFailure("Native inspector fixtures never write real secrets") }
}

@MainActor enum EnvironmentInspectorTests {
    static func fields(_ view: NSView) -> [NSTextField] {
        (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fields)
    }
    static func accessibility(_ object: Any, depth: Int = 0) -> [(String, String)] {
        guard depth < 40, let object = object as? NSObject else { return [] }
        func value(_ name: String) -> Any? {
            let selector = NSSelectorFromString(name)
            return object.responds(to: selector) ? object.perform(selector)?.takeUnretainedValue() : nil
        }
        let name = ["accessibilityLabel", "accessibilityTitle", "accessibilityValue"].compactMap { value($0) as? String }.first(where: { !$0.isEmpty }) ?? ""
        return [(value("accessibilityRole") as? String ?? "", name)] + (value("accessibilityChildren") as? [Any] ?? []).flatMap { accessibility($0, depth: depth + 1) }
    }
    @discardableResult
    static func press(_ object: Any, named name: String, depth: Int = 0) -> Bool {
        guard depth < 40, let object = object as? NSObject else { return false }
        let label = NSSelectorFromString("accessibilityLabel")
        let action = NSSelectorFromString("accessibilityPerformPress")
        if object.responds(to: label), object.responds(to: action),
           object.perform(label)?.takeUnretainedValue() as? String == name {
            _ = object.perform(action)
            return true
        }
        let children = NSSelectorFromString("accessibilityChildren")
        guard object.responds(to: children) else { return false }
        return (object.perform(children)?.takeUnretainedValue() as? [Any] ?? []).contains { press($0, named: name, depth: depth + 1) }
    }

    static func settle(_ host: NSView) async throws {
        for _ in 0..<10 {
            try await Task.sleep(for: .milliseconds(10))
            host.layoutSubtreeIfNeeded()
        }
    }
    static func replace(_ field: NSTextField, with text: String, window: NSWindow) {
        window.makeFirstResponder(field)
        guard let editor = field.currentEditor() as? NSTextView else { preconditionFailure("Field must accept native focus") }
        editor.insertText(text, replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
    }
    static func run(root: URL, defaults: UserDefaults) async throws {
        let model = WireboltModel(runner: InspectorRunner(), persistence: InspectorStore(),
            history: HistoryRepository(root: root.appending(path: "inspector-history")),
            cookieJar: CookieJar(storageURL: root.appending(path: "inspector-cookies")),
            proxyPreferences: ProxyPreferences(defaults: defaults))
        let longName = "service_base_url_for_integration_and_staging"
        let longValue = "http://192.168.100.200:8080/api/v1/integration/resources?include=metadata"
        model.workspace.environments = [EnvironmentDraft(id: "local", name: "Local", variables: [
            EnvironmentVariableDraft(id: "url", key: longName, value: .literal(longValue))])]
        model.selectedEnvironmentID = "local"
        let interface = WorkspaceUIState(defaults: defaults)
        var closeCount = 0
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 380, height: 700), styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let host = NSHostingView(rootView: EnvironmentEditor(model: model, interface: interface, closeTrigger: 0) { closeCount += 1 }.defaultAppStorage(defaults))
        window.contentView = host
        let undoManager = UndoManager()
        model.undoManager = undoManager
        window.orderFront(nil)
        defer { window.close() }
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface"))
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXManualAccessibility"))
        try await settle(host)
        let tree = accessibility(host)
        precondition(tree.filter { $0.0 == "AXButton" && $0.1 == "Globals" }.count == 1, "Globals appears once")
        precondition(!tree.contains { $0.0 == "AXButton" && ["Apply", "Cancel"].contains($0.1) }, "No staged-save buttons")
        guard var key = fields(host).first(where: { $0.accessibilityLabel() == "Variable name" }),
              var value = fields(host).first(where: { $0.accessibilityLabel() == "Value for \(longName)" }) else { preconditionFailure("Inspector fields are reachable") }
        for (field, full) in [(key, longName), (value, longValue)] {
            precondition(field.cell?.wraps == false && field.cell?.usesSingleLineMode == true)
            precondition(field.lineBreakMode == .byTruncatingMiddle)
            precondition(field.toolTip == full && field.accessibilityValue() == full, "Hover and VoiceOver expose the entire value")
        }
        print("inspectorGlobalsOnceAndFullAccessibilityValues: passed")

        replace(value, with: "http://localhost:8080", window: window)
        try await settle(host)
        precondition(model.workspace.environments[0].variables[0].value == .literal(longValue), "Typing never saves")
        window.makeFirstResponder(key)
        try await settle(host)
        precondition(model.workspace.environments[0].variables[0].value == .literal("http://localhost:8080"), "Focus loss commits")
        precondition(undoManager.undoActionName == "Edit Variable")
        undoManager.undo(); await model.finishUndoWork()
        precondition(model.workspace.environments[0].variables[0].value == .literal(longValue))
        undoManager.redo(); await model.finishUndoWork()
        precondition(model.workspace.environments[0].variables[0].value == .literal("http://localhost:8080"))
        print("inspectorFocusLossCommitAndWorkspaceUndoRedo: passed")

        replace(value, with: "return-value", window: window)
        (value.currentEditor() as? NSTextView)?.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        try await settle(host)
        precondition(model.workspace.environments[0].variables[0].value == .literal("return-value"), "Return commits")
        replace(key, with: "tab_key", window: window)
        (key.currentEditor() as? NSTextView)?.doCommand(by: #selector(NSResponder.insertTab(_:)))
        try await settle(host)
        precondition(model.workspace.environments[0].variables[0].key == "tab_key", "Tab commits")
        print("inspectorReturnAndTabCommit: passed")

        replace(value, with: "switch-value", window: window)
        precondition(press(host, named: "Globals"))
        try await settle(host)
        precondition(model.selectedEnvironmentID == nil && model.workspace.environments[0].variables[0].value == .literal("switch-value"), "Switching commits the active field")
        precondition(press(host, named: "Local"))
        try await settle(host)
        key = fields(host).first { $0.accessibilityLabel() == "Variable name" }!
        value = fields(host).first { $0.accessibilityLabel() == "Value for tab_key" }!
        print("inspectorSwitchCommits: passed")

        replace(key, with: "bad name", window: window)
        window.makeFirstResponder(nil)
        try await settle(host)
        precondition(key.stringValue == "bad name", "Validation keeps user text")
        precondition(press(host, named: "Globals"))
        try await settle(host)
        precondition(model.selectedEnvironmentID == "local" && key.stringValue == "bad name", "Invalid text blocks switching without disappearing")
        guard let close = interface.saveEnvironmentEdits else { preconditionFailure("Inspector installs its close handler") }
        let invalidClosed = await close()
        precondition(!invalidClosed && closeCount == 0)
        precondition(model.workspace.environments[0].variables[0].key == "tab_key")
        precondition(accessibility(host).contains { $0.1.contains("without spaces or braces") }, "Validation is inline")
        replace(key, with: "base_url", window: window)
        let validClosed = await close()
        precondition(validClosed && closeCount == 1, "Inspector close commits the active native field")
        precondition(model.workspace.environments[0].variables[0].key == "base_url")
        precondition(undoManager.undoActionName == "Rename Variable")
        print("inspectorCloseCommitsAndInvalidTextSurvives: passed")
        replace(key, with: "close_trigger", window: window)
        host.rootView = EnvironmentEditor(model: model, interface: interface, closeTrigger: 1) { closeCount += 1 }.defaultAppStorage(defaults)
        try await settle(host)
        precondition(closeCount == 2 && model.workspace.environments[0].variables[0].key == "close_trigger", "Toolbar close commits in-progress text")
        print("inspectorToolbarCloseCommits: passed")
        replace(key, with: "escape_key", window: window)
        (key.currentEditor() as? NSTextView)?.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        try await settle(host)
        precondition(closeCount == 3 && model.workspace.environments[0].variables[0].key == "escape_key", "Escape closes without dropping the active field")
        print("inspectorEscapeCommits: passed")
        try await layoutFixtures(model: model, defaults: defaults)
    }
    static func layoutFixtures(model: WireboltModel, defaults: UserDefaults) async throws {
        for density in [InterfaceDensity.compact, .comfortable] {
            for size in [DynamicTypeSize.large, .accessibility3] {
                let interface = WorkspaceUIState(defaults: defaults)
                let theme = ResolvedTheme(preset: .black, density: density)
                let view = EnvironmentEditor(model: model, interface: interface, closeTrigger: 0, onClose: {})
                    .environment(\.wireboltTheme, theme).dynamicTypeSize(size).defaultAppStorage(defaults)
                let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 340, height: 700), styleMask: [.titled], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                let host = NSHostingView(rootView: view)
                window.contentView = host
                window.orderFront(nil)
                try await settle(host)
                let inputs = fields(host).filter { $0.accessibilityLabel() == "Variable name" || $0.accessibilityLabel() == "Value for \(model.workspace.environments[0].variables[0].key)" }
                precondition(inputs.count == 2)
                for input in inputs {
                    let rect = input.convert(input.bounds, to: host)
                    precondition(rect.minX >= 0 && rect.maxX <= host.bounds.width, "Fields fit the narrow inspector at each density and text size")
                    precondition(input.bounds.height >= (input.font?.pointSize ?? 0), "Text is not clipped vertically")
                }
                precondition(!inputs[0].convert(inputs[0].bounds, to: host).intersects(inputs[1].convert(inputs[1].bounds, to: host)), "Name and value never overlap")
                if let directory = ProcessInfo.processInfo.environment["WIREBOLT_INSPECTOR_CAPTURE_DIRECTORY"] {
                    guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { preconditionFailure("Inspector capture failed") }
                    host.cacheDisplay(in: host.bounds, to: bitmap)
                    let name = "inspector-\(density)-\(size).png"
                    try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: directory).appending(path: name))
                }
                window.close()
            }
        }
        print("inspectorDensityAndLargeTextLayout: passed")
    }

}
