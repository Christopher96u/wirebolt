import Darwin
import Foundation

struct FeatureWorkloadSamples {
    let workspaceOpenFirstContentMilliseconds: Double
    let workspaceExpandCollapseMilliseconds: [Double]
    let workspaceFilterMilliseconds: [Double]
    let tabSwitchMilliseconds: [Double]
    let tabCloseRestoreMilliseconds: [Double]
    let typingFeedbackMilliseconds: [Double]
    let responseColdFirstViewportMilliseconds: Double
    let responseSearchMilliseconds: [Double]
    let responseRendererSwitchMilliseconds: [Double]
    let responseResidentDeltaMebibytes: Double
    let historyInsertMilliseconds: [Double]
    let historyRestoreMilliseconds: [Double]
    let historyEvictionMilliseconds: [Double]
    let importParseMilliseconds: [Double]
}

enum FeatureWorkloads {
    private static let samples = 15
    private static let responseBytes = 100 * 1024 * 1024

    static func measure() throws -> FeatureWorkloadSamples {
        let workspace = makeWorkspace()
        let openStarted = DispatchTime.now().uptimeNanoseconds
        let firstContent = workspace.first?.name ?? ""
        let openElapsed = milliseconds(since: openStarted)

        var checksum = firstContent.utf8.count
        let expand = (0 ..< samples).map { iteration in
            let started = DispatchTime.now().uptimeNanoseconds
            for row in stride(from: iteration % 2, to: workspace.count, by: 2) {
                checksum &+= workspace[row].id
            }
            return milliseconds(since: started)
        }
        let filter = (0 ..< samples).map { iteration in
            let query = iteration.isMultiple(of: 2) ? "request 99" : "example.com/99"
            let started = DispatchTime.now().uptimeNanoseconds
            checksum &+= workspace.reduce(into: 0) { matches, item in
                if item.searchText.contains(query) {
                    matches += 1
                }
            }
            return milliseconds(since: started)
        }

        let tabs = measureTabs(checksum: &checksum)
        let typing = measureTyping(checksum: &checksum)
        let response = try measureResponse(checksum: &checksum)
        let history = measureHistory(checksum: &checksum)
        let importSamples = try measureImport(checksum: &checksum)

        precondition(checksum > 0, "feature workloads were optimized away")
        return FeatureWorkloadSamples(
            workspaceOpenFirstContentMilliseconds: openElapsed,
            workspaceExpandCollapseMilliseconds: expand,
            workspaceFilterMilliseconds: filter,
            tabSwitchMilliseconds: tabs.switches,
            tabCloseRestoreMilliseconds: tabs.closeRestore,
            typingFeedbackMilliseconds: typing,
            responseColdFirstViewportMilliseconds: response.firstViewport,
            responseSearchMilliseconds: response.search,
            responseRendererSwitchMilliseconds: response.renderers,
            responseResidentDeltaMebibytes: response.residentDelta,
            historyInsertMilliseconds: history.insert,
            historyRestoreMilliseconds: history.restore,
            historyEvictionMilliseconds: history.eviction,
            importParseMilliseconds: importSamples
        )
    }

    private struct WorkspaceItem {
        let id: Int
        let name: String
        let url: String
        let searchText: String
    }

    private static func makeWorkspace() -> [WorkspaceItem] {
        (0 ..< 10_000).map {
            let name = "Request \($0)"
            let url = "https://example.com/\($0)"
            return WorkspaceItem(
                id: $0,
                name: name,
                url: url,
                searchText: "\(name)\u{0}\(url)".lowercased()
            )
        }
    }

    private static func measureTabs(
        checksum: inout Int
    ) -> (switches: [Double], closeRestore: [Double]) {
        let tabs = (0 ..< 30).map { "request-\($0)" }
        let lookup = Dictionary(uniqueKeysWithValues: tabs.enumerated().map { ($0.element, $0.offset) })
        let switches = (0 ..< samples).map { iteration in
            let started = DispatchTime.now().uptimeNanoseconds
            for offset in 0 ..< tabs.count {
                checksum &+= lookup[tabs[(offset + iteration) % tabs.count]] ?? 0
            }
            return milliseconds(since: started)
        }
        let closeRestore = (0 ..< samples).map { iteration in
            var restored = tabs
            let started = DispatchTime.now().uptimeNanoseconds
            restored.remove(at: iteration % restored.count)
            restored.insert("request-restored", at: iteration % restored.count)
            checksum &+= restored.count
            return milliseconds(since: started)
        }
        return (switches, closeRestore)
    }

    private static func measureTyping(checksum: inout Int) -> [Double] {
        let input = "https://api.example.com/v1/users?limit=100"
        return (0 ..< samples).map { _ in
            var value = ""
            let started = DispatchTime.now().uptimeNanoseconds
            for character in input {
                value.append(character)
                checksum &+= value.utf8.count
            }
            return milliseconds(since: started) / Double(input.count)
        }
    }

    private static func measureResponse(
        checksum: inout Int
    ) throws -> (
        firstViewport: Double,
        search: [Double],
        renderers: [Double],
        residentDelta: Double
    ) {
        let fileURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("wirebolt-performance-\(UUID().uuidString)")
        FileManager.default.createFile(atPath: fileURL.path, contents: nil)
        defer { try? FileManager.default.removeItem(at: fileURL) }

        let writer = try FileHandle(forWritingTo: fileURL)
        try writer.truncate(atOffset: UInt64(responseBytes))
        try writer.seek(toOffset: UInt64(responseBytes - 16))
        try writer.write(contentsOf: Data("wirebolt-needle!".utf8))
        try writer.close()

        let residentBefore = residentMebibytes()
        let reader = try FileHandle(forReadingFrom: fileURL)
        let viewportStarted = DispatchTime.now().uptimeNanoseconds
        let viewport = try reader.read(upToCount: 32 * 1024) ?? Data()
        let firstViewport = milliseconds(since: viewportStarted)
        checksum &+= viewport.count
        try reader.close()

        let descriptor = Darwin.open(fileURL.path, O_RDONLY)
        guard descriptor >= 0 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        defer { Darwin.close(descriptor) }

        let needle = Array("wirebolt-needle!".utf8)
        var buffer = [UInt8](repeating: 0, count: 256 * 1024)
        let search = try (0 ..< samples).map { _ in
            guard Darwin.lseek(descriptor, 0, SEEK_SET) >= 0 else {
                throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
            }
            let started = DispatchTime.now().uptimeNanoseconds
            var found = false
            while !found {
                let byteCount = buffer.withUnsafeMutableBytes {
                    Darwin.read(descriptor, $0.baseAddress, $0.count)
                }
                guard byteCount >= 0 else {
                    throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
                }
                guard byteCount > 0 else { break }
                found = containsNeedle(buffer, validCount: byteCount, needle: needle)
            }
            if found { checksum &+= 1 }
            return milliseconds(since: started)
        }

        let renderers = (0 ..< samples).map { iteration in
            let started = DispatchTime.now().uptimeNanoseconds
            switch iteration % 4 {
            case 0: checksum &+= String(decoding: viewport, as: UTF8.self).utf8.count
            case 1: checksum &+= viewport.prefix(512).map { String(format: "%02x", $0) }.joined().count
            case 2: checksum &+= viewport.withUnsafeBytes { $0.count }
            default: checksum &+= viewport.count
            }
            return milliseconds(since: started)
        }
        let residentDelta = max(residentMebibytes() - residentBefore, 0)
        return (firstViewport, search, renderers, residentDelta)
    }

    private static func containsNeedle(
        _ bytes: [UInt8],
        validCount: Int,
        needle: [UInt8]
    ) -> Bool {
        guard needle.count <= validCount else { return false }
        return bytes.withUnsafeBufferPointer { haystack in
            needle.withUnsafeBufferPointer { target in
                guard let haystackBase = haystack.baseAddress,
                      let targetBase = target.baseAddress
                else { return false }
                for offset in 0 ... (validCount - needle.count) {
                    if haystackBase[offset] == targetBase[0],
                       memcmp(haystackBase + offset, targetBase, needle.count) == 0
                    {
                        return true
                    }
                }
                return false
            }
        }
    }

    private struct HistoryEntry {
        let id: Int
        let status: Int
    }

    private static func measureHistory(
        checksum: inout Int
    ) -> (insert: [Double], restore: [Double], eviction: [Double]) {
        let entries = (0 ..< 100).map { HistoryEntry(id: $0, status: 200 + ($0 % 5)) }
        let insert = (0 ..< samples).map { _ in
            var history: [HistoryEntry] = []
            history.reserveCapacity(100)
            let started = DispatchTime.now().uptimeNanoseconds
            history.append(contentsOf: entries)
            checksum &+= history.count
            return milliseconds(since: started)
        }
        let restore = (0 ..< samples).map { iteration in
            let started = DispatchTime.now().uptimeNanoseconds
            checksum &+= entries.first(where: { $0.id == 99 - iteration })?.status ?? 0
            return milliseconds(since: started)
        }
        let eviction = (0 ..< samples).map { iteration in
            var history = entries
            let started = DispatchTime.now().uptimeNanoseconds
            history.append(HistoryEntry(id: 100 + iteration, status: 200))
            history.removeFirst(history.count - 100)
            checksum &+= history.count
            return milliseconds(since: started)
        }
        return (insert, restore, eviction)
    }

    private static func measureImport(checksum: inout Int) throws -> [Double] {
        let requests: [[String: Any]] = (0 ..< 5_000).map {
            [
                "name": "Request \($0)",
                "request": ["method": "GET", "url": "https://example.com/\($0)"],
            ]
        }
        let document: [String: Any] = [
            "info": ["name": "Large fixture", "schema": "postman-v2"],
            "item": requests,
        ]
        let data = try JSONSerialization.data(withJSONObject: document)
        return try (0 ..< samples).map { _ in
            let started = DispatchTime.now().uptimeNanoseconds
            let parsed = try JSONSerialization.jsonObject(with: data) as? [String: Any]
            checksum &+= (parsed?["item"] as? [[String: Any]])?.count ?? 0
            return milliseconds(since: started)
        }
    }

    private static func milliseconds(since started: UInt64) -> Double {
        Double(DispatchTime.now().uptimeNanoseconds - started) / 1_000_000
    }

    private static func residentMebibytes() -> Double {
        var info = mach_task_basic_info()
        var count = mach_msg_type_number_t(MemoryLayout<mach_task_basic_info>.size) / 4
        let status = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(MACH_TASK_BASIC_INFO), $0, &count)
            }
        }
        guard status == KERN_SUCCESS else { return 0 }
        return Double(info.resident_size) / 1_048_576
    }
}
