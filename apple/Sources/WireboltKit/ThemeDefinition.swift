import Foundation
import Observation

public enum ThemePreset: String, CaseIterable, Sendable {
    case black, paper, ember
    public var name: String { rawValue.capitalized }
}

public enum InterfaceDensity: String, CaseIterable, Sendable {
    case compact, comfortable
    public var controlHeight: Double { self == .compact ? 28 : 32 }
    public var rowHeight: Double { self == .compact ? 28 : 64 }
    public var bodySize: Double { self == .compact ? 13 : 16 }
    public var detailSize: Double { self == .compact ? 11 : 14 }
}

/// Opaque sRGB tokens. Alpha borders are composited over their intended surface
/// once, so SwiftUI and AppKit draw precisely the same color.
public struct ThemeDefinition: Equatable, Sendable {
    public let preset: ThemePreset
    public let modalScrim = PaletteColor(hex: 0x000000)
    public let geometry = ThemeGeometry()
    public let typography = ThemeTypography(density: .comfortable)
    public let window, sidebar, card, selected, tabActive, overlay: PaletteColor
    public let hairline, cardBorder: PaletteColor
    public let primary, secondary, muted, dim, lineNumber: PaletteColor
    public let sendBackground, sendForeground, variableBackground, variableForeground: PaletteColor
    public let syntaxKey, syntaxString, syntaxNumber, syntaxKeyword, syntaxPunctuation: PaletteColor
    public let success, warning, error: PaletteColor
    public let methodBackgrounds, methodForegrounds: [PaletteColor]

    public static func preset(_ preset: ThemePreset) -> Self {
        let values: [UInt32]
        switch preset {
        case .black:
            values = [0x000000, 0x020203, 0x101010, 0x262626, 0x1a1a1a, 0x0b0b0b,
                0x191919, 0x232323, 0xf1f3f7, 0xcfcfcf, 0xa3a3a3, 0x6b6b6b, 0x444444,
                0x363636, 0xededed, 0x1d1e24, 0xa8b4ff, 0x79c0ff, 0xa5d6ff, 0xffa657, 0xff7b72, 0xc9d1d9,
                0x4ade80, 0xffd426, 0xff7b7b,
                0x0f2a1b, 0x0f2233, 0x2e1d0c, 0x2e1214, 0x1e1838,
                0x4ade80, 0x4aa3ff, 0xffa24d, 0xff7b7b, 0xa5a0ff]
        case .paper:
            values = [0xfcfcfd, 0xf4f4f5, 0xffffff, 0xffffff, 0xededf0, 0xffffff,
                0xe4e4e7, 0xe4e4e7, 0x18181b, 0x3f3f46, 0x71717a, 0xa1a1aa, 0xc4c4cc,
                0x18181b, 0xffffff, 0xeef0ff, 0x4338ca, 0x0550ae, 0x0a3069, 0x953800, 0xcf222e, 0x24292f,
                0x15803d, 0xa16207, 0xb91c1c,
                0xdcfce7, 0xdbeafe, 0xffedd5, 0xfee2e2, 0xede9fe,
                0x15803d, 0x1d4ed8, 0xc2410c, 0xb91c1c, 0x6d28d9]
        case .ember:
            values = [0x120d0b, 0x150f0c, 0x1c1512, 0x2e221c, 0x261c17, 0x1a1310,
                0x2a1f1a, 0x302722, 0xf6ede7, 0xdccbc2, 0xb5a299, 0x7d6b62, 0x5a4a42,
                0xf97316, 0x1a0e07, 0x2e1e15, 0xfdba74, 0xfdba74, 0xfde2c4, 0xfca5a5, 0xf97316, 0xe7d8cf,
                0x4ade80, 0xfbbf24, 0xff8a80,
                0x132a18, 0x14212e, 0x33200e, 0x331517, 0x241c33,
                0x4ade80, 0x60b0ff, 0xffb066, 0xff8a80, 0xb8b0ff]
        }
        let c = values.map(PaletteColor.init(hex:))
        return Self(preset: preset, window: c[0], sidebar: c[1], card: c[2], selected: c[3], tabActive: c[4], overlay: c[5],
            hairline: c[6], cardBorder: c[7], primary: c[8], secondary: c[9], muted: c[10], dim: c[11], lineNumber: c[12],
            sendBackground: c[13], sendForeground: c[14], variableBackground: c[15], variableForeground: c[16],
            syntaxKey: c[17], syntaxString: c[18], syntaxNumber: c[19], syntaxKeyword: c[20], syntaxPunctuation: c[21],
            success: c[22], warning: c[23], error: c[24], methodBackgrounds: Array(c[25..<30]), methodForegrounds: Array(c[30..<35]))
    }
}

public struct ThemeGeometry: Equatable, Sendable {
    public let chip = 4.0, tab = 8.0, row = 10.0, card = 16.0, window = 12.0
}

public struct ThemeTypography: Equatable, Sendable {
    public let body, detail, tab, field: Double
    public let settingsLabel, settingsDetail, brand: Double
    public let chip = 10.0, title = 26.0
    public init(density: InterfaceDensity) {
        body = density.bodySize
        detail = density.detailSize
        tab = density == .compact ? 12 : 14
        field = density == .compact ? 11 : 13.5
        settingsLabel = density == .compact ? 13 : 15
        settingsDetail = density == .compact ? 11 : 13
        brand = 17
    }
}

/// Value passed into native renderers and used in their attributed-string cache keys.
public struct ResolvedTheme: Equatable, Sendable {
    public let definition: ThemeDefinition
    public let revision: UInt64
    public let density: InterfaceDensity
    public let increasedContrast: Bool
    public let primary, muted, hairline, cardBorder, focusBorder: PaletteColor
    public let geometry: ThemeGeometry
    public let typography: ThemeTypography
    public var isDark: Bool { definition.preset != .paper }

    public init(preset: ThemePreset, density: InterfaceDensity = .comfortable, increasedContrast: Bool = false, revision: UInt64 = 0) {
        let definition = ThemeDefinition.preset(preset)
        self.definition = definition
        geometry = definition.geometry
        typography = ThemeTypography(density: density)
        self.revision = revision
        self.density = density
        self.increasedContrast = increasedContrast
        let surfaces = [definition.window, definition.sidebar, definition.card]
        primary = Self.strengthen(definition.primary, on: surfaces, ratio: 7)
        muted = Self.strengthen(definition.muted, on: surfaces, ratio: increasedContrast ? 7 : 4.5)
        hairline = increasedContrast ? Self.strengthen(definition.hairline, on: surfaces, ratio: 3) : definition.hairline
        cardBorder = increasedContrast ? Self.strengthen(definition.cardBorder, on: surfaces, ratio: 3) : definition.cardBorder
        focusBorder = Self.strengthen(definition.cardBorder, on: surfaces, ratio: increasedContrast ? 4.5 : 3)
    }

    /// Preserve the authored hue, moving towards black/white only as far as needed.
    private static func strengthen(_ color: PaletteColor, on surfaces: [PaletteColor], ratio: Double) -> PaletteColor {
        if surfaces.allSatisfy({ color.contrast(with: $0) >= ratio }) { return color }
        let target = surfaces[0].relativeLuminance > 0.5 ? 0 : 255
        for step in 1...255 {
            func component(_ value: UInt8) -> UInt32 {
                UInt32((Double(value) + Double(target - Int(value)) * Double(step) / 255).rounded())
            }
            let adjusted = PaletteColor(hex: component(color.red) << 16 | component(color.green) << 8 | component(color.blue))
            if surfaces.allSatisfy({ adjusted.contrast(with: $0) >= ratio }) { return adjusted }
        }
        return PaletteColor(hex: target == 0 ? 0 : 0xffffff)
    }

    public func method(_ method: HTTPMethod, webSocket: Bool = false) -> (background: PaletteColor, foreground: PaletteColor) {
        let index: Int?
        if webSocket { index = 4 } else {
            index = switch method { case .get: 0; case .post: 1; case .put: 2; case .delete: 3; default: nil }
        }
        let adaptive = SemanticPalette.method(method, webSocket: webSocket)
        let foreground: PaletteColor
        if increasedContrast { foreground = isDark ? adaptive.darkHighContrast : adaptive.lightHighContrast }
        else if let index { foreground = definition.methodForegrounds[index] }
        else { foreground = isDark ? adaptive.dark : adaptive.light }
        if let index { return (definition.methodBackgrounds[index], foreground) }
        // Unmocked methods use the established semantic hues on a quiet shared surface.
        return (definition.tabActive, foreground)
    }
}

@MainActor @Observable
public final class ThemeStore {
    public static let shared = ThemeStore()
    public private(set) var resolved = ResolvedTheme(preset: .black)

    public init() {}

    public func update(selection: String, systemIsDark: Bool, increasedContrast: Bool, density: InterfaceDensity) {
        let preset = ThemePreset(rawValue: selection)
            ?? (selection == "light" ? .paper : selection == "dark" ? .black : systemIsDark ? .black : .paper)
        guard resolved.definition.preset != preset || resolved.increasedContrast != increasedContrast || resolved.density != density else { return }
        resolved = ResolvedTheme(preset: preset, density: density, increasedContrast: increasedContrast, revision: resolved.revision &+ 1)
    }
}
