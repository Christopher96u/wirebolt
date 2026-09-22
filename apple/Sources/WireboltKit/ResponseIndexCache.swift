import Foundation

/// Bounded reuse of layouts for immutable response files. File metadata also
/// invalidates a layout if a streaming response grows at the same URL.
public actor ResponseIndexCache {
    public static let shared = ResponseIndexCache()
    private struct Key: Hashable {
        let url: URL
        let size: Int
        let modified: Date?
        let columns: Int
        let prefix: String
        let wrapping: CodeTextWrapping?
    }
    private struct Entry {
        let key: Key
        let index: ResponseTextIndex
    }
    private struct Flight {
        let id: UUID
        let worker: Task<ResponseTextIndex, any Error>
        var waiters: Set<UUID>
    }
    private var flights: [Key: Flight] = [:]
    private var entries: [Entry] = []
    private let capacity: Int
    public init(capacity: Int = 8) { self.capacity = max(0, capacity) }

    /// A bounded first viewport while the complete sparse index is prepared.
    public func firstViewport(url: URL, columns: Int, prefix: String = "", wrapping: CodeTextWrapping? = nil) async throws -> ResponseTextIndex {
        try Task.checkCancellation()
        let metadata = try FileManager.default.attributesOfItem(atPath: url.path)
        let key = Key(url: url, size: (metadata[.size] as? NSNumber)?.intValue ?? 0, modified: metadata[.modificationDate] as? Date,
            columns: columns, prefix: prefix, wrapping: wrapping)
        if let entry = entries.first(where: { $0.key == key }) { return entry.index }
        let worker = Task.detached(priority: .userInitiated) {
            try ResponseTextIndex(url: url, columns: columns, prefix: prefix, wrapping: wrapping, rowLimit: 256)
        }
        let result = try await withTaskCancellationHandler { try await worker.value } onCancel: { worker.cancel() }
        try Task.checkCancellation()
        return result
    }

    public func index(url: URL, columns: Int, prefix: String = "", wrapping: CodeTextWrapping? = nil) async throws -> ResponseTextIndex {
        try Task.checkCancellation()
        let metadata = try FileManager.default.attributesOfItem(atPath: url.path)
        let key = Key(url: url, size: (metadata[.size] as? NSNumber)?.intValue ?? 0, modified: metadata[.modificationDate] as? Date,
            columns: columns, prefix: prefix, wrapping: wrapping)
        if let position = entries.firstIndex(where: { $0.key == key }) {
            let entry = entries.remove(at: position)
            entries.append(entry)
            return entry.index
        }
        let waiter = UUID()
        let flight: Flight
        if var pending = flights[key] {
            pending.waiters.insert(waiter)
            flights[key] = pending
            flight = pending
        } else {
            let worker = Task.detached(priority: .userInitiated) { try ResponseTextIndex(url: url, columns: columns, prefix: prefix, wrapping: wrapping) }
            flight = Flight(id: UUID(), worker: worker, waiters: [waiter])
            flights[key] = flight
        }
        defer { release(key, flightID: flight.id, waiter: waiter) }
        let index = try await withTaskCancellationHandler { try await flight.worker.value } onCancel: {
            Task { await self.release(key, flightID: flight.id, waiter: waiter) }
        }
        try Task.checkCancellation()
        entries.removeAll { $0.key == key }
        // Sparse checkpoints dominate retained memory. Exclude exceptionally large indexes.
        if capacity > 0, index.rowCount < 4_000_000 {
            entries.append(Entry(key: key, index: index))
            if entries.count > capacity { entries.removeFirst(entries.count - capacity) }
        }
        return index
    }
    private func release(_ key: Key, flightID: UUID, waiter: UUID) {
        guard var flight = flights[key], flight.id == flightID else { return }
        flight.waiters.remove(waiter)
        if flight.waiters.isEmpty {
            flight.worker.cancel()
            flights[key] = nil
        } else { flights[key] = flight }
    }
}

public enum ResponseTextPresentation {
    public static func usesIndex(byteCount: UInt64, preview: String) -> Bool {
        if byteCount > 64 * 1024 { return true }
        var lineLength = 0
        for unit in preview.utf16 {
            lineLength = unit == 10 ? 0 : lineLength + 1
            if lineLength > 2048 { return true }
        }
        return false
    }
}
