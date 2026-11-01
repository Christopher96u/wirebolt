import Foundation
import Testing
@testable import WireboltKit

@Suite("JSON tree values")
struct JSONNodeTests {
    @Test("Preserves the distinction between numbers and booleans")
    func scalarTypes() throws {
        let root = try #require(JSONNode.makeRoot(from: "{\"zero\":0,\"one\":1,\"yes\":true,\"no\":false}").first)
        let children = try #require(root.children)
        #expect(children.first { $0.key == "zero" }?.type == .number)
        #expect(children.first { $0.key == "zero" }?.value == "0")
        #expect(children.first { $0.key == "one" }?.value == "1")
        #expect(children.first { $0.key == "yes" }?.type == .boolean)
        #expect(children.first { $0.key == "no" }?.value == "false")
    }
}

@Suite("JSON tree parsing")
struct JSONNodeOrderTests {
    @Test("Object keys keep document order, including duplicates")
    func preservesKeyOrder() throws {
        let keys = ["zeta", "alpha", "middle", "b", "a", "alpha"]
        let json = "{" + keys.enumerated().map { "\"\($0.element)\":\($0.offset)" }.joined(separator: ",") + "}"
        let root = try #require(JSONNode.makeRoot(from: json).first)
        #expect(root.children?.map(\.key) == keys)
        #expect(root.value == "Object(6 items)")
    }

    @Test("Scalars keep their spelling and strings are decoded")
    func scalarSpelling() throws {
        let root = try #require(JSONNode.makeRoot(from: #"[9007199254740993, 1e+999, -0.50, "café 🚀 \"q\"", null]"#).first)
        let values = try #require(root.children).map(\.value)
        #expect(values == ["9007199254740993", "1e+999", "-0.50", "café 🚀 \"q\"", "null"])
        #expect(root.children?.map(\.key) == ["0", "1", "2", "3", "4"])
        #expect(root.children?[2].id == "root/2")
    }

    @Test("Invalid JSON produces no tree", arguments: ["", "{", "[1,]", #"{"a":}"#, "[01]", "tru", "[1] 2", "[\"a\nb\"]"])
    func invalid(json: String) {
        #expect(JSONNode.makeRoot(from: json).isEmpty)
    }

    @Test("The node limit summarises omitted items instead of materialising them")
    func nodeLimit() throws {
        let json = "{\"items\":[" + (0..<100).map { "{\"id\":\($0)}" }.joined(separator: ",") + "],\"after\":true}"
        let root = try #require(JSONNode.makeRoot(from: Data(json.utf8), nodeLimit: 10).first)
        let items = try #require(root.children?.first)
        #expect(items.value == "Array(100 items)")
        #expect(items.children?.last?.type == .truncated)
        #expect(items.children?.last?.value.hasSuffix("more items not shown") == true)
        #expect(root.children?.last?.type == .truncated, "siblings after the limit are summarised too")
    }
}

extension JSONNodeOrderTests {
    @Test func fileTreeKeepsBOMOrderAndSurrogateRepair() throws {
        let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try (Data([0xEF, 0xBB, 0xBF]) + Data(#"{"z":1,"a":2,"z":3}"#.utf8)).write(to: url)
        #expect(JSONNode.makeRoot(from: url).first?.children?.map(\.key) == ["z", "a", "z"])
        #expect(JSONNode.makeRoot(from: #"["\ud800\ud83d\ude80"]"#).first?.children?.first?.value == "�🚀")
    }
}
