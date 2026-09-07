import Foundation
import Testing
@testable import WireboltKit

@Suite struct ResponseTextIndexTests {
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
