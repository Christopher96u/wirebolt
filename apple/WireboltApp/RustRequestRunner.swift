import Foundation
import SystemConfiguration

final class RustRequestRunner: @unchecked Sendable, RequestRunner {
    var sessionTransport: AppRuntimeTransport { AppRuntimeTransport.native() }
    private static let proxyMonitor = SystemProxyMonitor()
    private let lock = NSLock()
    private var sessions: [RunID: AppRequestRun] = [:]

    init() {
        DispatchQueue.global(qos: .utility).async { _ = Self.proxyMonitor }
    }

    func proxySettingsChanged() { resetHttpEngines() }
    func resolveValues(_ values: [ValueSource], variables: [String: ValueSource]) async throws -> [String] {
        do {
            return try await sessionTransport.resolveValues(values: values, variables: variables)
        } catch let RuntimeRequestError.Failure(failure) { throw failure }
    }
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { continuation in
            let observer = RequestStreamObserver(continuation: continuation)
            lock.withLock {
                sessions[runID] = AppRequestRun.start(input: input.domain, observer: observer)
            }
            continuation.onTermination = { [weak self] _ in
                self?.lock.withLock { self?.sessions.removeValue(forKey: runID)?.cancel() }
            }
        }
    }
    func cancel(runID: RunID) {
        lock.withLock { sessions[runID]?.cancel() }
    }
}

private final class RequestStreamObserver: SessionRequestObserver {
    let continuation: AsyncThrowingStream<RunEvent, any Error>.Continuation
    init(continuation: AsyncThrowingStream<RunEvent, any Error>.Continuation) {
        self.continuation = continuation
    }
    func event(event: SessionRunEvent) {
        switch event {
        case .prepared(let snapshot): continuation.yield(.prepared(PreparedRunSnapshot(snapshot)))
        case .head(let head): continuation.yield(.head(head))
        case .complete(let completion): continuation.yield(.complete(completion))
        }
    }
    func bytes(data: Data) -> Bool {
        if case .terminated = continuation.yield(.chunk(data)) { return false }
        return true
    }
    func cookies(url: String, headers: [ResponseHeaderValue]) {
        continuation.yield(.cookies(ResponseCookies(url: url, headers: headers)))
    }
    func finish(failure: SessionFailure?) {
        continuation.finish(throwing: failure) }
}

/// The shared HTTP engine snapshots macOS proxy settings. Invalidate pooled engines
/// when those settings change; in-flight runs retain their existing engine.
private final class SystemProxyMonitor: @unchecked Sendable {
    private let store: SCDynamicStore?
    init() {
        store = SCDynamicStoreCreate(nil, "io.github.christopher96u.wirebolt.proxy-settings" as CFString, { _, _, _ in
            resetHttpEngines()
        }, nil)
        if let store {
            SCDynamicStoreSetNotificationKeys(store,
                ["State:/Network/Global/Proxies", "Setup:/Network/Global/Proxies"] as CFArray,
                ["State:/Network/Service/.*/Proxies", "Setup:/Network/Service/.*/Proxies"] as CFArray)
            SCDynamicStoreSetDispatchQueue(store, DispatchQueue(label: "io.github.christopher96u.wirebolt.proxy-settings"))
        }
    }
}
