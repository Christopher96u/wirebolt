import Foundation
import Testing
@testable import WireboltKit

@Suite("File-backed responses")
struct ResponseStorageTests {
    @Test("streams, searches and exports without retaining the complete body")
    func responseBodyStore() async throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let store = try ResponseBodyStore(runID: RunID(), directory: root)
        let body = Data((String(repeating: "0123456789", count: 20_000) + "needle").utf8)

        try await store.append(body.prefix(91_000))
        try await store.append(body.dropFirst(91_000))
        try await store.finish()

        #expect(await store.size() == UInt64(body.count))
        #expect(try await store.viewport().count == ResponseBodyStore.viewportByteCount)
        #expect(try await store.countOccurrences(of: "needle") == 1)

        let exported = root.appending(path: "export.body")
        try await store.export(to: exported)
        #expect(try Data(contentsOf: exported) == body)
    }

    @Test("history survives repository recreation and evicts past its quota")
    func persistentBoundedHistory() async throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let repository = HistoryRepository(root: root)
        let snapshot = PreparedRunSnapshot(
            method: "GET",
            url: "https://example.com",
            headers: [],
            body: PreparedBodySnapshot(byteCount: 0, contentType: nil, textPreview: nil, redacted: false),
            transport: TransportSettings()
        )
        for index in 0 ... HistoryRepository.maximumEntriesPerRequest {
            let runID = RunID()
            let body = try ResponseBodyStore(runID: runID, directory: root)
            try await body.append(Data("run \(index)".utf8))
            try await body.finish()
            try await repository.record(
                runID: runID,
                requestID: "health",
                prepared: snapshot,
                responseHead: nil,
                completion: RunCompletion(bytesReceived: UInt64(index), totalTimeNS: 1),
                failure: nil,
                body: body
            )
            await body.remove()
        }

        let reloaded = HistoryRepository(root: root)
        let entries = await reloaded.list(requestID: "health")
        #expect(entries.count == HistoryRepository.maximumEntriesPerRequest)
        #expect(entries.allSatisfy { FileManager.default.fileExists(atPath: $0.bodyPath) })
    }

    @Test("cookies honor host, path, TLS, expiry and persistence")
    func cookiePolicy() async throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let storage = root.appending(path: "cookies.json")
        let jar = CookieJar(storageURL: storage)
        let origin = try #require(URL(string: "https://api.example.com/account/login"))
        await jar.store(headers: [
            ResponseHeader(name: "Set-Cookie", value: "session=abc; Path=/account; Secure; HttpOnly; SameSite=Lax"),
            ResponseHeader(name: "Set-Cookie", value: "gone=1; Max-Age=0"),
        ], requestURL: origin)

        #expect(await jar.header(for: try #require(URL(string: "https://api.example.com/account/me"))) == "session=abc")
        #expect(await jar.header(for: try #require(URL(string: "http://api.example.com/account/me"))) == nil)
        #expect(await jar.header(for: try #require(URL(string: "https://sub.api.example.com/account/me"))) == nil)
        #expect(await jar.header(for: try #require(URL(string: "https://api.example.com/accounts"))) == nil)

        let restored = CookieJar(storageURL: storage)
        #expect(await restored.header(for: try #require(URL(string: "https://api.example.com/account/me"))) == "session=abc")
    }

    private func temporaryDirectory() -> URL {
        FileManager.default.temporaryDirectory
            .appending(path: "wirebolt-tests-\(UUID().uuidString)", directoryHint: .isDirectory)
    }
}
