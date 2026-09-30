import Foundation

/// An opaque sRGB color used by the semantic palette.
public struct PaletteColor: Equatable, Hashable, Sendable {
    public let red: UInt8
    public let green: UInt8
    public let blue: UInt8

    public init(hex: UInt32) {
        red = UInt8((hex >> 16) & 0xFF)
        green = UInt8((hex >> 8) & 0xFF)
        blue = UInt8(hex & 0xFF)
    }

    /// WCAG 2.x relative luminance.
    public var relativeLuminance: Double {
        func linear(_ component: UInt8) -> Double {
            let value = Double(component) / 255
            return value <= 0.04045 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
    }

    /// WCAG 2.x contrast ratio, from 1 to 21.
    public func contrast(with other: PaletteColor) -> Double {
        let (first, second) = (relativeLuminance, other.relativeLuminance)
        return (max(first, second) + 0.05) / (min(first, second) + 0.05)
    }
}

/// One semantic color in the four appearances the app supports.
public struct AdaptivePaletteColor: Equatable, Hashable, Sendable {
    public let light: PaletteColor
    public let dark: PaletteColor
    /// Used when Increase Contrast is on.
    public let lightHighContrast: PaletteColor
    public let darkHighContrast: PaletteColor

    init(light: UInt32, dark: UInt32, lightHighContrast: UInt32, darkHighContrast: UInt32) {
        self.light = PaletteColor(hex: light)
        self.dark = PaletteColor(hex: dark)
        self.lightHighContrast = PaletteColor(hex: lightHighContrast)
        self.darkHighContrast = PaletteColor(hex: darkHighContrast)
    }
}

public enum HTTPStatusClass: Equatable, Sendable {
    case informational, success, redirection, clientError, serverError

    public init(status: UInt16) {
        switch status {
        case 100 ..< 200: self = .informational
        case 200 ..< 300: self = .success
        case 300 ..< 400: self = .redirection
        case 400 ..< 500: self = .clientError
        default: self = .serverError
        }
    }
}

/// Text colors for HTTP methods and statuses. Every value keeps at least 4.5:1
/// contrast (7:1 with Increase Contrast) against the app's content, bar, and
/// sidebar backgrounds so 10–11 pt labels stay readable in both appearances.
public enum SemanticPalette {
    /// Approximate opaque backgrounds that labels are drawn on: content, bars, sidebar.
    public static let lightBackgrounds = [0xFFFFFF, 0xF6F6F6, 0xE8E8E8].map(PaletteColor.init(hex:))
    public static let darkBackgrounds = [0x1E1E1E, 0x2A2A2A, 0x2C2C2E].map(PaletteColor.init(hex:))

    public static let green = AdaptivePaletteColor(light: 0x15742C, dark: 0x4ADE80, lightHighContrast: 0x0F5621, darkHighContrast: 0x86EFAC)
    public static let blue = AdaptivePaletteColor(light: 0x0A62C4, dark: 0x5AB0FF, lightHighContrast: 0x084892, darkHighContrast: 0x9CCFFF)
    public static let orange = AdaptivePaletteColor(light: 0xA04D04, dark: 0xFFA24D, lightHighContrast: 0x773903, darkHighContrast: 0xFFC58A)
    public static let purple = AdaptivePaletteColor(light: 0x7E3FC6, dark: 0xC79BFF, lightHighContrast: 0x5F2A9E, darkHighContrast: 0xDDC2FF)
    public static let red = AdaptivePaletteColor(light: 0xC2222A, dark: 0xFF7B7B, lightHighContrast: 0x911A20, darkHighContrast: 0xFFABAB)
    public static let teal = AdaptivePaletteColor(light: 0x096F73, dark: 0x3DD6D0, lightHighContrast: 0x075355, darkHighContrast: 0x8AEAE5)
    public static let pink = AdaptivePaletteColor(light: 0xB0277A, dark: 0xFF7AC6, lightHighContrast: 0x8A1B5F, darkHighContrast: 0xFFB0DD)
    public static let indigo = AdaptivePaletteColor(light: 0x4B44D0, dark: 0xA5A0FF, lightHighContrast: 0x3A33A8, darkHighContrast: 0xC9C6FF)
    public static let yellow = AdaptivePaletteColor(light: 0x7E6003, dark: 0xFFD426, lightHighContrast: 0x5E4702, darkHighContrast: 0xFFE270)
    public static let neutral = AdaptivePaletteColor(light: 0x5F6368, dark: 0xA8ABB0, lightHighContrast: 0x44474B, darkHighContrast: 0xC9CCD0)

    /// One distinct hue per standard method; WebSocket requests share one fixed color.
    public static func method(_ method: HTTPMethod, webSocket: Bool = false) -> AdaptivePaletteColor {
        if webSocket { return indigo }
        return switch method {
        case .get: green
        case .post: blue
        case .put: orange
        case .patch: purple
        case .delete: red
        case .head: teal
        case .options: pink
        default: neutral
        }
    }

    public static func status(_ status: UInt16) -> AdaptivePaletteColor {
        switch HTTPStatusClass(status: status) {
        case .informational: blue
        case .success: green
        case .redirection: orange
        case .clientError: yellow
        case .serverError: red
        }
    }
}
