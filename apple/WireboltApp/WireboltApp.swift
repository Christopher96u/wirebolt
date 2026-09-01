import SwiftUI

@main
struct WireboltApp: App {
    @State private var model: WireboltModel
    @State private var interface: WorkspaceUIState

    init() {
        let persistence = try? RustWorkspacePersistence()
        _model = State(initialValue: WireboltModel(
            runner: RustRequestRunner(),
            persistence: persistence,
            gitCollaboration: persistence
        ))
        _interface = State(initialValue: WorkspaceUIState())
    }

    var body: some Scene {
        WindowGroup {
            ContentView(
                model: model,
                interface: interface,
                status: RustCore().status()
            )
        }
        .defaultSize(width: 1_248, height: 580)
        .windowResizability(.contentMinSize)
        .windowToolbarStyle(.unified)
        .commands {
            CommandGroup(after: .newItem) {
                Button("New Request") { interface.makeNewRequest(model: model) }
                    .keyboardShortcut("n", modifiers: [.command, .shift])
                Button("Import Postman Collection v2…") {
                    interface.isShowingImporter = true
                }
                .keyboardShortcut("i", modifiers: [.command, .shift])
            }
            CommandGroup(replacing: .saveItem) {
                Button("Save Request") {
                    guard let collectionID = model.workspace.collections.first?.id else { return }
                    Task { await model.saveCurrentRequest(collectionID: collectionID) }
                }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(model.draft.name.isEmpty || model.workspace.collections.isEmpty)
            }
            CommandGroup(after: .toolbar) {
                Button(interface.columnVisibility == .detailOnly ? "Show Sidebar" : "Hide Sidebar") {
                    interface.columnVisibility = interface.columnVisibility == .detailOnly ? .all : .detailOnly
                }
                .keyboardShortcut("s", modifiers: [.command, .control])
            }
            CommandMenu("Request") {
                Button("Focus URL") { interface.focusURLTrigger += 1 }
                    .keyboardShortcut("l", modifiers: .command)
                Button("Send") {
                    Task { await model.send() }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(model.draft.url.isEmpty || model.isRunning)
                Divider()
                Button("Close Request Tab") { interface.closeActiveTab(model: model) }
                    .keyboardShortcut("w", modifiers: .command)
                Button("Filter Requests") { interface.focusSearchTrigger += 1 }
                    .keyboardShortcut("f", modifiers: [.command, .shift])
            }
            CommandMenu("Navigate") {
                Button("Back") {}
                    .keyboardShortcut("[", modifiers: .command)
                    .disabled(true)
                Button("Forward") {}
                    .keyboardShortcut("]", modifiers: .command)
                    .disabled(true)
                Divider()
                Button("Git Collaboration…") {
                    model.isShowingGitCollaboration = true
                }
                .keyboardShortcut("g", modifiers: [.command, .shift])
            }
        }

        Settings {
            WireboltSettingsView()
        }
    }
}

private struct WireboltSettingsView: View {
    @AppStorage("interfaceAppearance") private var interfaceAppearance = "system"

    var body: some View {
        Form {
            Picker("Appearance", selection: $interfaceAppearance) {
                Text("System").tag("system")
                Text("Light").tag("light")
                Text("Dark").tag("dark")
            }
            .pickerStyle(.radioGroup)

            LabeledContent("Privacy") {
                Label("Local · No telemetry", systemImage: "lock")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding()
        .frame(width: 420, height: 220)
    }
}
