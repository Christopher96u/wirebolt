import Foundation
import WireboltStreamFFI

final class RustRequestRunner: @unchecked Sendable, RequestRunner {
    private let lock = NSLock()
    private var session: OpaquePointer?
    private var terminalBeforeInstall = false

    func events(for input: RunInput) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { continuation in
            let encoded: Data
            do {
                let encoder = JSONEncoder()
                encoder.outputFormatting = [.sortedKeys]
                encoded = try encoder.encode(input)
            } catch {
                continuation.finish(throwing: RustBridgeFailure.encoding)
                return
            }

            let callbackBox = RunCallbackBox(continuation: continuation, runner: self)
            let context = Unmanaged.passRetained(callbackBox).toOpaque()
            let callbacks = wirebolt_run_callbacks(
                on_head: wireboltOnHead,
                on_chunk: wireboltOnChunk,
                on_complete: wireboltOnComplete,
                on_error: wireboltOnError
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
            install(startedSession)
            continuation.onTermination = { [weak self] _ in self?.cancel() }
        }
    }

    func cancel() {
        lock.withLock {
            if let session { wirebolt_run_cancel(session) }
        }
    }

    fileprivate func finishFromCallback() {
        let active: OpaquePointer? = lock.withLock {
            guard let session else {
                terminalBeforeInstall = true
                return nil
            }
            self.session = nil
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

    private func install(_ newSession: OpaquePointer) {
        let shouldFree = lock.withLock {
            if terminalBeforeInstall {
                terminalBeforeInstall = false
                return true
            }
            session = newSession
            return false
        }
        if shouldFree { wirebolt_run_free(newSession) }
    }

    deinit {
        let active = lock.withLock { () -> OpaquePointer? in
            defer { session = nil }
            return session
        }
        if let active { wirebolt_run_free(active) }
    }
}

private final class RunCallbackBox: @unchecked Sendable {
    let continuation: AsyncThrowingStream<RunEvent, any Error>.Continuation
    let runner: RustRequestRunner
    private let bufferLock = NSLock()
    private var pendingBody = Data()

    init(
        continuation: AsyncThrowingStream<RunEvent, any Error>.Continuation,
        runner: RustRequestRunner
    ) {
        self.continuation = continuation
        self.runner = runner
    }

    func append(_ data: Data) {
        let batch: Data? = bufferLock.withLock {
            pendingBody.append(data)
            guard pendingBody.count >= 32 * 1024 else { return nil }
            defer { pendingBody.removeAll(keepingCapacity: true) }
            return pendingBody
        }
        if let batch { continuation.yield(.chunk(batch)) }
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

private let wireboltOnHead: @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<UInt8>?,
    UInt
) -> Void = { context, bytes, length in
    guard let box = callbackBox(context), let data = copiedData(bytes, length) else { return }
    do {
        box.continuation.yield(.head(try JSONDecoder().decode(ResponseHead.self, from: data)))
    } catch {
        box.runner.cancel()
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
    defer { box.runner.finishFromCallback() }
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
    defer { box.runner.finishFromCallback() }
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
