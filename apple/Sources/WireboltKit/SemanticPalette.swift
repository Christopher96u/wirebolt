import Foundation

public typealias PaletteColor = RgbColor
public extension RgbColor {
    init(hex: UInt32) { self.init(red: UInt8((hex >> 16) & 255), green: UInt8((hex >> 8) & 255), blue: UInt8(hex & 255)) }
    var relativeLuminance: Double { domainLuminance(color: self) }
    func contrast(with other: PaletteColor) -> Double { domainContrast(color: self, other: other) }
}
public typealias AdaptivePaletteColor = AdaptiveColor
public enum HTTPStatusClass: String, Equatable, Sendable {
    case informational, success, redirection, clientError, serverError
    public init(status: UInt16) { self = Self(rawValue: domainStatusClass(status: status))! }
}
public enum SemanticPalette {
    public static let lightBackgrounds = domainBackgrounds(dark: false)
    public static let darkBackgrounds = domainBackgrounds(dark: true)
    public static let green = domainSemantic(name: "green")
    public static let blue = domainSemantic(name: "blue")
    public static let orange = domainSemantic(name: "orange")
    public static let purple = domainSemantic(name: "purple")
    public static let red = domainSemantic(name: "red")
    public static let teal = domainSemantic(name: "teal")
    public static let pink = domainSemantic(name: "pink")
    public static let indigo = domainSemantic(name: "indigo")
    public static let yellow = domainSemantic(name: "yellow")
    public static let neutral = domainSemantic(name: "neutral")

    private static let methods = Dictionary(uniqueKeysWithValues: HTTPMethod.allCases.map { ($0.rawValue, domainMethodColor(method: $0.rawValue, webSocket: false)) })
    public static func method(_ method: HTTPMethod, webSocket: Bool = false) -> AdaptivePaletteColor {
        webSocket ? indigo : methods[method.rawValue] ?? neutral
    }
    public static func status(_ status: UInt16) -> AdaptivePaletteColor { domainStatusColor(status: status) }
}
