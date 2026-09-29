import Foundation
import Testing
@testable import WireboltKit

@Suite("Workspace undo")
@MainActor
struct WorkspaceUndoTests {
    private func makeModel(fails: Bool = false) -> (WireboltModel, UndoRecorder, UndoManager) {
        let persistence = UndoRecorder(fails: fails)
        let model = WireboltModel(runner: UndoStubRunner(), persistence: persistence)
        let undoManager = UndoManager()
        undoManager.groupsByEvent = false
        model.undoManager = undoManager
        return (model, persistence, undoManager)
    }

    private func undo(_ model: WireboltModel, _ undoManager: UndoManager) async {
        undoManager.undo()
        await model.finishUndoWork()
    }

    private func redo(_ model: WireboltModel, _ undoManager: UndoManager) async {
        undoManager.redo()
        await model.finishUndoWork()
    }

    private var nested: CollectionDraft {
        CollectionDraft(id: "api", name: "API", order: 3, groups: [
            GroupDraft(id: "child", name: "Child", parentID: "parent", order: 1),
            GroupDraft(id: "parent", name: "Parent", order: 0),
            GroupDraft(id: "grandchild", name: "Grandchild", parentID: "child", order: 0),
        ], requests: [
            RequestLocation(collectionID: "api", groupID: "grandchild", order: 4, request: RequestDraft(id: "deep", name: "Deep", url: "https://example.invalid/deep")),
            RequestLocation(collectionID: "api", groupID: "parent", order: 7, request: RequestDraft(id: "shallow", name: "Shallow")),
            RequestLocation(collectionID: "api", order: 2, request: RequestDraft(id: "root", name: "Root")),
        ])
    }

    private func sorted(_ collection: CollectionDraft?) -> CollectionDraft? {
        guard var collection else { return nil }
        collection.groups.sort { $0.id < $1.id }
        collection.requests.sort { $0.request.id < $1.request.id }
        return collection
    }

    @Test("Undoing a folder deletion restores its subtree with the same IDs, parents and order")
    func undoFolderDeletion() async throws {
        let (model, persistence, undoManager) = makeModel()
        model.workspace.collections = [nested]
        await model.deleteGroup(collectionID: "api", id: "parent")
        #expect(model.workspace.collections[0].groups.isEmpty)
        #expect(model.workspace.collections[0].requests.map(\.request.id) == ["root"])
        #expect(undoManager.undoActionName == "Delete “Parent”")

        await undo(model, undoManager)
        #expect(sorted(model.workspace.collections.first) == sorted(nested))
        let commands = await persistence.commands
        // Parents are recreated before children so persistence accepts every folder.
        let created = commands.compactMap { command -> String? in
            if case let .createGroup(_, group) = command { group.id } else { nil }
        }
        #expect(created == ["parent", "child", "grandchild"])
        #expect(commands.filter { if case .saveRequest = $0 { true } else { false } }.count == 2)
        #expect(undoManager.redoActionName == "Delete “Parent”")

        await redo(model, undoManager)
        #expect(model.workspace.collections[0].groups.isEmpty)
        #expect(undoManager.canUndo)
        await undo(model, undoManager)
        #expect(sorted(model.workspace.collections.first) == sorted(nested))
    }

    @Test("Undoing a collection deletion restores its folders and requests")
    func undoCollectionDeletion() async throws {
        let (model, persistence, undoManager) = makeModel()
        model.workspace.collections = [nested]
        await model.deleteCollection(id: "api")
        #expect(model.workspace.collections.isEmpty)
        await undo(model, undoManager)
        #expect(sorted(model.workspace.collections.first) == sorted(nested))
        #expect(await persistence.commands.first == .deleteCollection(id: "api"))
        #expect(await persistence.commands.dropFirst().first == .createCollection(CollectionDraft(id: "api", name: "API", order: 3)))
        #expect(model.requestMatches(nested.requests[0], normalizedQuery: "deep"))
    }

    @Test("Undoing a request deletion restores the saved definition; redo deletes it again")
    func undoRequestDeletion() async throws {
        let (model, persistence, undoManager) = makeModel()
        model.workspace.collections = [nested]
        let deep = nested.requests[0]
        await model.deleteRequest(collectionID: "api", requestID: "deep")
        #expect(model.workspace.location(collectionID: "api", requestID: "deep") == nil)
        await undo(model, undoManager)
        #expect(model.workspace.location(collectionID: "api", requestID: "deep") == deep)
        #expect(await persistence.commands.last == .saveRequest(collectionID: "api", location: deep))
        await redo(model, undoManager)
        #expect(model.workspace.location(collectionID: "api", requestID: "deep") == nil)
        #expect(await persistence.commands.last == .deleteRequest(collectionID: "api", id: "deep"))
    }

    @Test("Renames of requests, folders and collections undo to the previous name")
    func undoRenames() async throws {
        let (model, _, undoManager) = makeModel()
        model.workspace.collections = [nested]
        model.select(nested.requests[2])
        let tab = try #require(model.sessions.activeSession)
        await model.renameRequest(collectionID: "api", requestID: "root", name: "Renamed")
        await model.renameGroup(collectionID: "api", id: "child", name: "Renamed Folder")
        await model.renameCollection(id: "api", name: "Renamed API")
        #expect(tab.title == "Renamed")
        await undo(model, undoManager)
        await undo(model, undoManager)
        await undo(model, undoManager)
        #expect(sorted(model.workspace.collections.first) == sorted(nested))
        #expect(tab.title == "Root")
        #expect(!tab.isDirty)
        await redo(model, undoManager)
        #expect(model.workspace.location(collectionID: "api", requestID: "root")?.request.name == "Renamed")
    }

    @Test("Creating requests, folders and collections undoes by removing them")
    func undoCreation() async throws {
        let (model, _, undoManager) = makeModel()
        model.workspace.collections = [nested]
        let session = try #require(await model.createRequest(collectionID: "api", groupID: "parent"))
        #expect(undoManager.undoActionName == "New Request")
        await undo(model, undoManager)
        #expect(model.workspace.location(collectionID: "api", requestID: session.requestID) == nil)
        #expect(model.sessions.session(id: session.id) == nil)
        await redo(model, undoManager)
        #expect(model.workspace.location(collectionID: "api", requestID: session.requestID)?.groupID == "parent")

        let folder = try #require(await model.createGroup(collectionID: "api", parentID: "child", name: "Folder"))
        await undo(model, undoManager)
        #expect(!model.workspace.collections[0].groups.contains { $0.id == folder })

        await model.createCollection(name: "Payments")
        #expect(model.workspace.collections.count == 2)
        await undo(model, undoManager)
        #expect(model.workspace.collections.map(\.id) == ["api"])

        await model.duplicateRequest(collectionID: "api", requestID: "root")
        #expect(model.workspace.collections[0].requests.contains { $0.request.name == "Root Copy" })
        await undo(model, undoManager)
        #expect(!model.workspace.collections[0].requests.contains { $0.request.name == "Root Copy" })
    }

    @Test("Moves across collections and folders undo to the original location and order")
    func undoMoves() async throws {
        let (model, _, undoManager) = makeModel()
        model.workspace.collections = [nested, CollectionDraft(id: "other", name: "Other")]
        model.select(nested.requests[0])
        let tab = try #require(model.sessions.activeSession)
        await model.moveSidebarItem("request|api|deep", toCollectionID: "other", parentID: nil)
        #expect(tab.collectionID == "other")
        #expect(undoManager.undoActionName == "Move “Deep”")
        await model.moveSidebarItem("group|api|child", toCollectionID: "api", parentID: nil)
        #expect(model.workspace.collections[0].groups.first { $0.id == "child" }?.parentID == nil)
        await undo(model, undoManager)
        await undo(model, undoManager)
        #expect(sorted(model.workspace.collections.first) == sorted(nested))
        #expect(model.workspace.collections[1].requests.isEmpty)
        #expect(tab.collectionID == "api")
    }

    @Test("Move Up and Move Down reorder siblings persistently and undo to the previous order")
    func moveUpAndDown() async throws {
        let (model, persistence, undoManager) = makeModel()
        model.workspace.collections = [nested]
        #expect(model.workspace.collections[0].orderedChildren(parentID: nil) == ["group:parent", "request:root"])
        #expect(!model.canMoveSidebarItem("group|api|parent", by: -1))
        #expect(model.canMoveSidebarItem("group|api|parent", by: 1))
        await model.moveSidebarItem("group|api|parent", by: 1)
        #expect(model.workspace.collections[0].orderedChildren(parentID: nil) == ["request:root", "group:parent"])
        #expect(await persistence.commands.last == .reorderChildren(collectionID: "api", parentID: nil, items: ["request:root", "group:parent"]))
        #expect(!model.canMoveSidebarItem("group|api|parent", by: 1))
        await undo(model, undoManager)
        #expect(model.workspace.collections[0].orderedChildren(parentID: nil) == ["group:parent", "request:root"])
        await redo(model, undoManager)
        #expect(model.workspace.collections[0].orderedChildren(parentID: nil) == ["request:root", "group:parent"])
    }

    @Test("Move To rejects a folder's own subtree, other collections and its current place")
    func moveToValidation() {
        let (model, _, _) = makeModel()
        model.workspace.collections = [nested, CollectionDraft(id: "other", name: "Other")]
        #expect(!model.canMoveSidebarItem("group|api|parent", toCollectionID: "api", parentID: "grandchild"))
        #expect(!model.canMoveSidebarItem("group|api|parent", toCollectionID: "other", parentID: nil))
        #expect(!model.canMoveSidebarItem("group|api|parent", toCollectionID: "api", parentID: nil))
        #expect(model.canMoveSidebarItem("group|api|grandchild", toCollectionID: "api", parentID: "parent"))
        #expect(!model.canMoveSidebarItem("request|api|root", toCollectionID: "api", parentID: nil))
        #expect(model.canMoveSidebarItem("request|api|root", toCollectionID: "other", parentID: nil))
        #expect(!model.canMoveSidebarItem("request|api|root", toCollectionID: "api", parentID: "missing"))
    }

    @Test("A failed undo reports the failure and clears the stack instead of guessing")
    func failedUndoClearsStack() async throws {
        let persistence = UndoRecorder()
        let model = WireboltModel(runner: UndoStubRunner(), persistence: persistence)
        let undoManager = UndoManager()
        undoManager.groupsByEvent = false
        model.undoManager = undoManager
        model.workspace.collections = [nested]
        await model.deleteRequest(collectionID: "api", requestID: "root")
        await persistence.setFails(true)
        await undo(model, undoManager)
        #expect(model.operationFailure?.kind == "workspace")
        #expect(!undoManager.canUndo && !undoManager.canRedo)
        #expect(model.workspace.location(collectionID: "api", requestID: "root") == nil)
    }

    @Test("Failed mutations register nothing to undo")
    func failedMutationRegistersNothing() async {
        let (model, _, undoManager) = makeModel(fails: true)
        model.workspace.collections = [nested]
        await model.deleteGroup(collectionID: "api", id: "parent")
        #expect(!undoManager.canUndo)
        #expect(model.workspace.collections == [nested])
    }
}

private struct UndoStubRunner: RequestRunner {
    func events(for _: RunInput, runID _: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { $0.finish() }
    }
    func cancel(runID _: RunID) {}
}

private actor UndoRecorder: WorkspacePersistence {
    private var fails: Bool
    private(set) var commands: [WorkspaceCommand] = []

    init(fails: Bool = false) { self.fails = fails }

    func setFails(_ value: Bool) { fails = value }
    func load() async throws -> WorkspaceDraft { WorkspaceDraft(name: "Undo") }
    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}

    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta {
        if fails { throw RunFailure(kind: "workspace", issues: []) }
        commands.append(command)
        return WorkspaceDelta(version: UInt64(commands.count), kind: .collection, affectedIDs: [])
    }
}
