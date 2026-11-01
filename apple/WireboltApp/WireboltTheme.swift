import AppKit
import SwiftUI

extension PaletteColor {
    var nsColor: NSColor {
        NSColor(srgbRed: Double(red) / 255, green: Double(green) / 255, blue: Double(blue) / 255, alpha: 1)
    }
    var color: Color { Color(nsColor: nsColor) }
}

extension EnvironmentValues {
    @Entry var wireboltTheme = ResolvedTheme(preset: .black)
    @Entry var wireboltReduceMotion = false
}

/// Resolves preferences once at the scene boundary. Reading an immutable theme in a
/// leaf does not establish any dependency on drafts, run progress or workspace state.
struct ThemeScope: ViewModifier {
    @AppStorage("interfaceAppearance") private var selection = "system"
    @AppStorage("interfaceDensity") private var density = "comfortable"
    @AppStorage("interfaceReduceMotion") private var reducesMotion = false
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion
    private let store = ThemeStore.shared

    func body(content: Content) -> some View {
        content
            .environment(\.wireboltTheme, store.resolved)
            .environment(\.wireboltReduceMotion, systemReduceMotion || reducesMotion)
            .foregroundStyle(store.resolved.primary.color)
            // Neutral: color is reserved for methods, statuses and variables. macOS 26 tints
            // pop-up and menu titles, so the window tint is the muted text color; filled
            // controls (checkboxes, segments, default buttons) opt into `controlAccent`.
            .tint(store.resolved.muted.color)
            .background(store.resolved.definition.window.color)
            .transaction { if systemReduceMotion || reducesMotion { $0.animation = nil } }
            .onAppear(perform: resolve)
            .onChange(of: selection, resolve)
            .onChange(of: density, resolve)
            .onChange(of: colorScheme, resolve)
            .onChange(of: contrast, resolve)
            .onReceive(NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification)) {
                _ in resolve()
            }
    }

    private func resolve() {
        store.update(
            selection: selection, systemIsDark: colorScheme == .dark,
            increasedContrast: contrast == .increased || NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast,
            density: InterfaceDensity(rawValue: density) ?? .comfortable)
    }
}

/// Compatibility facade for shared chrome. Every value is derived from the store;
/// native renderers receive the resolved value explicitly as well.
@MainActor
enum WireboltTheme {
    static var current: ResolvedTheme { ThemeStore.shared.resolved }
    /// Fill for checkboxes, selected segments and default buttons, which opt in with
    /// `.tint(WireboltTheme.controlAccent)`. Neutral by design (see `ThemeDefinition.control`).
    static var controlAccent: Color { current.definition.control.color }
    static var paneBackground: Color { current.definition.window.color }
    static var sidebarBackground: Color { current.definition.sidebar.color }
    static var cardBackground: Color { current.definition.card.color }
    static var selectedBackground: Color { current.definition.selected.color }
    static var tabBackground: Color { current.definition.tabActive.color }
    static var unemphasizedSidebarSelection: Color { selectedBackground }
    static var barBackground: AnyShapeStyle { AnyShapeStyle(paneBackground) }
    static var separator: Color { current.hairline.color }
    static var primary: Color { current.primary.color }
    static var secondary: Color { current.definition.secondary.color }
    static var muted: Color { current.muted.color }
    static var dim: Color { current.definition.dim.color }
    static var success: Color { current.definition.success.color }
    static var danger: Color { current.definition.error.color }
    static var warning: Color { current.definition.warning.color }
    static var nsJSONKey: NSColor { current.definition.syntaxKey.nsColor }
    static var nsJSONString: NSColor { current.definition.syntaxString.nsColor }
    static var nsJSONNumber: NSColor { current.definition.syntaxNumber.nsColor }
    static var nsJSONBoolean: NSColor { current.definition.syntaxKeyword.nsColor }
    static var nsJSONNull: NSColor { nsJSONBoolean }
    static var jsonKey: Color { Color(nsColor: nsJSONKey) }
    static var jsonString: Color { Color(nsColor: nsJSONString) }
    static var jsonNumber: Color { Color(nsColor: nsJSONNumber) }
    static var jsonBoolean: Color { Color(nsColor: nsJSONBoolean) }
    static var jsonNull: Color { jsonBoolean }
    static var treeKey: Color { jsonKey }
    static var treeValue: Color { jsonString }
    static var webSocketColor: Color { current.method(.get, webSocket: true).foreground.color }

    enum Spacing {
        static let hairline: CGFloat = 1
        static let xxSmall: CGFloat = 2
        static let xSmall: CGFloat = 4
        static let small: CGFloat = 6
        static let medium: CGFloat = 8
        static let large: CGFloat = 12
        static let xLarge: CGFloat = 16
        static let xxLarge: CGFloat = 20
        static let section: CGFloat = 24
        static let canvas: CGFloat = 32
    }
    @MainActor enum Radius {
        static var small: CGFloat { current.geometry.chip }
        static var row: CGFloat { current.geometry.row }
        static var control: CGFloat { current.geometry.tab }
        static var group: CGFloat { current.geometry.card }
    }
    @MainActor enum Typography {
        static var body: Font { .system(size: current.typography.body) }
        static var detail: Font { .system(size: current.typography.detail) }
        static let methodLabel = Font.system(size: 10, weight: .bold)
        static let badge = Font.system(size: 10, weight: .medium).monospacedDigit()
        static var title: Font { .system(size: current.typography.title, weight: .semibold) }
        static var tab: Font { .system(size: current.typography.tab) }
        static var field: Font { .system(size: current.typography.field, design: .monospaced) }
    }
    static func methodColor(_ method: HTTPMethod, webSocket: Bool = false) -> Color {
        current.method(method, webSocket: webSocket).foreground.color
    }
    static func requestBarMethodColor(_ method: HTTPMethod) -> Color { methodColor(method) }
    static func statusColor(_ status: UInt16) -> Color {
        if current.increasedContrast {
            let color = SemanticPalette.status(status)
            return (current.isDark ? color.darkHighContrast : color.lightHighContrast).color
        }
        return switch HTTPStatusClass(status: status) {
        case .informational: methodColor(.post)
        case .success: success
        case .redirection: methodColor(.put)
        case .clientError: warning
        case .serverError: danger
        }
    }
    static func statusSymbol(_ status: UInt16) -> String {
        switch HTTPStatusClass(status: status) {
        case .informational: "info.circle.fill"
        case .success: "checkmark.circle.fill"
        case .redirection: "arrow.uturn.right.circle.fill"
        case .clientError: "exclamationmark.triangle.fill"
        case .serverError: "xmark.octagon.fill"
        }
    }
}

struct MethodChip: View {
    let method: HTTPMethod
    var webSocket = false
    @Environment(\.wireboltTheme) private var theme
    var body: some View {
        let colors = theme.method(method, webSocket: webSocket)
        Text(webSocket ? "WS" : method.rawValue)
            .font(WireboltTheme.Typography.methodLabel)
            .foregroundStyle(colors.foreground.color)
            .padding(.horizontal, WireboltTheme.Spacing.small)
            .padding(.vertical, WireboltTheme.Spacing.xxSmall)
            .background(colors.background.color, in: .rect(cornerRadius: theme.geometry.chip))
            .fixedSize()
    }
}

extension View {
    /// A borderless menu drawn like the other ghost controls: its title and icon take the
    /// muted text color whatever tint the surrounding view sets.
    func ghostMenuStyle() -> some View {
        menuStyle(.borderlessButton).tint(WireboltTheme.muted)
    }
}

/// The settings switch from the design: a primary track with a canvas knob when on, a
/// dim track when off. VoiceOver and Full Keyboard Access see a native toggle.
struct WireboltSwitchStyle: ToggleStyle {
    @Environment(\.wireboltTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.controlSize) private var controlSize
    @Environment(\.labelsVisibility) private var labelsVisibility

    func makeBody(configuration: Configuration) -> some View {
        let small = controlSize == .small || controlSize == .mini
        HStack(spacing: WireboltTheme.Spacing.medium) {
            if labelsVisibility != .hidden { configuration.label }
            Button {
                configuration.isOn.toggle()
            } label: {
                Capsule()
                    .fill(configuration.isOn ? theme.primary.color : theme.definition.dim.color)
                    .overlay(alignment: configuration.isOn ? .trailing : .leading) {
                        Circle().fill(theme.definition.window.color).padding(2)
                    }
                    .frame(width: small ? 30 : 36, height: small ? 17 : 20)
                    .animation(.snappy(duration: 0.15), value: configuration.isOn)
                    .contentShape(.capsule)
            }
            .buttonStyle(.plain)
        }
        .opacity(isEnabled ? 1 : 0.4)
        .accessibilityRepresentation { Toggle(isOn: configuration.$isOn) { configuration.label } }
    }
}

extension ToggleStyle where Self == WireboltSwitchStyle {
    static var wireboltSwitch: WireboltSwitchStyle { WireboltSwitchStyle() }
}

struct GhostIconStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.wireboltTheme) private var theme
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: theme.density == .compact ? 13 : 16, weight: .regular))
            .foregroundStyle(theme.muted.color)
            .frame(minWidth: theme.density.controlHeight, minHeight: theme.density.controlHeight)
            .background(configuration.isPressed ? theme.definition.selected.color : .clear, in: .rect(cornerRadius: theme.geometry.tab))
            .contentShape(.rect)
            .opacity(isEnabled ? 1 : 0.4)
    }
}
