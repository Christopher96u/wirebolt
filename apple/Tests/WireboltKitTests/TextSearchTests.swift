import Foundation
import Testing
@testable import WireboltKit

@Suite struct TextSearchTests {
    @Test func literalCaseWordAndRegexOptionsPreserveUTF16Ranges() throws {
        let text = "🚀 café CAFÉ cafeteria\nitem-12 item-34"
        let insensitive = try TextSearchQuery(text: "café").matches(in: text)
        #expect(insensitive.count == 2)
        #expect(insensitive.first?.location == 3)
        #expect(try TextSearchQuery(text: "café", matchCase: true).matches(in: text).count == 1)
        #expect(try TextSearchQuery(text: "cafe", wholeWord: true).matches(in: text).isEmpty)
        #expect(try TextSearchQuery(text: "item-\\d+", regularExpression: true).matches(in: text).count == 2)
        #expect(throws: (any Error).self) { try TextSearchQuery(text: "[", regularExpression: true).matches(in: text) }
        let scope = (text as NSString).range(of: "CAFÉ")
        #expect(try TextSearchQuery(text: "café").matches(in: text, range: scope) == [scope])
    }

    @Test func cappedSearchAndZeroLengthPatternsTerminate() throws {
        #expect(try TextSearchQuery(text: "x").matches(in: String(repeating: "x", count: 30_000)).count == 20_000)
        #expect(try TextSearchQuery(text: "(?=x)", regularExpression: true).matches(in: "xx").count == 2)
        #expect(try TextSearchQuery().matches(in: "x").isEmpty)
    }

    @Test func indexedSearchCrossesSoftWrapsNewlinesAndUnicode() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try Data("🚀 hello café world\r\nSECOND line\n".utf8).write(to: url)
        let index = try ResponseTextIndex(url: url, columns: 8, prefix: "HTTP/1.1 200\n\n")
        let matches = try index.search(TextSearchQuery(text: "café world\\r?\\nSECOND", regularExpression: true))
        #expect(matches.count == 1)
        let match = try #require(matches.first)
        #expect(match.start.row < match.end.row)
        let first = try #require(index.rows(start: match.start.row, count: 1).first)
        #expect((first.text as NSString).substring(from: match.start.column).hasPrefix("café"))
        let last = try #require(index.rows(start: match.end.row, count: 1).first)
        #expect((last.text as NSString).substring(to: match.end.column) == "SECOND")
        #expect(try index.search(TextSearchQuery(text: "200")).first?.start.row == 1)
        #expect(try index.search(TextSearchQuery(text: "absent")).isEmpty)
    }

    @Test func selectionIsAppliedBeforeTheMatchLimit() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try Data((String(repeating: "x\n", count: 25_000) + "🚀 x x\n").utf8).write(to: url)
        let index = try ResponseTextIndex(url: url, columns: 80)
        let selection = (start: ResponseTextIndex.SearchPosition(row: 25_000, column: 3),
                         end: ResponseTextIndex.SearchPosition(row: 25_000, column: 6))
        let matches = try index.search(TextSearchQuery(text: "x"), selection: selection)
        #expect(matches.count == 2)
        #expect(matches.first?.start == selection.start)
        #expect(matches.last?.end == selection.end)
    }
}
