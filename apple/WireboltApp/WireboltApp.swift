import SwiftUI

@main
struct WireboltApp: App {
    @State private var model = WireboltModel(
        runner: RustRequestRunner(),
        persistence: try? RustWorkspacePersistence()
    )

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
        }
    }
}
