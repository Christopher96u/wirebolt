import Testing
@testable import WireboltKit

@Suite("Request URL editing")
struct RequestURLTests {
    @Test("Imported URL parameters populate the table without marking a saved tab dirty")
    @MainActor
    func importedURL() {
        let draft = RequestDraft(url: "https://example.com/?a=1&a=2")
        let session = DocumentSession(draft: draft, savedDraft: draft)
        #expect(session.draft.query.map(\.name) == ["a", "a"])
        #expect(session.draft.url == "https://example.com/")
        #expect(session.draft.displayURL == draft.url)
        #expect(!session.isDirty)
    }

    @Test("Typing a query never feeds an encoded value back into the key")
    func incrementalTyping() {
        let text = "http://127.0.0.1:48173/json?limit=10&search=wirebolt"
        var draft = RequestDraft()
        var typed = ""
        for character in text {
            typed.append(character)
            draft.editURL(typed)
        }
        let input = RunInput(draft: draft, variables: [:])
        #expect(input.url == "http://127.0.0.1:48173/json")
        #expect(input.query.map(\.name) == ["limit", "search"])
        #expect(input.query.map(\.value) == [.literal("10"), .literal("wirebolt")])
        #expect(draft.displayURL == text)
    }

    @Test("Editing preserves duplicate keys, disabled fields and Unicode")
    func duplicateKeys() {
        var draft = RequestDraft(query: [RequestField(name: "off", value: .literal("hidden"), enabled: false)])
        draft.editURL("https://example.com/p?a=1&a=2&name=caf%C3%A9#anchor")
        let ids = draft.query.map(\.id)
        draft.editURL(draft.displayURL)
        #expect(draft.query.map(\.id) == ids)
        #expect(draft.query.map(\.name) == ["a", "a", "name", "off"])
        #expect(draft.query[2].value == .literal("café"))
        #expect(draft.url == "https://example.com/p#anchor")
        draft.editURL("https://example.com/p")
        #expect(draft.query.map(\.name) == ["off"])
    }
}
