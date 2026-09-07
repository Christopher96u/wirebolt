import Foundation
import Observation

public enum WebSocketEvent: Sendable {
    case connected([ResponseHeader])
    case message(Data, binary: Bool, outgoing: Bool, control: String? = nil)
    case closed
}

public protocol WebSocketTransport: Sendable {
    func events() -> AsyncThrowingStream<WebSocketEvent, any Error>
    func send(_ data: Data, binary: Bool) -> Bool
    func disconnect()
}

public protocol WebSocketConnecting: Sendable {
    func connection(for input: RunInput) -> any WebSocketTransport
}

public struct WebSocketMessage: Identifiable, Sendable {
    public let id = UUID()
    public let timestamp = Date()
    public let data: Data
    public let binary: Bool
    public let outgoing: Bool
    public let system: Bool
    public let control: String?
}

public enum WebSocketStatus: Equatable, Sendable {
    case disconnected, connecting, connected
}

public enum WebSocketBinaryEncoding: String, CaseIterable, Sendable {
    case base64 = "Base64"
    case hex = "Hex"

    public var contentType: String {
        self == .hex ? "application/octet-stream; encoding=hex" : "application/octet-stream"
    }

    public func decode(_ text: String) throws -> Data {
        let compact = text.filter { !$0.isWhitespace }
        switch self {
        case .base64:
            guard let data = Data(base64Encoded: compact) else {
                throw WebSocketUIError("Enter a valid Base64 binary message.")
            }
            return data
        case .hex:
            let bytes = Array(compact.utf8)
            guard bytes.count.isMultiple(of: 2) else { throw WebSocketUIError("Enter a valid hexadecimal binary message.") }
            func nibble(_ byte: UInt8) -> UInt8? {
                switch byte {
                case 48...57: byte - 48
                case 65...70: byte - 55
                case 97...102: byte - 87
                default: nil
                }
            }
            var data = Data(capacity: bytes.count / 2)
            for offset in stride(from: 0, to: bytes.count, by: 2) {
                guard let high = nibble(bytes[offset]), let low = nibble(bytes[offset + 1]) else {
                    throw WebSocketUIError("Enter a valid hexadecimal binary message.")
                }
                data.append(high << 4 | low)
            }
            return data
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
    @ObservationIgnored private var connection: (any WebSocketTransport)?
    @ObservationIgnored private var receiver: Task<Void, Never>?
    @ObservationIgnored private var generation = UUID()
    private var retainedBytes = 0

    public init() {}

    public func connect(input: RunInput, connector: any WebSocketConnecting) {
        guard status == .disconnected else { return }
        let generation = UUID()
        self.generation = generation
        errorMessage = nil
        headers = []
        status = .connecting
        let connection = connector.connection(for: input)
        self.connection = connection
        receiver = Task { [weak self] in
            do {
                for try await event in connection.events() {
                    guard let self, self.generation == generation, !Task.isCancelled else { break }
                    switch event {
                    case let .connected(headers):
                        self.headers = headers
                        self.status = .connected
                        self.append(data: Data("Connected to \(input.url)".utf8), binary: false, outgoing: false, system: true)
                    case let .message(data, binary, outgoing, control):
                        self.append(data: data, binary: binary, outgoing: outgoing, control: control)
                    case .closed: self.status = .disconnected
                    }
                }
            } catch {
                if self?.generation == generation, !Task.isCancelled { self?.errorMessage = error.localizedDescription }
            }
            connection.disconnect()
            if self?.generation == generation {
                self?.append(data: Data((self?.errorMessage ?? "Disconnected").utf8), binary: false, outgoing: false, system: true)
                self?.status = .disconnected
                self?.connection = nil
            }
        }
    }

    public func disconnect() {
        if status == .connected { append(data: Data("Disconnected".utf8), binary: false, outgoing: false, system: true) }
        generation = UUID()
        receiver?.cancel()
        connection?.disconnect()
        connection = nil
        receiver = nil
        status = .disconnected
    }

    public func send(body: RequestBody) async {
        guard status == .connected, let connection else { return }
        do {
            let data: Data
            let binary: Bool
            switch body {
            case let .file(path, _):
                data = try await Task.detached {
                    let url = URL(fileURLWithPath: path)
                    let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
                    guard size <= 16 * 1024 * 1024 else { throw WebSocketUIError("The message exceeds the 16 MB limit.") }
                    return try Data(contentsOf: url, options: .mappedIfSafe)
                }.value
                binary = true
            case let .text(contentType, value):
                binary = contentType?.hasPrefix("application/octet-stream") == true
                if binary {
                    let encoding: WebSocketBinaryEncoding = contentType == WebSocketBinaryEncoding.hex.contentType ? .hex : .base64
                    data = try encoding.decode(value)
                } else { data = Data(value.utf8) }
            case let .json(value):
                data = Data(value.utf8)
                _ = try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
                binary = false
            case let .raw(_, value), let .xml(value), let .html(value): data = Data(value.utf8); binary = false
            case .empty: data = Data(); binary = false
            default: throw WebSocketUIError("Choose a Text, JSON, Binary or File message.")
            }
            guard data.count <= 16 * 1024 * 1024 else { throw WebSocketUIError("The message exceeds the 16 MB limit.") }
            guard connection.send(data, binary: binary) else { throw WebSocketUIError("The message could not be queued. Check the connection and try again.") }
            errorMessage = nil
        } catch { errorMessage = error.localizedDescription }
    }

    private func append(data: Data, binary: Bool, outgoing: Bool, system: Bool = false, control: String? = nil) {
        while !messages.isEmpty && (messages.count >= 1_000 || retainedBytes + data.count > 16 * 1024 * 1024) {
            retainedBytes -= messages.removeFirst().data.count
        }
        messages.append(WebSocketMessage(data: data, binary: binary, outgoing: outgoing, system: system, control: control))
        retainedBytes += data.count
    }

    public func clearMessages() { messages = []; retainedBytes = 0 }

    deinit { receiver?.cancel(); connection?.disconnect() }
}

public struct WebSocketUIError: LocalizedError, Sendable {
    public let errorDescription: String?
    public init(_ message: String) { errorDescription = message }
}
