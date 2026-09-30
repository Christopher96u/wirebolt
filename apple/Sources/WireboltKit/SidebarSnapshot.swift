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
        /// The enclosing folder or collection row; nil at the top level.
        public let parentID: String?
        /// Everything but the identity, shared: SwiftUI copies every row of the outline when
        /// it diffs the list, and a request row otherwise copies its whole draft each time.
        private let storage: Storage

        public var content: Content { storage.content }
        var ancestors: [Int] { storage.ancestors }
        var search: String { storage.search }

        private final class Storage: Sendable {
            let content: Content
            let ancestors: [Int]
            let search: String
            init(content: Content, ancestors: [Int], search: String) {
                self.content = content
                self.ancestors = ancestors
                self.search = search
            }
        }

        init(id: String, depth: Int, content: Content, parentID: String?, ancestors: [Int], search: String) {
            self.id = id
            self.depth = depth
            self.parentID = parentID
            storage = Storage(content: content, ancestors: ancestors, search: search)
        }

        public var title: String {
            switch content {
            case .collection(let collection): collection.name
            case .group(_, let group): group.name
            case .request(let location): location.request.name
            }
        }

        var isCollectionHeader: Bool {
            if case .collection = content { true } else { false }
        }

        public var isContainer: Bool {
            if case .request = content { false } else { true }
        }

        /// Collections are expanded unless collapsed, folders collapsed unless expanded;
        /// a filter shows every match with its ancestors expanded.
        public func isExpanded(collapsed: Set<String>, expanded: Set<String>, filtering: Bool) -> Bool {
            switch content {
            case .collection(let collection): filtering || !collapsed.contains(collection.id)
            case .group(let collection, let group): filtering || expanded.contains(collection.id + ":" + group.id)
            case .request: false
            }
        }

        var collectionID: String {
            switch content {
            case .collection(let collection): collection.id
            case .group(let collection, _): collection.id
            case .request(let location): location.collectionID
            }
        }

        /// The drag and move identifier ("kind|collection|id"); collections cannot move.
        public var moveIdentifier: String? {
            switch content {
            case .collection: nil
            case .group(let collection, let group): "group|\(collection.id)|\(group.id)"
            case .request(let location): "request|\(location.collectionID)|\(location.request.id)"
            }
        }
    }
    public let rows: [Row]

    public init(collections: [CollectionDraft]) {
        var rows: [Row] = []
        for collection in collections.sorted(by: { ($0.order, $0.name, $0.id) < ($1.order, $1.name, $1.id) }) {
            var ancestors: [Int] = []
            if collection.id != WorkspaceDraft.rootCollectionID {
                ancestors = [rows.count]
                rows.append(Row(id: "collection:" + collection.id, depth: 0, content: .collection(collection), parentID: nil, ancestors: [], search: Self.normalize(collection.name)))
            }
            // Children are ordered by index so sorting moves integers rather than request
            // drafts; the order is the same (order, folders first, name, id).
            let groups = collection.groups, requests = collection.requests
            let groupsByParent = Dictionary(grouping: groups.indices, by: { groups[$0].parentID ?? "" })
            let requestsByParent = Dictionary(grouping: requests.indices, by: { requests[$0].groupID ?? "" })
            var visited = Set<String>()
            func append(parent: String, ancestors: [Int]) {
                // (order, kind: 0 folder / 1 request, index)
                var children: [(Int, Int, Int)] = []
                for index in groupsByParent[parent] ?? [] { children.append((groups[index].order, 0, index)) }
                for index in requestsByParent[parent] ?? [] { children.append((requests[index].order, 1, index)) }
                func name(_ child: (Int, Int, Int)) -> String { child.1 == 0 ? groups[child.2].name : requests[child.2].request.name }
                func id(_ child: (Int, Int, Int)) -> String { child.1 == 0 ? groups[child.2].id : requests[child.2].id }
                children.sort { lhs, rhs in
                    if lhs.0 != rhs.0 { return lhs.0 < rhs.0 }
                    if lhs.1 != rhs.1 { return lhs.1 < rhs.1 }
                    let (left, right) = (name(lhs), name(rhs))
                    if left != right { return left < right }
                    return id(lhs) < id(rhs)
                }
                for child in children {
                    let rowID: String
                    let search: String
                    let content: Row.Content
                    if child.1 == 0 {
                        let group = groups[child.2]
                        guard visited.insert(group.id).inserted else { continue }
                        rowID = "group:" + collection.id + ":" + group.id
                        search = group.name
                        content = .group(collection, group)
                    } else {
                        let location = requests[child.2]
                        rowID = "request:" + location.id
                        search = location.request.name + "\u{0}" + location.request.method.rawValue + "\u{0}" + location.request.url
                        content = .request(location)
                    }
                    let position = rows.count
                    rows.append(Row(id: rowID, depth: ancestors.count, content: content, parentID: ancestors.last.map { rows[$0].id },
                                    ancestors: ancestors, search: Self.normalize(search)))
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
            var matching = Array(repeating: false, count: rows.count)
            var utf8Query = query
            let finder = utf8Query.withUTF8 { SubstringPrefilter(query: Array($0)) }
            for (index, row) in rows.enumerated() {
                if Self.matches(row.search, query: query, finder: finder) || row.ancestors.contains(where: { matching[$0] }) {
                    matching[index] = true
                    included[index] = true
                    for parent in row.ancestors { included[parent] = true }
                }
            }
            var result: [Row] = []
            for index in rows.indices where included[index] { result.append(rows[index]) }
            return result
        }
        var hidden = Array(repeating: false, count: rows.count)
        var result: [Row] = []
        for (index, row) in rows.enumerated() {
            if row.ancestors.contains(where: { hidden[$0] }) { continue }
            result.append(row)
            switch row.content {
            case .collection(let collection): if collapsedCollections.contains(collection.id) { hidden[index] = true }
            case .group(let collection, let group): if !expandedGroups.contains(collection.id + ":" + group.id) { hidden[index] = true }
            case .request: break
            }
        }
        return result
    }

    /// `search.contains(query)`, with a byte scan that rules out most rows first. Row search
    /// text is in canonical composed form, so for an ASCII query a row can only match if its
    /// UTF-8 contains the query's bytes; `contains` still decides the rows that do.
    static func matches(_ search: String, query: String, finder: SubstringPrefilter?) -> Bool {
        if let finder {
            var search = search
            guard search.withUTF8({ finder.mayContain($0) }) else { return false }
        }
        return search.contains(query)
    }

    /// The neighbor `delta` (±1) places away among the row's siblings, skipping descendants;
    /// matches `CollectionDraft.orderedChildren` without sorting every sibling.
    public func sibling(of rowID: String, by delta: Int) -> Row? {
        guard delta == 1 || delta == -1, let index = rows.firstIndex(where: { $0.id == rowID }) else { return nil }
        let row = rows[index]
        var position = index + delta
        while rows.indices.contains(position) {
            let candidate = rows[position]
            if candidate.depth < row.depth { return nil }
            if candidate.depth == row.depth {
                return candidate.parentID == row.parentID && candidate.collectionID == row.collectionID
                    && !candidate.isCollectionHeader ? candidate : nil
            }
            position += delta
        }
        return nil
    }

    /// Case- and diacritic-folded, in canonical composed form and native UTF-8, so filtering
    /// can scan the bytes without bridging.
    static func normalize(_ text: String) -> String {
        // ASCII (most names and URLs) only needs lowercasing, which is what folding does to it.
        var text = text
        if let ascii = text.withUTF8({ bytes -> String? in
            guard !bytes.contains(where: { $0 >= 0x80 }) else { return nil }
            return String(unsafeUninitializedCapacity: bytes.count) { buffer in
                for (index, byte) in bytes.enumerated() { buffer[index] = byte >= 0x41 && byte <= 0x5A ? byte | 0x20 : byte }
                return bytes.count
            }
        }) { return ascii }
        var folded = text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: Locale(identifier: "en_US_POSIX"))
            .precomposedStringWithCanonicalMapping
        folded.makeContiguousUTF8()
        return folded
    }

    /// The full folding path, for checking the ASCII shortcut.
    static func foldedForComparison(_ text: String) -> String {
        text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: Locale(identifier: "en_US_POSIX"))
            .precomposedStringWithCanonicalMapping
    }
}

/// A byte-level substring test for ASCII queries, used to skip rows before the exact
/// (canonical-equivalence) `String.contains` check.
struct SubstringPrefilter: Sendable {
    let query: [UInt8]

    /// Nil when the query has non-ASCII bytes, which need the exact check for every row.
    init?(query: [UInt8]) {
        guard !query.isEmpty, query.allSatisfy({ $0 < 0x80 }) else { return nil }
        self.query = query
    }

    func mayContain(_ text: UnsafeBufferPointer<UInt8>) -> Bool {
        let count = query.count
        guard text.count >= count, let first = query.first else { return false }
        var index = 0
        let last = text.count - count
        while index <= last {
            if text[index] == first {
                var offset = 1
                while offset < count && text[index + offset] == query[offset] { offset += 1 }
                if offset == count { return true }
            }
            index += 1
        }
        return false
    }
}

/// Keyboard movement over the visible sidebar rows, following NSOutlineView conventions.
public enum SidebarNavigation {
    public enum Outcome: Equatable, Sendable {
        case select(String)
        case expand(String)
        case collapse(String)
        case none
    }

    /// Up/Down move over every visible row; without a current row they start at an end.
    public static func step(from current: String?, by delta: Int, in rows: [SidebarSnapshot.Row]) -> String? {
        guard !rows.isEmpty else { return nil }
        guard let index = current.flatMap({ id in rows.firstIndex { $0.id == id } }) else {
            return delta > 0 ? rows.first?.id : rows.last?.id
        }
        return rows[min(max(0, index + delta), rows.count - 1)].id
    }

    /// Left collapses an expanded container, otherwise selects the parent.
    public static func left(from current: String?, in rows: [SidebarSnapshot.Row],
                            isExpanded: (SidebarSnapshot.Row) -> Bool) -> Outcome {
        guard let row = current.flatMap({ id in rows.first { $0.id == id } }) else { return .none }
        if row.isContainer, isExpanded(row) { return .collapse(row.id) }
        if let parent = row.parentID, rows.contains(where: { $0.id == parent }) { return .select(parent) }
        return .none
    }

    /// Right expands a collapsed container, otherwise selects its first child.
    public static func right(from current: String?, in rows: [SidebarSnapshot.Row],
                             isExpanded: (SidebarSnapshot.Row) -> Bool) -> Outcome {
        guard let index = current.flatMap({ id in rows.firstIndex { $0.id == id } }), rows[index].isContainer
        else { return .none }
        if !isExpanded(rows[index]) { return .expand(rows[index].id) }
        let next = index + 1
        return rows.indices.contains(next) && rows[next].parentID == rows[index].id ? .select(rows[next].id) : .none
    }

    /// Type-select: the next row, from the current one, whose title starts with `prefix`.
    /// A single character moves past the current row so repeating it cycles through matches.
    public static func typeSelect(_ prefix: String, from current: String?, in rows: [SidebarSnapshot.Row]) -> String? {
        let prefix = SidebarSnapshot.normalize(prefix)
        guard !prefix.isEmpty, !rows.isEmpty else { return nil }
        let index = current.flatMap { id in rows.firstIndex { $0.id == id } }
        let start = index.map { prefix.count == 1 ? $0 + 1 : $0 } ?? 0
        for offset in 0..<rows.count {
            let row = rows[(start + offset) % rows.count]
            if SidebarSnapshot.normalize(row.title).hasPrefix(prefix) { return row.id }
        }
        return nil
    }
}

/// Stable, typed sibling identities; folders and requests share one ordering space.
public extension CollectionDraft {
    func orderedChildren(parentID: String?) -> [String] {
        let groups = groups.filter { $0.parentID == parentID }.map {
            ($0.order, 0, $0.name, $0.id, "group:" + $0.id)
        }
        let requests = requests.filter { $0.groupID == parentID }.map {
            ($0.order, 1, $0.request.name, $0.request.id, "request:" + $0.request.id)
        }
        return (groups + requests).sorted {
            ($0.0, $0.1, $0.2, $0.3) < ($1.0, $1.1, $1.2, $1.3)
        }.map { $0.4 }
    }

    /// Folders under `rootID` (or every folder for nil), parents before children, so they
    /// can be recreated in order.
    func subtreeGroups(of rootID: String?) -> [GroupDraft] {
        var result = rootID.flatMap { id in groups.first { $0.id == id } }.map { [$0] } ?? []
        var frontier = rootID.map { [$0] } ?? [nil]
        var visited = Set(result.map(\.id))
        while !frontier.isEmpty {
            let parent = frontier.removeFirst()
            for group in groups where group.parentID == parent && visited.insert(group.id).inserted {
                result.append(group)
                frontier.append(group.id)
            }
        }
        return result
    }

    /// Folders in sidebar order with their nesting depth, for "Move To" menus.
    func folderOutline() -> [(group: GroupDraft, depth: Int)] {
        var result: [(GroupDraft, Int)] = []
        var visited = Set<String>()
        func append(parent: String?, depth: Int) {
            let children = groups.filter { $0.parentID == parent }
                .sorted { ($0.order, $0.name, $0.id) < ($1.order, $1.name, $1.id) }
            for group in children where visited.insert(group.id).inserted {
                result.append((group, depth))
                append(parent: group.id, depth: depth + 1)
            }
        }
        append(parent: nil, depth: 0)
        return result
    }
}
