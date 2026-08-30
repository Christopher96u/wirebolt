import SwiftUI

@main
struct WireboltApp: App {
    @State private var model: WireboltModel

    init() {
        let persistence = try? RustWorkspacePersistence()
        _model = State(initialValue: WireboltModel(
            runner: RustRequestRunner(),
            persistence: persistence,
            gitCollaboration: persistence
        ))
    }

    var body: some Scene {
        WindowGroup {
            ContentView(model: model, status: RustCore().status())
        }
        .defaultSize(width: 1_180, height: 760)
        .windowResizability(.contentMinSize)
        .commands {
            CommandGroup(after: .newItem) {
                Button("New Request") { model.makeNewRequest() }
                    .keyboardShortcut("n", modifiers: .command)
            }
            CommandMenu("Git") {
                Button("Git Collaboration…") {
                    model.isShowingGitCollaboration = true
                }
                .keyboardShortcut("g", modifiers: [.command, .shift])
            }
        }
    }
}
