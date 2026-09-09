import Foundation

public actor ResponseBodyStore {
    public static let viewportByteCount = 32 * 1024
    public static let searchChunkByteCount = 256 * 1024

    public nonisolated let url: URL
    private let ownsFile: Bool
    private var handle: FileHandle?
    private var byteCount: UInt64 = 0
    private var jsonDocument: JSONResponseDocument?
    private var attemptedJSON = false

    public init(runID: RunID, directory: URL = FileManager.default.temporaryDirectory) throws {
        url = directory.appending(path: "wirebolt-response-\(runID.description).body")
        ownsFile = true
        guard FileManager.default.createFile(atPath: url.path, contents: nil) else {
            throw CocoaError(.fileWriteUnknown)
        }
        handle = try FileHandle(forWritingTo: url)
    }

    public init(existingURL: URL) throws {
        url = existingURL
        ownsFile = false
        let values = try existingURL.resourceValues(forKeys: [.fileSizeKey])
        byteCount = UInt64(values.fileSize ?? 0)
        handle = nil
    }

    deinit {
        try? handle?.close()
        if ownsFile { try? FileManager.default.removeItem(at: url) }
    }

    public func append(_ data: Data) throws {
        jsonDocument = nil
        attemptedJSON = false
        try handle?.write(contentsOf: data)
        byteCount += UInt64(data.count)
    }

    public func finish() throws {
        try handle?.synchronize()
        try handle?.close()
        handle = nil
    }

    public func size() -> UInt64 { byteCount }

    public func formattedJSON() throws -> JSONResponseDocument? {
        guard handle == nil else { return nil }
        if attemptedJSON { return jsonDocument }
        do { jsonDocument = try JSONResponseDocument(sourceURL: url) }
        catch is JSONPresentationError { jsonDocument = nil }
        attemptedJSON = true
        return jsonDocument
    }

    public func viewport(offset: UInt64 = 0, length: Int = viewportByteCount) throws -> Data {
        let reader = try FileHandle(forReadingFrom: url)
        defer { try? reader.close() }
        try reader.seek(toOffset: min(offset, byteCount))
        return try reader.read(upToCount: max(length, 0)) ?? Data()
    }

    public func countOccurrences(of query: String) throws -> Int {
        let needle = Data(query.utf8)
        guard needle.isEmpty == false else { return 0 }
        let reader = try FileHandle(forReadingFrom: url)
        defer { try? reader.close() }
        var carry = Data()
        var matches = 0
        while true {
            let chunk = try autoreleasepool {
                try reader.read(upToCount: Self.searchChunkByteCount) ?? Data()
            }
            guard chunk.isEmpty == false else { break }
            var searchable = carry
            searchable.append(chunk)
            var range = searchable.startIndex ..< searchable.endIndex
            while let match = searchable.range(of: needle, options: [], in: range) {
                matches += 1
                range = match.upperBound ..< searchable.endIndex
            }
            carry = searchable.suffix(max(needle.count - 1, 0))
        }
        return matches
    }

    public func export(to destination: URL) throws {
        try handle?.synchronize()
        if FileManager.default.fileExists(atPath: destination.path) {
            try FileManager.default.removeItem(at: destination)
        }
        try FileManager.default.copyItem(at: url, to: destination)
    }

    public func remove() {
        try? handle?.close()
        handle = nil
        if ownsFile { try? FileManager.default.removeItem(at: url) }
    }
}

public struct RunHistoryEntry: Codable, Equatable, Identifiable, Sendable {
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

    private let root: URL
    private let indexURL: URL
    private var entries: [RunHistoryEntry]

    public init(root: URL? = nil) {
        let applicationSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        )[0]
        self.root = root ?? applicationSupport
            .appending(path: "Wirebolt", directoryHint: .isDirectory)
            .appending(path: "History", directoryHint: .isDirectory)
        indexURL = self.root.appending(path: "index.json")
        if let data = try? Data(contentsOf: indexURL),
           let decoded = try? JSONDecoder().decode([RunHistoryEntry].self, from: data)
        {
            entries = decoded
        } else {
            entries = []
        }
    }

    public func record(
        runID: RunID,
        requestID: String,
        prepared: PreparedRunSnapshot,
        responseHead: ResponseHead?,
        completion: RunCompletion?,
        failure: RunFailure?,
        body: ResponseBodyStore
    ) async throws {
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let bodyURL = root.appending(path: "\(runID.description).body")
        try await body.export(to: bodyURL)
        entries.append(RunHistoryEntry(
            id: runID.description,
            requestID: requestID,
            createdAt: Date(),
            prepared: prepared,
            responseHead: responseHead,
            completion: completion,
            failure: failure,
            bodyPath: bodyURL.path
        ))
        let matching = entries.filter { $0.requestID == requestID }.sorted { $0.createdAt > $1.createdAt }
        for evicted in matching.dropFirst(Self.maximumEntriesPerRequest) {
            entries.removeAll { $0.id == evicted.id }
            try? FileManager.default.removeItem(atPath: evicted.bodyPath)
        }
        var totalBytes = entries.reduce(UInt64(0)) { total, entry in
            total + Self.fileSize(atPath: entry.bodyPath)
        }
        for evicted in entries.sorted(by: { $0.createdAt < $1.createdAt })
            where totalBytes > Self.maximumStoredBytes
        {
            let bytes = Self.fileSize(atPath: evicted.bodyPath)
            entries.removeAll { $0.id == evicted.id }
            try? FileManager.default.removeItem(atPath: evicted.bodyPath)
            totalBytes = totalBytes > bytes ? totalBytes - bytes : 0
        }
        try persist()
    }

    public func list(requestID: String) -> [RunHistoryEntry] {
        entries.filter { $0.requestID == requestID }.sorted { $0.createdAt > $1.createdAt }
    }

    public func clear(requestID: String) throws {
        let removed = entries.filter { $0.requestID == requestID }
        entries.removeAll { $0.requestID == requestID }
        for entry in removed { try? FileManager.default.removeItem(atPath: entry.bodyPath) }
        try persist()
    }

    private func persist() throws {
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let data = try JSONEncoder().encode(entries)
        try data.write(to: indexURL, options: .atomic)
    }

    private static func fileSize(atPath path: String) -> UInt64 {
        let attributes = try? FileManager.default.attributesOfItem(atPath: path)
        return (attributes?[.size] as? NSNumber)?.uint64Value ?? 0
    }
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
    private var cookies: [CookieSnapshot] = []
    private let storageURL: URL?

    public init(storageURL: URL? = nil) {
        self.storageURL = storageURL
        if let storageURL,
           let data = try? Data(contentsOf: storageURL),
           let saved = try? JSONDecoder().decode([CookieSnapshot].self, from: data)
        {
            cookies = saved
        }
    }

    @discardableResult
    public func store(headers: [ResponseHeader], requestURL: URL) -> [CookieSnapshot] {
        var received: [CookieSnapshot] = []
        for header in headers where header.name.caseInsensitiveCompare("set-cookie") == .orderedSame {
            guard let cookie = Self.parse(header.value, requestURL: requestURL) else { continue }
            received.append(cookie)
            cookies.removeAll { $0.id == cookie.id }
            if cookie.expiresAt.map({ $0 > Date() }) != false { cookies.append(cookie) }
        }
        removeExpired()
        persist()
        return received
    }

    public func header(for url: URL) -> String? {
        removeExpired()
        let host = url.host?.lowercased() ?? ""
        let requestPath = url.path.isEmpty ? "/" : url.path
        let values = cookies.filter { cookie in
            (host == cookie.domain || (!cookie.hostOnly && host.hasSuffix(".\(cookie.domain)")))
                && Self.pathMatches(requestPath, cookiePath: cookie.path)
                && (!cookie.secure || url.scheme == "https")
        }
        .sorted { $0.path.count > $1.path.count }
        .map { "\($0.name)=\($0.value)" }
        return values.isEmpty ? nil : values.joined(separator: "; ")
    }

    public func all() -> [CookieSnapshot] {
        removeExpired()
        return cookies
    }

    private func removeExpired() {
        cookies.removeAll { $0.expiresAt.map { $0 <= Date() } == true }
    }

    private static func parse(_ source: String, requestURL: URL) -> CookieSnapshot? {
        let segments = source.split(separator: ";").map {
            $0.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        guard let first = segments.first,
              let separator = first.firstIndex(of: "=")
        else { return nil }
        let name = String(first[..<separator])
        let value = String(first[first.index(after: separator)...])
        var domain = requestURL.host?.lowercased() ?? ""
        var hostOnly = true
        var path = defaultPath(for: requestURL)
        var secure = false
        var httpOnly = false
        var sameSite = ""
        var expiresAt: Date?
        for attribute in segments.dropFirst() {
            let pieces = attribute.split(separator: "=", maxSplits: 1).map(String.init)
            switch pieces[0].lowercased() {
            case "domain" where pieces.count == 2:
                domain = pieces[1].trimmingCharacters(in: CharacterSet(charactersIn: ".")).lowercased()
                hostOnly = false
            case "path" where pieces.count == 2: path = pieces[1]
            case "secure": secure = true
            case "httponly": httpOnly = true
            case "samesite" where pieces.count == 2: sameSite = pieces[1]
            case "max-age" where pieces.count == 2:
                expiresAt = Int(pieces[1]).map { Date().addingTimeInterval(TimeInterval($0)) }
            case "expires" where pieces.count == 2:
                expiresAt = parseCookieDate(pieces[1])
            default: break
            }
        }
        let requestHost = requestURL.host?.lowercased() ?? ""
        guard hostOnly || requestHost == domain || requestHost.hasSuffix(".\(domain)") else {
            return nil
        }
        return CookieSnapshot(
            name: name,
            value: value,
            domain: domain,
            hostOnly: hostOnly,
            path: path,
            secure: secure,
            httpOnly: httpOnly,
            sameSite: sameSite,
            expiresAt: expiresAt
        )
    }

    private static func pathMatches(_ requestPath: String, cookiePath: String) -> Bool {
        guard requestPath.hasPrefix(cookiePath) else { return false }
        return requestPath.count == cookiePath.count
            || cookiePath.hasSuffix("/")
            || requestPath.dropFirst(cookiePath.count).first == "/"
    }

    private static func defaultPath(for url: URL) -> String {
        let path = url.path
        guard path.hasPrefix("/"), path != "/",
              let separator = path.lastIndex(of: "/"), separator != path.startIndex
        else { return "/" }
        return String(path[..<separator])
    }

    private static func parseCookieDate(_ value: String) -> Date? {
        for format in ["EEE, dd MMM yyyy HH:mm:ss zzz", "EEEE, dd-MMM-yy HH:mm:ss zzz"] {
            let formatter = DateFormatter()
            formatter.locale = Locale(identifier: "en_US_POSIX")
            formatter.timeZone = TimeZone(secondsFromGMT: 0)
            formatter.dateFormat = format
            if let date = formatter.date(from: value) { return date }
        }
        return nil
    }

    private func persist() {
        guard let storageURL else { return }
        do {
            try FileManager.default.createDirectory(
                at: storageURL.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            try JSONEncoder().encode(cookies).write(to: storageURL, options: .atomic)
        } catch {
            // Cookie persistence is best effort; a failed cache write must never fail a request.
        }
    }
}
