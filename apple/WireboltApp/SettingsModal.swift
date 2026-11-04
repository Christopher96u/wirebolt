import AppKit
import SwiftUI

/// Stays mounted after its first presentation so unapplied proxy edits survive closing.
struct SettingsModal: View {
    @Bindable var model: WireboltModel
    @Environment(\.wireboltTheme) private var theme
    @State private var hasPresented = false
    @State private var focus = SettingsModalFocus.shared

    var body: some View {
        GeometryReader { geometry in
            if hasPresented || model.isShowingAppSettings {
                let inset: Double = geometry.size.width >= 1_000 ? 32 : 16
                ZStack {
                    theme.definition.modalScrim.color.opacity(theme.isDark ? 0.6 : 0.3)
                    WireboltSettingsView(model: model)
                        .frame(width: max(0, geometry.size.width - inset * 2), height: max(0, geometry.size.height - inset * 2))
                        .clipShape(.rect(cornerRadius: theme.geometry.card))
                        .overlay { RoundedRectangle(cornerRadius: theme.geometry.card).strokeBorder(theme.cardBorder.color) }
                        .shadow(color: theme.definition.modalScrim.color.opacity(0.35), radius: 24, y: 8)
                        .accessibilityElement(children: .contain)
                        .accessibilityLabel("Settings")
                }
                .opacity(model.isShowingAppSettings ? 1 : 0)
                .allowsHitTesting(model.isShowingAppSettings)
                .accessibilityHidden(!model.isShowingAppSettings)
                .disabled(!model.isShowingAppSettings)
            }
        }
        .background(SettingsModalWindowProbe(focus: focus).allowsHitTesting(false).accessibilityHidden(true))
        .allowsHitTesting(model.isShowingAppSettings)
        .accessibilityHidden(!model.isShowingAppSettings)
        .onChange(of: model.isShowingAppSettings, initial: true) {
            if model.isShowingAppSettings {
                hasPresented = true
                guard focus.origin == nil else { return }
                focus.capture()
            } else if let origin = focus.origin, let window = focus.window, window.attachedSheet == nil {
                // Wait for the workspace's native controls to be enabled again.
                DispatchQueue.main.async {
                    guard !model.isShowingAppSettings else { return }
                    _ = origin.restore()
                    focus.origin = nil
                }
            }
        }
    }
}

@MainActor private final class SettingsModalFocus {
    static let shared = SettingsModalFocus()
    weak var window: NSWindow?
    var origin: WorkspaceFocusCapture?

    func capture() {
        origin = window.map(WorkspaceFocusCapture.init(window:))
    }
}

/// Capture before disabling the workspace's native field editor.
@MainActor
func presentAppSettings(_ model: WireboltModel, focus: WorkspaceFocusCapture? = nil) {
    guard !model.isShowingAppSettings else { return }
    if let focus {
        SettingsModalFocus.shared.window = focus.window
        SettingsModalFocus.shared.origin = focus
    } else { SettingsModalFocus.shared.capture() }
    model.isShowingAppSettings = true
}

private struct SettingsModalWindowProbe: NSViewRepresentable {
    let focus: SettingsModalFocus
    func makeNSView(context: Context) -> Probe { Probe(focus: focus) }
    func updateNSView(_ view: Probe, context: Context) { focus.window = view.window }
    final class Probe: NSView {
        let focus: SettingsModalFocus
        init(focus: SettingsModalFocus) { self.focus = focus; super.init(frame: .zero) }
        required init?(coder: NSCoder) { return nil }
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); focus.window = window }
    }
}
