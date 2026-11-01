import Foundation
#if canImport(WireboltBridge)
import WireboltBridge
#endif

// Coordinate and cancellation adaptation shared by the response facades.
extension NSRange {
    var responseRange: ResponseTextRange { ResponseTextRange(location: UInt64(max(0, location)), length: UInt64(max(0, length))) }
}
extension ResponseTextRange {
    var nsRange: NSRange { NSRange(location: Int(clamping: location), length: Int(clamping: length)) }
}
func responseCall<T>(_ operation: (AppResponseCancellation) throws -> T) throws -> T {
    try Task.checkCancellation()
    let cancellation = ResponseTaskContext.cancellation ?? AppResponseCancellation()
    do {
        let result = try operation(cancellation)
        try Task.checkCancellation()
        return result
    } catch ResponseError.Cancelled { throw CancellationError() }
}

public struct TextSearchQuery: Equatable, Sendable {
    public var text: String
    public var matchCase: Bool
    public var wholeWord: Bool
    public var regularExpression: Bool
    public static let maximumMatches = 20_000

    public init(text: String = "", matchCase: Bool = false, wholeWord: Bool = false, regularExpression: Bool = false) {
        self.text = text; self.matchCase = matchCase
        self.wholeWord = wholeWord; self.regularExpression = regularExpression
    }
    var rust: SearchQuery { SearchQuery(text: text, matchCase: matchCase, wholeWord: wholeWord, regularExpression: regularExpression) }
    public func matches(in source: String, range: NSRange? = nil) throws -> [NSRange] {
        try responseCall { try appTextSearch(query: rust, source: source, scope: range?.responseRange, cancellation: $0).map(\.nsRange) }
    }
}

private enum ResponseTaskContext {
    @TaskLocal static var cancellation: AppResponseCancellation?
}

/// Native task cancellation signals Rust's atomic token while synchronous Rust
/// work is running. No polling callback crosses the foreign boundary.
func responseAsync<T: Sendable>(_ operation: @Sendable () throws -> T) async throws -> T {
    let cancellation = AppResponseCancellation()
    return try await withTaskCancellationHandler {
        try ResponseTaskContext.$cancellation.withValue(cancellation) { try operation() }
    } onCancel: { cancellation.cancel() }
}
func responseTask<T: Sendable>(priority: TaskPriority, operation: @escaping @Sendable () throws -> T) -> Task<T, any Error> {
    Task.detached(priority: priority) { try await responseAsync(operation) }
}
func responseTask<T: Sendable>(priority: TaskPriority, operation: @escaping @Sendable () -> T) -> Task<T, Never> {
    let cancellation = AppResponseCancellation()
    return Task.detached(priority: priority) {
        await withTaskCancellationHandler {
            ResponseTaskContext.$cancellation.withValue(cancellation) { operation() }
        } onCancel: { cancellation.cancel() }
    }
}
