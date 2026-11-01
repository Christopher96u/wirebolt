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

extension TextSearchTests {
    @Test func indexedICUMatchesFoundationForUnicodeAndSelections() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let source = "🚀e\u{301} Straße STRASSE\n東京 café CAFÉ\n!"
        try Data(source.utf8).write(to: url)
        let index = try ResponseTextIndex(url: url, columns: 100)
        let rowStarts = [0, ("🚀e\u{301} Straße STRASSE\n" as NSString).length, (source as NSString).length - 1]
        for (text, regex) in [(#"\X"#, true), ("(?i)strasse", true), ("café", false), (#"!\z"#, true), ("e\u{301}", false)] {
            let pattern = regex ? text : NSRegularExpression.escapedPattern(for: text)
            let expression = try NSRegularExpression(pattern: pattern, options: [.anchorsMatchLines, .caseInsensitive])
            let expected = expression.matches(in: source, range: NSRange(location: 0, length: (source as NSString).length)).map(\.range)
            let actual = try index.search(TextSearchQuery(text: text, regularExpression: regex)).map {
                NSRange(location: rowStarts[$0.start.row] + $0.start.column,
                        length: rowStarts[$0.end.row] + $0.end.column - rowStarts[$0.start.row] - $0.start.column)
            }
            #expect(actual == expected)
        }
        let selected = try index.search(TextSearchQuery(text: "café"), selection: (.init(row: 1, column: 8), .init(row: 1, column: 12)))
        #expect(selected.count == 1)
        #expect(selected[0].start == .init(row: 1, column: 8))
    }

    @Test func indexedCancellationInterruptsAnActiveICUSearch() async throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try Data((String(repeating: "a", count: 2000) + "!").utf8).write(to: url)
        let index = try ResponseTextIndex(url: url, columns: 100)
        let worker = responseTask(priority: .userInitiated) {
            try index.search(TextSearchQuery(text: "(a|aa)+$", regularExpression: true))
        }
        DispatchQueue.global().asyncAfter(deadline: .now() + .milliseconds(10)) { worker.cancel() }
        await #expect(throws: CancellationError.self) { try await worker.value }
        #expect(try index.search(TextSearchQuery(text: "!")).first?.end.column == 1)
    }

    @Test func preservesICUDialectAndUnicodeLineEndings() throws {
        let source = "a\rb\nc\r\nd\u{85}e\u{2028}f\u{2029}g"
        for pattern in [".", "^.", ".$"] {
            let hits = try TextSearchQuery(text: pattern, regularExpression: true).matches(in: source)
            #expect(hits.map(\.location) == [0, 2, 4, 7, 9, 11, 13])
        }
        #expect(try TextSearchQuery(text: "(?i)strasse", regularExpression: true).matches(in: "Straße STRASSE").count == 2)
        #expect(try TextSearchQuery(text: #"\X"#, regularExpression: true).matches(in: "👨‍👩‍👧‍👦e\u{301}").count == 2)
    }
    @Test func nativeTaskCancellationInterruptsRustSearch() async throws {
        let worker = responseTask(priority: .userInitiated) {
            try TextSearchQuery(text: "(a|aa)+$", regularExpression: true).matches(in: String(repeating: "a", count: 2000) + "!")
        }
        // GCD delivers the cancellation even while the search occupies a cooperative thread.
        DispatchQueue.global().asyncAfter(deadline: .now() + .milliseconds(2)) { worker.cancel() }
        await #expect(throws: CancellationError.self) { try await worker.value }
    }
}
