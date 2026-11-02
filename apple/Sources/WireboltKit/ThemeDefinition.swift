import Foundation
import Observation

public typealias ThemePreset = DomainThemePreset

extension DomainThemePreset: RawRepresentable, CaseIterable {
    public static var allCases: [Self] { [.black, .paper, .ember] }
    public var rawValue: String {
        domainThemePresetValue(preset: self)
    }
    public init?(rawValue: String) {
        guard let value = domainThemePresetFromValue(value: rawValue) else { return nil }
        self = value
    }
    public var name: String { rawValue.capitalized }
}

public enum InterfaceDensity: String, CaseIterable, Sendable {
    case compact, comfortable
    private static let compactTokens = domainDensity(compact: true)
    private static let comfortableTokens = domainDensity(compact: false)
    private var tokens: DensityTokens { self == .compact ? Self.compactTokens : Self.comfortableTokens }
    public var controlHeight: Double { tokens.controlHeight }
    public var rowHeight: Double { tokens.rowHeight }
    public var bodySize: Double { tokens.bodySize }
    public var detailSize: Double { tokens.detailSize }
}

/// Opaque sRGB tokens. Alpha borders are composited over their intended surface
/// once, so SwiftUI and AppKit draw precisely the same color.
public struct ThemeDefinition: Equatable, Sendable {
    let domain: ThemeTokens
    public var preset: ThemePreset { domain.preset }
    public let modalScrim = PaletteColor(hex: 0x000000)
    public let geometry = ThemeGeometry()
    public let typography = ThemeTypography(density: .comfortable)
    public var window: PaletteColor { domain.window }
    public var sidebar: PaletteColor { domain.sidebar }
    public var card: PaletteColor { domain.card }
    public var selected: PaletteColor { domain.selected }
    public var tabActive: PaletteColor { domain.tabActive }
    public var overlay: PaletteColor { domain.overlay }
    public var hairline: PaletteColor { domain.hairline }
    public var cardBorder: PaletteColor { domain.cardBorder }
    public var primary: PaletteColor { domain.primary }
    public var secondary: PaletteColor { domain.secondary }
    public var muted: PaletteColor { domain.muted }
    public var dim: PaletteColor { domain.dim }
    public var lineNumber: PaletteColor { domain.lineNumber }
    public var sendBackground: PaletteColor { domain.sendBackground }
    public var sendForeground: PaletteColor { domain.sendForeground }
    public var variableBackground: PaletteColor { domain.variableBackground }
    public var variableForeground: PaletteColor { domain.variableForeground }
    public var syntaxKey: PaletteColor { domain.syntaxKey }
    public var syntaxString: PaletteColor { domain.syntaxString }
    public var syntaxNumber: PaletteColor { domain.syntaxNumber }
    public var syntaxKeyword: PaletteColor { domain.syntaxKeyword }
    public var syntaxPunctuation: PaletteColor { domain.syntaxPunctuation }
    public var success: PaletteColor { domain.success }
    public var warning: PaletteColor { domain.warning }
    public var error: PaletteColor { domain.error }
    public var methodBackgrounds: [PaletteColor] { domain.methodBackgrounds }
    public var methodForegrounds: [PaletteColor] { domain.methodForegrounds }
    public var control: PaletteColor { domain.control }
    private static let presets = Dictionary(uniqueKeysWithValues: ThemePreset.allCases.map {
        ($0, Self(domain: domainTheme(preset: $0)))
    })
    public static func preset(_ preset: ThemePreset) -> Self { presets[preset] ?? Self(domain: domainTheme(preset: preset)) }
}

public struct ThemeGeometry: Equatable, Sendable {
    private static let tokens = domainGeometry()
    public var chip: Double { Self.tokens.chip }
    public var tab: Double { Self.tokens.tab }
    public var row: Double { Self.tokens.row }
    public var card: Double { Self.tokens.card }
    public var window: Double { Self.tokens.window }
}

public struct ThemeTypography: Equatable, Sendable {
    private let tokens: TypographyTokens
    public var body: Double { tokens.body }
    public var detail: Double { tokens.detail }
    public var tab: Double { tokens.tab }
    public var field: Double { tokens.field }
    public var settingsLabel: Double { tokens.settingsLabel }
    public var settingsDetail: Double { tokens.settingsDetail }
    public var brand: Double { tokens.brand }
    public var chip: Double { tokens.chip }
    public var title: Double { tokens.title }
    public init(density: InterfaceDensity) { tokens = domainTypography(compact: density == .compact) }
}

/// Value passed into native renderers and used in their attributed-string cache keys.
public struct ResolvedTheme: Equatable, Sendable {
    private let tokens: ResolvedTokens
    public let definition: ThemeDefinition
    public let revision: UInt64
    public let density: InterfaceDensity
    public let customID: String?
    public let increasedContrast: Bool
    public let geometry: ThemeGeometry
    public let typography: ThemeTypography
    public var isDark: Bool { definition.preset != .paper }
    public var primary: PaletteColor { tokens.primary }
    public var muted: PaletteColor { tokens.muted }
    public var hairline: PaletteColor { tokens.hairline }
    public var cardBorder: PaletteColor { tokens.cardBorder }
    public var focusBorder: PaletteColor { tokens.focusBorder }

    public init(preset: ThemePreset, density: InterfaceDensity = .comfortable, increasedContrast: Bool = false, revision: UInt64 = 0) {
        customID = nil
        tokens = domainResolveTheme(preset: preset, increased: increasedContrast)
        definition = .preset(preset)
        geometry = definition.geometry
        typography = ThemeTypography(density: density)
        self.revision = revision
        self.density = density
        self.increasedContrast = increasedContrast
    }
    public init(custom: CustomTheme, density: InterfaceDensity = .comfortable, increasedContrast: Bool = false, revision: UInt64 = 0) {
        tokens = domainResolveCustomTheme(tokens: custom.tokens, increased: increasedContrast)
        definition = ThemeDefinition(domain: tokens.definition)
        customID = custom.id
        geometry = definition.geometry
        typography = ThemeTypography(density: density)
        self.revision = revision
        self.density = density
        self.increasedContrast = increasedContrast
    }
    private static let methodKinds = domainMethodChipKinds()
    public func method(_ method: HTTPMethod, webSocket: Bool = false) -> (background: PaletteColor, foreground: PaletteColor) {
        let kind = webSocket ? .webSocket : Self.methodKinds[method.rawValue] ?? .custom
        guard let chip = tokens.chips[kind] else { return (definition.tabActive, primary) }
        return (chip.background, chip.foreground)
    }
}

@MainActor @Observable
public final class ThemeStore {
    public static let shared = ThemeStore()
    public private(set) var resolved = ResolvedTheme(preset: .black)

    public private(set) var customThemes: [CustomTheme] = []
    public private(set) var notices: [String] = []
    public private(set) var isWritable = true
    public private(set) var errorMessage: String?
    private var library: AppThemeLibrary?

    /// A nil URL is an in-memory renderer only; tests must supply their own temporary URL to persist.
    public init(libraryURL: URL? = nil) {
        library = libraryURL.map { AppThemeLibrary.open(path: $0.path) }
        refreshLibrary()
    }

    public func loadLibrary(at url: URL) {
        library = AppThemeLibrary.open(path: url.path)
        refreshLibrary()
    }

    private func refreshLibrary() {
        guard let snapshot = library?.snapshot() else { return }
        customThemes = snapshot.themes
        notices = snapshot.notices
        isWritable = snapshot.writable
    }

    private nonisolated static func fileOperation<T: Sendable>(_ operation: @escaping @Sendable () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                continuation.resume(with: Result { try operation() })
            }
        }
    }

    public func clearError() { errorMessage = nil }
    public func report(_ error: any Error) {
        if case let DomainError.Invalid(reason) = error { errorMessage = reason }
        else { errorMessage = error.localizedDescription }
    }
    public func importTheme(from url: URL) async -> ThemePreview? {
        do {
            // Bounded file IO and JSONC parsing stay off the cooperative pool and UI thread.
            guard url.isFileURL else { throw CocoaError(.fileReadUnsupportedScheme) }
            let preview = try await Self.fileOperation { try appImportVscodeTheme(path: url.path) }
            errorMessage = nil
            return preview
        } catch { report(error); return nil }
    }
    public func save(name: String, preview: ThemePreview) async -> CustomTheme? {
        guard let library else { return nil }
        do {
            let saved = try await Self.fileOperation { try library.add(name: name, tokens: preview.tokens) }
            refreshLibrary()
            errorMessage = nil
            return saved
        } catch { report(error); return nil }
    }
    public func rename(id: String, name: String) async -> Bool {
        guard let library else { return false }
        do {
            try await Self.fileOperation { try library.rename(id: id, name: name) }
            refreshLibrary()
            errorMessage = nil
            return true
        } catch { report(error); return false }
    }
    public func delete(id: String, selection: String) async -> String? {
        guard let library else { return nil }
        do {
            let fallback = try await Self.fileOperation { try library.delete(id: id, selection: selection) }
            refreshLibrary()
            errorMessage = nil
            return fallback
        } catch { report(error); return nil }
    }

    public func update(selection: String, systemIsDark: Bool, increasedContrast: Bool, density: InterfaceDensity) {
        let custom = customThemes.first { $0.id == selection }
        let candidate: ResolvedTheme
        if let custom {
            candidate = ResolvedTheme(custom: custom, density: density, increasedContrast: increasedContrast, revision: resolved.revision)
        } else {
            candidate = ResolvedTheme(preset: domainSelectPreset(selection: selection, dark: systemIsDark),
                density: density, increasedContrast: increasedContrast, revision: resolved.revision)
        }
        guard candidate != resolved else { return }
        if let custom {
            resolved = ResolvedTheme(custom: custom, density: density, increasedContrast: increasedContrast, revision: resolved.revision &+ 1)
        } else {
            resolved = ResolvedTheme(preset: candidate.definition.preset, density: density,
                increasedContrast: increasedContrast, revision: resolved.revision &+ 1)
        }
    }
}
