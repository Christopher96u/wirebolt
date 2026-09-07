import Foundation
import Testing
@testable import WireboltKit

@Suite struct CodeFoldingTests {
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
