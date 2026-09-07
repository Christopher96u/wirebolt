import Foundation

/// Sparse checkpoints give the response a continuous scrollbar without retaining
/// its body, a string per line, or TextKit attributes for the entire file.
public struct ResponseTextIndex: Sendable {
    public struct Row: Equatable, Sendable {
        public let number: Int
        public let line: Int
        public let continuation: Bool
        public let byteOffset: UInt64
        public let text: String
        public let columns: Int
    }
    private struct Checkpoint: Sendable {
        let row: Int
        let line: Int
        let continuation: Bool
        let offset: UInt64
    }
    public let url: URL
    public let columns: Int
    public let rowCount: Int
    public let lineCount: Int
    public let maximumColumns: Int
    private let checkpoints: [Checkpoint]
    private let prefixRows: [Row]
    private let prefixLines: Int
    private let prefix: String

    public init(url: URL, columns: Int, prefix: String = "") throws {
        self.url = url
        self.prefix = prefix
        self.columns = max(1, columns)
        var prefixRows: [Row] = []
        let lines = prefix.isEmpty ? [] : Array(prefix.components(separatedBy: "\n").dropLast(prefix.hasSuffix("\n") ? 1 : 0))
        for (lineNumber, line) in lines.enumerated() {
            var remaining = line[...]
            var continuation = false
            repeat {
                let text = String(remaining.prefix(self.columns))
                prefixRows.append(Row(number: prefixRows.count, line: lineNumber, continuation: continuation, byteOffset: 0, text: text, columns: text.count))
                remaining = remaining.dropFirst(text.count)
                continuation = true
            } while !remaining.isEmpty
        }
        self.prefixRows = prefixRows
        prefixLines = lines.count
        var checkpoints: [Checkpoint] = []
        var count = 0
        var maximum = 0
        var lastLine = 0
        try Self.scan(url: url, columns: self.columns, from: nil, collectText: false) { row in
            if row.number.isMultiple(of: 128) {
                checkpoints.append(Checkpoint(row: row.number, line: row.line, continuation: row.continuation, offset: row.byteOffset))
            }
            count = row.number + 1
            maximum = max(maximum, row.columns)
            lastLine = row.line
            return true
        }
        rowCount = count + prefixRows.count
        lineCount = lastLine + prefixLines + 1
        maximumColumns = max(maximum, prefixRows.map(\.columns).max() ?? 0)
        self.checkpoints = checkpoints
    }

    public func rows(start: Int, count: Int) throws -> [Row] {
        guard count > 0, start < rowCount else { return [] }
        let start = max(0, start)
        let bodyStart = max(0, start - prefixRows.count)
        let checkpoint = checkpoints[min(bodyStart / 128, checkpoints.count - 1)]
        var rows = Array(prefixRows.dropFirst(start).prefix(count))
        if rows.count == count { return rows }
        try Self.scan(url: url, columns: columns, from: checkpoint, collectText: true) { row in
            if row.number >= bodyStart {
                rows.append(Row(number: row.number + prefixRows.count, line: row.line + prefixLines,
                    continuation: row.continuation, byteOffset: row.byteOffset, text: row.text, columns: row.columns))
            }
            return rows.count < count
        }
        return rows
    }

    public struct SearchPosition: Equatable, Sendable {
        public let row: Int
        public let column: Int
        public init(row: Int, column: Int) { self.row = row; self.column = column }
    }

    public struct SearchMatch: Equatable, Sendable {
        public let start: SearchPosition
        public let end: SearchPosition
    }

    /// Search runs on the original text, so a soft wrap never changes a match.
    /// Only the capped positions survive the search; the mapped source is released.
    public func search(_ query: TextSearchQuery, selection: (start: SearchPosition, end: SearchPosition)? = nil) throws -> [SearchMatch] {
        guard !query.text.isEmpty else { return [] }
        let data = try Data(contentsOf: url, options: .mappedIfSafe)
        try Task.checkCancellation()
        let source = prefix + String(decoding: data, as: UTF8.self)
        func offset(for position: SearchPosition) throws -> Int {
            guard let row = try rows(start: position.row, count: 1).first else { return source.utf16.count }
            let column = min(max(0, position.column), row.text.utf16.count)
            if position.row < prefixRows.count {
                var offset = 0
                for index in 0..<position.row {
                    offset += prefixRows[index].text.utf16.count
                    if !prefixRows[index + 1].continuation { offset += 1 }
                }
                return offset + column
            }
            return prefix.utf16.count + String(decoding: data.prefix(Int(row.byteOffset)), as: UTF8.self).utf16.count + column
        }
        let scope: NSRange?
        if let selection {
            let start = try offset(for: selection.start), end = try offset(for: selection.end)
            scope = NSRange(location: min(start, end), length: abs(end - start))
        } else { scope = nil }
        let ranges = try query.matches(in: source, range: scope)
        guard !ranges.isEmpty else { return [] }
        let events = ranges.enumerated().flatMap { index, range in
            [(range.location, index, false), (NSMaxRange(range), index, true)]
        }.sorted { $0.0 < $1.0 }
        var starts = [SearchPosition?](repeating: nil, count: ranges.count)
        var ends = starts
        var eventIndex = 0
        var sourceOffset = 0
        func consume(number: Int, raw: String, last: Bool = false) {
            let string = raw as NSString
            let endOffset = sourceOffset + string.length
            while eventIndex < events.count && (events[eventIndex].0 < endOffset || last && events[eventIndex].0 == endOffset) {
                let event = events[eventIndex]
                let local = min(string.length, max(0, event.0 - sourceOffset))
                let visible = string.substring(to: local).replacingOccurrences(of: "\r", with: "").replacingOccurrences(of: "\n", with: "")
                let position = SearchPosition(row: number, column: visible.utf16.count)
                if event.2 { ends[event.1] = position } else { starts[event.1] = position }
                eventIndex += 1
            }
            sourceOffset = endOffset
        }
        for (index, row) in prefixRows.enumerated() {
            let newline = index + 1 < prefixRows.count ? !prefixRows[index + 1].continuation : prefix.hasSuffix("\n")
            consume(number: row.number, raw: row.text + (newline ? "\n" : ""))
        }
        var previous: Row?
        try Self.scan(url: url, columns: columns, from: nil, collectText: false) { row in
            if let previous {
                let bytes = data[Int(previous.byteOffset)..<Int(row.byteOffset)]
                consume(number: previous.number + prefixRows.count, raw: String(decoding: bytes, as: UTF8.self))
            }
            previous = row
            return eventIndex < events.count
        }
        if eventIndex < events.count, let previous {
            consume(number: previous.number + prefixRows.count,
                raw: String(decoding: data[Int(previous.byteOffset)...], as: UTF8.self), last: true)
        }
        return ranges.indices.compactMap { index in
            guard let start = starts[index], let end = ends[index] else { return nil }
            return SearchMatch(start: start, end: end)
        }
    }

    public func firstMatch(_ query: String) throws -> Int? {
        guard !query.isEmpty else { return nil }
        if let row = prefixRows.first(where: { $0.text.contains(query) }) { return row.number }
        let reader = try FileHandle(forReadingFrom: url)
        defer { try? reader.close() }
        let needle = Data(query.utf8)
        var carry = Data()
        var offset: UInt64 = 0
        while true {
            try Task.checkCancellation()
            let chunk = try autoreleasepool { try reader.read(upToCount: 256 * 1024) ?? Data() }
            guard !chunk.isEmpty else { return nil }
            var data = carry
            data.append(chunk)
            if let match = data.range(of: needle) {
                let byteOffset = offset - UInt64(carry.count) + UInt64(match.lowerBound)
                let checkpoint = checkpoints.last(where: { $0.offset <= byteOffset })
                var found: Int?
                var preceding = checkpoint?.row ?? 0
                try Self.scan(url: url, columns: columns, from: checkpoint, collectText: false) { row in
                    if row.byteOffset > byteOffset { found = preceding; return false }
                    preceding = row.number
                    return true
                }
                return (found ?? preceding) + prefixRows.count
            }
            offset += UInt64(chunk.count)
            carry = Data(data.suffix(max(0, needle.count - 1)))
        }
    }

    private static func scan(url: URL, columns: Int, from checkpoint: Checkpoint?, collectText: Bool, consume: (Row) -> Bool) throws {
        let reader = try FileHandle(forReadingFrom: url)
        defer { try? reader.close() }
        var offset = checkpoint?.offset ?? 0
        try reader.seek(toOffset: offset)
        var rowOffset = offset
        var row = checkpoint?.row ?? 0
        var line = checkpoint?.line ?? 0
        var continuation = checkpoint?.continuation ?? false
        var column = 0
        var wordBreak: (offset: UInt64, column: Int)?
        var bytes: [UInt8] = []
        if collectText { bytes.reserveCapacity(min(columns, 16384) * 4) }
        func emit() -> Bool {
            let text = collectText ? String(decoding: bytes, as: UTF8.self).replacingOccurrences(of: "\r", with: "") : ""
            let keepGoing = consume(Row(number: row, line: line, continuation: continuation, byteOffset: rowOffset, text: text, columns: column))
            row += 1
            bytes.removeAll(keepingCapacity: true)
            column = 0
            wordBreak = nil
            return keepGoing
        }
        while true {
            try Task.checkCancellation()
            let chunk = try autoreleasepool { try reader.read(upToCount: 256 * 1024) ?? Data() }
            if chunk.isEmpty { _ = emit(); return }
            for byte in chunk {
                if byte == 10 {
                    if !emit() { return }
                    line += 1
                    continuation = false
                    rowOffset = offset + 1
                } else {
                    if byte & 0xc0 != 0x80 {
                        if column >= columns {
                            if let split = wordBreak, split.offset > rowOffset {
                                let suffixCount = Int(offset - split.offset)
                                let suffix = collectText ? Array(bytes.suffix(suffixCount)) : []
                                let remainingColumns = column - split.column
                                if collectText { bytes.removeLast(suffixCount) }
                                column = split.column
                                if !emit() { return }
                                bytes = suffix
                                column = remainingColumns
                                rowOffset = split.offset
                            } else {
                                if !emit() { return }
                                rowOffset = offset
                            }
                            continuation = true
                        }
                        column += byte == 9 ? 4 - column % 4 : 1
                    }
                    if collectText { bytes.append(byte) }
                    if byte == 32 || byte == 9 { wordBreak = (offset + 1, column) }
                }
                offset += 1
            }
        }
    }
}
