import Foundation

struct JSONNode: Identifiable, Sendable {
    static let nodeLimit = 200_000
    let id: String
    let key: String
    let type: JsonNodeKind
    let value: String
    let children: [JSONNode]?
    private init(_ node: JsonTreeNode) {
        id = node.id
        key = node.key
        type = node.kind
        value = node.value
        children = node.children?.map(JSONNode.init)
    }
    static func makeRoot(from text: String) -> [JSONNode] { makeRoot(from: Data(text.utf8)) }
    static func makeRoot(from url: URL, nodeLimit: Int = nodeLimit) -> [JSONNode] {
        guard let root = try? responseCall({ try appJsonTreeFile(path: url.path, nodeLimit: UInt64(max(1, nodeLimit)), cancellation: $0) })
        else { return [] }
        return [JSONNode(root)]
    }
    static func makeRoot(from data: Data, nodeLimit: Int = nodeLimit) -> [JSONNode] {
        guard let root = try? responseCall({ try appJsonTree(data: data, nodeLimit: UInt64(max(1, nodeLimit)), cancellation: $0) }) else {
            return []
        }
        return [JSONNode(root)]
    }
}
