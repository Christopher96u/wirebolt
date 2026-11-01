import Foundation
import CryptoKit
import Testing
@testable import WireboltKit

@Suite("File-backed responses")
struct ResponseStorageTests {
    @Test("Cookie paths and IPv6 hosts match Foundation URL projections")
    func foundationCookieURLs() async throws {
        for source in ["https://h/v1/users/", "https://h/v1/users//", "https://h/v1/users%2F", "http://[::1]/v1/users/", "http://[0:0:0:0:0:0:0:1]/v1/users/", "http://[::0001]/v1/users/", "http://[::FFFF:192.0.2.1]/v1/users/"] {
            let url = try #require(URL(string: source))
            let jar = CookieJar()
            let received = await jar.store(headers: [ResponseHeader(name: "Set-Cookie", value: "name=value")], requestURL: url)
            let path = url.path
            let parent = path.lastIndex(of: "/").map { String(path[..<$0]) }
            #expect(received.first?.path == (parent?.isEmpty == false ? parent : "/"))
            #expect(received.first?.domain == url.host?.lowercased())
            await jar.store(headers: [ResponseHeader(name: "Set-Cookie", value: "scoped=value; Path=/account/")], requestURL: url)
            let destination = try #require(URL(string: "/account/", relativeTo: url))
            #expect(await jar.header(for: destination) == nil)
        }
    }

    @Test("Cookie file names match standardized Foundation symlink resolution")
    func foundationCookieFileNames() throws {
        let root = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let target = root.appending(path: "workspace")
        let alias = root.appending(path: "alias")
        try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: target)
        let workspaces = [URL(fileURLWithPath: "/tmp"), URL(fileURLWithPath: "/private/tmp"), URL(fileURLWithPath: "/var/tmp"), target, alias, alias.appending(path: "missing"), target.appending(path: "../workspace/")]
        for workspace in workspaces {
            let path = workspace.standardizedFileURL.resolvingSymlinksInPath().path
            let hash = SHA256.hash(data: Data(path.utf8)).prefix(16).map { String(format: "%02x", $0) }.joined()
            #expect(CookieJar.storageURL(forWorkspaceAt: workspace, in: root).lastPathComponent == "\(hash).json")
        }
    }

    @Test("Cookie expiry fixtures and Rust parsing match the original Foundation formatters")
    func foundationCookieDates() async throws {
        let fixture = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .appending(path: "../../../crates/wirebolt-app/tests/fixtures/cookie-dates.json")
        let rows = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: fixture)) as? [[Any]])
        let formatters = ["EEE, dd MMM yyyy HH:mm:ss zzz", "EEEE, dd-MMM-yy HH:mm:ss zzz"].map { format in
            let formatter = DateFormatter()
            formatter.locale = Locale(identifier: "en_US_POSIX")
            formatter.timeZone = TimeZone(secondsFromGMT: 0)
            formatter.dateFormat = format
            return formatter
        }
        let origin = try #require(URL(string: "https://example.com/"))
        let jar = CookieJar()
        for row in rows {
            let source = try #require(row[0] as? String)
            let expected = formatters.lazy.compactMap { $0.date(from: source) }.first
            let fixtureDate = (row[1] as? NSNumber).map { Date(timeIntervalSince1970: $0.doubleValue) }
            #expect(expected == fixtureDate, "Fixture: \(source)")
            let received = await jar.store(headers: [ResponseHeader(name: "Set-Cookie", value: "name=value; Expires=\(source)")], requestURL: origin)
            #expect(received.first?.expiresAt == expected, "Cookie: \(source)")
        }
    }

    @Test("Response cookies exclude values retained from earlier hosts")
    func responseCookies() async throws {
        let jar = CookieJar()
        let previousURL = try #require(URL(string: "https://old.example.com"))
        let currentURL = try #require(URL(string: "https://new.example.com"))
        await jar.store(headers: [ResponseHeader(name: "Set-Cookie", value: "old=one; Path=/")], requestURL: previousURL)
        let received = await jar.store(headers: [ResponseHeader(name: "Set-Cookie", value: "new=two; Path=/")], requestURL: currentURL)
        #expect(received.map(\.name) == ["new"])
        #expect(await jar.all().count == 2)
    }

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
        let entries = try await reloaded.list(requestID: "health")
        #expect(entries.count == HistoryRepository.maximumEntriesPerRequest)
        #expect(try await reloaded.count(requestID: "health") == HistoryRepository.maximumEntriesPerRequest)
        #expect(try await reloaded.count(requestID: "missing") == 0)
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
            ResponseHeader(name: "Set-Cookie", value: "remember=1; Path=/account; Secure; Max-Age=3600"),
            ResponseHeader(name: "Set-Cookie", value: "gone=1; Max-Age=0"),
        ], requestURL: origin)

        let sent = await jar.header(for: try #require(URL(string: "https://api.example.com/account/me")))
        #expect(Set(sent?.components(separatedBy: "; ") ?? []) == ["session=abc", "remember=1"])
        #expect(await jar.header(for: try #require(URL(string: "http://api.example.com/account/me"))) == nil)
        #expect(await jar.header(for: try #require(URL(string: "https://sub.api.example.com/account/me"))) == nil)
        #expect(await jar.header(for: try #require(URL(string: "https://api.example.com/accounts"))) == nil)

        // Session cookies end with the app; cookies with an expiry survive a relaunch.
        let restored = CookieJar(storageURL: storage)
        #expect(await restored.header(for: try #require(URL(string: "https://api.example.com/account/me"))) == "remember=1")
        #expect(!String(decoding: try Data(contentsOf: storage), as: UTF8.self).contains("session"))
    }

    private func temporaryDirectory() -> URL {
        FileManager.default.temporaryDirectory
            .appending(path: "wirebolt-tests-\(UUID().uuidString)", directoryHint: .isDirectory)
    }
}

extension ResponseStorageTests {
    @Test func batchedTransportIngressIsVisibleAtDisplayAndReadBoundaries() async throws {
        let store = try ResponseBodyStore(runID: RunID())
        for _ in 0..<100 { try await store.append(Data("café 🚀".utf8)) }
        let expected = String(repeating: "café 🚀", count: 100)
        #expect(await store.size() == UInt64(expected.utf8.count))
        #expect(await store.progress().text == expected)
        try await store.append(Data("tail".utf8))
        #expect(try await store.viewport() == Data((expected + "tail").utf8))
        try await store.finish()
        await #expect(throws: (any Error).self) { try await store.append(Data()) }
    }
}

extension ResponseStorageTests {
    @Test func cookiePathsDecodeURLsAndIgnoreEmptyAttributes() async throws {
        let jar = CookieJar()
        let origin = try #require(URL(string: "https://api.example.com/caf%C3%A9/login"))
        let cookies = await jar.store(headers: [ResponseHeader(name: "Set-Cookie", value: "name=value; Domain=; Path=")], requestURL: origin)
        #expect(cookies.first?.path == "/café")
        #expect(cookies.first?.hostOnly == true)
        let destination = try #require(URL(string: "https://api.example.com/caf%C3%A9/me"))
        #expect(await jar.header(for: destination) == "name=value")
    }
}

extension ResponseStorageTests {
    @Test func cookieDatesKeepLegacyFormatsAndPreEpochExpiry() async throws {
        let jar = CookieJar()
        let origin = try #require(URL(string: "https://example.com/"))
        for (source, seconds) in [
            ("Wed, 31 Dec 1969 23:59:59 GMT", -1.0),
            ("Sunday, 06-Nov-50 08:49:37 GMT", -604_422_623.0),
            ("Wed, 09 Jun 2027 10:18:14 PST", 1_812_565_094.0),
        ] {
            let received = await jar.store(headers: [ResponseHeader(name: "Set-Cookie", value: "name=value; Expires=\(source)")], requestURL: origin)
            #expect(received.first?.expiresAt == Date(timeIntervalSince1970: seconds))
        }
    }
}
