import Testing
@testable import WireboltKit

struct BulkFieldTests {
    @Test func unrelatedEditsPreserveCredentialsAndRedaction() {
        let original = [RequestField(id: "secret", name: "X-Token", value: .secret("fixture.token"), sensitive: true),
                        RequestField(id: "private", name: "X-Private", value: .literal("fixture"), sensitive: true)]
        let result = RequestField.parseBulk("X-Token: fixture.token\nX-Private: fixture\nX-New: added", preserving: original)
        #expect(Array(result.prefix(2)) == original)
        #expect(result.last?.value == .literal("added"))
    }
    @Test func togglesRenamesAndReferenceEditsKeepMetadata() {
        let original = [RequestField(id: "secret", name: "X-Token", value: .secret("fixture.token"), sensitive: true)]
        let renamed = RequestField.parseBulk("# X-Renamed: fixture.token", preserving: original)
        #expect(renamed.first?.id == "secret")
        #expect(renamed.first?.value == .secret("fixture.token"))
        #expect(renamed.first?.sensitive == true)
        #expect(renamed.first?.enabled == false)
        let changed = RequestField.parseBulk("X-Token: fixture.replacement", preserving: original)
        #expect(changed.first?.value == .secret("fixture.replacement"))
        #expect(changed.first?.sensitive == true)
    }
    @Test func duplicateFieldsKeepTheirOwnProtectionWhenReordered() {
        let original = [RequestField(id: "a", name: "X", value: .literal("one")),
                        RequestField(id: "b", name: "X", value: .secret("two"), sensitive: true)]
        #expect(RequestField.parseBulk("X: two\nX: one", preserving: original) == original.reversed())
    }
}
