import AppKit
import SwiftUI

/// Production Appearance views hosted inside this process, with disposable preferences/storage.
@MainActor enum CustomThemeNativeTests {
    private final class ThemeWindow: NSWindow { override var canBecomeKey: Bool { true } }
    private static func press(_ name: String, window: NSWindow) async {
        let pressed = NativeTestSupport.press(window.contentView!, named: name) || NativeTestSupport.press(window, named: name)
        if !pressed { FileHandle.standardError.write(Data("Missing theme action \(name): \(NativeTestSupport.elements(window).map(\.name))\n".utf8)) }
        precondition(pressed, "Missing theme action \(name)")
        await NativeTestSupport.settle(window.contentView!)
    }
    private static func capture(_ host: NSView, directory: URL?, name: String, isDark: Bool) throws {
        guard let directory else { return }
        // The app delegate aligns AppKit with the chosen theme. This isolated
        // capture host has no app delegate, so mirror that platform adaptation.
        let appearance = NSAppearance(named: isDark ? .darkAqua : .aqua)
        host.window?.appearance = appearance
        host.appearance = appearance
        NativeTestSupport.flush(host)
        try NativeTestSupport.capture(host, directory: directory, name: name)
    }

    static func run(root: URL, defaults: UserDefaults, screenshots: URL? = nil) async throws {
        let previousAppearance = defaults.object(forKey: "interfaceAppearance")
        let restoreAccessibility = NativeTestSupport.enableAccessibility()
        defer {
            defaults.set(previousAppearance, forKey: "interfaceAppearance")
            restoreAccessibility()
        }
        let store = ThemeStore(libraryURL: root.appending(path: "custom-themes.json"))
        let preview = domainDeriveTheme(background: PaletteColor(hex: 0x101923), accent: PaletteColor(hex: 0x64cec7))
        guard let custom = await store.save(name: "Harbor", preview: preview) else { preconditionFailure("Could not save theme: \(store.errorMessage ?? "no error")") }
        defaults.set("black", forKey: "interfaceAppearance")
        let window = ThemeWindow(contentRect: NSRect(x: 30, y: 30, width: 1040, height: 1120),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let host = NSHostingView(rootView: AppearanceSettingsView().modifier(ThemeScope())
            .environment(\.customThemeStore, store).defaultAppStorage(defaults))
        window.contentView = host
        window.orderFront(nil)
        window.makeKey()
        defer { window.close() }
        await NativeTestSupport.settle(host)
        precondition(NativeTestSupport.elements(host).contains { $0.name == "Harbor theme" }, "Custom themes must be in the picker")
        await press("Harbor theme", window: window)
        precondition(defaults.string(forKey: "interfaceAppearance") == custom.id && store.resolved.customID == custom.id)
        try capture(host, directory: screenshots, name: "01-two-color-appearance", isDark: store.resolved.isDark)

        await press("Custom theme from 2 colors", window: window)
        guard let sheet = window.attachedSheet, let sheetHost = sheet.contentView else { preconditionFailure("Missing custom theme preview sheet: \(NSApp.windows.map { $0.title }); page \(NativeTestSupport.elements(host).map(\.name))") }
        await NativeTestSupport.settle(sheetHost)
        guard NativeTestSupport.elements(sheetHost).contains(where: { $0.name == "Theme live preview" }) else { preconditionFailure("Missing live preview: \(NativeTestSupport.elements(sheetHost).map(\.name))") }
        guard NativeTestSupport.elements(sheetHost).contains(where: { $0.name == "Background" }) else { preconditionFailure("Missing Background: \(NativeTestSupport.elements(sheetHost).map(\.name))") }
        guard NativeTestSupport.elements(sheetHost).contains(where: { $0.name == "Accent" }) else { preconditionFailure("Missing Accent: \(NativeTestSupport.elements(sheetHost).map(\.name))") }
        try capture(sheetHost, directory: screenshots, name: "03-live-preview", isDark: store.resolved.isDark)
        await press("Save Theme", window: sheet)
        await NativeTestSupport.settle(host)
        precondition(store.customThemes.count == 2, "The preview must save into the picker")
        let saved = store.customThemes.last!
        precondition(defaults.string(forKey: "interfaceAppearance") == saved.id)
        await press("Delete…", window: window)
        await NativeTestSupport.settle(host)
        let confirmation = NSApp.windows.first { candidate in
            NativeTestSupport.elements(candidate).contains { $0.name == "Delete Theme" }
        } ?? window

        await press("Delete Theme", window: confirmation)
        await NativeTestSupport.settle(host)
        precondition(store.customThemes.count == 1 && !store.customThemes.contains { $0.id == saved.id })
        precondition(defaults.string(forKey: "interfaceAppearance") == "system", "Deleting the active theme must follow the system")

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
              let importedTheme = await store.save(name: imported.name, preview: imported) else { preconditionFailure("Could not import fixture") }
        await NativeTestSupport.settle(host)
        await press("Linen theme", window: window)
        precondition(store.resolved.customID == importedTheme.id && !store.resolved.isDark)
        try capture(host, directory: screenshots, name: "02-imported-appearance", isDark: store.resolved.isDark)
        let invalid = root.appending(path: "invalid.json")
        try Data("{bad input".utf8).write(to: invalid)
        let invalidPreview = await store.importTheme(from: invalid)
        precondition(invalidPreview == nil)
        await NativeTestSupport.settle(host)
        precondition(NativeTestSupport.elements(host).contains { $0.name.contains("Invalid VS Code theme") }, "Import errors must be visible on Appearance")
        try capture(host, directory: screenshots, name: "04-import-error", isDark: store.resolved.isDark)
        print("customThemePickerSelectPreviewSaveDeleteImportErrors: passed")
    }
}
