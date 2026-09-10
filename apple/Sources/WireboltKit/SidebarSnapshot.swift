import Foundation

/// A workspace's ordered tree, shared by rendering, filtering and keyboard navigation.
public struct SidebarSnapshot: Sendable {
    public struct Row: Identifiable, Sendable {
        public enum Content: Sendable {
            case collection(CollectionDraft)
            case group(CollectionDraft, GroupDraft)
            case request(RequestLocation)
        }
        public let id: String
        public let depth: Int
        public let content: Content
        let ancestors: [Int]
        let search: String
    }
    public let rows: [Row]

    public init(collections: [CollectionDraft]) {
        var rows: [Row] = []
        for collection in collections.sorted(by: { ($0.order, $0.name, $0.id) < ($1.order, $1.name, $1.id) }) {
            var ancestors: [Int] = []
            if collection.id != WorkspaceDraft.rootCollectionID {
                ancestors = [rows.count]
                rows.append(Row(id: "collection:" + collection.id, depth: 0, content: .collection(collection), ancestors: [], search: Self.normalize(collection.name)))
            }
            let groups = Dictionary(grouping: collection.groups, by: { $0.parentID ?? "" })
            let requests = Dictionary(grouping: collection.requests, by: { $0.groupID ?? "" })
            var visited = Set<String>()
            func append(parent: String, ancestors: [Int]) {
                let children: [(Int, Int, String, String, Row.Content)] = (groups[parent] ?? []).map { ($0.order, 0, $0.name, $0.id, .group(collection, $0)) }
                    + (requests[parent] ?? []).map { ($0.order, 1, $0.request.name, $0.id, .request($0)) }
                for (_, _, name, id, content) in children.sorted(by: { ($0.0, $0.1, $0.2, $0.3) < ($1.0, $1.1, $1.2, $1.3) }) {
                    let rowID: String
                    let search: String
                    switch content {
                    case .group(_, let group):
                        guard visited.insert(group.id).inserted else { continue }
                        rowID = "group:" + collection.id + ":" + id
                        search = name
                    case .request(let location):
                        rowID = "request:" + id
                        search = [name, location.request.method.rawValue, location.request.url].joined(separator: "\u{0}")
                    case .collection: continue
                    }
                    let position = rows.count
                    rows.append(Row(id: rowID, depth: ancestors.count, content: content, ancestors: ancestors, search: Self.normalize(search)))
                    if case .group(_, let group) = content { append(parent: group.id, ancestors: ancestors + [position]) }
                }
            }
            append(parent: "", ancestors: ancestors)
        }
        self.rows = rows
    }

    public func visible(query: String, collapsedCollections: Set<String>, expandedGroups: Set<String>) -> [Row] {
        let query = Self.normalize(query.trimmingCharacters(in: .whitespacesAndNewlines))
        if !query.isEmpty {
            var included = Array(repeating: false, count: rows.count)
            var matching = Set<Int>()
            for (index, row) in rows.enumerated() {
                if row.search.contains(query) || row.ancestors.contains(where: { matching.contains($0) }) {
                    matching.insert(index)
                    included[index] = true
                    for parent in row.ancestors { included[parent] = true }
                }
            }
            return rows.enumerated().compactMap { included[$0.offset] ? $0.element : nil }
        }
        var hidden = Set<Int>()
        var result: [Row] = []
        for (index, row) in rows.enumerated() {
            if row.ancestors.contains(where: { hidden.contains($0) }) { continue }
            result.append(row)
            switch row.content {
            case .collection(let collection): if collapsedCollections.contains(collection.id) { hidden.insert(index) }
            case .group(let collection, let group): if !expandedGroups.contains(collection.id + ":" + group.id) { hidden.insert(index) }
            case .request: break
            }
        }
        return result
    }

    private static func normalize(_ text: String) -> String {
        text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: Locale(identifier: "en_US_POSIX"))
    }
}
