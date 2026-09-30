import AppKit
import SwiftUI

enum WireboltTheme {
    /// Brand indigo sampled from the approved UI reference: #6159E5.
    static let primaryAccent = Color(
        red: 97.0 / 255.0,
        green: 89.0 / 255.0,
        blue: 229.0 / 255.0
    )
    static let paneBackground = Color(nsColor: .textBackgroundColor)
    /// Selected sidebar row while the sidebar is not focused. The system's unemphasized
    /// selection gray nearly disappears over the dark vibrant sidebar, so Dark Mode uses a
    /// translucent white that stays visible over any wallpaper tint.
    static let unemphasizedSidebarSelection = Color(nsColor: adaptiveColor(
        light: .unemphasizedSelectedContentBackgroundColor,
        dark: NSColor(white: 1, alpha: 0.16)
    ))
    static let barBackground = AnyShapeStyle(.bar)
    static let separator = Color(nsColor: .separatorColor)
    /// Adaptive success green (also the 2xx status color).
    static let success = Color(nsColor: nsColor(SemanticPalette.green))
    /// Adaptive error red for inline error text (also the 5xx status color).
    static let danger = Color(nsColor: nsColor(SemanticPalette.red))
    /// Adaptive warning text color (also the 4xx status color).
    static let warning = Color(nsColor: nsColor(SemanticPalette.yellow))

    static let nsJSONKey = adaptiveColor(
        light: NSColor(srgbRed: 0.56, green: 0.24, blue: 0.22, alpha: 1),
        dark: NSColor(srgbRed: 0.50, green: 0.72, blue: 0.84, alpha: 1)
    )
    static let nsJSONString = adaptiveColor(
        light: NSColor(srgbRed: 0.22, green: 0.46, blue: 0.59, alpha: 1),
        dark: NSColor(srgbRed: 0.82, green: 0.58, blue: 0.49, alpha: 1)
    )
    static let nsJSONNumber = adaptiveColor(
        light: NSColor(srgbRed: 0.24, green: 0.44, blue: 0.56, alpha: 1),
        dark: NSColor(srgbRed: 0.75, green: 0.69, blue: 0.46, alpha: 1)
    )
    static let nsJSONBoolean = adaptiveColor(
        light: NSColor(srgbRed: 0.31, green: 0.49, blue: 0.29, alpha: 1),
        dark: NSColor(srgbRed: 0.62, green: 0.72, blue: 0.43, alpha: 1)
    )
    static let nsJSONNull = adaptiveColor(
        light: NSColor(srgbRed: 0.52, green: 0.30, blue: 0.56, alpha: 1),
        dark: NSColor(srgbRed: 0.72, green: 0.54, blue: 0.77, alpha: 1)
    )

    static let jsonKey = Color(nsColor: nsJSONKey)
    static let jsonString = Color(nsColor: nsJSONString)
    static let jsonNumber = Color(nsColor: nsJSONNumber)
    static let jsonBoolean = Color(nsColor: nsJSONBoolean)
    static let jsonNull = Color(nsColor: nsJSONNull)
    static let treeKey = Color(red: 0.25, green: 0.50, blue: 0.68)
    static let treeValue = Color(red: 0.64, green: 0.29, blue: 0.27)

    /// The single method color used by the sidebar, URL bar, and tabs. Each
    /// standard method has its own hue; WebSocket requests use one fixed color.
    /// Spacing scale in points. Prefer these over one-off values in new or touched views.
    enum Spacing {
        static let xxSmall: CGFloat = 2
        static let xSmall: CGFloat = 4
        static let small: CGFloat = 6
        static let medium: CGFloat = 8
        static let large: CGFloat = 12
        static let xLarge: CGFloat = 16
        static let xxLarge: CGFloat = 20
    }

    /// Corner radii for rows, controls, and grouped content.
    enum Radius {
        static let small: CGFloat = 3
        static let row: CGFloat = 5
        static let control: CGFloat = 6
        static let group: CGFloat = 10
    }

    /// Fixed-size text styles for dense editor chrome.
    enum Typography {
        /// Primary text in panels, rows, and toolbars.
        static let body = Font.system(size: 13)
        /// Secondary rows, table cells, and hints.
        static let detail = Font.system(size: 11)
        /// Compact method labels in lists.
        static let methodLabel = Font.system(size: 10, weight: .semibold)
        /// Counts in tab badges.
        static let badge = Font.system(size: 10, weight: .medium).monospacedDigit()
    }

    static func methodColor(_ method: HTTPMethod, webSocket: Bool = false) -> Color {
        if webSocket { return webSocketColor }
        return switch method {
        case .get: methodGET
        case .post: methodPOST
        case .put: methodPUT
        case .patch: methodPATCH
        case .delete: methodDELETE
        case .head: methodHEAD
        case .options: methodOPTIONS
        default: methodCustom
        }
    }

    /// Kept for existing call sites; identical to `methodColor(_:)`.
    static func requestBarMethodColor(_ method: HTTPMethod) -> Color {
        methodColor(method)
    }

    static let webSocketColor = Color(nsColor: nsColor(SemanticPalette.method(.get, webSocket: true)))

    /// 1xx informational, 2xx green, 3xx orange, 4xx yellow warning, 5xx red.
    static func statusColor(_ status: UInt16) -> Color {
        switch HTTPStatusClass(status: status) {
        case .informational: statusInformational
        case .success: success
        case .redirection: statusRedirection
        case .clientError: warning
        case .serverError: danger
        }
    }

    /// SF Symbol that pairs with `statusColor(_:)` so status is never conveyed by color alone.
    static func statusSymbol(_ status: UInt16) -> String {
        switch HTTPStatusClass(status: status) {
        case .informational: "info.circle.fill"
        case .success: "checkmark.circle.fill"
        case .redirection: "arrow.uturn.right.circle.fill"
        case .clientError: "exclamationmark.triangle.fill"
        case .serverError: "xmark.octagon.fill"
        }
    }

    private static let methodGET = Color(nsColor: nsColor(SemanticPalette.method(.get)))
    private static let methodPOST = Color(nsColor: nsColor(SemanticPalette.method(.post)))
    private static let methodPUT = Color(nsColor: nsColor(SemanticPalette.method(.put)))
    private static let methodPATCH = Color(nsColor: nsColor(SemanticPalette.method(.patch)))
    private static let methodDELETE = Color(nsColor: nsColor(SemanticPalette.method(.delete)))
    private static let methodHEAD = Color(nsColor: nsColor(SemanticPalette.method(.head)))
    private static let methodOPTIONS = Color(nsColor: nsColor(SemanticPalette.method(.options)))
    private static let methodCustom = Color(nsColor: nsColor(SemanticPalette.neutral))
    private static let statusInformational = Color(nsColor: nsColor(SemanticPalette.status(100)))
    private static let statusRedirection = Color(nsColor: nsColor(SemanticPalette.status(300)))

    /// Resolves light, dark, and Increase Contrast variants at draw time.
    private static func nsColor(_ color: AdaptivePaletteColor) -> NSColor {
        func make(_ value: PaletteColor) -> NSColor {
            NSColor(srgbRed: CGFloat(value.red) / 255, green: CGFloat(value.green) / 255, blue: CGFloat(value.blue) / 255, alpha: 1)
        }
        let (light, dark) = (make(color.light), make(color.dark))
        let (lightHigh, darkHigh) = (make(color.lightHighContrast), make(color.darkHighContrast))
        return NSColor(name: nil) { appearance in
            switch appearance.bestMatch(from: [.aqua, .darkAqua, .accessibilityHighContrastAqua, .accessibilityHighContrastDarkAqua]) {
            case .darkAqua: dark
            case .accessibilityHighContrastAqua: lightHigh
            case .accessibilityHighContrastDarkAqua: darkHigh
            default: light
            }
        }
    }

    private static func adaptiveColor(light: NSColor, dark: NSColor) -> NSColor {
        NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
        }
    }
}
