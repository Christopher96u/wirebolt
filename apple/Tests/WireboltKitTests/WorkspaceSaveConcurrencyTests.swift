import Foundation
import Testing
@testable import WireboltKit

@Suite("Workspace save boundaries")
@MainActor
struct WorkspaceSaveConcurrencyTests {
    @Test("Edits made while a save awaits storage remain dirty after success or failure")
    func preservesEditsDuringSave() async {
        for fails in [false, true] {
            let persistence = SuspendedWorkspaceSave(fails: fails)
            let model = WireboltModel(runner: SaveBoundaryRunner(), persistence: persistence)
            let session = model.sessions.open(draft: RequestDraft(id: "a", url: "https://example.com/original"), collectionID: "api")
            session.draft.url = "https://example.com/saved"
            let saving = Task { await model.save(session, fallbackCollectionID: nil) }
            await persistence.waitUntilSaving()
            session.draft.url = "https://example.com/newer"
            await persistence.resume()
            #expect(await saving.value == !fails)
            #expect(session.draft.url == "https://example.com/newer")
            #expect(session.savedDraft?.url == (fails ? "https://example.com/original" : "https://example.com/saved"))
            #expect(session.isDirty)
        }
    }

    @Test("Save All captures later documents when their save starts")
    func capturesEachLiveDraft() async {
        let persistence = SuspendedWorkspaceSave()
        let model = WireboltModel(runner: SaveBoundaryRunner(), persistence: persistence)
        let a = model.sessions.open(draft: RequestDraft(id: "a"), collectionID: "api")
        let b = model.sessions.open(draft: RequestDraft(id: "b"), collectionID: "other")
        a.draft.note = "Save A"; b.draft.note = "Earlier B"
        let saving = Task { await model.saveAllDirtySessions() }
        await persistence.waitUntilSaving()
        a.draft.note = "Newer A"; b.draft.note = "Latest B"
        await persistence.resume()
        #expect(await saving.value)
        #expect(a.draft.note == "Newer A" && a.isDirty)
        #expect(b.draft.note == "Latest B" && !b.isDirty)
        #expect(await persistence.saved.map(\.request.note) == ["Save A", "Latest B"])
    }
}

private struct SaveBoundaryRunner: RequestRunner {
    func events(for _: RunInput, runID _: RunID) -> AsyncThrowingStream<RunEvent, any Error> { AsyncThrowingStream { $0.finish() } }
    func cancel(runID _: RunID) {}
}

private actor SuspendedWorkspaceSave: WorkspacePersistence {
    let fails: Bool
    private var started = false
    private var resumed = false
    private var waiting: CheckedContinuation<Void, Never>?
    private var writing: CheckedContinuation<Void, Never>?
    private(set) var saved: [RequestLocation] = []
    init(fails: Bool = false) { self.fails = fails }
    func load() async throws -> WorkspaceDraft { WorkspaceDraft(name: "Saves") }
    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}
    func waitUntilSaving() async {
        if started { return }
        await withCheckedContinuation { waiting = $0 }
    }
    func resume() {
        resumed = true
        writing?.resume()
        writing = nil
    }
    func apply(_ command: SessionWorkspaceCommand) async throws -> WorkspaceDelta {
        if !resumed {
            started = true
            waiting?.resume()
            waiting = nil
            await withCheckedContinuation { writing = $0 }
        }
        if fails { throw WorkspaceMutationError.unsupported }
        if case let .saveRequest(location) = command { saved.append(RequestLocation(domain: location)) }
        return WorkspaceDelta(version: UInt64(saved.count), kind: .request, affectedIDs: [])
    }
}
