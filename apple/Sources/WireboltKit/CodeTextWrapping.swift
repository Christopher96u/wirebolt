import CoreText
import Foundation
#if canImport(WireboltBridge)
import WireboltBridge
#endif

/// Native font identity and metrics; Rust owns wrapping policy and byte mapping.
public struct CodeTextWrapping: Hashable, Sendable {
    public let fontName: String
    public let fontSize: Double
    public let width: Double
    public init(fontName: String, fontSize: Double, width: Double) {
        self.fontName = fontName; self.fontSize = fontSize; self.width = max(1, width)
    }
    public init(fontName: String, fontSize: Double, columns: Int) {
        self.init(fontName: fontName, fontSize: fontSize, width: (Double(max(1, columns)) + 0.25) * Self.advance(fontName: fontName, fontSize: fontSize))
    }
    public static func advance(fontName: String, fontSize: Double) -> Double {
        let font = CTFontCreateWithName(fontName as CFString, fontSize, nil)
        return CTLineGetTypographicBounds(CTLineCreateWithAttributedString(NSAttributedString(string: " ",
            attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font])), nil, nil, nil)
    }
    var rust: WrapLayout { WrapLayout(fontName: fontName, fontSize: fontSize, width: width, advance: Self.advance(fontName: fontName, fontSize: fontSize)) }
    public static func breakLength(in source: NSString, start: Int, fitting: Int) -> Int {
        CodeWrapBreaks(source as String).length(start: start, fitting: fitting)
    }
}

/// A batch of Rust-produced break coordinates for a TextKit shaping paragraph.
/// Native layout can consult the snapshot without crossing the bridge per row.
struct CodeWrapBreaks {
    private let snapshot: WrapBreakSnapshot
    init(_ text: String) { snapshot = appWrapBreaks(text: text) }
    func length(start: Int, fitting: Int, limit: Int? = nil) -> Int {
        let length = min(Int(clamping: snapshot.length), limit ?? Int.max)
        guard start < length, fitting > 0 else { return 0 }
        guard start + fitting < length else { return fitting }
        let end = Int(clamping: snapshot.previous[start + fitting])
        return end > Int(clamping: snapshot.nextContent[start]) ? end - start : fitting
    }
}

/// CoreText is the only platform dependency of the Rust response index. Each call
/// measures a bounded batch of paragraphs, never an individual viewport row.
final class NativeTextShaper: AppTextShaper {
    func measure(requests: [ShapingRequest], layout: WrapLayout) -> [ShapingMetrics] {
        let font = CTFontCreateWithName(layout.fontName as CFString, layout.fontSize, nil)
        var interval = layout.advance * 4
        let paragraph = withUnsafePointer(to: &interval) { pointer in
            var setting = CTParagraphStyleSetting(spec: .defaultTabInterval, valueSize: MemoryLayout<Double>.size, value: pointer)
            return CTParagraphStyleCreate(&setting, 1)
        }
        let attributes: [NSAttributedString.Key: Any] = [NSAttributedString.Key(kCTFontAttributeName as String): font,
            NSAttributedString.Key(kCTParagraphStyleAttributeName as String): paragraph]
        var groupWidths: [String: [Double]] = [:]
        return requests.map { request in
            let source = request.text as NSString
            let typesetter = CTTypesetterCreateWithAttributedStringAndOptions(NSAttributedString(string: request.text, attributes: attributes),
                [kCTTypesetterOptionAllowUnboundedLayout: true] as CFDictionary)!
            let line = CTTypesetterCreateLine(typesetter, CFRange(location: 0, length: source.length))
            var widths = [Double](repeating: 0, count: max(0, request.boundaries.count - 1))
            var glyphStarts = [Bool](repeating: false, count: source.length + 1)
            glyphStarts[0] = true; glyphStarts[source.length] = true
            var clusterAt = [Int](repeating: 0, count: source.length + 1)
            for cluster in 0..<max(0, request.boundaries.count - 1) {
                let start = Int(clamping: request.boundaries[cluster])
                let end = Int(clamping: request.boundaries[cluster + 1])
                for offset in start..<end { clusterAt[offset] = cluster }
            }
            clusterAt[source.length] = widths.count
            for rawRun in CTLineGetGlyphRuns(line) as NSArray {
                let run = rawRun as! CTRun
                let count = CTRunGetGlyphCount(run)
                var indices = [CFIndex](repeating: 0, count: count)
                var advances = [CGSize](repeating: .zero, count: count)
                CTRunGetStringIndices(run, CFRange(location: 0, length: 0), &indices)
                CTRunGetAdvances(run, CFRange(location: 0, length: 0), &advances)
                for glyph in 0..<count {
                    let offset = indices[glyph]
                    guard offset >= 0 && offset <= source.length else { continue }
                    glyphStarts[offset] = true
                    let cluster = clusterAt[offset]
                    if cluster < widths.count { widths[cluster] += advances[glyph].width }
                }
            }
            var groups: [ShapingGroup] = []
            var previous = 0
            for end in glyphStarts.indices.dropFirst() where glyphStarts[end] {
                let start = previous; previous = end
                let first = clusterAt[start]
                let last = clusterAt[end]
                // CoreText can omit long runs of control characters. Never
                // allocate a quadratic table for those non-glyph spans.
                guard first < last, last - first > 1, last - first <= 32,
                      request.boundaries[first] == UInt64(start), request.boundaries[last] == UInt64(end) else { continue }
                let key = source.substring(with: NSRange(location: start, length: end - start))
                let stride = last - first + 1
                let measured: [Double]
                if let cached = groupWidths[key] { measured = cached }
                else {
                    var values = [Double](repeating: 0, count: stride * stride)
                    for lower in 0..<(stride - 1) { for upper in (lower + 1)..<stride {
                        let location = Int(clamping: request.boundaries[first + lower])
                        let end = Int(clamping: request.boundaries[first + upper])
                        let partial = CTTypesetterCreateLine(typesetter, CFRange(location: location, length: end - location))
                        values[lower * stride + upper] = CTLineGetTypographicBounds(partial, nil, nil, nil)
                    } }
                    measured = values; groupWidths[key] = values
                }
                groups.append(ShapingGroup(first: UInt64(first), count: UInt64(last - first), widths: measured))
            }
            return ShapingMetrics(advances: widths, groups: groups)
        }
    }

}

/// Shows spaces as middle dots for "Show Invisible Characters" in views that draw text
/// themselves. U+00B7 has the space advance in a monospaced font and one UTF-16 unit,
/// so columns, wrapping and selection offsets are unchanged.
public enum InvisibleSpaces {
    public static let marker = "\u{00B7}"

    /// Runs of consecutive spaces, in UTF-16 offsets.
    public static func runs(in text: NSString) -> [NSRange] { appSpaceRuns(text: text as String).map(\.nsRange) }

    /// Replaces every space with the marker and applies `attributes` to the markers.
    public static func marking(_ source: NSAttributedString, runs suppliedRuns: [NSRange]? = nil, attributes: [NSAttributedString.Key: Any]) -> NSAttributedString {
        let runs = suppliedRuns ?? runs(in: source.string as NSString)
        guard !runs.isEmpty else { return source }
        let marked = NSMutableAttributedString(attributedString: source)
        for run in runs {
            marked.replaceCharacters(in: run, with: String(repeating: marker, count: run.length))
            marked.addAttributes(attributes, range: run)
        }
        return marked
    }
}
