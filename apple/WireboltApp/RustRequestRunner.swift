import Foundation
import SystemConfiguration
import WireboltStreamFFI

final class RustRequestRunner: @unchecked Sendable, RequestRunner {
    private static let proxyMonitor = SystemProxyMonitor()

    init() {
        // SCDynamicStore setup is kept off the launch path; it only invalidates pooled engines.
        DispatchQueue.global(qos: .utility).async { _ = Self.proxyMonitor }
    }

    func proxySettingsChanged() { resetHttpEngines() }

    private let lock = NSLock()
    private var sessions: [RunID: OpaquePointer] = [:]
    private var terminalBeforeInstall: Set<RunID> = []

    func resolveValues(_ values: [ValueSource], variables: [String: ValueSource]) async throws -> [String] {
        // The common literal URL needs no bridge work or credential lookup.
        if values.allSatisfy({ if case let .literal(text) = $0 { return !text.contains("{{") }; return false }) {
            return values.map(\.editableValue)
        }
        return try await Task.detached(priority: .userInitiated) {
            do {
                let encoder = JSONEncoder()
                return try resolveRequestValues(valuesJson: String(decoding: encoder.encode(values), as: UTF8.self),
                    variablesJson: String(decoding: encoder.encode(variables), as: UTF8.self))
            } catch let RequestPreparationError.InvalidRequest(reason) {
                throw (try? JSONDecoder().decode(RunFailure.self, from: Data(reason.utf8))) ?? RunFailure(kind: "invalid_request", issues: [])
            }
        }.value
    }

    func events(
        for input: RunInput,
        runID: RunID
    ) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { continuation in
            PerformanceProbe.beginPrepare(runID: runID.description)
            let encoded: Data
            do {
                let encoder = JSONEncoder()
                encoder.outputFormatting = [.sortedKeys]
                encoded = try encoder.encode(input)
            } catch {
                PerformanceProbe.endPrepare(runID: runID.description)
                continuation.finish(throwing: RustBridgeFailure.encoding)
                return
            }
            PerformanceProbe.endPrepare(runID: runID.description)

            let callbackBox = RunCallbackBox(
                runID: runID,
                continuation: continuation,
                runner: self
            )
            let context = Unmanaged.passRetained(callbackBox).toOpaque()
            let callbacks = wirebolt_run_callbacks(
                on_prepared: wireboltOnPrepared,
                on_head: wireboltOnHead,
                on_chunk: wireboltOnChunk,
                on_complete: wireboltOnComplete,
                on_error: wireboltOnError,
                on_cookies: wireboltOnCookies
            )
            let startedSession = encoded.withUnsafeBytes { bytes in
                wirebolt_run_start(
                    bytes.bindMemory(to: UInt8.self).baseAddress,
                    UInt(bytes.count),
                    callbacks,
                    context
                )
            }
            guard let startedSession else {
                Unmanaged<RunCallbackBox>.fromOpaque(context).release()
                continuation.finish(throwing: RustBridgeFailure.start)
                return
            }
            PerformanceProbe.dispatched(runID: runID.description)
            install(startedSession, runID: runID)
            continuation.onTermination = { [weak self] _ in self?.cancel(runID: runID) }
        }
    }

    func cancel(runID: RunID) {
        lock.withLock {
            if let session = sessions[runID] { wirebolt_run_cancel(session) }
        }
    }

    fileprivate func finishFromCallback(runID: RunID) {
        let active: OpaquePointer? = lock.withLock {
            guard let session = sessions.removeValue(forKey: runID) else {
                terminalBeforeInstall.insert(runID)
                return nil
            }
            return session
        }
        if let active {
            let address = UInt(bitPattern: active)
            DispatchQueue.global(qos: .utility).async {
                if let session = OpaquePointer(bitPattern: address) {
                    wirebolt_run_free(session)
                }
            }
        }
    }

    private func install(_ newSession: OpaquePointer, runID: RunID) {
        let shouldFree = lock.withLock {
            if terminalBeforeInstall.remove(runID) != nil {
                return true
            }
            sessions[runID] = newSession
            return false
        }
        if shouldFree { wirebolt_run_free(newSession) }
    }

    deinit {
        let active = lock.withLock { () -> [OpaquePointer] in
            defer { sessions.removeAll() }
            return Array(sessions.values)
        }
        for session in active { wirebolt_run_free(session) }
    }
}

private final class RunCallbackBox: @unchecked Sendable {
    let runID: RunID
    let continuation: AsyncThrowingStream<RunEvent, any Error>.Continuation
    let runner: RustRequestRunner
    private let bufferLock = NSLock()
    private var pendingBody = Data()
    private var emittedFirstViewport = false

    init(
        runID: RunID,
        continuation: AsyncThrowingStream<RunEvent, any Error>.Continuation,
        runner: RustRequestRunner
    ) {
        self.runID = runID
        self.continuation = continuation
        self.runner = runner
    }

    func append(_ data: Data) {
        let result: (Data?, Bool) = bufferLock.withLock {
            pendingBody.append(data)
            let isFirstViewport = !emittedFirstViewport && !data.isEmpty
            emittedFirstViewport = emittedFirstViewport || isFirstViewport
            guard pendingBody.count >= 32 * 1024 else { return (nil, isFirstViewport) }
            defer { pendingBody.removeAll(keepingCapacity: true) }
            return (pendingBody, isFirstViewport)
        }
        if result.1 { PerformanceProbe.firstViewport(runID: runID.description) }
        if let batch = result.0 { continuation.yield(.chunk(batch)) }
    }

    func flush() {
        let remainder: Data? = bufferLock.withLock {
            guard !pendingBody.isEmpty else { return nil }
            defer { pendingBody.removeAll(keepingCapacity: false) }
            return pendingBody
        }
        if let remainder { continuation.yield(.chunk(remainder)) }
    }
}

private enum RustBridgeFailure: Error {
    case encoding
    case start
    case invalidCallback
}

private let wireboltOnPrepared: @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?,
    UInt
) -> Void = { context, bytes, length in
    guard let box = callbackBox(context), let data = copiedData(bytes, length) else { return }
    do {
        box.continuation.yield(.prepared(
            try JSONDecoder().decode(PreparedRunSnapshot.self, from: data)
        ))
    } catch {
        box.runner.cancel(runID: box.runID)
    }
}

private let wireboltOnHead: @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?,
    UInt
) -> Void = { context, bytes, length in
    guard let box = callbackBox(context), let data = copiedData(bytes, length) else { return }
    do {
        PerformanceProbe.firstHead(runID: box.runID.description)
        box.continuation.yield(.head(try JSONDecoder().decode(ResponseHead.self, from: data)))
    } catch {
        box.runner.cancel(runID: box.runID)
    }
}

private let wireboltOnChunk: @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?,
    UInt
) -> UInt8 = { context, bytes, length in
    guard let box = callbackBox(context), let data = copiedData(bytes, length) else { return 0 }
    box.append(data)
    return 1
}

private let wireboltOnComplete: @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?,
    UInt
) -> Void = { context, bytes, length in
    guard let context else { return }
    let box = Unmanaged<RunCallbackBox>.fromOpaque(context).takeRetainedValue()
    defer { box.runner.finishFromCallback(runID: box.runID) }
    box.flush()
    guard let data = copiedData(bytes, length),
          let completion = try? JSONDecoder().decode(RunCompletion.self, from: data)
    else {
        box.continuation.finish(throwing: RustBridgeFailure.invalidCallback)
        return
    }
    box.continuation.yield(.complete(completion))
    box.continuation.finish()
}

private let wireboltOnError: @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?,
    UInt
) -> Void = { context, bytes, length in
    guard let context else { return }
    let box = Unmanaged<RunCallbackBox>.fromOpaque(context).takeRetainedValue()
    defer { box.runner.finishFromCallback(runID: box.runID) }
    box.flush()
    guard let data = copiedData(bytes, length),
          let failure = try? JSONDecoder().decode(RunFailure.self, from: data)
    else {
        box.continuation.finish(throwing: RustBridgeFailure.invalidCallback)
        return
    }
    box.continuation.finish(throwing: failure)
}

private func callbackBox(_ context: UnsafeMutableRawPointer?) -> RunCallbackBox? {
    context.map { Unmanaged<RunCallbackBox>.fromOpaque($0).takeUnretainedValue() }
}

private func copiedData(_ bytes: UnsafePointer<UInt8>?, _ length: UInt) -> Data? {
    guard length == 0 || bytes != nil else { return nil }
    guard let bytes else { return Data() }
    return Data(bytes: bytes, count: Int(length))
}

private let wireboltOnCookies: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, UInt) -> Void = { context, bytes, length in
    guard let box = callbackBox(context), let data = copiedData(bytes, length) else { return }
    do { box.continuation.yield(.cookies(try JSONDecoder().decode(ResponseCookies.self, from: data))) }
    catch { box.runner.cancel(runID: box.runID) }
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
