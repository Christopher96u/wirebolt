import Foundation

public actor ResponseBodyStore {
    public static let viewportByteCount = 32 * 1024
    public static let searchChunkByteCount = 256 * 1024
    public nonisolated let url: URL
    nonisolated let rust: AppResponseBody
    // Only native object identity is mirrored here; Rust caches the presentations.
    private var presentations: [String: JSONResponseDocument] = [:]
    // The current transport delivers Swift Data chunks. Batch that ingress until
    // display cadence or 256 KiB; Rust owns all durable body state and previews.
    private var pending: [Data] = []
    private var pendingBytes = 0
    private var writable = true
    private var writeError: (any Error)?
    public init(runID: RunID, directory: URL = FileManager.default.temporaryDirectory) throws {
        rust = try AppResponseBody(runId: runID.description, directory: directory.path)
        url = URL(fileURLWithPath: rust.path())
    }
    init(rust: AppResponseBody) {
        self.rust = rust
        url = URL(fileURLWithPath: rust.path())
        writable = false
    }
    public init(existingURL: URL) throws {
        rust = try AppResponseBody.existing(path: existingURL.path)
        url = existingURL
        writable = false
    }
    public func append(_ data: Data) throws {
        if let writeError { throw writeError }
        guard writable else { throw CocoaError(.fileWriteUnknown) }
        guard !data.isEmpty else { return }
        presentations = [:]
        pending.append(data)
        pendingBytes += data.count
        if pendingBytes >= Self.searchChunkByteCount || pending.count >= 64 { try flushIngress() }
    }
    func flushIngress() throws {
        if let writeError { throw writeError }
        guard !pending.isEmpty else { return }
        do { try rust.appendBatch(chunks: pending) } catch {
            writeError = error
            throw error
        }
        pending.removeAll(keepingCapacity: true)
        pendingBytes = 0
    }
    public struct Progress: Sendable {
        public let bytes: UInt64
        public let data: Data
        public let text: String
        public let truncated: Bool
    }
    public func progress() -> Progress {
        do { try flushIngress() } catch { writeError = error }
        let result = rust.progress()
        return Progress(bytes: result.bytes, data: result.data, text: result.text, truncated: result.truncated)
    }
    public func finish() throws {
        try flushIngress()
        try rust.finish()
        writable = false
    }
    public func size() -> UInt64 { rust.size() + UInt64(pendingBytes) }
    public func formattedJSON(decodingUnicodeEscapes decodes: Bool = false) async throws -> JSONResponseDocument? {
        guard !writable else { return nil }
        let rust = rust
        guard let document = try await responseAsync({ try responseCall { try rust.formattedJson(decode: decodes, cancellation: $0) } })
        else { return nil }
        let metadata = document.metadata()
        if let cached = presentations[metadata.path] { return cached }
        let result = JSONResponseDocument(document)
        presentations[metadata.path] = result
        return result
    }
    public func viewport(offset: UInt64 = 0, length: Int = viewportByteCount) throws -> Data {
        try flushIngress()
        return try rust.viewport(offset: offset, length: UInt64(max(0, length)))
    }
    public func countOccurrences(of query: String) throws -> Int {
        try flushIngress()
        return Int(clamping: try rust.countOccurrences(query: query))
    }
    public func export(to destination: URL) throws {
        try flushIngress()
        try rust.export(destination: destination.path)
    }
    public func remove() {
        pending = []
        pendingBytes = 0
        writable = false
        rust.remove()
    }
}

public struct RunHistoryEntry: Equatable, Identifiable, Sendable {
    public let id: String
    public let requestID: String
    public let createdAt: Date
    public let prepared: PreparedRunSnapshot
    public let responseHead: ResponseHead?
    public let completion: RunCompletion?
    public let failure: RunFailure?
    public let bodyPath: String
}

public actor HistoryRepository {
    public static let maximumEntriesPerRequest = 100
    public static let maximumStoredBytes: UInt64 = 2 * 1024 * 1024 * 1024
    nonisolated let rust: AppRunHistory
    public init(root: URL? = nil) {
        let directory =
            root
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appending(path: "Wirebolt/History", directoryHint: .isDirectory)
        rust = AppRunHistory(root: directory.path)
    }
    public func record(
        runID: RunID, requestID: String, prepared: PreparedRunSnapshot, responseHead: ResponseHead?,
        completion: RunCompletion?, failure: RunFailure?, body: ResponseBodyStore
    ) async throws {
        let entry = RunHistoryEntry(
            id: runID.description, requestID: requestID, createdAt: Date(), prepared: prepared,
            responseHead: responseHead, completion: completion, failure: failure, bodyPath: "")
        try await body.flushIngress()
        try rust.recordTyped(entry: entry.sessionRecord, body: body.rust)
    }
    public func list(requestID: String) throws -> [RunHistoryEntry] {
        try rust.listTyped(requestId: requestID).map(RunHistoryEntry.init)
    }
    public func count(requestID: String) throws -> Int { Int(clamping: try rust.count(requestId: requestID)) }
    public func clear(requestID: String) throws { try rust.clear(requestId: requestID) }
}

public struct CookieSnapshot: Codable, Equatable, Identifiable, Sendable {
    public var id: String { "\(domain)\u{0}\(path)\u{0}\(name)" }
    public let name: String
    public let value: String
    public let domain: String
    public let hostOnly: Bool
    public let path: String
    public let secure: Bool
    public let httpOnly: Bool
    public let sameSite: String
    public let expiresAt: Date?
}

public actor CookieJar {
    nonisolated let rust: AppCookieJar
    public init(storageURL: URL? = nil) { rust = AppCookieJar(storage: storageURL?.path) }
    public nonisolated static func storageURL(forWorkspaceAt workspaceURL: URL, in directory: URL) -> URL {
        URL(fileURLWithPath: appCookieStoragePath(workspace: workspaceURL.standardizedFileURL.path, directory: directory.path))
    }
    @discardableResult
    public func store(headers: [ResponseHeader], requestURL: URL) -> [CookieSnapshot] {
        rust.store(headers: headers.map { ResponseHeaderValue(name: $0.name, value: $0.value) }, requestUrl: requestURL.absoluteString).map(
            CookieSnapshot.init)
    }
    public func header(for url: URL) -> String? { rust.header(url: url.absoluteString) }
    public func all() -> [CookieSnapshot] { rust.all().map(CookieSnapshot.init) }
    public func delete(id: CookieSnapshot.ID) { rust.delete(id: id) }
    public func removeAll() { rust.removeAll() }
}
extension CookieSnapshot {
    var sessionRecord: StoredCookie {
        StoredCookie(
            name: name, value: value, domain: domain, hostOnly: hostOnly, path: path,
            secure: secure, httpOnly: httpOnly, sameSite: sameSite,
            expiresAt: expiresAt?.timeIntervalSince1970
        )
    }
    init(_ cookie: StoredCookie) {
        self.init(
            name: cookie.name, value: cookie.value, domain: cookie.domain, hostOnly: cookie.hostOnly,
            path: cookie.path, secure: cookie.secure, httpOnly: cookie.httpOnly, sameSite: cookie.sameSite,
            expiresAt: cookie.expiresAt.map { Date(timeIntervalSince1970: $0) })
    }
}

extension RunHistoryEntry {
    var sessionRecord: SessionHistoryEntry {
        SessionHistoryEntry(
            id: id, requestId: requestID, createdAt: createdAt.timeIntervalSinceReferenceDate,
            prepared: prepared.sessionRecord, responseHead: responseHead?.sessionRecord, completion: completion?.sessionRecord,
            failure: failure?.sessionRecord, bodyPath: bodyPath)
    }
    init(_ record: SessionHistoryEntry) {
        self.init(
            id: record.id, requestID: record.requestId, createdAt: Date(timeIntervalSinceReferenceDate: record.createdAt),
            prepared: PreparedRunSnapshot(record.prepared), responseHead: record.responseHead.map(ResponseHead.init),
            completion: record.completion.map(RunCompletion.init), failure: record.failure.map(RunFailure.init), bodyPath: record.bodyPath)
    }
}
