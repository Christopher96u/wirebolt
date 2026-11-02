import AppKit
import SwiftUI

/// Production Appearance views hosted inside this process, with disposable preferences/storage.
@MainActor enum CustomThemeNativeTests {
    private final class ThemeWindow: NSWindow { override var canBecomeKey: Bool { true } }
    private static func check(_ condition: Bool, _ message: String = "Theme assertion failed", line: UInt = #line) {
        if !condition { fatalError("\(message), line \(line)") }
    }
    private struct Element {
        let object: NSObject
        let role: String
        let name: String
        let frame: NSRect
    }
    private static func elements(_ object: Any, depth: Int = 0) -> [Element] {
        guard depth < 40, let object = object as? NSObject else { return [] }
        func value(_ key: String) -> Any? {
            object.responds(to: NSSelectorFromString(key)) ? object.value(forKey: key) : nil
        }
        func text(_ key: String) -> String {
            let raw = value(key)
            return (raw as? NSAttributedString)?.string ?? (raw as? String) ?? ""
        }
        let name = ["accessibilityLabel", "accessibilityTitle", "accessibilityValue"].map(text).first { !$0.isEmpty } ?? ""
        let current = Element(object: object, role: text("accessibilityRole"), name: name, frame: (value("accessibilityFrame") as? NSValue)?.rectValue ?? .zero)
        return [current] + (value("accessibilityChildren") as? [Any] ?? []).flatMap { elements($0, depth: depth + 1) }
    }
    private static func flush(_ host: NSView) {
        RunLoop.current.run(until: Date().addingTimeInterval(0.08))
        host.layoutSubtreeIfNeeded(); host.displayIfNeeded()
    }
    private static func settle(_ host: NSView) async {
        try? await Task.sleep(for: .milliseconds(120))
        flush(host)
    }
    private static func press(_ name: String, window: NSWindow, accessibility: Bool = false) async {
        guard let host = window.contentView,
              let item = (elements(window) + elements(host)).first(where: { ["AXButton", "AXMenuItem"].contains($0.role) && $0.name == name }) else {
            fatalError("Missing theme action \(name): \(elements(window.contentView!).map(\.name))")
        }
        if accessibility {
            // SwiftUI AX nodes implement the action selector without declaring
            // NSAccessibilityProtocol conformance. Invoke that known BOOL ABI locally.
            let selector = NSSelectorFromString("accessibilityPerformPress")
            check(item.object.responds(to: selector), "Missing press selector on \(type(of: item.object)): \(name)")
            typealias Press = @convention(c) (AnyObject, Selector) -> Bool
            let action = unsafeBitCast(item.object.method(for: selector), to: Press.self)
            if let button = item.object as? NSButton { button.performClick(nil) }
            else { _ = action(item.object, selector) }
            await settle(host)
            return
        }
        window.makeKey()
        let frame = window.convertFromScreen(item.frame)
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            let event = NSEvent.mouseEvent(with: type, location: NSPoint(x: frame.midX, y: frame.midY), modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                eventNumber: 1, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0)!
            window.sendEvent(event)
        }
        await settle(host)
    }
    private static func capture(_ host: NSView, directory: URL?, name: String) throws {
        guard let directory else { return }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { fatalError("Could not capture Appearance") }
        host.cacheDisplay(in: host.bounds, to: bitmap)
        try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appending(path: name + ".png"))
    }

    static func run(root: URL, defaults: UserDefaults, screenshots: URL? = nil) async throws {
        let store = ThemeStore(libraryURL: root.appending(path: "custom-themes.json"))
        let preview = domainDeriveTheme(background: PaletteColor(hex: 0x101923), accent: PaletteColor(hex: 0x64cec7))
        guard let custom = await store.save(name: "Harbour", preview: preview) else { fatalError("Could not save theme: \(store.errorMessage ?? "no error")") }
        defaults.set("black", forKey: "interfaceAppearance")
        let window = ThemeWindow(contentRect: NSRect(x: 30, y: 30, width: 1040, height: 1120),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let host = NSHostingView(rootView: AppearanceSettingsView().modifier(ThemeScope())
            .environment(\.customThemeStore, store).defaultAppStorage(defaults))
        window.contentView = host
        window.orderFront(nil)
        window.makeKey()
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface"))
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXManualAccessibility"))
        defer { window.close() }
        await settle(host)
        check(elements(host).contains { $0.name == "Harbour theme" }, "Custom themes must be in the picker")
        await press("Harbour theme", window: window)
        check(defaults.string(forKey: "interfaceAppearance") == custom.id && store.resolved.customID == custom.id)
        try capture(host, directory: screenshots, name: "01-two-colour-appearance")

        await press("Custom theme from 2 colors", window: window)
        guard let sheet = window.attachedSheet, let sheetHost = sheet.contentView else { fatalError("Missing custom theme preview sheet: \(NSApp.windows.map { $0.title }); page \(elements(host).map(\.name))") }
        await settle(sheetHost)
        guard elements(sheetHost).contains(where: { $0.name == "Theme live preview" }) else { fatalError("Missing live preview: \(elements(sheetHost).map(\.name))") }
        guard elements(sheetHost).contains(where: { $0.name == "Background" }) else { fatalError("Missing Background: \(elements(sheetHost).map(\.name))") }
        guard elements(sheetHost).contains(where: { $0.name == "Accent" }) else { fatalError("Missing Accent: \(elements(sheetHost).map(\.name))") }
        try capture(sheetHost, directory: screenshots, name: "03-live-preview")
        await press("Save Theme", window: sheet)
        await settle(host)
        check(store.customThemes.count == 2, "The preview must save into the picker")
        let saved = store.customThemes.last!
        check(defaults.string(forKey: "interfaceAppearance") == saved.id)
        await press("Delete…", window: window, accessibility: true)
        await settle(host)
        let confirmation = NSApp.windows.first { candidate in
            elements(candidate).contains { $0.name == "Delete Theme" }
        } ?? window

        await press("Delete Theme", window: confirmation, accessibility: true)
        await settle(host)
        check(store.customThemes.count == 1 && !store.customThemes.contains { $0.id == saved.id })
        check(defaults.string(forKey: "interfaceAppearance") == "system", "Deleting the active theme must follow the system")

        let importedURL = root.appending(path: "linen.json")
        let source = """
        { /* original VS Code-format fixture */
          "name": "Linen", "include": "./parent.json",
          "colors": { "editor.background": "#fbf7ed", "focusBorder": "#9b4266", },
          "tokenColors": [{"scope": "string", "settings": {"foreground": "#35724d"}},],
        }
        """
        try Data(source.utf8).write(to: importedURL)
        guard let imported = await store.importTheme(from: importedURL),
              let importedTheme = await store.save(name: imported.name, preview: imported) else { fatalError("Could not import fixture") }
        await settle(host)
        await press("Linen theme", window: window)
        check(store.resolved.customID == importedTheme.id && !store.resolved.isDark)
        try capture(host, directory: screenshots, name: "02-imported-appearance")
        let invalid = root.appending(path: "invalid.json")
        try Data("{bad input".utf8).write(to: invalid)
        let invalidPreview = await store.importTheme(from: invalid)
        check(invalidPreview == nil)
        await settle(host)
        check(elements(host).contains { $0.name.contains("Invalid VS Code theme") }, "Import errors must be visible on Appearance")
        try capture(host, directory: screenshots, name: "04-import-error")
        print("custom_theme_picker_select_preview_save_delete_import_errors=passed")
    }
}
