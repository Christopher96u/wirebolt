import SwiftUI

@main
struct WireboltApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView(status: RustCore().status())
        }
        .defaultSize(width: 760, height: 520)
    }
}
