import Foundation

/// Logical line offsets in TextKit's UTF-16 coordinates.
public struct TextLineIndex: Sendable {
    public private(set) var starts: [Int]
    public private(set) var length: Int

    public init(_ text: String = "") {
        let units = Array(text.utf16)
        starts = [0]
        for (offset, unit) in units.enumerated() where unit == 10 { starts.append(offset + 1) }
        length = units.count
    }

    public mutating func replace(_ range: NSRange, with text: String) {
        precondition(range.location >= 0 && NSMaxRange(range) <= length)
        let units = Array(text.utf16)
        let first = upperBound(range.location)
        let last = upperBound(NSMaxRange(range))
        let delta = units.count - range.length
        let inserted = units.enumerated().compactMap { $0.element == 10 ? range.location + $0.offset + 1 : nil }
        starts.replaceSubrange(first..<last, with: inserted)
        for index in (first + inserted.count)..<starts.count { starts[index] += delta }
        length += delta
    }

    public func line(at offset: Int) -> Int { max(0, upperBound(offset) - 1) }

    private func upperBound(_ offset: Int) -> Int {
        var lower = 0, upper = starts.count
        while lower < upper {
            let middle = (lower + upper) / 2
            if starts[middle] <= offset { lower = middle + 1 } else { upper = middle }
        }
        return lower
    }
}
