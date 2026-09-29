import Foundation
import Testing
@testable import WireboltKit

@Suite("App lifecycle")
struct LifecycleTests {
    @Test("Dirty detection lists each edited document once, in tab order")
    @MainActor func dirtySessionsInTabOrder() throws {
        let model = WireboltModel(runner: LifecycleRunner(), persistence: SaveRecorder())
        let first = model.sessions.open(draft: RequestDraft(id: "a", name: "A"), collectionID: "api")
        let second = model.sessions.open(draft: RequestDraft(id: "b", name: "B"), collectionID: "api")
        _ = model.sessions.open(draft: RequestDraft(id: "c", name: "C"), collectionID: "api")
        #expect(!model.hasUnsavedRequestChanges)
        second.draft.url = "https://example.com/b"
        first.draft.url = "https://example.com/a"
        #expect(model.dirtySessions.map(\.id) == [first.id, second.id])
    }

    @Test("Save all persists every dirty document at its own collection")
    @MainActor func saveAllPersistsEveryDirtyDocument() async throws {
        let persistence = SaveRecorder()
        let model = WireboltModel(runner: LifecycleRunner(), persistence: persistence)
        model.workspace.collections = [
            CollectionDraft(id: "api", name: "API", requests: [RequestLocation(collectionID: "api", request: RequestDraft(id: "a"))]),
            CollectionDraft(id: "other", name: "Other", requests: [RequestLocation(collectionID: "other", request: RequestDraft(id: "b"))]),
        ]
        let first = model.sessions.open(draft: RequestDraft(id: "a"), collectionID: "api")
        let second = model.sessions.open(draft: RequestDraft(id: "b"), collectionID: "other")
        first.draft.url = "https://example.com/a"
        second.draft.url = "https://example.com/b"
        #expect(await model.saveAllDirtySessions())
        #expect(!model.hasUnsavedRequestChanges)
        let saved = await persistence.saved
        #expect(saved.map(\.collectionID) == ["api", "other"])
        #expect(saved.map(\.location.request.url) == ["https://example.com/a", "https://example.com/b"])
    }

    @Test("A failed save keeps the remaining documents dirty and reports failure")
    @MainActor func failedSaveAllStops() async throws {
        let model = WireboltModel(runner: LifecycleRunner(), persistence: SaveRecorder(fails: true))
        let session = model.sessions.open(draft: RequestDraft(id: "a"), collectionID: "api")
        session.draft.url = "https://example.com"
        #expect(!(await model.saveAllDirtySessions()))
        #expect(session.isDirty)
        #expect(model.operationFailure != nil)
    }

    @Test("Discarding reverts saved documents and closes never-saved ones")
    @MainActor func discardRevertsAndCloses() throws {
        let model = WireboltModel(runner: LifecycleRunner(), persistence: SaveRecorder())
        let saved = model.sessions.open(draft: RequestDraft(id: "a", name: "A"), collectionID: "api")
        saved.draft.name = "Renamed"
        let temporary = model.sessions.openTemporary()
        #expect(model.dirtySessions.count == 2)
        model.discardUnsavedChanges()
        #expect(!model.hasUnsavedRequestChanges)
        #expect(saved.draft.name == "A")
        #expect(model.sessions.session(id: temporary.id) == nil)
        #expect(model.sessions.session(id: saved.id) != nil)
    }

    @Test("Editor layout round-trips tabs, selection and split through storage")
    @MainActor func layoutRoundTrip() async throws {
        let a = RequestLocation(collectionID: "api", request: RequestDraft(id: "a", name: "A"))
        let b = RequestLocation(collectionID: "api", request: RequestDraft(id: "b", name: "B"))
        let c = RequestLocation(collectionID: "api", request: RequestDraft(id: "c", name: "C"))
        let workspace = WorkspaceDraft(name: "Layout", collections: [CollectionDraft(id: "api", name: "API", requests: [a, b, c])])

        let original = WireboltModel(runner: LifecycleRunner(), persistence: LayoutPersistence(workspace: workspace))
        await original.loadWorkspace()
        original.select(b)
        original.select(c)
        original.select(b)
        let bTab = try #require(original.sessions.activeSession)
        _ = original.sessions.split(tabID: bTab.id)
        original.sessions.openTemporary()
        let firstGroupID = original.sessions.groups[0].id
        original.sessions.select(tabID: original.sessions.groups[0].tabIDs[2], in: firstGroupID)

        let defaults = try #require(UserDefaults(suiteName: "wirebolt.lifecycle.\(UUID().uuidString)"))
        SessionLayoutStore(defaults: defaults).save(original.sessions.layout, forWorkspace: "/tmp/workspace")
        let layout = try #require(SessionLayoutStore(defaults: defaults).layout(forWorkspace: "/tmp/workspace"))
        #expect(SessionLayoutStore(defaults: defaults).layout(forWorkspace: "/tmp/other") == nil)

        let restored = WireboltModel(runner: LifecycleRunner(), persistence: LayoutPersistence(workspace: workspace))
        await restored.loadWorkspace(restoring: layout)
        #expect(restored.sessions.groups.count == 2)
        let titles = restored.sessions.groups.map { $0.tabIDs.compactMap { restored.sessions.session(id: $0)?.requestID } }
        #expect(titles == [["a", "b", "c"], ["b"]])
        #expect(restored.sessions.activeGroupID == restored.sessions.groups[0].id)
        #expect(restored.sessions.activeSession?.requestID == "c")
        #expect(restored.sessions.groupCount == 2)
        #expect(!restored.hasUnsavedRequestChanges)
    }

    @Test("Restoring skips deleted requests and falls back to the first request")
    @MainActor func layoutRestoreFallsBack() async throws {
        let a = RequestLocation(collectionID: "api", request: RequestDraft(id: "a", name: "A"))
        let workspace = WorkspaceDraft(name: "Layout", collections: [CollectionDraft(id: "api", name: "API", requests: [a])])
        let stale = SessionLayout(groups: [.init(tabs: [.init(collectionID: "api", requestID: "gone")], selectedIndex: 0)])
        let model = WireboltModel(runner: LifecycleRunner(), persistence: LayoutPersistence(workspace: workspace))
        #expect(!model.hasLoadedWorkspace)
        await model.loadWorkspace(restoring: stale)
        #expect(model.hasLoadedWorkspace)
        #expect(model.sessions.activeSession?.requestID == "a")
        #expect(model.sessions.groups.count == 1)
    }

    @Test("Restored tabs show the latest recorded response without blocking load")
    @MainActor func restoredTabShowsLatestHistory() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "wirebolt-lifecycle-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let history = HistoryRepository(root: root)
        let body = try ResponseBodyStore(runID: RunID(), directory: FileManager.default.temporaryDirectory)
        try await body.append(Data("{\"ok\":true}".utf8))
        try await body.finish()
        let prepared = PreparedRunSnapshot(method: "GET", url: "https://example.com/a", headers: [],
            body: PreparedBodySnapshot(byteCount: 0, contentType: nil, textPreview: nil, redacted: false), transport: TransportSettings())
        try await history.record(runID: RunID(), requestID: "a", prepared: prepared,
            responseHead: ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1),
            completion: nil, failure: nil, body: body)

        let a = RequestLocation(collectionID: "api", request: RequestDraft(id: "a", name: "A"))
        let workspace = WorkspaceDraft(name: "Layout", collections: [CollectionDraft(id: "api", name: "API", requests: [a])])
        let model = WireboltModel(runner: LifecycleRunner(), persistence: LayoutPersistence(workspace: workspace), history: history)
        await model.loadWorkspace(restoring: SessionLayout(groups: [.init(tabs: [.init(collectionID: "api", requestID: "a")], selectedIndex: 0)]))
        let session = try #require(model.sessions.activeSession)
        for _ in 0 ..< 200 where session.responseHead == nil { try await Task.sleep(for: .milliseconds(5)) }
        #expect(session.responseHead?.status == 200)
        #expect(session.responseText == "{\"ok\":true}")
    }

    @Test("Menu command state changes only when availability changes")
    @MainActor func commandStateTracksAvailability() async throws {
        let model = WireboltModel(runner: LifecycleRunner())
        let commands = WorkspaceCommandState(model: model)
        #expect(!commands.hasActiveSession)
        let session = model.sessions.open(draft: RequestDraft(id: "a", name: "A"), collectionID: "api")
        await settle()
        #expect(commands.hasActiveSession)
        #expect(!commands.hasURL)
        #expect(commands.hasName)
        session.draft.url = "h"
        await settle()
        #expect(commands.hasURL)
        session.beginRun(RunID())
        await settle()
        #expect(commands.isRunning)
        #expect(commands.tabCount == 1)
    }

    @Test("Opened files are routed to the matching importer")
    func importFormatDetection() {
        #expect(ImportFormat.detect(fileExtension: "txt", contents: "  curl https://example.com") == .curl)
        #expect(ImportFormat.detect(fileExtension: "har", contents: #"{"log":{"entries":[]}}"#) == .har)
        #expect(ImportFormat.detect(fileExtension: "json", contents: #"{"info":{"schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[]}"#) == .postmanV2)
        #expect(ImportFormat.detect(fileExtension: "json", contents: #"{"version":1,"nodes":[]}"#) == .legacyWorkspaceV1)
        #expect(ImportFormat.detect(fileExtension: "json", contents: #"{"hello":"world"}"#) == nil)
        #expect(ImportFormat.detect(fileExtension: "txt", contents: "hello") == nil)
    }

    @Test("History and cookies load lazily from disk on first use")
    func lazyRuntimeStorage() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "wirebolt-lazy-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let cookies = root.appending(path: "cookies.json")
        let writer = CookieJar(storageURL: cookies)
        await writer.store(headers: [ResponseHeader(name: "Set-Cookie", value: "id=1")], requestURL: URL(string: "https://example.com")!)
        let reader = CookieJar(storageURL: cookies)
        #expect(await reader.header(for: URL(string: "https://example.com/")!) == "id=1")
        #expect(await HistoryRepository(root: root.appending(path: "missing")).list(requestID: "a").isEmpty)
    }

    @MainActor private func settle() async {
        for _ in 0 ..< 5 { await Task.yield() }
    }
}

private struct LifecycleRunner: RequestRunner {
    func events(for _: RunInput, runID _: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { $0.finish() }
    }

    func cancel(runID _: RunID) {}
}

private struct LayoutPersistence: WorkspacePersistence {
    let workspace: WorkspaceDraft
    func load() async throws -> WorkspaceDraft { workspace }
    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}
}

private actor SaveRecorder: WorkspacePersistence {
    let fails: Bool
    private(set) var saved: [(collectionID: String, location: RequestLocation)] = []

    init(fails: Bool = false) { self.fails = fails }

    func load() async throws -> WorkspaceDraft { WorkspaceDraft(name: "Saves") }
    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}

    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta {
        if fails { throw RunFailure(kind: "workspace", issues: []) }
        if case let .saveRequest(collectionID, location) = command { saved.append((collectionID, location)) }
        return WorkspaceDelta(version: UInt64(saved.count), kind: .request, affectedIDs: [])
    }
}
