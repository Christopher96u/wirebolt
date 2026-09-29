import Foundation
import Testing
@testable import WireboltKit

@Suite struct ResponseIndexCacheTests {
    @Test func firstViewportUpgradesToFullIndexAndInvalidatesWithFile() async throws {
        let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try Data(String(repeating: "row\n", count: 1000).utf8).write(to: url)
        let cache = ResponseIndexCache()
        let first = try await cache.firstViewport(url: url, columns: 80)
        #expect(!first.isComplete && first.rowCount == 256)
        #expect(try first.rows(start: 250, count: 100).count == 6)
        let full = try await cache.index(url: url, columns: 80)
        #expect(full.isComplete && full.rowCount == 1001)
        #expect(try await cache.firstViewport(url: url, columns: 80).isComplete)
        try Data("replacement".utf8).write(to: url)
        let changed = try await cache.firstViewport(url: url, columns: 80)
        #expect(try changed.rows(start: 0, count: 1).first?.text == "replacement")
    }

    @Test func cacheSeparatesFilesWidthsPrefixesAndFileRevisions() async throws {
        let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try Data("abcdef\n東京🚀".utf8).write(to: url)
        let cache = ResponseIndexCache(capacity: 2)
        let narrow = try await cache.index(url: url, columns: 3)
        let wide = try await cache.index(url: url, columns: 30)
        #expect(narrow.rowCount > wide.rowCount)
        let prefixed = try await cache.index(url: url, columns: 30, prefix: "HTTP/1.1 200 OK\n")
        #expect(try prefixed.rows(start: 0, count: 1).first?.text == "HTTP/1.1 200 OK")
        let cached = try await cache.index(url: url, columns: 30)
        #expect(try cached.rows(start: 0, count: 5) == wide.rows(start: 0, count: 5))
        try Data("replacement\nwith\nmore\nlines".utf8).write(to: url)
        let changed = try await cache.index(url: url, columns: 30)
        #expect(changed.rowCount == 4)
        #expect(try changed.rows(start: 0, count: 1).first?.text == "replacement")
    }

    @Test func cancelledReadDoesNotPoisonNextRequest() async throws {
        let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try Data("hello\nworld".utf8).write(to: url)
        let cache = ResponseIndexCache()
        let task = Task { try await cache.index(url: url, columns: 80) }
        task.cancel()
        do { _ = try await task.value } catch is CancellationError {}
        let index = try await cache.index(url: url, columns: 80)
        #expect(try index.rows(start: 0, count: 2).map(\.text) == ["hello", "world"])
    }

    @Test func wrappedLayoutsAreKeyedByColumnCountNotPixelWidth() async throws {
        let url = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try Data(String(repeating: "{\"message\":\"café 東京 🚀 wraps across several visual rows\"}\n", count: 400).utf8).write(to: url)
        let font = "Menlo-Regular"
        let advance = CodeTextWrapping.advance(fontName: font, fontSize: 12)
        func columns(forWidth width: Double) -> Int { max(1, Int(width / advance)) }
        // Two widths inside the same column, as produced by a pixel-by-pixel live resize.
        let narrow = columns(forWidth: 40 * advance + 1), slightlyWider = columns(forWidth: 40 * advance + advance * 0.9)
        #expect(narrow == slightlyWider)
        let first = CodeTextWrapping(fontName: font, fontSize: 12, columns: narrow)
        #expect(first == CodeTextWrapping(fontName: font, fontSize: 12, columns: slightlyWider))
        #expect(first.width > Double(narrow) * advance && first.width < Double(narrow + 1) * advance)
        let cache = ResponseIndexCache()
        let full = try await cache.index(url: url, columns: narrow, wrapping: first)
        #expect(full.isComplete && full.rowCount > 400)
        // A cache hit returns the complete layout instead of a bounded first viewport.
        #expect(try await cache.firstViewport(url: url, columns: slightlyWider,
            wrapping: CodeTextWrapping(fontName: font, fontSize: 12, columns: slightlyWider)).isComplete)
        let other = try await cache.firstViewport(url: url, columns: narrow + 1,
            wrapping: CodeTextWrapping(fontName: font, fontSize: 12, columns: narrow + 1))
        #expect(!other.isComplete)
        // ASCII rows wrap at exactly the column count.
        #expect(try full.rows(start: 0, count: 8).allSatisfy { $0.columns <= narrow })
    }

    @Test func longLinesUseIndexedPresentationBelowByteThreshold() {
        #expect(ResponseTextPresentation.usesIndex(byteCount: 6000, preview: String(repeating: "東京", count: 1500)))
        #expect(ResponseTextPresentation.usesIndex(byteCount: 65_537, preview: "small preview"))
        #expect(!ResponseTextPresentation.usesIndex(byteCount: 6000, preview: String(repeating: "small\n", count: 1000)))
    }
}
