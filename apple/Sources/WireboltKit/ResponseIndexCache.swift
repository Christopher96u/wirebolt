import Foundation

/// Swift only schedules background work and forwards cancellation. Rust owns the cache.
public actor ResponseIndexCache {
    public static let shared = ResponseIndexCache()
    private let rust: AppResponseIndexCache
    public init(capacity: Int = 8) { rust = AppResponseIndexCache(capacity: UInt64(max(0, capacity))) }
    public func firstViewport(url: URL, columns: Int, prefix: String = "", wrapping: CodeTextWrapping? = nil) async throws
        -> ResponseTextIndex
    {
        try await read(url: url, columns: columns, prefix: prefix, wrapping: wrapping, first: true)
    }
    public func index(url: URL, columns: Int, prefix: String = "", wrapping: CodeTextWrapping? = nil) async throws -> ResponseTextIndex {
        try await read(url: url, columns: columns, prefix: prefix, wrapping: wrapping, first: false)
    }
    private func read(url: URL, columns: Int, prefix: String, wrapping: CodeTextWrapping?, first: Bool) async throws -> ResponseTextIndex {
        try Task.checkCancellation()
        let cancellation = AppResponseCancellation()
        let rust = rust
        let worker = Task.detached(priority: .userInitiated) {
            try rust.index(
                options: ResponseTextIndex.options(url: url, columns: columns, prefix: prefix, wrapping: wrapping),
                firstViewport: first, adapter: wrapping == nil ? nil : NativeTextShaper(), cancellation: cancellation)
        }
        do {
            let result = try await withTaskCancellationHandler {
                try await worker.value
            } onCancel: {
                cancellation.cancel()
            }
            try Task.checkCancellation()
            return ResponseTextIndex(url: url, columns: columns, wrapping: wrapping, rust: result)
        } catch ResponseError.Cancelled { throw CancellationError() }
    }
}

public enum ResponseTextPresentation {
    public static func usesIndex(byteCount: UInt64, preview: String) -> Bool { appUsesTextIndex(bytes: byteCount, preview: preview) }
}
