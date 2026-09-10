import Foundation
import Testing
@testable import WireboltKit

@Suite struct CodeFoldingTests {
    @Test(arguments: [" ", "café", "東京🚀", "123", ""])
    func incrementalEditsMatchFullFolding(_ replacement: String) throws {
        let source = "{\n \"message\": \"hello\",\n \"items\": [\n  1, 2\n ]\n}"
        let initial = CodeProjection(source: source)
        let range = (source as NSString).range(of: "hello")
        let changed = (source as NSString).replacingCharacters(in: range, with: replacement)
        let updated = try #require(initial.updatingUnfoldedSource(changed, range: range, replacement: replacement, removed: "hello"))
        #expect(updated.folds == CodeProjection(source: changed).folds)
        #expect(updated.text == changed)
    }

    @Test(arguments: ["\n", "\r", "\\", "\"", "{", "}", "[", "]"])
    func structuralEditsRequireReparsing(_ replacement: String) {
        let initial = CodeProjection(source: "{\n 1\n}")
        #expect(initial.updatingUnfoldedSource("", range: NSRange(location: 2, length: 0), replacement: replacement, removed: "") == nil)
    }

    @Test func ignoresBracketsInsideEscapedStringsAndKeepsOriginalLineNumbers() {
        let source = "{\n  \"quoted\": \"[\\\"{\",\n  \"items\": [\n    0, 1, true, \"東京🚀\"\n  ]\n}"
        let projection = CodeProjection(source: source, collapsed: [2])
        #expect(projection.folds.map(\.line) == [0, 2])
        #expect(projection.text == "{\n  \"quoted\": \"[\\\"{\",\n  \"items\": […]\n}")
        #expect(projection.replacing(displayRange: NSRange(location: 0, length: 0), with: " ") == " " + source)
        let hidden = (projection.text as NSString).range(of: "…")
        #expect(projection.replacing(displayRange: hidden, with: "2") == "{\n  \"quoted\": \"[\\\"{\",\n  \"items\": [2]\n}")
    }

    @Test func nestedFoldsRoundTripUnicodeAndCrossFoldReplacement() {
        let source = "{\n \"🚀\": [\n  0,\n  1\n ]\n}"
        let projection = CodeProjection(source: source, collapsed: [0, 1])
        #expect(projection.text == "{…}")
        #expect(projection.replacing(displayRange: NSRange(location: 0, length: 3), with: "[]") == "[]")
        #expect(CodeProjection(source: source).text == source)
        let expanded = CodeProjection(source: source, collapsed: [1])
        #expect(expanded.sourceOffset((expanded.text as NSString).length) == (source as NSString).length)
        #expect(expanded.displayOffset((source as NSString).length) == (expanded.text as NSString).length)
    }

    @Test func markupPreservesQuotedAttributesCommentsCDATAAndUnicode() {
        let source = "<root attr=\">\">\n  <ns:item>\n    東京🚀\n  </ns:item>\n  <!-- <fake>\n  </fake> -->\n  <![CDATA[<fake>\n  </fake>]]>\n</root>"
        let projection = CodeProjection(source: source, collapsed: [1], syntax: .xml)
        #expect(projection.folds.map(\.line) == [0, 1, 4, 6])
        #expect(projection.text.contains("<ns:item>…</ns:item>"))
        #expect(projection.replacing(displayRange: NSRange(location: 0, length: 0), with: " ") == " " + source)
        #expect(CodeProjection(source: source, collapsed: [0], syntax: .xml).text == "<root attr=\">\">…</root>")
    }

    @Test func htmlVoidElementsAndRawTextDoNotCreateFalseFolds() {
        let source = "<DIV>\n<br><img src=\"a>b\"><input>\n<SCRIPT>\nif (a < b) { value = '<fake>'; }\n</script>\n<p>\nbody\n</p>\n</div>"
        let projection = CodeProjection(source: source, collapsed: [2], syntax: .html)
        #expect(projection.folds.map(\.line) == [0, 2, 5])
        #expect(projection.text.contains("<SCRIPT>…</script>"))
        #expect(CodeProjection(source: "<root>\n<unclosed>", syntax: .xml).folds.isEmpty)
        #expect(CodeProjection(source: "{\ntext\n}", syntax: .none).folds.isEmpty)
    }
}
