import Foundation

struct RustWebSocketRunner: WebSocketConnecting {
    func connection(for input: RunInput) -> any WebSocketTransport { RustWebSocketConnection(input: input) }
}

private final class RustWebSocketConnection: @unchecked Sendable, WebSocketTransport {
    private let input: RunInput
    private let lock = NSLock()
    private var session: AppSocketConnection?
    private var disconnected = false
    private let buffer = WebSocketEventBuffer()

    init(input: RunInput) { self.input = input }

    func events() -> AsyncThrowingStream<WebSocketEvent, any Error> {
        lock.withLock {
            if session == nil && !disconnected {
                session = AppSocketConnection.start(input: input.domain, observer: SocketStreamObserver(buffer: buffer))
            }
        }
        return AsyncThrowingStream(unfolding: { [self] in
            try await withTaskCancellationHandler {
                try Task.checkCancellation()
                return try await buffer.next()
            } onCancel: { self.disconnect() }
        })
    }

    func send(_ data: Data, binary: Bool) -> Bool {
        lock.withLock { !disconnected && (session?.send(data: data, binary: binary) ?? false) }
    }
    func disconnect() {
        buffer.finish(CancellationError(), discardingPending: true)
        lock.withLock {
            disconnected = true
            session?.cancel()
            session = nil
        }
    }
}

private final class SocketStreamObserver: SessionSocketObserver {
    let buffer: WebSocketEventBuffer
    init(buffer: WebSocketEventBuffer) { self.buffer = buffer }
    func event(event: SocketEvent) -> Bool {
        switch event {
        case .connected(let headers):
            buffer.push(.connected(headers.map { ResponseHeader(name: $0.name, value: $0.value) }))
        case .message(let data, let binary, let outgoing, let control):
            buffer.push(.message(data, binary: binary, outgoing: outgoing, control: control))
        }
    }
    func finish(reason: String?) { buffer.finish(reason.map(WebSocketUIError.init)) }
}
