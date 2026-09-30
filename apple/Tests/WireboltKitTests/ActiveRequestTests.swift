import Foundation
import Observation
import Testing
@testable import WireboltKit

@Suite("Active request flags")
struct ActiveRequestTests {
    @MainActor private final class Changes {
        var count = 0
    }

    /// Counts changes to one request's flag, as a sidebar row observes it.
    @MainActor private func observe(_ store: DocumentSessionStore, _ requestID: String, into changes: Changes) {
        withObservationTracking {
            _ = store.isActiveRequest(collectionID: "api", requestID: requestID)
        } onChange: {
            Task { @MainActor in changes.count += 1 }
        }
    }

    @Test("Each saved request reports whether it is the active document")
    @MainActor func tracksSelection() {
        let store = DocumentSessionStore()
        #expect(!store.isActiveRequest(collectionID: "api", requestID: "a"))
        let a = store.open(draft: RequestDraft(id: "a"), collectionID: "api")
        #expect(store.isActiveRequest(collectionID: "api", requestID: "a"))
        let b = store.open(draft: RequestDraft(id: "b"), collectionID: "api")
        #expect(!store.isActiveRequest(collectionID: "api", requestID: "a"))
        #expect(store.isActiveRequest(collectionID: "api", requestID: "b"))
        #expect(!store.isActiveRequest(collectionID: "other", requestID: "b"))
        store.select(tabID: a.id)
        #expect(store.isActiveRequest(collectionID: "api", requestID: "a"))
        #expect(!store.isActiveRequest(collectionID: "api", requestID: "b"))
        // Unsaved documents select no saved request.
        _ = store.open(draft: RequestDraft(id: "scratch"), forceNewSession: true)
        #expect(!store.isActiveRequest(collectionID: "api", requestID: "a"))
        #expect(!store.isActiveRequest(collectionID: "api", requestID: "b"))
        store.select(tabID: b.id)
        #expect(store.isActiveRequest(collectionID: "api", requestID: "b"))
    }

    @Test("Moving the active request to another collection moves its selection")
    @MainActor func followsRelocation() {
        let store = DocumentSessionStore()
        let a = store.open(draft: RequestDraft(id: "a"), collectionID: "api")
        a.relocate(to: "moved")
        store.refreshActiveRequest()
        #expect(store.isActiveRequest(collectionID: "moved", requestID: "a"))
        #expect(!store.isActiveRequest(collectionID: "api", requestID: "a"))
    }

    @Test("Switching documents notifies only the requests whose state changed")
    @MainActor func notifiesOnlyChangedRows() async {
        let store = DocumentSessionStore()
        let a = store.open(draft: RequestDraft(id: "a"), collectionID: "api")
        _ = store.open(draft: RequestDraft(id: "b"), collectionID: "api")
        _ = store.isActiveRequest(collectionID: "api", requestID: "c")
        let (changesA, changesB, changesC) = (Changes(), Changes(), Changes())
        observe(store, "a", into: changesA)
        observe(store, "b", into: changesB)
        observe(store, "c", into: changesC)
        store.select(tabID: a.id)
        await Task.yield()
        await Task.yield()
        #expect(changesA.count == 1)
        #expect(changesB.count == 1)
        #expect(changesC.count == 0)
    }
}
