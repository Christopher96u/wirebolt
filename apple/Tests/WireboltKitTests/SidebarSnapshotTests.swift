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

    /// The byte prefilter must never change what the exact, canonical-equivalence match finds.
    @Test func filteringMatchesExactStringSearch() {
        let names = ["Café 東京", "Cafe\u{301} decomposed", "Kelvin \u{212A}", "Greek\u{037E}question", "a\u{20DD} circled",
                     "👨‍👩‍👧 family", "🇺🇸 flag", "Plain GET", "ÅNGSTRÖM", "naïve"]
        // A collection name no query matches, so only direct request matches are visible.
        let collection = CollectionDraft(id: "c", name: "§", requests: names.enumerated().map { index, name in
            RequestLocation(collectionID: "c", request: RequestDraft(id: "r\(index)", name: name, url: "https://example.invalid/\(index)"))
        })
        let snapshot = SidebarSnapshot(collections: [collection])
        for query in ["cafe", "café", "e", "k", "K", ";", "a", "👨", "🇺", "get", "angstrom", "naive", "ï", "東京", "example.invalid/3",
                      "missing", " cafe ", "\u{212A}", "\u{037E}"] {
            let normalized = SidebarSnapshot.normalize(query.trimmingCharacters(in: .whitespacesAndNewlines))
            let expected = snapshot.rows.filter { !$0.isContainer && $0.search.contains(normalized) }.map(\.id)
            let visible = snapshot.visible(query: query, collapsedCollections: [], expandedGroups: [])
            let requestIDs = visible.filter { !$0.isContainer }.map(\.id)
            #expect(requestIDs == expected, "query \(query)")
        }
    }

    @Test func tiedOrderIsDeterministicAndRootCollectionHasNoHeader() {
        var collection = fixture()
        collection.id = WorkspaceDraft.rootCollectionID
        let snapshot = SidebarSnapshot(collections: [collection])
        #expect(snapshot.rows.first?.depth == 0)
        #expect(!snapshot.rows.contains { if case .collection = $0.content { true } else { false } })
        #expect(Set(snapshot.rows.map(\.id)).count == snapshot.rows.count)
    }

    @Test func rowsKnowTheirParentTitleAndMoveIdentity() {
        let rows = SidebarSnapshot(collections: [fixture()]).rows
        #expect(rows.map(\.parentID) == [nil, "collection:c", "group:c:folder", "group:c:nested", "collection:c"])
        #expect(rows.map(\.title) == ["Collection", "Folder", "Nested", "Café 東京", "Root"])
        #expect(rows.map(\.moveIdentifier) == [nil, "group|c|folder", "group|c|nested", "request|c|inside", "request|c|root"])
    }

    @Test func arrowKeysVisitEveryVisibleRowAndFollowOutlineConventions() {
        let snapshot = SidebarSnapshot(collections: [fixture()])
        var expanded: Set<String> = []
        func rows() -> [SidebarSnapshot.Row] { snapshot.visible(query: "", collapsedCollections: [], expandedGroups: expanded) }
        func isExpanded(_ row: SidebarSnapshot.Row) -> Bool { row.isExpanded(collapsed: [], expanded: expanded, filtering: false) }

        // Up/Down include collections and folders, and start at an end without a selection.
        #expect(SidebarNavigation.step(from: nil, by: 1, in: rows()) == "collection:c")
        #expect(SidebarNavigation.step(from: nil, by: -1, in: rows()) == "request:c/root")
        #expect(SidebarNavigation.step(from: "collection:c", by: 1, in: rows()) == "group:c:folder")
        #expect(SidebarNavigation.step(from: "request:c/root", by: 1, in: rows()) == "request:c/root")

        // Right expands a collapsed folder, then moves to its first child.
        #expect(SidebarNavigation.right(from: "group:c:folder", in: rows(), isExpanded: isExpanded) == .expand("group:c:folder"))
        expanded.insert("c:folder")
        #expect(SidebarNavigation.right(from: "group:c:folder", in: rows(), isExpanded: isExpanded) == .select("group:c:nested"))
        expanded.insert("c:nested")
        #expect(SidebarNavigation.step(from: "group:c:nested", by: 1, in: rows()) == "request:c/inside")
        #expect(SidebarNavigation.right(from: "request:c/inside", in: rows(), isExpanded: isExpanded) == .none)

        // Left goes to the parent from a leaf and collapses an expanded container.
        #expect(SidebarNavigation.left(from: "request:c/inside", in: rows(), isExpanded: isExpanded) == .select("group:c:nested"))
        #expect(SidebarNavigation.left(from: "group:c:nested", in: rows(), isExpanded: isExpanded) == .collapse("group:c:nested"))
        expanded.remove("c:nested")
        #expect(SidebarNavigation.left(from: "group:c:nested", in: rows(), isExpanded: isExpanded) == .select("group:c:folder"))
        #expect(SidebarNavigation.left(from: "collection:c", in: rows(), isExpanded: isExpanded) == .collapse("collection:c"))
    }

    @Test func typeSelectMatchesTitlesAndCyclesOnRepeatedLetters() {
        var collection = fixture()
        collection.requests.append(RequestLocation(collectionID: "c", order: 1, request: RequestDraft(id: "second", name: "Reports")))
        let rows = SidebarSnapshot(collections: [collection]).visible(query: "", collapsedCollections: [], expandedGroups: ["c:folder", "c:nested"])
        #expect(SidebarNavigation.typeSelect("r", from: nil, in: rows) == "request:c/root")
        #expect(SidebarNavigation.typeSelect("r", from: "request:c/root", in: rows) == "request:c/second")
        #expect(SidebarNavigation.typeSelect("r", from: "request:c/second", in: rows) == "request:c/root")
        #expect(SidebarNavigation.typeSelect("rep", from: "request:c/root", in: rows) == "request:c/second")
        #expect(SidebarNavigation.typeSelect("cafe", from: nil, in: rows) == "request:c/inside")
        #expect(SidebarNavigation.typeSelect("zzz", from: nil, in: rows) == nil)
    }

    @Test func subtreeAndOutlineListParentsBeforeChildren() {
        let collection = fixture()
        #expect(collection.subtreeGroups(of: "folder").map(\.id) == ["folder", "nested"])
        #expect(collection.subtreeGroups(of: nil).map(\.id) == ["folder", "nested"])
        #expect(collection.folderOutline().map { "\($0.group.id):\($0.depth)" } == ["folder:0", "nested:1"])
    }

    @Test func siblingsSkipDescendantsAndStayInTheirContainer() {
        var collection = fixture()
        collection.requests.append(RequestLocation(collectionID: "c", order: 1, request: RequestDraft(id: "last", name: "Last")))
        let snapshot = SidebarSnapshot(collections: [collection, CollectionDraft(id: "d", name: "D", order: 1,
            requests: [RequestLocation(collectionID: "d", request: RequestDraft(id: "other"))])])
        #expect(snapshot.sibling(of: "request:c/root", by: -1)?.id == "group:c:folder")
        #expect(snapshot.sibling(of: "group:c:folder", by: 1)?.id == "request:c/root")
        #expect(snapshot.sibling(of: "request:c/last", by: 1) == nil)
        #expect(snapshot.sibling(of: "group:c:nested", by: -1) == nil)
        #expect(snapshot.sibling(of: "request:d/other", by: -1) == nil)
    }
}

