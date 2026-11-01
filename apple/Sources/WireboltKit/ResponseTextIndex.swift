import Foundation

private typealias RustSearchPosition = SearchPosition
public struct ResponseTextIndex: Sendable {
    public struct Row: Equatable, Sendable {
        public let number: Int
        public let line: Int
        public let continuation: Bool
        public let byteOffset: UInt64
        public let text: String
        public let columns: Int
        public let indent: Double
        let spaceRanges: [NSRange]?
        public init(number: Int, line: Int, continuation: Bool, byteOffset: UInt64, text: String, columns: Int, indent: Double = 0) {
            self.init(
                number: number, line: line, continuation: continuation, byteOffset: byteOffset, text: text, columns: columns,
                indent: indent, spaceRanges: nil)
        }
        private init(
            number: Int, line: Int, continuation: Bool, byteOffset: UInt64, text: String, columns: Int, indent: Double,
            spaceRanges: [NSRange]?
        ) {
            self.number = number
            self.line = line
            self.continuation = continuation
            self.byteOffset = byteOffset
            self.text = text
            self.columns = columns
            self.indent = indent
            self.spaceRanges = spaceRanges
        }
        public static func == (lhs: Row, rhs: Row) -> Bool {
            lhs.number == rhs.number && lhs.line == rhs.line && lhs.continuation == rhs.continuation
                && lhs.byteOffset == rhs.byteOffset && lhs.text == rhs.text && lhs.columns == rhs.columns && lhs.indent == rhs.indent
        }
        fileprivate init(_ row: TextRow) {
            self.init(
                number: Int(clamping: row.number), line: Int(clamping: row.line), continuation: row.continuation,
                byteOffset: row.byteOffset, text: row.text, columns: Int(clamping: row.columns), indent: row.indent,
                spaceRanges: row.spaces.map(\.nsRange))
        }
        fileprivate var rust: TextRow {
            TextRow(
                number: UInt64(max(0, number)), line: UInt64(max(0, line)), continuation: continuation,
                byteOffset: byteOffset, text: text, columns: UInt64(max(0, columns)), indent: indent,
                spaces: (spaceRanges ?? []).map(\.responseRange))
        }
    }
    public let url: URL
    public let columns: Int
    public let wrapping: CodeTextWrapping?
    public let rowCount: Int
    public let lineCount: Int
    public let isComplete: Bool
    public let maximumColumns: Int
    private let rust: AppTextIndex
    init(url: URL, columns: Int, wrapping: CodeTextWrapping?, rust: AppTextIndex) {
        self.url = url
        self.columns = max(1, columns)
        self.wrapping = wrapping
        self.rust = rust
        let metadata = rust.metadata()
        rowCount = Int(clamping: metadata.rowCount)
        lineCount = Int(clamping: metadata.lineCount)
        isComplete = metadata.isComplete
        maximumColumns = Int(clamping: metadata.maximumColumns)
    }
    static func options(url: URL, columns: Int, prefix: String, wrapping: CodeTextWrapping?) -> IndexOptions {
        IndexOptions(path: url.path, columns: UInt64(max(1, columns)), prefix: prefix, wrapping: wrapping?.rust)
    }
    public init(url: URL, columns: Int, prefix: String = "", wrapping: CodeTextWrapping? = nil, rowLimit: Int = .max) throws {
        let index = try responseCall {
            try AppTextIndex(
                options: Self.options(url: url, columns: columns, prefix: prefix, wrapping: wrapping),
                rowLimit: UInt64(max(1, rowLimit)), adapter: wrapping == nil ? nil : NativeTextShaper(), cancellation: $0)
        }
        self.init(url: url, columns: columns, wrapping: wrapping, rust: index)
    }
    public func rows(start: Int, count: Int) throws -> [Row] {
        try responseCall { try rust.rows(start: UInt64(max(0, start)), count: UInt64(max(0, count)), cancellation: $0).map(Row.init) }
    }
    public func row(preserving anchor: Row) throws -> Int {
        Int(clamping: try responseCall { try rust.preserving(anchor: anchor.rust, cancellation: $0) })
    }
    public func row(containingByteOffset offset: UInt64) throws -> Int {
        Int(clamping: try responseCall { try rust.rowAtByte(offset: offset, cancellation: $0) })
    }
    public struct SearchPosition: Equatable, Sendable {
        public let row: Int
        public let column: Int
        public init(row: Int, column: Int) {
            self.row = row
            self.column = column
        }
        fileprivate var rust: RustSearchPosition { RustSearchPosition(row: UInt64(max(0, row)), column: UInt64(max(0, column))) }
        fileprivate init(_ position: RustSearchPosition) {
            row = Int(clamping: position.row)
            column = Int(clamping: position.column)
        }
    }
    public struct SearchMatch: Equatable, Sendable {
        public let start: SearchPosition
        public let end: SearchPosition
    }
    public func search(_ query: TextSearchQuery, selection: (start: SearchPosition, end: SearchPosition)? = nil) throws -> [SearchMatch] {
        try responseCall {
            try rust.search(query: query.rust, start: selection?.start.rust, end: selection?.end.rust, cancellation: $0)
                .map { SearchMatch(start: SearchPosition($0.start), end: SearchPosition($0.end)) }
        }
    }
    public func firstMatch(_ query: String) throws -> Int? {
        try responseCall { try rust.firstMatch(query: query, cancellation: $0).map { Int(clamping: $0) } }
    }
}
