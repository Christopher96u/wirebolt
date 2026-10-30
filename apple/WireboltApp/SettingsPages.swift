import AppKit
import SwiftUI

struct GeneralSettingsPage: View {
    @Bindable var model: WireboltModel
    let openWorkspaceSettings: () -> Void
    @AppStorage("editor.wordWrap") private var wraps = true
    @AppStorage("editor.showInvisibles") private var invisibles = false
    @AppStorage("editor.scrollBeyondLastLine") private var beyond = true

    var body: some View {
        SettingsPage(title: "General", subtitle: "Workspace and editing preferences.") {
            SettingsCard {
                SettingsRow(title: "Workspace", detail: model.workspace.name, last: model.workspaceLocation == nil) {
                    Button("Workspace settings", action: openWorkspaceSettings)
                }
                if let location = model.workspaceLocation {
                    SettingsRow(title: "Workspace folder", detail: location.path, last: true) {
                        Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([location]) }
                    }
                }
            }
            SettingsCard {
                SettingsRow(title: "Word wrap", detail: "Wrap long lines in request and response editors.") {
                    Toggle("Word wrap", isOn: $wraps).labelsHidden().toggleStyle(.switch)
                }
                SettingsRow(title: "Show invisible characters", detail: "Display spaces, tabs and line breaks.") {
                    Toggle("Show invisible characters", isOn: $invisibles).labelsHidden().toggleStyle(.switch)
                }
                SettingsRow(title: "Scroll beyond the last line", detail: "Keep the last lines of an editor near the top of the viewport.", last: true) {
                    Toggle("Scroll beyond the last line", isOn: $beyond).labelsHidden().toggleStyle(.switch)
                }
            }
        }
    }
}

struct KeyboardSettingsPage: View {
    private struct ShortcutGroup: Identifiable {
        let title: String
        let shortcuts: [(String, String)]
        var id: String { title }
    }
    @Environment(\.wireboltTheme) private var theme
    private let groups = [
        ShortcutGroup(title: "Requests", shortcuts: [("New request", "⌘N"), ("New tab", "⌘T"),
            ("Save request", "⌘S"), ("Send request or message", "⌘↩"), ("Cancel request", "⌘."),
            ("Connect or disconnect WebSocket", "⌃⌘↩"), ("Edit URL", "⌘L"), ("Add key", "⇧⌘K")]),
        ShortcutGroup(title: "Navigation", shortcuts: [("Focus sidebar", "⌘0"), ("Focus response", "⌥⌘0"),
            ("Split right", "⇧⌘D"), ("Next tab", "⌃⇥"), ("Previous tab", "⌃⇧⇥"),
            ("Select tab 1–9", "⌘1–9"), ("Request sections", "⌥⌘1–6"), ("Response sections", "⌃⌘1–5")]),
        ShortcutGroup(title: "Workspace", shortcuts: [("Open workspace", "⌘O"), ("New folder", "⌥⌘N"),
            ("New collection", "⇧⌘N"), ("Filter requests", "⇧⌘F"), ("Git collaboration", "⌃⌘G"),
            ("Settings", "⌘,"), ("Close tab", "⌘W"), ("Close window", "⇧⌘W")]),
        ShortcutGroup(title: "Editing", shortcuts: [("Undo", "⌘Z"), ("Redo", "⇧⌘Z"),
            ("Find", "⌘F"), ("Bulk edit", "⌘B"), ("Copy cURL", "⌥⇧⌘C")])
    ]

    var body: some View {
        SettingsPage(title: "Keyboard", subtitle: "Built-in shortcuts. Commands apply to the focused workspace or editor.") {
            ForEach(groups) { group in
                VStack(alignment: .leading, spacing: WireboltTheme.Spacing.large) {
                    Text(group.title).foregroundStyle(theme.muted.color)
                    SettingsCard {
                        ForEach(group.shortcuts.indices, id: \.self) { index in
                            SettingsRow(title: group.shortcuts[index].0, detail: "", last: index == group.shortcuts.count - 1) {
                                Text(group.shortcuts[index].1)
                                    .font(.system(size: theme.typography.settingsDetail, design: .monospaced))
                                    .foregroundStyle(theme.definition.secondary.color)
                            }
                        }
                    }
                }
            }
        }
    }
}

struct PrivacySettingsPage: View {
    @State private var showingPolicy = false
    @Environment(\.wireboltTheme) private var theme

    var body: some View {
        SettingsPage(title: "Privacy", subtitle: "Your workspace stays under your control.") {
            SettingsCard {
                SettingsRow(title: "Analytics", detail: "Wirebolt does not automatically collect usage analytics or identifiers.") {
                    Label("Not collected", systemImage: "checkmark.shield").foregroundStyle(theme.definition.success.color)
                }
                SettingsRow(title: "Crash reports", detail: "You choose which diagnostics to share when reporting a problem.") {
                    Text("Not uploaded automatically").foregroundStyle(theme.muted.color)
                }
                SettingsRow(title: "Local data", detail: "Requests and environments stay in your workspace. Secrets stay in this Mac’s Keychain.") {
                    Image(systemName: "lock").foregroundStyle(theme.muted.color).accessibilityHidden(true)
                }
                SettingsRow(title: "Privacy policy", detail: "How Wirebolt handles your workspace and connections.", last: true) {
                    Button("Privacy Policy") { showingPolicy = true }
                }
            }
        }
        .sheet(isPresented: $showingPolicy) {
            VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xLarge) {
                Text("Privacy Policy").font(WireboltTheme.Typography.title)
                Text("Wirebolt stores your workspace on this Mac. It does not automatically send analytics, identifiers, crash reports or diagnostics.")
                Text("Requests connect to the addresses you choose and may use your configured proxy. Sign-in and Git sync connect to their services when you use those features.")
                Text("You control which workspace files or diagnostic details you share with others.")
                HStack { Spacer(); Button("Done") { showingPolicy = false }.keyboardShortcut(.defaultAction) }
            }
            .font(.system(size: theme.typography.settingsLabel))
            .controlSize(.large)
            .padding(WireboltTheme.Spacing.section)
            .frame(width: 520)
            .background(theme.definition.overlay.color)
            .onExitCommand { showingPolicy = false }
        }
    }
}
