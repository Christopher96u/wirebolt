import Foundation
import WireboltStreamFFI

@main
enum BridgeSmoke {
    static func main() async {
        let handshake = coreHandshake()
        let streamABIVersion = wirebolt_stream_abi_version()

        guard handshake.product == "Wirebolt" else {
            fatalError("unexpected product")
        }
        guard handshake.streamAbiVersion == streamABIVersion else {
            fatalError("bridge versions disagree")
        }
        // A control operation on the UniFFI surface, not the streaming C ABI.
        resetHttpEngines()

        let runner = RustRequestRunner()
        let draft = RequestDraft(url: "not an absolute URL")
        do {
            for try await _ in runner.events(for: RunInput(draft: draft, variables: [:])) {}
            fatalError("invalid request unexpectedly succeeded")
        } catch let failure as RunFailure {
            guard failure.kind == "invalid_request",
                  failure.issues.first?.path == "url"
            else {
                fatalError("unexpected structured bridge failure")
            }
        } catch {
            fatalError("unexpected bridge error: \(error)")
        }

        let temporaryWorkspace = FileManager.default.temporaryDirectory
            .appending(path: "wirebolt-open-smoke-\(UUID().uuidString)", directoryHint: .isDirectory)
        defer { try? FileManager.default.removeItem(at: temporaryWorkspace) }
        do {
            let bridge = try WorkspaceBridge.openOrCreate(
                path: temporaryWorkspace.path,
                name: "Existing"
            )
            try bridge.saveCollection(id: "custom", name: "Custom")
            let persistence = try RustWorkspacePersistence(path: temporaryWorkspace, mode: .open)
            let workspace = try await persistence.load()
            guard workspace.collections.map(\.id) == ["custom"] else {
                fatalError("opening a workspace mutated its collections")
            }
        } catch {
            fatalError("workspace open smoke failed: \(error)")
        }

        print("bridge_smoke=passed core=\(handshake.coreVersion) stream_abi=\(streamABIVersion)")
    }
}
