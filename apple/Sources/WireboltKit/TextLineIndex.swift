import Foundation

/// A value facade over Rust's UTF-16 line index. Rust edit deltas update the native mirror.
public struct TextLineIndex: Sendable {
    public private(set) var starts: [Int]
    public private(set) var length: Int
    private var rust: AppLineIndex
    public init(_ text: String = "") {
        rust = AppLineIndex(text: text)
        let snapshot = rust.snapshot()
        starts = snapshot.starts.map { Int(clamping: $0) }; length = Int(clamping: snapshot.length)
    }
    public mutating func replace(_ range: NSRange, with text: String) {
        precondition(range.location >= 0 && NSMaxRange(range) <= length)
        let update = try! rust.replacing(range: range.responseRange, text: text)
        rust = update.index
        let change = update.change
        let first = Int(clamping: change.first), removed = Int(clamping: change.removed)
        let inserted = change.inserted.map { Int(clamping: $0) }
        starts.replaceSubrange(first..<(first + removed), with: inserted)
        let delta = Int(clamping: change.delta)
        for position in (first + inserted.count)..<starts.count { starts[position] += delta }
        length = Int(clamping: change.length)
    }
    public func line(at offset: Int) -> Int { Int(clamping: rust.line(offset: UInt64(max(0, offset)))) }
}
