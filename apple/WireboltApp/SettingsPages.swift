import AppKit
import SwiftUI

struct GeneralSettingsPage: View {
    @Bindable var model: WireboltModel
    let openWorkspaceSettings: () -> Void
    @State private var clearingHistory = false
    @AppStorage("editor.wordWrap") private var wraps = true
    @AppStorage("editor.showInvisibles") private var invisibles = false
    @AppStorage("editor.scrollBeyondLastLine") private var beyond = true

    var body: some View {
        SettingsPage(title: "General", subtitle: "Workspace and editing preferences.") {
            SettingsCard {
                SettingsRow(title: "Workspace", detail: model.workspace.name, last: model.workspaceLocation == nil) {
                    Button("Workspace Settings", action: openWorkspaceSettings)
                }
                if let location = model.workspaceLocation {
                    SettingsRow(title: "Workspace folder", detail: location.path, last: true) {
                        Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([location]) }
                    }
                }
            }
            SettingsCard {
                SettingsRow(title: "Word wrap", detail: "Wrap long lines in request and response editors.") {
                    Toggle("Word wrap", isOn: $wraps).labelsHidden().toggleStyle(.wireboltSwitch)
                }
                SettingsRow(title: "Show invisible characters", detail: "Display spaces, tabs and line breaks.") {
                    Toggle("Show invisible characters", isOn: $invisibles).labelsHidden().toggleStyle(.wireboltSwitch)
                }
                SettingsRow(title: "Scroll beyond the last line", detail: "Keep the last lines of an editor near the top of the viewport.", last: true) {
                    Toggle("Scroll beyond the last line", isOn: $beyond).labelsHidden().toggleStyle(.wireboltSwitch)
                }
            }
            VStack(alignment: .leading, spacing: WireboltTheme.Spacing.large) {
                Text("History").foregroundStyle(WireboltTheme.muted)
                SettingsCard {
                    SettingsRow(title: "Response history", detail: "Clear stored responses from every workspace.", last: true) {
                        Button("Clear All History…", role: .destructive) { clearingHistory = true }
                    }
                }
            }
        }
        .alert("Clear All History", isPresented: $clearingHistory) {
            Button("Cancel", role: .cancel) {}
            Button("Clear All History", role: .destructive) { Task { await model.clearAllHistory() } }
        } message: {
            Text("This removes stored responses from every workspace. Responses already open in tabs stay available.")
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
        ShortcutGroup(title: "Requests", shortcuts: [("New request", "⌘N"),
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
