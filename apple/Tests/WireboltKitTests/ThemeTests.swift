import Testing
@testable import WireboltKit

struct ThemeTests {
    @Test func persistedPresetSpellingsAndLegacySelections() {
        for (preset, value) in [(ThemePreset.black, "black"), (.paper, "paper"), (.ember, "ember")] {
            #expect(preset.rawValue == value)
            #expect(ThemePreset(rawValue: value) == preset)
            #expect(domainSelectPreset(selection: value, dark: true) == preset)
            #expect(domainSelectPreset(selection: value, dark: false) == preset)
        }
        #expect(domainSelectPreset(selection: "dark", dark: false) == .black)
        #expect(domainSelectPreset(selection: "light", dark: true) == .paper)
        #expect(ThemePreset(rawValue: "dark") == nil)
    }

    @Test func textContrastOnAllSurfaces() {
        for preset in ThemePreset.allCases {
            for increased in [false, true] {
                let theme = ResolvedTheme(preset: preset, increasedContrast: increased)
                for surface in [theme.definition.window, theme.definition.sidebar, theme.definition.card] {
                    #expect(theme.primary.contrast(with: surface) >= 7)
                    #expect(theme.muted.contrast(with: surface) >= (increased ? 7 : 4.5))
                    #expect(theme.focusBorder.contrast(with: surface) >= (increased ? 4.5 : 3))
                    if increased { #expect(theme.hairline.contrast(with: surface) >= 3) }
                }
            }
        }
    }

    @Test("Native controls are neutral, visible on every surface and keep white glyphs legible")
    func neutralControlFill() {
        let white = PaletteColor(hex: 0xffffff)
        for preset in ThemePreset.allCases {
            let definition = ThemeDefinition.preset(preset)
            let control = definition.control
            for surface in [definition.window, definition.sidebar, definition.card] {
                #expect(control.contrast(with: surface) >= 3)
            }
            // Checkmarks, selected segment labels and default button titles are white.
            #expect(white.contrast(with: control) >= 4.5)
            // Monochrome: no channel strays far from the others, unlike the variable accent.
            let channels = [control.red, control.green, control.blue].map(Int.init)
            #expect(channels.max()! - channels.min()! <= 32)
        }
    }

    @Test @MainActor func resolutionAndRevision() {
        let store = ThemeStore()
        store.update(selection: "system", systemIsDark: false, increasedContrast: false, density: .comfortable)
        #expect(store.resolved.definition.preset == .paper)
        let revision = store.resolved.revision
        store.update(selection: "system", systemIsDark: false, increasedContrast: false, density: .comfortable)
        #expect(store.resolved.revision == revision)
        store.update(selection: "ember", systemIsDark: false, increasedContrast: true, density: .compact)
        #expect(store.resolved.definition.preset == .ember)
        #expect(store.resolved.revision > revision)
        #expect(store.resolved.density.controlHeight >= 28)
        store.update(selection: "system", systemIsDark: true, increasedContrast: false, density: .comfortable)
        #expect(store.resolved.definition.preset == .black)
        #expect(store.resolved.geometry.card == 16)
    }

    @Test func methodsKeepDistinctSemanticHues() {
        for preset in ThemePreset.allCases {
            let theme = ResolvedTheme(preset: preset)
            let colors = HTTPMethod.allCases.map { theme.method($0).foreground }
            #expect(Set(colors).count == colors.count)
            #expect(theme.method(.get, webSocket: true).foreground != theme.method(.get).foreground)
            for method in HTTPMethod.allCases {
                let chip = theme.method(method)
                #expect(chip.foreground.contrast(with: chip.background) >= 4.5)
            }
        }
    }
}

extension ThemeTests {
    @Test func generatedPresetsKeepUserDefaultsValuesAndCustomChips() throws {
        #expect(ThemePreset.allCases.map(\.rawValue) == ["black", "paper", "ember"])
        for preset in ThemePreset.allCases {
            #expect(ThemePreset(rawValue: preset.rawValue) == preset)
            let theme = ResolvedTheme(preset: preset)
            let custom = try #require(HTTPMethod(rawValue: "PROPFIND"))
            let another = try #require(HTTPMethod(rawValue: "CUSTOM"))
            #expect(theme.method(custom).foreground == theme.method(another).foreground)
            #expect(theme.method(custom).background == theme.method(another).background)
        }
        #expect(ThemePreset(rawValue: "system") == nil)
    }
}
