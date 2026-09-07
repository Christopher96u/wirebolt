import Foundation
import WireboltStreamFFI

struct RustWebSocketRunner: WebSocketConnecting {
    func connection(for input: RunInput) -> any WebSocketTransport { RustWebSocketConnection(input: input) }
}

private final class RustWebSocketConnection: @unchecked Sendable, WebSocketTransport {
    private let input: RunInput
    private let lock = NSLock()
    private var session: OpaquePointer?
    private var started = false
    private var disconnected = false

    init(input: RunInput) { self.input = input }

    func events() -> AsyncThrowingStream<WebSocketEvent, any Error> {
        AsyncThrowingStream(bufferingPolicy: .bufferingOldest(2)) { continuation in
            do {
                let data = try JSONEncoder().encode(input)
                let box = SocketCallbackBox(owner: self, continuation: continuation)
                let context = Unmanaged.passRetained(box).toOpaque()
                let active: Bool = lock.withLock {
                    guard !started, !disconnected else { return false }
                    started = true
                    session = data.withUnsafeBytes { bytes in
                        wirebolt_socket_start(bytes.bindMemory(to: UInt8.self).baseAddress, UInt(bytes.count), socketEvent, context)
                    }
                    return session != nil
                }
                if !active {
                    Unmanaged<SocketCallbackBox>.fromOpaque(context).release()
                    continuation.finish(throwing: WebSocketUIError("The connection could not be started."))
                }
                continuation.onTermination = { [weak self] _ in self?.disconnect() }
            } catch { continuation.finish(throwing: error) }
        }
    }

    func send(_ data: Data, binary: Bool) -> Bool {
        lock.withLock {
            guard let session, !disconnected else { return false }
            return data.withUnsafeBytes { bytes in
                wirebolt_socket_send(session, binary ? 1 : 0, bytes.bindMemory(to: UInt8.self).baseAddress, UInt(bytes.count)) != 0
            }
        }
    }

    func disconnect() {
        lock.withLock {
            disconnected = true
            if let session { wirebolt_socket_cancel(session) }
        }
    }

    func finished() {
        let address: UInt? = lock.withLock {
            defer { session = nil; disconnected = true }
            return session.map { UInt(bitPattern: $0) }
        }
        if let address {
            DispatchQueue.global(qos: .utility).async {
                if let pointer = OpaquePointer(bitPattern: address) { wirebolt_socket_free(pointer) }
            }
        }
    }
}

private final class SocketCallbackBox: @unchecked Sendable {
    let owner: RustWebSocketConnection
    let continuation: AsyncThrowingStream<WebSocketEvent, any Error>.Continuation
    init(owner: RustWebSocketConnection, continuation: AsyncThrowingStream<WebSocketEvent, any Error>.Continuation) {
        self.owner = owner
        self.continuation = continuation
    }
}

private func socketEvent(_ context: UnsafeMutableRawPointer?, _ kind: UInt8, _ bytes: UnsafePointer<UInt8>?, _ length: UInt) -> UInt8 {
    guard let context else { return 0 }
    let reference = Unmanaged<SocketCallbackBox>.fromOpaque(context)
    let box = kind == 3 || kind == 4 ? reference.takeRetainedValue() : reference.takeUnretainedValue()
    let data = bytes.map { Data(bytes: $0, count: Int(length)) } ?? Data()
    if kind == 3 || kind == 4 {
        if kind == 4 { box.continuation.finish(throwing: WebSocketUIError(String(decoding: data, as: UTF8.self))) }
        else { box.continuation.finish() }
        box.owner.finished()
        return 1
    }
    let event: WebSocketEvent = kind == 0 ? .connected((try? JSONDecoder().decode([ResponseHeader].self, from: data)) ?? []) : .message(data, binary: kind == 2 || kind == 6 || kind >= 7, outgoing: kind == 5 || kind == 6 || kind == 9, control: kind == 7 ? "Ping" : kind >= 8 ? "Pong" : nil)
    switch box.continuation.yield(event) {
    case .enqueued: return 1
    case .dropped:
        box.continuation.finish(throwing: WebSocketUIError("Messages arrived faster than they could be displayed."))
        return 0
    case .terminated: return 0
    @unknown default: return 0
    }
}
