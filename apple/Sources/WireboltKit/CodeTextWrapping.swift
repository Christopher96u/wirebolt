import CoreText
import Foundation

public struct CodeTextWrapping: Hashable, Sendable {
    public let fontName: String
    public let fontSize: Double
    public let width: Double

    public init(fontName: String, fontSize: Double, width: Double) {
        self.fontName = fontName
        self.fontSize = fontSize
        self.width = max(1, width)
    }

    /// A layout keyed by whole columns, so pixel-level resizes inside one column
    /// reuse the same wrap index. A quarter-column of slack keeps the column count
    /// stable against floating-point rounding; it stays inside the editor's margin.
    public init(fontName: String, fontSize: Double, columns: Int) {
        let advance = Self.advance(fontName: fontName, fontSize: fontSize)
        self.init(fontName: fontName, fontSize: fontSize, width: (Double(max(1, columns)) + 0.25) * advance)
    }

    public static func advance(fontName: String, fontSize: Double) -> Double {
        Metrics(CodeTextWrapping(fontName: fontName, fontSize: fontSize, width: 1)).advance
    }

    private static let breakAfter = Set(" \t})]?|/&.,;¢°′″‰℃、。｡､￠，．：；？！％・･ゝゞヽヾーァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゕゖㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ々〻ｧｨｩｪｫｬｭｮｯｰ”〉》」』】〕）］｝｣".utf16)
    private static let breakBefore = Set("([{‘“〈《「『【〔（［｛｢£¥＄￡￥+＋".utf16)

    public static func breakLength(in source: NSString, start: Int, fitting: Int) -> Int {
        guard start < source.length, fitting > 0 else { return 0 }
        guard start + fitting < source.length else { return fitting }
        let firstContent = (start..<source.length).first { source.character(at: $0) != 32 && source.character(at: $0) != 9 } ?? source.length
        for end in stride(from: start + fitting, through: start + 1, by: -1) {
            if end > firstContent && (breakAfter.contains(source.character(at: end - 1)) || breakBefore.contains(source.character(at: end))),
                source.rangeOfComposedCharacterSequence(at: end).location == end {
                return end - start
            }
        }
        return fitting
    }

    struct Metrics {
        let font: CTFont
        let advance: Double

        init(_ layout: CodeTextWrapping) {
            font = CTFontCreateWithName(layout.fontName as CFString, layout.fontSize, nil)
            advance = CTLineGetTypographicBounds(CTLineCreateWithAttributedString(NSAttributedString(
                string: " ", attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font])), nil, nil, nil)
        }
    }

    private final class ShapingWindow {
        let source: NSString
        let attributes: [NSAttributedString.Key: Any]
        let windowLength: Int
        var range = NSRange(location: 0, length: 0)
        var typesetter: CTTypesetter?
        init(source: NSString, attributes: [NSAttributedString.Key: Any], windowLength: Int) {
            self.source = source; self.attributes = attributes; self.windowLength = windowLength
        }
        func prepare(from start: Int, length: Int = 0) -> CTTypesetter {
            if typesetter == nil || start < range.location || NSMaxRange(range) < min(source.length, start + max(windowLength / 2, length)) {
                range = source.rangeOfComposedCharacterSequences(for: NSRange(location: start, length: min(source.length - start, max(windowLength, length))))
                typesetter = CTTypesetterCreateWithAttributedStringAndOptions(NSAttributedString(string: source.substring(with: range), attributes: attributes),
                    [kCTTypesetterOptionAllowUnboundedLayout: true] as CFDictionary)
            }
            return typesetter!
        }
        func fitting(from start: Int, width: Double) -> Int {
            let typesetter = prepare(from: start)
            return max(1, CTTypesetterSuggestClusterBreak(typesetter, start - range.location, width))
        }
        func width(of requested: NSRange) -> Double {
            let typesetter = prepare(from: requested.location, length: requested.length)
            let line = CTTypesetterCreateLine(typesetter, CFRange(location: requested.location - range.location, length: requested.length))
            return CTLineGetTypographicBounds(line, nil, nil, nil)
        }
    }

    struct Paragraph {
        let source: NSString
        let byteOffsets: [Int]
        let advance: Double
        let indentation: Double
        private let shaper: ShapingWindow?
        let layout: CodeTextWrapping

        init(bytes: Data, layout: CodeTextWrapping, metrics: Metrics? = nil) {
            self.layout = layout
            // Preserve byte positions even when malformed UTF-8 becomes a replacement character.
            let raw = Array(bytes)
            var units: [UInt16] = []
            var offsets = [0]
            var cursor = 0
            while cursor < raw.count {
                let start = cursor, first = raw[cursor]
                var scalar: UInt32 = UInt32(first)
                var length = 1
                if first >= 0x80 {
                    let expected = (0xC2...0xDF).contains(first) ? 2 : (0xE0...0xEF).contains(first) ? 3 : (0xF0...0xF4).contains(first) ? 4 : 0
                    scalar = expected == 2 ? UInt32(first & 0x1F) : expected == 3 ? UInt32(first & 0x0F) : UInt32(first & 0x07)
                    var valid = expected > 0
                    if expected > 1 {
                        for index in 1..<expected {
                            guard start + index < raw.count else { valid = false; break }
                            let byte = raw[start + index]
                            guard (0x80...0xBF).contains(byte), !(index == 1 && (
                                first == 0xE0 && byte < 0xA0 || first == 0xED && byte > 0x9F ||
                                first == 0xF0 && byte < 0x90 || first == 0xF4 && byte > 0x8F)) else { valid = false; break }
                            scalar = scalar << 6 | UInt32(byte & 0x3F)
                            length += 1
                        }
                    }
                    if !valid { scalar = 0xFFFD }
                }
                cursor += length
                if scalar > 0xFFFF {
                    let value = scalar - 0x10000
                    units.append(UInt16(0xD800 + (value >> 10)))
                    offsets.append(start)
                    units.append(UInt16(0xDC00 + (value & 0x3FF)))
                } else { units.append(UInt16(scalar)) }
                offsets.append(cursor)
            }
            if units.last == 13 { units.removeLast(); offsets.removeLast() }
            source = NSString(characters: units, length: units.count)
            byteOffsets = offsets
            let metrics = metrics ?? Metrics(layout)
            let attributes: [NSAttributedString.Key: Any] = [NSAttributedString.Key(kCTFontAttributeName as String): metrics.font]
            advance = metrics.advance
            var columns = 0
            for unit in units {
                if unit == 32 { columns += 1 }
                else if unit == 9 { columns += 4 - columns % 4 }
                else { break }
            }
            indentation = min(Double(columns) * advance, max(0, layout.width - advance))
            if units.allSatisfy({ (32...126).contains($0) }) { shaper = nil }
            else {
                var interval = advance * 4
                let paragraph = withUnsafePointer(to: &interval) { pointer in
                    var setting = CTParagraphStyleSetting(spec: .defaultTabInterval, valueSize: MemoryLayout<Double>.size, value: pointer)
                    return CTParagraphStyleCreate(&setting, 1)
                }
                var styled = attributes
                styled[NSAttributedString.Key(kCTParagraphStyleAttributeName as String)] = paragraph
                shaper = ShapingWindow(source: source, attributes: styled, windowLength: max(2048, Int(min(32768, layout.width / max(1, advance) * 8))))
            }
        }

        func next(from start: Int, indent: Double) -> NSRange {
            let available = max(1, layout.width - indent)
            let fit = shaper.map { $0.fitting(from: start, width: available) }
                ?? min(source.length - start, max(1, Int(available / max(1, advance))))
            var count = min(source.length - start, CodeTextWrapping.breakLength(in: source, start: start, fitting: fit))
            // TextKit hangs trailing whitespace on the preceding visual line.
            // Keep those original bytes there instead of introducing an indent.
            if (start..<(start + count)).contains(where: { source.character(at: $0) != 32 && source.character(at: $0) != 9 }) {
                while start + count < source.length && (source.character(at: start + count) == 32 || source.character(at: start + count) == 9) {
                    count += 1
                }
            }
            return NSRange(location: start, length: count)
        }

        func columns(in range: NSRange) -> Int {
            guard let shaper else { return range.length }
            return Int(ceil(shaper.width(of: range) / max(1, advance)))
        }
    }
}
