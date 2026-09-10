import Foundation
import Testing
@testable import WireboltKit

@Suite struct ResponseIndexCacheTests {
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

    @Test func longLinesUseIndexedPresentationBelowByteThreshold() {
        #expect(ResponseTextPresentation.usesIndex(byteCount: 6000, preview: String(repeating: "東京", count: 1500)))
        #expect(ResponseTextPresentation.usesIndex(byteCount: 65_537, preview: "small preview"))
        #expect(!ResponseTextPresentation.usesIndex(byteCount: 6000, preview: String(repeating: "small\n", count: 1000)))
    }
}
