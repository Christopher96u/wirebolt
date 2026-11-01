import Foundation
import Observation

public enum ThemePreset: String, CaseIterable, Sendable {
    case black, paper, ember
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
    public var preset: ThemePreset { ThemePreset(domain: domain.preset) }
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
        ($0, Self(domain: domainTheme(preset: $0.domain)))
    })
    public static func preset(_ preset: ThemePreset) -> Self { presets[preset]! }
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
        tokens = domainResolveTheme(preset: preset.domain, increased: increasedContrast)
        definition = .preset(preset)
        geometry = definition.geometry
        typography = ThemeTypography(density: density)
        self.revision = revision; self.density = density; self.increasedContrast = increasedContrast
    }
    public func method(_ method: HTTPMethod, webSocket: Bool = false) -> (background: PaletteColor, foreground: PaletteColor) {
        let chip = tokens.chips[webSocket ? "WS" : method.rawValue] ?? tokens.chips["CUSTOM"]!
        return (chip.background, chip.foreground)
    }
}

@MainActor @Observable
public final class ThemeStore {
    public static let shared = ThemeStore()
    public private(set) var resolved = ResolvedTheme(preset: .black)

    public init() {}

    public func update(selection: String, systemIsDark: Bool, increasedContrast: Bool, density: InterfaceDensity) {
        let preset = ThemePreset(domain: domainSelectPreset(selection: selection, dark: systemIsDark))
        guard resolved.definition.preset != preset || resolved.increasedContrast != increasedContrast || resolved.density != density else { return }
        resolved = ResolvedTheme(preset: preset, density: density, increasedContrast: increasedContrast, revision: resolved.revision &+ 1)
    }
}

private extension ThemePreset {
    init(domain: DomainThemePreset) {
        self = switch domain {
        case .black: .black
        case .paper: .paper
        case .ember: .ember
        }
    }
    var domain: DomainThemePreset {
        switch self {
        case .black: .black
        case .paper: .paper
        case .ember: .ember
        }
    }
}
