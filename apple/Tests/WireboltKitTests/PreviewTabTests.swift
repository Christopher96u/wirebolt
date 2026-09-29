import Foundation
import Testing
@testable import WireboltKit

@Suite("Preview tabs")
struct PreviewTabTests {
    private static func location(_ id: String) -> RequestLocation {
        RequestLocation(collectionID: "api", request: RequestDraft(id: id, name: id.uppercased(), url: "https://example.com/\(id)"))
    }

    @Test("Single clicks replace the untouched preview tab in place")
    @MainActor func previewReplacesInPlace() {
        let store = DocumentSessionStore()
        let pinned = store.open(draft: RequestDraft(id: "pinned"), collectionID: "api")
        let first = store.openPreview(draft: Self.location("a").request, collectionID: "api")
        let second = store.openPreview(draft: Self.location("b").request, collectionID: "api")
        #expect(store.activeGroup?.tabIDs == [pinned.id, second.id])
        #expect(store.activeSession?.id == second.id)
        #expect(store.isPreview(tabID: second.id))
        #expect(!store.isPreview(tabID: pinned.id))
        #expect(store.session(id: first.id) == nil)
        #expect(store.activeGroup?.backwardTabIDs.contains(first.id) == false)
    }

    @Test("Editing or sending a preview tab keeps it open")
    @MainActor func touchingPins() {
        let store = DocumentSessionStore()
        let edited = store.openPreview(draft: Self.location("a").request, collectionID: "api")
        edited.draft.url = "https://example.com/edited"
        #expect(!store.isPreview(tabID: edited.id))
        let sent = store.openPreview(draft: Self.location("b").request, collectionID: "api")
        #expect(store.activeGroup?.tabIDs == [edited.id, sent.id])
        sent.beginRun(RunID())
        let third = store.openPreview(draft: Self.location("c").request, collectionID: "api")
        #expect(store.activeGroup?.tabIDs == [edited.id, sent.id, third.id])
        #expect(store.isPreview(tabID: third.id))
    }

    @Test("Writing an unchanged value does not keep a preview tab")
    @MainActor func unchangedWriteStaysPreview() {
        let store = DocumentSessionStore()
        let session = store.openPreview(draft: Self.location("a").request, collectionID: "api")
        session.draft.url = session.draft.url
        #expect(store.isPreview(tabID: session.id))
    }

    @Test("Keep Open pins the preview tab; an already open request is only selected")
    @MainActor func pinAndReselect() {
        let store = DocumentSessionStore()
        let first = store.openPreview(draft: Self.location("a").request, collectionID: "api")
        store.pin(tabID: first.id)
        #expect(!store.isPreview(tabID: first.id))
        let second = store.openPreview(draft: Self.location("b").request, collectionID: "api")
        #expect(store.activeGroup?.tabIDs == [first.id, second.id])
        let again = store.openPreview(draft: Self.location("a").request, collectionID: "api")
        #expect(again.id == first.id)
        #expect(store.activeGroup?.tabIDs == [first.id, second.id])
        #expect(store.isPreview(tabID: second.id))
    }

    @Test("Closing the preview tab clears it; explicit opens are never previews")
    @MainActor func closeAndExplicitOpen() {
        let store = DocumentSessionStore()
        let preview = store.openPreview(draft: Self.location("a").request, collectionID: "api")
        store.close(.one(preview.id))
        #expect(store.activeGroup?.previewTabID == nil)
        let explicit = store.open(draft: Self.location("b").request, collectionID: "api")
        #expect(!store.isPreview(tabID: explicit.id))
        let next = store.openPreview(draft: Self.location("c").request, collectionID: "api")
        #expect(store.activeGroup?.tabIDs == [explicit.id, next.id])
    }

    @Test("Model selection previews on request and pins by default")
    @MainActor func modelSelection() {
        let model = WireboltModel(runner: PreviewRunner())
        model.select(Self.location("a"), preview: true)
        model.select(Self.location("b"), preview: true)
        #expect(model.sessions.activeGroup?.tabIDs.count == 1)
        model.select(Self.location("c"))
        #expect(model.sessions.activeGroup?.tabIDs.count == 2)
        #expect(model.sessions.activeSession.map { model.sessions.isPreview(tabID: $0.id) } == false)
    }

    @Test("The persisted layout remembers which tab is the preview")
    @MainActor func layoutRoundTrip() throws {
        let store = DocumentSessionStore()
        let pinned = store.open(draft: Self.location("a").request, collectionID: "api")
        _ = store.openPreview(draft: Self.location("b").request, collectionID: "api")
        store.select(tabID: pinned.id)
        let layout = store.layout
        #expect(layout.groups.first?.previewIndex == 1)
        let data = try JSONEncoder().encode(layout)
        let decoded = try JSONDecoder().decode(SessionLayout.self, from: data)

        let restored = DocumentSessionStore()
        let locations = ["a": Self.location("a"), "b": Self.location("b")]
        restored.restore(decoded) { locations[$0.requestID] }
        let tabIDs = try #require(restored.activeGroup?.tabIDs)
        #expect(tabIDs.count == 2)
        #expect(!restored.isPreview(tabID: tabIDs[0]))
        #expect(restored.isPreview(tabID: tabIDs[1]))
        #expect(restored.activeSession?.id == tabIDs[0])
    }

    @Test("Layouts saved before preview tabs decode with every tab kept open")
    @MainActor func legacyLayoutDecodes() throws {
        let legacy = #"{"groups":[{"tabs":[{"collectionID":"api","requestID":"a"}],"selectedIndex":0}],"activeGroupIndex":0}"#
        let layout = try JSONDecoder().decode(SessionLayout.self, from: Data(legacy.utf8))
        #expect(layout.groups.first?.previewIndex == nil)
    }
}

private struct PreviewRunner: RequestRunner {
    func events(for _: RunInput, runID _: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { $0.finish() }
    }

    func cancel(runID _: RunID) {}
}
