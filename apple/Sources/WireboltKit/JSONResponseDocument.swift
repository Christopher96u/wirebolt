import Foundation

/// A disposable presentation of a response; the received bytes remain untouched.
public final class JSONResponseDocument: Sendable {
    public let url: URL
    public let byteCount: UInt64
    public let preview: String

    public init(sourceURL: URL) throws {
        let destination = FileManager.default.temporaryDirectory
            .appending(path: "wirebolt-json-\(UUID().uuidString).body")
        guard FileManager.default.createFile(atPath: destination.path, contents: nil) else {
            throw CocoaError(.fileWriteUnknown)
        }
        do {
            let size = try sourceURL.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
            let reader = try FileHandle(forReadingFrom: sourceURL)
            defer { try? reader.close() }
            let writer = try FileHandle(forWritingTo: destination)
            defer { try? writer.close() }
            var parser = JSONPresentationParser(reader: reader, writer: writer,
                outputLimit: max(1024 * 1024, UInt64(size) * 16))
            try parser.run()
            url = destination
            byteCount = parser.written
            let limit = byteCount <= 1024 * 1024 ? 1024 * 1024 : ResponseBodyStore.viewportByteCount
            preview = String(decoding: parser.preview.prefix(limit), as: UTF8.self)
        } catch {
            try? FileManager.default.removeItem(at: destination)
            throw error
        }
    }

    deinit { try? FileManager.default.removeItem(at: url) }
}

enum JSONPresentationError: Error {
    case invalidJSON
    case presentationLimit
}

/// Token spelling is copied verbatim, including duplicate keys and large numbers.
private struct JSONPresentationParser {
    let reader: FileHandle
    let writer: FileHandle
    let outputLimit: UInt64
    private var input: [UInt8] = []
    private var offset = 0
    private var output: [UInt8] = []
    private(set) var preview = Data()
    private(set) var written: UInt64 = 0

    init(reader: FileHandle, writer: FileHandle, outputLimit: UInt64) {
        self.reader = reader; self.writer = writer; self.outputLimit = outputLimit
        output.reserveCapacity(64 * 1024)
    }

    mutating func run() throws {
        try value(depth: 0)
        try whitespace()
        guard try peek() == nil else { throw JSONPresentationError.invalidJSON }
        try flush()
    }

    private mutating func peek() throws -> UInt8? {
        if offset == input.count {
            try Task.checkCancellation()
            input = Array(try reader.read(upToCount: 64 * 1024) ?? Data())
            offset = 0
        }
        return offset < input.count ? input[offset] : nil
    }

    private mutating func take() throws -> UInt8 {
        guard let byte = try peek() else { throw JSONPresentationError.invalidJSON }
        offset += 1
        return byte
    }

    private mutating func emit(_ byte: UInt8) throws {
        guard written + UInt64(output.count) < outputLimit else { throw JSONPresentationError.presentationLimit }
        output.append(byte)
        if output.count == 64 * 1024 { try flush() }
    }

    private mutating func flush() throws {
        guard !output.isEmpty else { return }
        let data = Data(output)
        try writer.write(contentsOf: data)
        preview.append(data.prefix(max(0, 1024 * 1024 - preview.count)))
        written += UInt64(data.count)
        output.removeAll(keepingCapacity: true)
    }

    private mutating func whitespace() throws {
        while let byte = try peek(), [9, 10, 13, 32].contains(byte) { offset += 1 }
    }

    private mutating func newline(_ depth: Int) throws {
        try emit(10)
        for _ in 0..<(depth * 2) { try emit(32) }
    }

    private mutating func expect(_ byte: UInt8) throws {
        guard try take() == byte else { throw JSONPresentationError.invalidJSON }
        try emit(byte)
    }

    private mutating func value(depth: Int) throws {
        guard depth <= 256 else { throw JSONPresentationError.presentationLimit }
        try whitespace()
        guard let byte = try peek() else { throw JSONPresentationError.invalidJSON }
        switch byte {
        case 123: try container(object: true, depth: depth)
        case 91: try container(object: false, depth: depth)
        case 34: try string()
        case 116: for byte in "true".utf8 { try expect(byte) }
        case 102: for byte in "false".utf8 { try expect(byte) }
        case 110: for byte in "null".utf8 { try expect(byte) }
        case 45, 48...57: try number()
        default: throw JSONPresentationError.invalidJSON
        }
    }

    private mutating func container(object: Bool, depth: Int) throws {
        try expect(object ? 123 : 91)
        let closing: UInt8 = object ? 125 : 93
        try whitespace()
        if try peek() == closing { try expect(closing); return }
        while true {
            try newline(depth + 1)
            if object {
                try string()
                try whitespace()
                try expect(58)
                try emit(32)
            }
            try value(depth: depth + 1)
            try whitespace()
            if try peek() == closing {
                try newline(depth)
                try expect(closing)
                return
            }
            try expect(44)
            try whitespace()
        }
    }

    private mutating func string() throws {
        try expect(34)
        while true {
            let byte = try take()
            guard byte >= 32 else { throw JSONPresentationError.invalidJSON }
            try emit(byte)
            if byte == 34 { return }
            if byte == 92 {
                let escape = try take()
                try emit(escape)
                if escape == 117 {
                    for _ in 0..<4 {
                        let hex = try take()
                        guard (48...57).contains(hex) || (65...70).contains(hex) || (97...102).contains(hex)
                        else { throw JSONPresentationError.invalidJSON }
                        try emit(hex)
                    }
                } else if ![34, 47, 92, 98, 102, 110, 114, 116].contains(escape) {
                    throw JSONPresentationError.invalidJSON
                }
            } else if byte >= 128 {
                let continuationCount: Int
                switch byte {
                case 194...223: continuationCount = 1
                case 224...239: continuationCount = 2
                case 240...244: continuationCount = 3
                default: throw JSONPresentationError.invalidJSON
                }
                for position in 0..<continuationCount {
                    let next = try take()
                    guard (128...191).contains(next),
                          !(position == 0 && byte == 224 && next < 160),
                          !(position == 0 && byte == 237 && next > 159),
                          !(position == 0 && byte == 240 && next < 144),
                          !(position == 0 && byte == 244 && next > 143)
                    else { throw JSONPresentationError.invalidJSON }
                    try emit(next)
                }
            }
        }
    }

    private mutating func number() throws {
        if try peek() == 45 { try expect(45) }
        if try peek() == 48 { try expect(48) }
        else { try digits() }
        if try peek() == 46 { try expect(46); try digits() }
        if let byte = try peek(), byte == 101 || byte == 69 {
            try expect(byte)
            if let sign = try peek(), sign == 43 || sign == 45 { try expect(sign) }
            try digits()
        }
    }

    private mutating func digits() throws {
        guard let byte = try peek(), (48...57).contains(byte) else { throw JSONPresentationError.invalidJSON }
        while let byte = try peek(), (48...57).contains(byte) { try expect(byte) }
    }
}
