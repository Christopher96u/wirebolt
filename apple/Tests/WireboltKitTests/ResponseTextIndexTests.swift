import Foundation
import CoreText
import Testing
@testable import WireboltKit

@Suite struct ResponseTextIndexTests {
    private func wrapping(width: Double = 160) -> CodeTextWrapping {
        let font = CTFontCreateUIFontForLanguage(.userFixedPitch, 12, nil)!
        return CodeTextWrapping(fontName: CTFontCopyPostScriptName(font) as String, fontSize: 12, width: width)
    }

    @Test func pixelWrappingPreservesUnicodeAndCheckpointIndentation() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let text = "  " + String(repeating: "path/to?q=cafe\u{301}, 東京 👨‍👩‍👧‍👦 🚀; ", count: 4000)
        let bytes = Data(text.utf8)
        try bytes.write(to: url)
        let index = try ResponseTextIndex(url: url, columns: 20, wrapping: wrapping())
        let rows = try index.rows(start: 0, count: index.rowCount)
        #expect(rows.map(\.text).joined() == text)
        #expect(rows.first?.indent == 0)
        #expect(rows.dropFirst().allSatisfy { $0.continuation && $0.indent > 0 })
        let clusterBoundaries = Set(text.indices.map { text[..<$0].utf8.count } + [text.utf8.count])
        #expect(rows.allSatisfy { clusterBoundaries.contains(Int($0.byteOffset)) })
        for start in stride(from: 127, to: index.rowCount, by: 128) {
            let actual = try index.rows(start: start, count: 3)
            let expected = Array(rows.dropFirst(start).prefix(3))
            #expect(actual == expected)
        }
        #expect(try Data(contentsOf: url) == bytes)
    }

    @Test func pixelWrappingKeepsRawPrefixAndSearchPositions() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let body = "  /json?limit=10&search=wirebolt\r\n\n"
        try Data(body.utf8).write(to: url)
        let prefix = "HTTP/1.1 200 OK\nX-Long-Header: abcdefghijklmnop\n\n"
        let index = try ResponseTextIndex(url: url, columns: 20, prefix: prefix, wrapping: wrapping())
        let rows = try index.rows(start: 0, count: index.rowCount)
        #expect(rows.map { ($0.number > 0 && !$0.continuation ? "\n" : "") + $0.text }.joined() == prefix + body.replacingOccurrences(of: "\r", with: ""))
        let match = try #require(index.search(TextSearchQuery(text: "search=wirebolt")).first)
        let start = try #require(index.rows(start: match.start.row, count: 1).first)
        #expect((start.text as NSString).substring(from: match.start.column).hasPrefix("search="))
        #expect(try index.firstMatch("search=wirebolt") == match.start.row)
        #expect(rows.suffix(2).map(\.text) == ["", ""])
    }

    @Test func pixelWrappingDecodesMalformedUTF8AndNarrowClusters() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let bytes = Data([0xF0, 0x9F, 0x41, 0xED, 0xA0, 0x80, 0x80]) + Data("👨‍👩‍👧‍👦e\u{301}".utf8)
        try bytes.write(to: url)
        let index = try ResponseTextIndex(url: url, columns: 1, wrapping: wrapping(width: 1))
        let rows = try index.rows(start: 0, count: index.rowCount)
        #expect(rows.map(\.text).joined() == String(decoding: bytes, as: UTF8.self))
        #expect(rows.suffix(2).map(\.text) == ["👨‍👩‍👧‍👦", "e\u{301}"])
        try Data().write(to: url)
        #expect(try ResponseTextIndex(url: url, columns: 1, wrapping: wrapping()).rowCount == 1)
    }

    @Test func rawHeadersPrecedeEveryBodyRowWithoutCopyingTheResponseFile() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let body = (0..<400).map { "body-\($0)" }.joined(separator: "\n")
        try Data(body.utf8).write(to: url)
        let index = try ResponseTextIndex(url: url, columns: 80, prefix: "HTTP/1.1 200 OK\nContent-Type: text/plain\n\n")
        #expect(index.rowCount == 403)
        #expect(index.lineCount == 403)
        #expect(try index.rows(start: 0, count: 4).map(\.text) == ["HTTP/1.1 200 OK", "Content-Type: text/plain", "", "body-0"])
        #expect(try index.rows(start: 401, count: 2).map(\.text) == ["body-398", "body-399"])
        #expect(try index.firstMatch("Content-Type") == 1)
        #expect(try index.firstMatch("body-399") == 402)
        #expect(try String(contentsOf: url, encoding: .utf8) == body)
    }

    @Test func unwrappedLongUnicodeLineIsNotTruncatedOrSplit() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let line = String(repeating: "café 東京 🚀", count: 10000)
        try Data(line.utf8).write(to: url)
        let index = try ResponseTextIndex(url: url, columns: Int.max / 4)
        #expect(index.rowCount == 1)
        #expect(try index.rows(start: 0, count: 1).first?.text == line)
    }

    @Test func indexesAcrossCheckpointsAndUTF8ChunkBoundaries() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        let lines = (0..<10000).map { "\($0): café 東京 🚀 " + String(repeating: "x", count: 30) }
        try Data(lines.joined(separator: "\n").utf8).write(to: url)
        let index = try ResponseTextIndex(url: url, columns: 18)
        #expect(index.lineCount == 10_000)
        #expect(index.rowCount > index.lineCount)
        let first = try index.rows(start: 0, count: 10).filter { $0.line == 0 }
        #expect(first.map(\.text).joined() == lines[0])
        let last = try index.rows(start: index.rowCount - 10, count: 10).filter { $0.line == 9999 }
        #expect(last.map(\.text).joined() == lines.last)
        #expect(last.allSatisfy { $0.line == 9999 })
        #expect(last.first?.continuation == false)
        #expect(try index.firstMatch("9999: café 東京") == last.first?.number)
        #expect(try index.firstMatch("absent") == nil)
    }

    @Test func emptyAndTrailingNewlineHaveAnEditableFinalLine() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try Data().write(to: url)
        #expect(try ResponseTextIndex(url: url, columns: 80).rowCount == 1)
        try Data("1234\n".utf8).write(to: url)
        let index = try ResponseTextIndex(url: url, columns: 4)
        #expect(index.rowCount == 2)
        #expect(try index.rows(start: 0, count: 2).map(\.text) == ["1234", ""])
    }
}
