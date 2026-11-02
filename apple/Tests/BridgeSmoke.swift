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
        guard handshake.streamAbiVersion == 5, streamABIVersion == WIREBOLT_STREAM_ABI_VERSION else {
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
            guard failure.kind == .invalidRequest,
                  failure.issues.first?.path == "url"
            else {
                fatalError("unexpected structured bridge failure")
            }
        } catch {
            fatalError("unexpected bridge error: \(error)")
        }

        do { try await verifySlowStreaming(runner) } catch { fatalError("streaming bridge smoke failed: \(error)") }

        let temporaryWorkspace = FileManager.default.temporaryDirectory
            .appending(path: "wirebolt-open-smoke-\(UUID().uuidString)", directoryHint: .isDirectory)
        defer { try? FileManager.default.removeItem(at: temporaryWorkspace) }
        do {
            let bridge = try WorkspaceBridge.openOrCreate(
                path: temporaryWorkspace.path,
                name: "Existing"
            )
            _ = try bridge.applySessionCommand(command: .createCollection(collection: CollectionDraft(id: "custom", name: "Custom").domain))
            let persistence = try RustWorkspacePersistence(path: temporaryWorkspace, mode: .open)
            let workspace = WorkspaceDraft(domain: try await persistence.loadResult().workspace)
            guard workspace.collections.map(\.id) == ["custom"] else {
                fatalError("opening a workspace mutated its collections")
            }
            let proxy = ProxyDocument.manual(routes: [ProxyRouteDocument(destination: .all, endpoint: "http://localhost:8080")])
            _ = try await persistence.apply(.saveWorkspaceProxy(proxy: proxy))
            let withProxy = WorkspaceDraft(domain: try await persistence.loadResult().workspace)
            let normalizedProxy = try ProxyFormDraft(configuration: withProxy.proxy).document()
            precondition(normalizedProxy == proxy, "workspace proxy must cross the Swift/Rust persistence bridge")
            let request = RequestDraft(id: "proxy-request", name: "Proxy override", url: "http://localhost:18765/json", note: "# Markdown\n\n**Bold** and `code`\n\n- café 🚀", proxy: .direct)
            try await persistence.save(request: request, in: "custom")
            let savedRequest = WorkspaceDraft(domain: try await persistence.loadResult().workspace).collections[0].requests[0].request
            precondition(savedRequest.note == request.note, "Markdown source must survive TOML round-trip unchanged")
            precondition(savedRequest.proxy == .direct, "request override must survive disk round-trip")
            _ = try await persistence.apply(.createGroup(collectionId: "custom", group: GroupDraft(id: "folder", name: "Folder").domain))
            let second = RequestLocation(collectionID: "custom", order: 0, request: RequestDraft(id: "second", name: "Second"))
            _ = try await persistence.apply(.saveRequest(location: second.domain))
            _ = try await persistence.apply(.reorderChildren(collectionId: "custom", parentId: nil,
                items: ["request:second", "group:folder", "request:proxy-request"]))
            let reopened = try RustWorkspacePersistence(path: temporaryWorkspace, mode: .open)
            let reordered = WorkspaceDraft(domain: try await reopened.loadResult().workspace)
            precondition(reordered.collections[0].orderedChildren(parentID: nil) == ["request:second", "group:folder", "request:proxy-request"], "mixed sibling order must survive reopening through Swift/Rust")
            _ = try await persistence.apply(.saveWorkspaceProxy(proxy: nil))
            let inherited = WorkspaceDraft(domain: try await persistence.loadResult().workspace)
            precondition(inherited.proxy == nil, "resetting workspace proxy must restore inheritance")
            let broken = temporaryWorkspace.appending(path: "collections/custom/requests/broken.toml")
            try Data(#"name = "private-fixture"#.utf8).write(to: broken)
            let partial = try await persistence.loadResult()
            precondition(partial.diagnostics.problems.count == 1)
            precondition(partial.diagnostics.problems[0].kind == .invalidToml)
            precondition(partial.diagnostics.notice?.contains("broken.toml") == true)
            precondition(partial.diagnostics.notice?.contains("private-fixture") == false)
            precondition(partial.workspace.collections[0].requests.count == 2)
            try FileManager.default.removeItem(at: broken)
            await verifyWorkspaceOperations(persistence)

        } catch {
            fatalError("workspace open smoke failed: \(error)")
        }

        await verifyTypedSocket()
        await verifySessionBridge()

        print("bridge_smoke=passed core=\(handshake.coreVersion) stream_abi=\(streamABIVersion)")
    }
    private static func verifyTypedSocket() async {
        let (events, continuation) = AsyncStream<String?>.makeStream()
        let input = RunInput(draft: RequestDraft(url: "not a URL"), variables: [:])
        let socketState = AppSessionSocket()
        let socketGeneration = socketState.begin() ?? ""
        let socket = AppSocketConnection.start(input: input.domain, state: socketState, generation: socketGeneration, observer: SocketSmokeObserver(continuation: continuation))
        var terminals = 0
        for await reason in events {
            precondition(reason != nil, "Invalid typed WebSocket input must report preparation failure")
            terminals += 1
        }
        precondition(terminals == 1)
        socket.cancel()
    }

    @MainActor
    private static func verifyWorkspaceOperations(_ persistence: RustWorkspacePersistence) async {
        let model = WireboltModel(runner: RustRequestRunner(), persistence: persistence, gitCollaboration: persistence)
        let undo = UndoManager(); undo.groupsByEvent = false; model.undoManager = undo
        await model.loadWorkspace()
        precondition(model.hasLoadedWorkspace && !model.isLoadingWorkspace)
        guard let session = await model.createRequest(collectionID: "custom", groupID: "folder") else { fatalError("workspace command did not create request") }
        session.draft.url = "https://example.invalid/workspace-bridge"
        let saved = await model.save(session, fallbackCollectionID: nil)
        precondition(saved)
        undo.undo(); await model.finishUndoWork()
        precondition(model.workspace.location(collectionID: "custom", requestID: session.requestID) == nil)
        undo.redo(); await model.finishUndoWork()
        precondition(model.workspace.location(collectionID: "custom", requestID: session.requestID)?.request.url == "https://example.invalid/workspace-bridge")
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("wirebolt-bruno-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder) }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try #"{"name":"Typed Bruno"}"#.write(to: folder.appendingPathComponent("bruno.json"), atomically: true, encoding: .utf8)
            try "meta {\n  name: Ping\n}\nget {\n  url: https://example.invalid/bruno\n}\n".write(
                to: folder.appendingPathComponent("ping.bru"), atomically: true, encoding: .utf8)
            await model.importBrunoCollection(at: folder)
            precondition(model.importFailureMessage == nil && model.importSummary?.requestCount == 1)
            precondition(model.workspace.collections.contains {
                $0.name == "Typed Bruno" && $0.requests.first?.request.url == "https://example.invalid/bruno"
            }, "typed Bruno bundles must retain collection and request contents through persistence")
        } catch { fatalError("Bruno fixture failed: \(error)") }
    }

    @MainActor
    private static func verifySessionBridge() async {
        let store = DocumentSessionStore()
        let draft = RequestDraft(id: "session-smoke", name: "Session", url: "https://example.com")
        let first = store.openPreview(draft: draft, collectionID: "fixture")
        let replacement = store.openPreview(draft: RequestDraft(id: "replacement"), collectionID: "fixture")
        precondition(store.session(id: first.id) == nil && store.isPreview(tabID: replacement.id))
        replacement.draft.note = "edited"
        precondition(store.close(.all).map(\.id) == [replacement.id], "Rust close policy must protect dirty projections")
        let session = store.open(draft: draft, collectionID: "fixture")
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        sessionRun.sink.event(event: .head(head: ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1))); session.mirror()
        _ = sessionRun.sink.bytes(data: Data("café 🌍".utf8)); session.mirror()
        sessionRun.sink.event(event: .complete(completion: RunCompletion(bytesReceived: 10, totalTimeNS: 1))); await sessionRun.complete()
        precondition(session.responseText == "café 🌍" && !session.isRunning)
        precondition(session.bodyStore != nil && session.completion?.bytesReceived == 10)
        let stale = RunID()
        _ = sessionRun.sink.bytes(data: Data("late".utf8)); session.mirror()
        precondition(session.responseText == "café 🌍", "stale native events must not mutate the Rust run")
        store.pin(tabID: session.id)
        let restored = DocumentSessionStore()
        restored.restore(store.layout) { tab in
            RequestLocation(collectionID: tab.collectionID, request: tab.requestID == draft.id ? draft : RequestDraft(id: tab.requestID))
        }
        precondition(restored.activeSession?.requestID == draft.id, "typed session layout must cross the bridge")
    }

}

private final class SocketSmokeObserver: SessionSocketObserver {
    let continuation: AsyncStream<String?>.Continuation
    init(continuation: AsyncStream<String?>.Continuation) { self.continuation = continuation }
    func update(snapshot: SessionSocketSnapshot) -> Bool { true }
    func finish(reason: String?) {
        continuation.yield(reason)
        continuation.finish()
    }
}
