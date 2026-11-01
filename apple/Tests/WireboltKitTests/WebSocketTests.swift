import Foundation
import Observation
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
        for value in ["Zh==", "Zm9=", " aG Vs bG8=\n"] {
            let compact = value.filter { !$0.isWhitespace }
            #expect(try WebSocketBinaryEncoding.base64.decode(value) == Data(base64Encoded: compact))
        }
    }

    @Test("Malformed bridge message IDs receive a usable native identity")
    func malformedMessageIdentity() {
        var record = SessionSocketMessage(
            id: "invalid", timestamp: 42, data: Data("message".utf8), binary: false,
            outgoing: true, notice: nil, control: nil, text: "message")
        let message = WebSocketMessage(record)
        #expect(message.data == record.data)
        #expect(message.timestamp == Date(timeIntervalSinceReferenceDate: 42))
        #expect(message.outgoing)
        #expect(message.text == "message")
        record.id = UUID().uuidString
        #expect(WebSocketMessage(record).id == UUID(uuidString: record.id))
    }

    @Test("JSON sends match Foundation acceptance and localized failures")
    @MainActor
    func foundationJSONParity() async {
        let state = WebSocketDocumentState()
        let socket = SocketFixture()
        state.connect(input: input, connector: SocketConnector(socket: socket))
        socket.yield(.connected([]))
        await eventually { state.status == .connected }
        let values = [
            "{\"a\":1,}", "\u{feff}[1,]", "[\"comma,}\",]", "null", "1e-400",
            String(repeating: "[", count: 512) + "0" + String(repeating: "]", count: 512),
            "{", "[,]", "{,}", "[1,,]", " \u{feff}{}", "1e400", "\"\\uD800\"",
            String(repeating: "[", count: 513) + "0" + String(repeating: "]", count: 513),
        ]
        for value in values {
            var failure: String?
            do { _ = try JSONSerialization.jsonObject(with: Data(value.utf8), options: [.fragmentsAllowed]) }
            catch { failure = error.localizedDescription }
            let sent = socket.sent.count
            await state.send(body: .json(value: value))
            #expect(state.errorMessage == failure, "JSON: \(value.prefix(80))")
            #expect(socket.sent.count == sent + (failure == nil ? 1 : 0))
            if failure == nil { #expect(socket.sent.last?.data == Data(value.utf8)) }
        }
        state.disconnect()
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

    @Test("A normal server close keeps the last send error and failed notice")
    @MainActor func closeAfterInvalidJSON() async {
        let state = WebSocketDocumentState()
        let socket = SocketFixture()
        state.connect(input: input, connector: SocketConnector(socket: socket))
        socket.yield(.connected([]))
        await eventually { state.status == .connected }
        await state.send(body: .json(value: "{"))
        let error = state.errorMessage
        #expect(error != nil)
        socket.yield(.closed)
        socket.finish()
        await eventually { state.messages.last?.notice == .failed }
        #expect(state.status == .disconnected)
        #expect(state.errorMessage == error)
        #expect(state.messages.last?.text == error)
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
        await state.send(body: binaryBody)
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

    @Test("Batched socket projections ignore duplicate and cleared messages")
    @MainActor
    func batchedProjection() async throws {
        let state = WebSocketDocumentState()
        let socket = SocketFixture()
        let connector = RetainedSocketConnector(socket: socket)
        state.connect(input: input, connector: connector)
        let retained = try #require(connector.retained)
        let generation = retained.snapshot(since: 0).generation
        retained.connected(generation: generation, headers: [], url: input.url)
        retained.append(generation: generation, data: Data("first".utf8), binary: false, outgoing: false, control: nil)
        let first = retained.snapshot(since: 0)
        socket.yield(.state(first))
        socket.yield(.state(first))
        await eventually { state.messages.count == 2 }
        #expect(state.messages.map(\.text) == ["Connected to \(input.url)", "first"])
        let statusChanged = Mutex(false)
        withObservationTracking { _ = state.status } onChange: { statusChanged.withLock { $0 = true } }
        state.clearMessages()
        socket.yield(.state(first))
        retained.append(generation: generation, data: Data("second".utf8), binary: false, outgoing: false, control: nil)
        socket.yield(.state(retained.snapshot(since: 0)))
        await eventually { state.messages.last?.text == "second" }
        #expect(state.messages.map(\.text) == ["second"])
        #expect(!statusChanged.withLock { $0 })
        retained.finish(generation: generation, error: nil)
        socket.yield(.state(retained.snapshot(since: first.nextSequence)))
        socket.finish()
        await eventually { state.status == .disconnected }
        #expect(state.messages.map(\.notice) == [nil, .disconnected])
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
    func connection(for input: RunInput, state: AppSessionSocket, generation: String) -> any WebSocketTransport { socket }
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

private final class RetainedSocketConnector: WebSocketConnecting, Sendable {
    let socket: SocketFixture
    private let holder = Mutex<AppSessionSocket?>(nil)
    var retained: AppSessionSocket? { holder.withLock { $0 } }
    init(socket: SocketFixture) { self.socket = socket }
    func connection(for input: RunInput, state: AppSessionSocket, generation: String) -> any WebSocketTransport {
        holder.withLock { $0 = state }
        return socket
    }
}

extension WebSocketTests {
    @Test func callbackBufferAcceptsTheFullRustRetentionCap() async throws {
        let retained = AppSessionSocket()
        let generation = try #require(retained.begin())
        let limit = Int(sessionSocketMessageLimit())
        let bytes = Int(sessionSocketMessageBytes())
        for _ in 0..<limit-1 {
            retained.append(generation: generation, data: Data(), binary: false, outgoing: false, control: nil)
        }
        retained.append(generation: generation, data: Data(repeating: 65, count: bytes), binary: true, outgoing: false, control: nil)
        let snapshot = retained.snapshot(since: 0)
        #expect(snapshot.messages.count == limit)
        let buffer = WebSocketEventBuffer()
        #expect(buffer.push(.state(snapshot)))
        guard case let .state(received) = try await buffer.next() else { Issue.record("Expected retained snapshot"); return }
        #expect(received.messages.count == limit)
        #expect(received.messages.last?.data.count == bytes)
        buffer.finish()
    }
}
