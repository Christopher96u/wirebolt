import Foundation
import Synchronization
import Testing
@testable import WireboltKit

@Suite("WebSocket documents")
struct WebSocketTests {
    @Test("Templated WebSocket URLs and notes survive reopening without duplicate tabs")
    @MainActor
    func persistedDocumentKind() {
        let store = DocumentSessionStore()
        let draft = RequestDraft(id: "socket", url: "{{endpoint}}", webSocket: true, note: "café 東京 🚀")
        let first = store.open(draft: draft, collectionID: "fixture")
        #expect(first.kind == .webSocket)
        #expect(first.note == draft.note)
        #expect(!first.isDirty)
        #expect(store.open(draft: draft, collectionID: "fixture").id == first.id)
        first.note += " edited"
        #expect(first.isDirty)
        #expect(first.draft.note == "café 東京 🚀 edited")
    }

    @Test("Binary decoding preserves bytes and rejects malformed input")
    func binaryDecoding() throws {
        let bytes = Data([0, 1, 127, 128, 255])
        #expect(try WebSocketBinaryEncoding.base64.decode("AA F/gP8=\n") == bytes)
        #expect(try WebSocketBinaryEncoding.hex.decode("00 01 7f 80 FF\n") == bytes)
        for malformed in ["a", "zz", "0x00", "東京"] {
            #expect(throws: WebSocketUIError.self) { try WebSocketBinaryEncoding.hex.decode(malformed) }
        }
        #expect(throws: WebSocketUIError.self) { try WebSocketBinaryEncoding.base64.decode("AA==!") }
        #expect(try WebSocketBinaryEncoding.hex.decode("").isEmpty)
    }

    @Test("Reconnect ignores events from the disconnected transport")
    @MainActor
    func reconnect() async {
        let state = WebSocketDocumentState()
        let first = SocketFixture()
        state.connect(input: input, connector: SocketConnector(socket: first))
        #expect(state.status == .connecting)
        first.yield(.connected([ResponseHeader(name: "upgrade", value: "websocket")]))
        await eventually { state.status == .connected }
        #expect(state.headers.count == 1)
        state.disconnect()
        #expect(first.disconnections > 0)
        let second = SocketFixture()
        state.connect(input: input, connector: SocketConnector(socket: second))
        #expect(state.headers.isEmpty)
        first.yield(.message(Data("stale".utf8), binary: false, outgoing: false))
        first.finish()
        second.yield(.connected([]))
        second.yield(.message(Data("current".utf8), binary: false, outgoing: false))
        await eventually { state.messages.contains { $0.data == Data("current".utf8) } }
        #expect(!state.messages.contains { $0.data == Data("stale".utf8) })
        #expect(state.status == .connected)
        second.finish()
        await eventually { state.status == .disconnected }
    }

    @Test("Text, JSON, binary and file messages use the selected transport representation")
    @MainActor
    func sendsPayloads() async throws {
        let state = WebSocketDocumentState()
        let socket = SocketFixture()
        state.connect(input: input, connector: SocketConnector(socket: socket))
        socket.yield(.connected([]))
        await eventually { state.status == .connected }
        await state.send(body: .text(contentType: "text/plain", value: "café 東京"))
        await state.send(body: .json(value: "{\"count\":1}"))
        let binaryBody = RequestBody.text(contentType: WebSocketBinaryEncoding.hex.contentType, value: "00 FF")
        let restoredBody = try JSONDecoder().decode(RequestBody.self, from: JSONEncoder().encode(binaryBody))
        await state.send(body: restoredBody)
        let file = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try Data([1, 2, 3]).write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }
        await state.send(body: .file(path: file.path, contentType: nil))
        #expect(socket.sent.map(\.data) == [Data("café 東京".utf8), Data("{\"count\":1}".utf8), Data([0, 255]), Data([1, 2, 3])])
        #expect(socket.sent.map(\.binary) == [false, false, true, true])
        await state.send(body: .json(value: "{"))
        #expect(state.errorMessage != nil)
        #expect(socket.sent.count == 4)
        socket.acceptsMessages = false
        await state.send(body: .text(contentType: nil, value: "queue is full"))
        #expect(state.errorMessage?.contains("queued") == true)
        state.disconnect()
    }

    @Test("Message history is bounded and control frames retain their identity")
    @MainActor
    func boundsHistory() async {
        let state = WebSocketDocumentState()
        let socket = SocketFixture()
        state.connect(input: input, connector: SocketConnector(socket: socket))
        socket.yield(.connected([]))
        for index in 0..<1_010 {
            socket.yield(.message(Data(String(index).utf8), binary: false, outgoing: false))
        }
        socket.yield(.message(Data([42]), binary: true, outgoing: false, control: "Ping"))
        await eventually { state.messages.last?.control == "Ping" }
        #expect(state.messages.count == 1_000)
        #expect(state.messages.first?.data == Data("11".utf8))
        state.clearMessages()
        #expect(state.messages.isEmpty)
        state.disconnect()
    }

    @Test("Lifecycle notices are typed and message text is decoded once for search")
    @MainActor
    func noticesAndSearchText() async {
        let state = WebSocketDocumentState()
        let socket = SocketFixture()
        state.connect(input: input, connector: SocketConnector(socket: socket))
        socket.yield(.connected([]))
        socket.yield(.message(Data("Café 東京".utf8), binary: false, outgoing: true))
        socket.yield(.message(Data([7]), binary: true, outgoing: false, control: "Ping"))
        await eventually { state.messages.count == 3 }
        #expect(state.messages.map(\.notice) == [.connected, nil, nil])
        #expect(state.messages[0].system && !state.messages[1].system)
        #expect(state.messages[1].text == "Café 東京")
        #expect(state.messages[1].matches("café") && state.messages[1].matches(""))
        #expect(!state.messages[1].matches("paris"))
        #expect(state.messages[2].matches("ping"))
        state.disconnect()
        #expect(state.messages.last?.notice == .disconnected)

        let failing = SocketFixture()
        state.connect(input: input, connector: SocketConnector(socket: failing))
        failing.yield(.connected([]))
        await eventually { state.status == .connected }
        failing.fail(WebSocketUIError("Connection reset"))
        await eventually { state.status == .disconnected }
        #expect(state.messages.last?.notice == .failed)
        #expect(state.messages.last?.text == "Connection reset")
    }

    private var input: RunInput { RunInput(draft: RequestDraft(url: "ws://127.0.0.1/echo"), variables: [:]) }

    @MainActor
    private func eventually(_ condition: @MainActor () -> Bool) async {
        for _ in 0..<500 {
            if condition() { return }
            try? await Task.sleep(for: .milliseconds(1))
        }
        #expect(condition())
    }
}

private struct SocketConnector: WebSocketConnecting {
    let socket: SocketFixture
    func connection(for input: RunInput) -> any WebSocketTransport { socket }
}

private final class SocketFixture: WebSocketTransport {
    struct Sent: Sendable { let data: Data; let binary: Bool }
    struct State { var sent: [Sent] = []; var disconnections = 0; var accepts = true }
    private let state = Mutex(State())
    private let channel = AsyncThrowingStream<WebSocketEvent, any Error>.makeStream()
    var sent: [Sent] { state.withLock { $0.sent } }
    var disconnections: Int { state.withLock { $0.disconnections } }
    var acceptsMessages: Bool {
        get { state.withLock { $0.accepts } }
        set { state.withLock { $0.accepts = newValue } }
    }
    func events() -> AsyncThrowingStream<WebSocketEvent, any Error> { channel.stream }
    func yield(_ event: WebSocketEvent) { channel.continuation.yield(event) }
    func finish() { channel.continuation.finish() }
    func fail(_ error: any Error) { channel.continuation.finish(throwing: error) }
    func send(_ data: Data, binary: Bool) -> Bool {
        state.withLock {
            guard $0.accepts else { return false }
            $0.sent.append(Sent(data: data, binary: binary))
            return true
        }
    }
    func disconnect() { state.withLock { $0.disconnections += 1 } }
}
