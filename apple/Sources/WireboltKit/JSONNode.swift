import Foundation

/// One row of the JSON tree renderer. Parsing preserves document key order and
/// number spelling, and stops materialising rows after `nodeLimit` so a huge
/// response cannot allocate an unbounded tree.
struct JSONNode: Identifiable, Sendable {
    static let nodeLimit = 200_000

    let id: String
    let key: String
    let type: String
    let value: String
    let children: [JSONNode]?

    static func makeRoot(from text: String) -> [JSONNode] {
        makeRoot(from: Data(text.utf8))
    }

    static func makeRoot(from data: Data, nodeLimit: Int = nodeLimit) -> [JSONNode] {
        data.withUnsafeBytes { raw in
            var parser = Parser(bytes: raw.bindMemory(to: UInt8.self), budget: max(1, nodeLimit))
            guard let root = try? parser.document() else { return [] }
            return [root]
        }
    }

    private enum ParseError: Error { case invalid, cancelled }

    private struct Parser {
        let bytes: UnsafeBufferPointer<UInt8>
        var budget: Int
        var offset = 0
        var steps = 0

        init(bytes: UnsafeBufferPointer<UInt8>, budget: Int) {
            self.bytes = bytes
            self.budget = budget
        }

        mutating func document() throws -> JSONNode {
            if bytes.starts(with: [0xEF, 0xBB, 0xBF]) { offset = 3 }
            let root = try value(key: "Root", path: "root", depth: 0)
            whitespace()
            guard offset == bytes.count else { throw ParseError.invalid }
            return root
        }

        private var peek: UInt8? { offset < bytes.count ? bytes[offset] : nil }

        private mutating func whitespace() {
            while offset < bytes.count, [9, 10, 13, 32].contains(bytes[offset]) { offset += 1 }
        }

        private mutating func expect(_ byte: UInt8) throws {
            guard peek == byte else { throw ParseError.invalid }
            offset += 1
        }

        private mutating func checkCancellation() throws {
            steps += 1
            if steps & 0xFFF == 0, Task.isCancelled { throw ParseError.cancelled }
        }

        private static func escape(_ key: String) -> String {
            key.replacingOccurrences(of: "~", with: "~0").replacingOccurrences(of: "/", with: "~1")
        }

        mutating func value(key: String, path: String, depth: Int) throws -> JSONNode {
            guard depth <= 512 else { throw ParseError.invalid }
            try checkCancellation()
            budget -= 1
            whitespace()
            switch peek {
            case UInt8(ascii: "{"), UInt8(ascii: "["):
                let object = peek == UInt8(ascii: "{")
                offset += 1
                let closing = object ? UInt8(ascii: "}") : UInt8(ascii: "]")
                var children: [JSONNode] = []
                var omitted = 0
                whitespace()
                if peek == closing { offset += 1 } else {
                    while true {
                        whitespace()
                        let childKey: String
                        if object {
                            childKey = try string()
                            whitespace()
                            try expect(UInt8(ascii: ":"))
                        } else { childKey = String(children.count + omitted) }
                        if budget > 0 {
                            children.append(try value(key: childKey, path: path + "/" + Self.escape(childKey), depth: depth + 1))
                        } else {
                            try skipValue(depth: depth + 1)
                            omitted += 1
                        }
                        whitespace()
                        if peek == closing { offset += 1; break }
                        try expect(UInt8(ascii: ","))
                    }
                }
                let count = children.count + omitted
                if omitted > 0 {
                    children.append(JSONNode(id: path + "/…", key: "…", type: "Truncated",
                        value: "\(omitted) more items not shown", children: nil))
                }
                let type = object ? "Object" : "Array"
                return JSONNode(id: path, key: key, type: type, value: "\(type)(\(count) items)", children: children)
            case UInt8(ascii: "\""):
                return JSONNode(id: path, key: key, type: "String", value: try string(), children: nil)
            case UInt8(ascii: "t"):
                try literal("true")
                return JSONNode(id: path, key: key, type: "Boolean", value: "true", children: nil)
            case UInt8(ascii: "f"):
                try literal("false")
                return JSONNode(id: path, key: key, type: "Boolean", value: "false", children: nil)
            case UInt8(ascii: "n"):
                try literal("null")
                return JSONNode(id: path, key: key, type: "Null", value: "null", children: nil)
            default:
                let start = offset
                try number()
                return JSONNode(id: path, key: key, type: "Number",
                    value: String(decoding: UnsafeBufferPointer(rebasing: bytes[start..<offset]), as: UTF8.self), children: nil)
            }
        }

        private mutating func skipValue(depth: Int) throws {
            guard depth <= 512 else { throw ParseError.invalid }
            try checkCancellation()
            whitespace()
            switch peek {
            case UInt8(ascii: "{"), UInt8(ascii: "["):
                let object = peek == UInt8(ascii: "{")
                offset += 1
                let closing = object ? UInt8(ascii: "}") : UInt8(ascii: "]")
                whitespace()
                if peek == closing { offset += 1; return }
                while true {
                    whitespace()
                    if object {
                        _ = try string(decode: false)
                        whitespace()
                        try expect(UInt8(ascii: ":"))
                    }
                    try skipValue(depth: depth + 1)
                    whitespace()
                    if peek == closing { offset += 1; return }
                    try expect(UInt8(ascii: ","))
                }
            case UInt8(ascii: "\""): _ = try string(decode: false)
            case UInt8(ascii: "t"): try literal("true")
            case UInt8(ascii: "f"): try literal("false")
            case UInt8(ascii: "n"): try literal("null")
            default: try number()
            }
        }

        private mutating func literal(_ word: StaticString) throws {
            let count = word.utf8CodeUnitCount
            guard offset + count <= bytes.count else { throw ParseError.invalid }
            for index in 0..<count where bytes[offset + index] != word.utf8Start[index] { throw ParseError.invalid }
            offset += count
        }

        private mutating func number() throws {
            if peek == UInt8(ascii: "-") { offset += 1 }
            if peek == UInt8(ascii: "0") { offset += 1 } else { try digits() }
            if peek == UInt8(ascii: ".") { offset += 1; try digits() }
            if peek == UInt8(ascii: "e") || peek == UInt8(ascii: "E") {
                offset += 1
                if peek == UInt8(ascii: "+") || peek == UInt8(ascii: "-") { offset += 1 }
                try digits()
            }
        }

        private mutating func digits() throws {
            guard let byte = peek, (48...57).contains(byte) else { throw ParseError.invalid }
            while let byte = peek, (48...57).contains(byte) { offset += 1 }
        }

        private mutating func hex4() throws -> UInt32 {
            guard offset + 4 <= bytes.count else { throw ParseError.invalid }
            var value: UInt32 = 0
            for _ in 0..<4 {
                let byte = bytes[offset]
                let digit: UInt8 = switch byte {
                case 48...57: byte - 48
                case 65...70: byte - 55
                case 97...102: byte - 87
                default: throw ParseError.invalid
                }
                value = value << 4 | UInt32(digit)
                offset += 1
            }
            return value
        }

        /// Decodes a string token; malformed UTF-8 becomes U+FFFD like the text renderers.
        private mutating func string(decode: Bool = true) throws -> String {
            try expect(UInt8(ascii: "\""))
            let start = offset
            var decoded: [UInt8]?
            while true {
                guard let byte = peek else { throw ParseError.invalid }
                if byte == UInt8(ascii: "\"") {
                    defer { offset += 1 }
                    guard decode else { return "" }
                    if let decoded { return String(decoding: decoded, as: UTF8.self) }
                    return String(decoding: UnsafeBufferPointer(rebasing: bytes[start..<offset]), as: UTF8.self)
                }
                guard byte >= 32 else { throw ParseError.invalid }
                guard byte == UInt8(ascii: "\\") else {
                    if decode { decoded?.append(byte) }
                    offset += 1
                    continue
                }
                if decode, decoded == nil { decoded = Array(bytes[start..<offset]) }
                offset += 1
                guard let escape = peek else { throw ParseError.invalid }
                offset += 1
                let scalar: UInt32
                switch escape {
                case UInt8(ascii: "\""), UInt8(ascii: "\\"), UInt8(ascii: "/"): scalar = UInt32(escape)
                case UInt8(ascii: "b"): scalar = 8
                case UInt8(ascii: "f"): scalar = 12
                case UInt8(ascii: "n"): scalar = 10
                case UInt8(ascii: "r"): scalar = 13
                case UInt8(ascii: "t"): scalar = 9
                case UInt8(ascii: "u"):
                    let unit = try hex4()
                    if (0xD800..<0xDC00).contains(unit), offset + 6 <= bytes.count,
                       bytes[offset] == UInt8(ascii: "\\"), bytes[offset + 1] == UInt8(ascii: "u") {
                        let mark = offset
                        offset += 2
                        let low = try hex4()
                        if (0xDC00..<0xE000).contains(low) {
                            scalar = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
                        } else { offset = mark; scalar = 0xFFFD }
                    } else if (0xD800..<0xE000).contains(unit) { scalar = 0xFFFD } else { scalar = unit }
                default: throw ParseError.invalid
                }
                if decode, let character = Unicode.Scalar(scalar) {
                    decoded?.append(contentsOf: UTF8.encode(character)!)
                }
            }
        }
    }
}
