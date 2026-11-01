import Foundation

/// Native outline projection. Rust builds and filters the index once per workspace revision.
public struct SidebarSnapshot: Sendable {
    public struct Row: Identifiable, Sendable {
        public enum Content: Sendable {
            case collection(CollectionDraft)
            case group(CollectionDraft, GroupDraft)
            case request(RequestLocation)
        }
        public let id: String
        public let depth: Int
        public let parentID: String?
        private let storage: Storage
        private final class Storage: Sendable {
            let metadata: SidebarRow
            let content: Content
            init(metadata: SidebarRow, content: Content) {
                self.metadata = metadata
                self.content = content
            }
        }
        init(metadata: SidebarRow, content: Content) {
            id = metadata.id
            depth = Int(metadata.depth)
            parentID = metadata.parentId
            storage = Storage(metadata: metadata, content: content)
        }
        var metadata: SidebarRow { storage.metadata }
        public var workspaceMetadata: SidebarRow { storage.metadata }
        public var content: Content { storage.content }
        var ancestors: [Int] { metadata.ancestors.map(Int.init) }
        var search: String { metadata.search }
        public var requestPath: String { metadata.requestPath }
        public var title: String { metadata.title }
        var isCollectionHeader: Bool { metadata.groupIndex == nil && metadata.requestIndex == nil }
        public var isContainer: Bool { metadata.requestIndex == nil }
        var collectionID: String { metadata.collectionId }
        public var moveIdentifier: String? { metadata.moveIdentifier }
        /// Expansion is presentation state; metadata keeps rendering free of bridge calls.
        public func isExpanded(collapsed: Set<String>, expanded: Set<String>, filtering: Bool) -> Bool {
            guard isContainer else { return false }
            return filtering || (isCollectionHeader ? !collapsed.contains(collectionID) : expanded.contains(String(id.dropFirst(6))))
        }
    }
    public let rows: [Row]
    private let index: DomainSidebar
    public init(collections: [CollectionDraft]) {
        let content = collections.map { ($0, $0.groups, $0.requests) }
        index = DomainSidebar(
            collections: content.map { collection, groups, requests in
                DomainSidebarCollection(
                    id: collection.id, name: collection.name, order: Int64(collection.order), groups: groups.map(\.domain),
                    requests: requests.map { location in
                        let source = location.domain
                        return DomainSidebarRequest(
                            id: source.request.id, collectionId: source.collectionId, groupId: source.groupId, order: source.order,
                            name: source.request.name, method: source.request.method, url: source.request.url)
                    })
            })
        rows = index.rows().map { metadata in
            let (collection, groups, requests) = content[Int(metadata.collectionIndex)]
            let rowContent: Row.Content
            if let request = metadata.requestIndex {
                rowContent = .request(requests[Int(request)])
            } else if let group = metadata.groupIndex {
                rowContent = .group(collection, groups[Int(group)])
            } else {
                rowContent = .collection(collection)
            }
            return Row(metadata: metadata, content: rowContent)
        }
    }
    public static func requestPath(from url: String) -> String { domainRequestPath(url: url) }
    public func visible(query: String, collapsedCollections: Set<String>, expandedGroups: Set<String>) -> [Row] {
        index.visible(query: query, collapsed: Array(collapsedCollections), expanded: Array(expandedGroups)).map { rows[Int($0)] }
    }
    public func canMove(_ identifier: String, by delta: Int) -> Bool {
        guard let id = workspaceMoveRowId(identifier: identifier) else { return false }
        return index.sibling(id: id, delta: Int64(delta)) != nil
    }
    public func sibling(of rowID: String, by delta: Int) -> Row? {
        index.sibling(id: rowID, delta: Int64(delta)).map { rows[Int($0)] }
    }
    static func normalize(_ text: String) -> String { domainNormalizeSearch(text: text) }
    static func foldedForComparison(_ text: String) -> String { domainNormalizeSearch(text: text) }
}
public enum SidebarNavigation {
    public enum Outcome: Equatable, Sendable {
        case select(String), expand(String), collapse(String), none
        init(domain: SidebarOutcome) {
            self =
                switch domain {
                case .select(let id): .select(id)
                case .expand(let id): .expand(id)
                case .collapse(let id): .collapse(id)
                case .none: .none
                }
        }
    }
    public static func step(from current: String?, by delta: Int, in rows: [SidebarSnapshot.Row]) -> String? {
        domainSidebarStep(rows: rows.map(\.metadata), current: current, delta: Int64(delta))
    }
    public static func left(from current: String?, in rows: [SidebarSnapshot.Row], isExpanded: (SidebarSnapshot.Row) -> Bool) -> Outcome {
        let expanded = current.flatMap { id in rows.first { $0.id == id } }.map(isExpanded) ?? false
        return Outcome(domain: domainSidebarArrow(rows: rows.map(\.metadata), current: current, right: false, expanded: expanded))
    }
    public static func right(from current: String?, in rows: [SidebarSnapshot.Row], isExpanded: (SidebarSnapshot.Row) -> Bool) -> Outcome {
        let expanded = current.flatMap { id in rows.first { $0.id == id } }.map(isExpanded) ?? false
        return Outcome(domain: domainSidebarArrow(rows: rows.map(\.metadata), current: current, right: true, expanded: expanded))
    }
    public static func typeSelect(_ prefix: String, from current: String?, in rows: [SidebarSnapshot.Row]) -> String? {
        domainSidebarTypeSelect(rows: rows.map(\.metadata), current: current, prefix: prefix)
    }
}
extension CollectionDraft {
    public func orderedChildren(parentID: String?) -> [String] { domainOrderedChildren(collection: domain, parent: parentID) }
    public func subtreeGroups(of rootID: String?) -> [GroupDraft] {
        let groups = groups
        return domainSubtree(collection: domain, root: rootID).map { groups[Int($0)] }
    }
    public func folderOutline() -> [(group: GroupDraft, depth: Int)] {
        let groups = groups
        return domainFolderOutline(collection: domain).map { (groups[Int($0[0])], Int($0[1])) }
    }
}
