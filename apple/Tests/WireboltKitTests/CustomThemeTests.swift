import Foundation
import Testing
@testable import WireboltKit

struct CustomThemeTests {
    @Test @MainActor func customPickerSelectionRenameDeleteAndPersistence() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "wirebolt-theme-tests-\(UUID())")
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appending(path: "themes.json")
        let store = ThemeStore(libraryURL: file)
        let preview = domainDeriveTheme(background: PaletteColor(hex: 0x081320), accent: PaletteColor(hex: 0x55aaff))
        let custom = try #require(await store.save(name: "Ocean", preview: preview))
        #expect(store.customThemes.map(\.name) == ["Ocean"])
        store.update(selection: .custom(id: custom.id), systemIsDark: false, increasedContrast: false, density: .comfortable)
        #expect(store.resolved.customID == custom.id)
        #expect(store.resolved.definition.window == custom.tokens.window)
        let revision = store.resolved.revision
        store.update(selection: .custom(id: custom.id), systemIsDark: false, increasedContrast: false, density: .comfortable)
        #expect(store.resolved.revision == revision)
        #expect(await store.rename(id: custom.id, name: "Deep ocean"))
        let reopened = ThemeStore(libraryURL: file)
        #expect(reopened.customThemes.first?.name == "Deep ocean")
        #expect(reopened.customThemes.first?.id == custom.id)
        let fallback = try #require(await store.delete(id: custom.id, selection: .custom(id: custom.id)))
        #expect(fallback == .system)
        store.update(selection: fallback, systemIsDark: false, increasedContrast: false, density: .comfortable)
        #expect(store.resolved.customID == nil && store.resolved.definition.preset == .paper)
        #expect(ThemeStore(libraryURL: file).customThemes.isEmpty)
    }

    @Test @MainActor func customPreviewKeepsContrastAndTypedMethods() {
        let preview = domainDeriveTheme(background: PaletteColor(hex: 0x888888), accent: PaletteColor(hex: 0xff2200))
        #expect(!preview.notices.isEmpty)
        for increased in [false, true] {
            let theme = ResolvedTheme(custom: CustomTheme(id: "preview", name: preview.name, tokens: preview.tokens), increasedContrast: increased)
            for surface in [theme.definition.window, theme.definition.sidebar, theme.definition.card] {
                #expect(theme.primary.contrast(with: surface) >= 7)
                #expect(theme.muted.contrast(with: surface) >= (increased ? 7 : 4.5))
            }
            #expect(Set(HTTPMethod.allCases.map { theme.method($0).foreground }).count == HTTPMethod.allCases.count)
            for method in HTTPMethod.allCases {
                let colors = theme.method(method)
                #expect(colors.foreground.contrast(with: colors.background) >= (increased ? 7 : 4.5))
            }
        }
    }

    @Test @MainActor func importErrorsAndCorruptLibraryArePresented() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "wirebolt-theme-errors-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appending(path: "themes.json")
        try Data("broken".utf8).write(to: file)
        let store = ThemeStore(libraryURL: file)
        #expect(store.customThemes.isEmpty && !store.notices.isEmpty)
        let invalid = root.appending(path: "invalid.json")
        try Data("{invalid".utf8).write(to: invalid)
        #expect(await store.importTheme(from: invalid) == nil)
        #expect(store.errorMessage?.contains("Invalid VS Code theme") == true)
        store.clearError()
        #expect(store.errorMessage == nil)
        let valid = root.appending(path: "valid.json")
        try Data("{ /* own fixture */ \"name\":\"Imported\", \"include\":\"base.json\", \"colors\":{}, }".utf8).write(to: valid)
        let preview = try #require(await store.importTheme(from: valid))
        #expect(preview.name == "Imported")
        #expect(preview.notices.contains { $0.contains("Parent files are not resolved") })
        #expect(await store.save(name: "Imported", preview: preview) != nil)
    }
}
