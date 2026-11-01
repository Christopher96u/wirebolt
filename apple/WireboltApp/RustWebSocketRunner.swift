import Foundation

struct RustWebSocketRunner: WebSocketConnecting {
    func connection(for input: RunInput, state: AppSessionSocket, generation: String) -> any WebSocketTransport {
        RustWebSocketConnection(input: input, state: state, generation: generation)
    }
}

private final class RustWebSocketConnection: @unchecked Sendable, WebSocketTransport {
    private let input: RunInput
    private let state: AppSessionSocket
    private let generation: String
    private let lock = NSLock()
    private var session: AppSocketConnection?
    private var disconnected = false
    private let buffer = WebSocketEventBuffer()

    init(input: RunInput, state: AppSessionSocket, generation: String) {
        self.input = input
        self.state = state
        self.generation = generation
    }

    func events() -> AsyncThrowingStream<WebSocketEvent, any Error> {
        lock.withLock {
            if session == nil && !disconnected {
                session = AppSocketConnection.start(input: input.domain, state: state, generation: generation, observer: SocketStreamObserver(buffer: buffer))
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
    func update(snapshot: SessionSocketSnapshot) -> Bool { buffer.push(.state(snapshot)) }
    func finish(reason: String?) { buffer.finish() }
}
