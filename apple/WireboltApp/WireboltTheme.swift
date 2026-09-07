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
    static let barBackground = AnyShapeStyle(.bar)
    static let separator = Color(nsColor: .separatorColor)
    static let success = Color(red: 0.31, green: 0.80, blue: 0.34)

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

    static func methodColor(_ method: HTTPMethod) -> Color {
        switch method {
        case .get, .head, .options: Color(red: 0.31, green: 0.78, blue: 0.35)
        case .post: Color(red: 0.20, green: 0.59, blue: 0.86)
        case .put: Color(red: 0.82, green: 0.61, blue: 0.18)
        case .patch: Color(red: 0.28, green: 0.63, blue: 0.82)
        case .delete: Color(red: 0.86, green: 0.28, blue: 0.29)
        default: primaryAccent
        }
    }

    static func requestBarMethodColor(_ method: HTTPMethod) -> Color {
        method == .patch ? success : methodColor(method)
    }

    static func statusColor(_ status: UInt16) -> Color {
        switch status {
        case 200 ..< 300: success
        case 300 ..< 400: .orange
        default: .red
        }
    }

    private static func adaptiveColor(light: NSColor, dark: NSColor) -> NSColor {
        NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
        }
    }
}
