import Foundation
import Testing
@testable import WireboltKit

@Suite struct SidebarSnapshotTests {
    private func fixture() -> CollectionDraft {
        CollectionDraft(id: "c", name: "Collection", groups: [
            GroupDraft(id: "nested", name: "Nested", parentID: "folder"),
            GroupDraft(id: "folder", name: "Folder")
        ], requests: [
            RequestLocation(collectionID: "c", groupID: "nested", request: RequestDraft(id: "inside", name: "Café 東京", url: "https://example.invalid/needle")),
            RequestLocation(collectionID: "c", request: RequestDraft(id: "root", name: "Root"))
        ])
    }

    @Test func expansionAndFilteringPreserveAncestorsAndIdentity() {
        let snapshot = SidebarSnapshot(collections: [fixture()])
        let closed = snapshot.visible(query: "", collapsedCollections: [], expandedGroups: [])
        #expect(closed.map(\.id) == ["collection:c", "group:c:folder", "request:c/root"])
        let expanded = snapshot.visible(query: "", collapsedCollections: [], expandedGroups: ["c:folder", "c:nested"])
        #expect(expanded.map(\.depth) == [0, 1, 2, 3, 1])
        for query in ["cafe", "東京", "needle", "GET", "Collection", "Folder"] {
            let matches = snapshot.visible(query: query, collapsedCollections: ["c"], expandedGroups: [])
            #expect(matches.contains { $0.id == "request:c/inside" })
            #expect(matches.first?.id == "collection:c")
            #expect(matches.map(\.id).allSatisfy { id in expanded.contains { $0.id == id } })
        }
        #expect(snapshot.visible(query: "missing", collapsedCollections: [], expandedGroups: []).isEmpty)
        #expect(snapshot.visible(query: "", collapsedCollections: ["c"], expandedGroups: []).count == 1)
    }

    @Test func tiedOrderIsDeterministicAndRootCollectionHasNoHeader() {
        var collection = fixture()
        collection.id = WorkspaceDraft.rootCollectionID
        let snapshot = SidebarSnapshot(collections: [collection])
        #expect(snapshot.rows.first?.depth == 0)
        #expect(!snapshot.rows.contains { if case .collection = $0.content { true } else { false } })
        #expect(Set(snapshot.rows.map(\.id)).count == snapshot.rows.count)
    }
}
