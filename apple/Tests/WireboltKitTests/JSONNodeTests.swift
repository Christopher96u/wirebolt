import Testing
@testable import WireboltKit

@Suite("JSON tree values")
struct JSONNodeTests {
    @Test("Preserves the distinction between numbers and booleans")
    func scalarTypes() throws {
        let root = try #require(JSONNode.makeRoot(from: "{\"zero\":0,\"one\":1,\"yes\":true,\"no\":false}").first)
        let children = try #require(root.children)
        #expect(children.first { $0.key == "zero" }?.type == "Number")
        #expect(children.first { $0.key == "zero" }?.value == "0")
        #expect(children.first { $0.key == "one" }?.value == "1")
        #expect(children.first { $0.key == "yes" }?.type == "Boolean")
        #expect(children.first { $0.key == "no" }?.value == "false")
    }
}
