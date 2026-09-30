import Foundation
import Testing
@testable import WireboltKit

struct WorkspaceIdentityTests {
    @Test("Renaming the workspace persists one trimmed command and updates the title source")
    @MainActor
    func renamesWorkspace() async {
        let persistence = KeychainRecorder()
        let model = WireboltModel(runner: SilentRunner(), persistence: persistence)
        await model.loadWorkspace()

        #expect(await model.renameWorkspace("  Payments API  "))
        #expect(await model.renameWorkspace("   ") == false)

        #expect(model.workspace.name == "Payments API")
        #expect(await persistence.commands == [.renameWorkspace(name: "Payments API")])
    }
}
