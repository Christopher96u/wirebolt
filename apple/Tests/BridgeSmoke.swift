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
            for try await _ in runner.events(
                for: RunInput(draft: draft, variables: [:]),
                runID: RunID()
            ) {}
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
            let proxy = ProxyDocument.manual(routes: [ProxyRouteDocument(destination: "all", endpoint: "http://localhost:8080")])
            _ = try await persistence.apply(.saveWorkspaceProxy(proxy))
            let withProxy = try await persistence.load()
            let normalizedProxy = try ProxyFormDraft(configuration: withProxy.proxy).document()
            precondition(normalizedProxy == proxy, "workspace proxy must cross the Swift/Rust persistence bridge")
            let request = RequestDraft(id: "proxy-request", name: "Proxy override", url: "http://localhost:18765/json", note: "# Markdown\n\n**Bold** and `code`\n\n- café 🚀", proxy: .direct)
            try await persistence.save(request: request, in: "custom")
            let savedRequest = try await persistence.load().collections[0].requests[0].request
            precondition(savedRequest.note == request.note, "Markdown source must survive TOML round-trip unchanged")
            precondition(savedRequest.proxy == .direct, "request override must survive disk round-trip")
            _ = try await persistence.apply(.createGroup(collectionID: "custom", group: GroupDraft(id: "folder", name: "Folder")))
            let second = RequestLocation(collectionID: "custom", order: 0, request: RequestDraft(id: "second", name: "Second"))
            _ = try await persistence.apply(.saveRequest(collectionID: "custom", location: second))
            _ = try await persistence.apply(.reorderChildren(collectionID: "custom", parentID: nil,
                items: ["request:second", "group:folder", "request:proxy-request"]))
            let reopened = try RustWorkspacePersistence(path: temporaryWorkspace, mode: .open)
            let reordered = try await reopened.load()
            precondition(reordered.collections[0].orderedChildren(parentID: nil) == ["request:second", "group:folder", "request:proxy-request"], "mixed sibling order must survive reopening through Swift/Rust")
            _ = try await persistence.apply(.saveWorkspaceProxy(nil))
            let inherited = try await persistence.load()
            precondition(inherited.proxy == nil, "resetting workspace proxy must restore inheritance")

        } catch {
            fatalError("workspace open smoke failed: \(error)")
        }

        print("bridge_smoke=passed core=\(handshake.coreVersion) stream_abi=\(streamABIVersion)")
    }
}
