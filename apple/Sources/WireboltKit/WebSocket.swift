import Foundation
import Observation

public enum WebSocketEvent: Sendable {
    case connected([ResponseHeader])
    case message(Data, binary: Bool, outgoing: Bool, control: String? = nil)
    case closed
    case state(SessionSocketSnapshot)
}

public protocol WebSocketTransport: Sendable {
    func events() -> AsyncThrowingStream<WebSocketEvent, any Error>
    func send(_ data: Data, binary: Bool) -> Bool
    func disconnect()
}

public protocol WebSocketConnecting: Sendable {
    func connection(for input: RunInput, state: AppSessionSocket, generation: String) -> any WebSocketTransport
}

/// A connection lifecycle entry shown in the message list.
public typealias WebSocketNotice = SessionSocketNotice

public struct WebSocketMessage: Identifiable, Sendable {
    public let id: UUID
    public let timestamp: Date
    public let data: Data
    public let binary: Bool
    public let outgoing: Bool
    public let notice: WebSocketNotice?
    public let control: String?
    /// UTF-8 text decoded once on append so filtering and rows never decode per render.
    public let text: String
    public var system: Bool { notice != nil }

    init(data: Data, binary: Bool, outgoing: Bool, notice: WebSocketNotice?, control: String?) {
        id = UUID()
        timestamp = Date()
        self.data = data
        self.binary = binary
        self.outgoing = outgoing
        self.notice = notice
        self.control = control
        text = String(decoding: data, as: UTF8.self)
    }

    init(_ record: SessionSocketMessage) {
        // A malformed bridge identity receives a fresh UUID on projection;
        // the document retains that projection across repeated snapshots.
        id = UUID(uuidString: record.id) ?? UUID()
        timestamp = Date(timeIntervalSinceReferenceDate: record.timestamp)
        data = record.data
        binary = record.binary
        outgoing = record.outgoing
        notice = record.notice
        control = record.control
        text = record.text
    }

    public func matches(_ query: String) -> Bool {
        query.isEmpty || text.localizedCaseInsensitiveContains(query) || control?.localizedCaseInsensitiveContains(query) == true
    }
}

public typealias WebSocketStatus = SessionSocketStatus

public enum WebSocketBinaryEncoding: String, CaseIterable, Sendable {
    case base64 = "Base64"
    case hex = "Hex"

    public var contentType: String {
        self == .hex ? "application/octet-stream; encoding=hex" : "application/octet-stream"
    }

    public func decode(_ text: String) throws -> Data {
        do { return try sessionDecodeBinary(text: text, hex: self == .hex) } catch let WorkspaceBridgeError.OperationFailed(reason) {
            throw WebSocketUIError(reason)
        }
    }
}

@MainActor
@Observable
public final class WebSocketDocumentState {
    public private(set) var status = WebSocketStatus.disconnected
    public private(set) var messages: [WebSocketMessage] = []
    public private(set) var errorMessage: String?
    public private(set) var headers: [ResponseHeader] = []
    @ObservationIgnored private let rust = AppSessionSocket()
    @ObservationIgnored private var connection: (any WebSocketTransport)?
    @ObservationIgnored private var receiver: Task<Void, Never>?
    @ObservationIgnored private var generation = ""
    @ObservationIgnored private var firstSequence: UInt64 = 0
    @ObservationIgnored private var nextSequence: UInt64 = 0
    public init() {}
    public func connect(input: RunInput, connector: any WebSocketConnecting) {
        guard let generation = rust.begin() else { return }
        self.generation = generation
        mirror()
        let connection = connector.connection(for: input, state: rust, generation: generation)
        self.connection = connection
        receiver = Task { [weak self] in
            var failure: String?
            var ownsState = false
            do {
                for try await event in connection.events() {
                    guard let self, self.generation == generation, !Task.isCancelled else { break }
                    if case .state(let snapshot) = event {
                        ownsState = true
                        self.apply(snapshot)
                        continue
                    }
                    switch event {
                    case .connected(let headers):
                        self.rust.connected(
                            generation: generation, headers: headers.map { ResponseHeaderValue(name: $0.name, value: $0.value) },
                            url: input.url)
                    case .message(let data, let binary, let outgoing, let control):
                        self.rust.append(generation: generation, data: data, binary: binary, outgoing: outgoing, control: control)
                    case .closed: self.rust.closed(generation: generation)
                    case .state: break
                    }
                    self.mirror()
                }
            } catch { if !Task.isCancelled { failure = error.localizedDescription } }
            connection.disconnect()
            guard let self, self.generation == generation else { return }
            if !ownsState {
                self.rust.finish(generation: generation, error: failure)
                self.mirror()
            }
            self.connection = nil
        }
    }
    public func disconnect() {
        rust.disconnect()
        generation = ""
        receiver?.cancel()
        connection?.disconnect()
        connection = nil
        receiver = nil
        mirror()
    }
    public func send(body: RequestBody) async {
        guard status == .connected, let connection else { return }
        do {
            let value = try await Task.detached { try sessionMessageBody(body: body) }.value
            guard connection.send(value.data, binary: value.binary) else {
                throw WebSocketUIError("The message could not be queued. Check the connection and try again.")
            }
            rust.setError(error: nil)
        } catch let WorkspaceBridgeError.OperationFailed(reason) { rust.setError(error: Self.localizedFailure(reason, body: body)) } catch {
            rust.setError(error: error.localizedDescription)
        }
        mirror()
    }
    public func clearMessages() {
        rust.clear()
        mirror()
    }
    private func mirror() {
        apply(rust.snapshot(since: nextSequence))
    }
    private func apply(_ state: SessionSocketSnapshot) {
        guard state.nextSequence >= nextSequence, state.firstSequence >= firstSequence else { return }
        let next = state.status
        if status != next { status = next }
        if errorMessage != state.error { errorMessage = state.error }
        let nextHeaders = state.headers.map { ResponseHeader(name: $0.name, value: $0.value) }
        if headers != nextHeaders { headers = nextHeaders }
        let removed = min(messages.count, Int(clamping: state.firstSequence - firstSequence))
        if removed > 0 { messages.removeFirst(removed) }
        let start = state.nextSequence - UInt64(state.messages.count)
        let skip = Int(clamping: nextSequence > start ? nextSequence - start : 0)
        if skip < state.messages.count { messages.append(contentsOf: state.messages.dropFirst(skip).map(WebSocketMessage.init)) }
        firstSequence = state.firstSequence
        nextSequence = state.nextSequence
    }
    // Foundation owns localized Cocoa diagnostics. Successful parsing and file I/O
    // stay in Rust; failures use the native localized error wording.
    private static func localizedFailure(_ reason: String, body: RequestBody) -> String {
        do {
            switch body {
            case .json(let value): _ = try JSONSerialization.jsonObject(with: Data(value.utf8), options: [.fragmentsAllowed])
            case .file(let path, _):
                let url = URL(fileURLWithPath: path)
                let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
                if size <= 16 * 1024 * 1024 { _ = try Data(contentsOf: url, options: .mappedIfSafe) }
            default: break
            }
        } catch { return error.localizedDescription }
        return reason
    }
    deinit {
        receiver?.cancel()
        connection?.disconnect()
    }
}

public struct WebSocketUIError: LocalizedError, Sendable {
    public let errorDescription: String?
    public init(_ message: String) { errorDescription = message }
}

/// A byte-bounded handoff from the native callback to one async consumer.
/// Full buffers slow the producer; cancellation wakes both sides immediately.
final class WebSocketEventBuffer: @unchecked Sendable {
    private let condition = NSCondition()
    private let byteLimit: Int
    private let eventLimit: Int
    private var pending: [(WebSocketEvent, Int)?] = []
    private var head = 0
    private var bytes = 0
    private var ended = false
    private var failure: (any Error)?
    private var waiter: CheckedContinuation<WebSocketEvent?, any Error>?

    init(byteLimit: Int = Int(sessionSocketMessageBytes()) + 64 * (Int(sessionSocketMessageLimit()) + 1), eventLimit: Int = 256) {
        self.byteLimit = max(1, byteLimit)
        self.eventLimit = max(1, eventLimit)
    }

    func push(_ event: WebSocketEvent) -> Bool {
        let cost: Int = switch event {
        case let .message(data, _, _, _): data.count + 64
        case let .connected(headers): headers.reduce(64) { $0 + $1.name.utf8.count + $1.value.utf8.count }
        case .closed: 64
        case let .state(snapshot): snapshot.messages.reduce(64) { $0 + $1.data.count + 64 }
        }
        condition.lock()
        guard cost <= byteLimit else {
            condition.unlock()
            finish(WebSocketUIError("The message exceeds the receive buffer limit."))
            return false
        }
        while !ended && waiter == nil && (pending.count - head >= eventLimit || bytes + cost > byteLimit) {
            condition.wait()
        }
        guard !ended else { condition.unlock(); return false }
        if let consumer = waiter {
            waiter = nil
            condition.unlock()
            consumer.resume(returning: event)
        } else {
            pending.append((event, cost)); bytes += cost
            condition.unlock()
        }
        return true
    }

    func next() async throws -> WebSocketEvent? {
        try await withCheckedThrowingContinuation { continuation in
            condition.lock()
            if head < pending.count, let (event, cost) = pending[head] {
                pending[head] = nil; head += 1; bytes -= cost
                if head >= 128 { pending.removeFirst(head); head = 0 }
                condition.broadcast()
                condition.unlock()
                continuation.resume(returning: event)
            } else if ended {
                let error = failure
                condition.unlock()
                if let error { continuation.resume(throwing: error) }
                else { continuation.resume(returning: nil) }
            } else {
                precondition(waiter == nil, "Only one WebSocket receiver is supported")
                waiter = continuation
                condition.broadcast()
                condition.unlock()
            }
        }
    }

    func finish(_ error: (any Error)? = nil, discardingPending: Bool = false) {
        condition.lock()
        ended = true
        if let error { failure = error }
        if discardingPending { pending.removeAll(); head = 0; bytes = 0 }
        let consumer = waiter
        waiter = nil
        let error = failure
        condition.broadcast()
        condition.unlock()
        if let consumer {
            if let error { consumer.resume(throwing: error) }
            else { consumer.resume(returning: nil) }
        }
    }
}
