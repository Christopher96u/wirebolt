import CoreText
import Foundation

public struct CodeTextWrapping: Equatable, Sendable {
    public let fontName: String
    public let fontSize: Double
    public let width: Double

    public init(fontName: String, fontSize: Double, width: Double) {
        self.fontName = fontName
        self.fontSize = fontSize
        self.width = max(1, width)
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

    struct Paragraph {
        let source: NSString
        let byteOffsets: [Int]
        let advance: Double
        let indentation: Double
        let typesetter: CTTypesetter?
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
            if units.allSatisfy({ (32...126).contains($0) }) { typesetter = nil }
            else {
                var interval = advance * 4
                let paragraph = withUnsafePointer(to: &interval) { pointer in
                    var setting = CTParagraphStyleSetting(spec: .defaultTabInterval, valueSize: MemoryLayout<Double>.size, value: pointer)
                    return CTParagraphStyleCreate(&setting, 1)
                }
                var styled = attributes
                styled[NSAttributedString.Key(kCTParagraphStyleAttributeName as String)] = paragraph
                // The scanner bounds its input. Core Text's default complexity
                // limit can otherwise reject a valid Unicode buffer entirely.
                typesetter = CTTypesetterCreateWithAttributedStringAndOptions(
                    NSAttributedString(string: source as String, attributes: styled),
                    [kCTTypesetterOptionAllowUnboundedLayout: true] as CFDictionary)
            }
        }

        func next(from start: Int, indent: Double) -> NSRange {
            let available = max(1, layout.width - indent)
            let fit = typesetter.map { max(1, CTTypesetterSuggestClusterBreak($0, start, available)) }
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
            guard let typesetter else { return range.length }
            let line = CTTypesetterCreateLine(typesetter, CFRange(location: range.location, length: range.length))
            return Int(ceil(CTLineGetTypographicBounds(line, nil, nil, nil) / max(1, advance)))
        }
    }
}
