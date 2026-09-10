import AppKit
import CoreText

/// Retains shaping for a long horizontal line and draws only visible glyphs.
@MainActor
final class IndexedTextLine {
    private struct Run {
        let value: CTRun
        let lower: Double
        let upper: Double
    }
    private let line: CTLine
    private let runs: [Run]
    private let baseline: Double
    private let margin: Double

    init(_ attributed: NSAttributedString, fontSize: Double) {
        let colored = NSMutableAttributedString(attributedString: attributed)
        attributed.enumerateAttribute(.foregroundColor, in: NSRange(location: 0, length: attributed.length)) { color, range, _ in
            if let color = color as? NSColor { colored.addAttribute(.foregroundColor, value: color.cgColor, range: range) }
        }
        line = CTLineCreateWithAttributedString(colored)
        baseline = NSLayoutManager().defaultBaselineOffset(for: NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular))
        margin = fontSize * 4
        runs = (CTLineGetGlyphRuns(line) as! [CTRun]).compactMap { run in
            let count = CTRunGetGlyphCount(run)
            guard count > 0 else { return nil }
            var positions = [CGPoint](repeating: .zero, count: count)
            CTRunGetPositions(run, CFRange(location: 0, length: count), &positions)
            let lower = positions.map(\.x).min() ?? 0
            let upper = positions.map(\.x).max() ?? 0
            return Run(value: run, lower: lower - fontSize * 4, upper: upper + fontSize * 4)
        }
    }

    func draw(at point: CGPoint, visible: CGRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        let left = visible.minX - point.x - margin, right = visible.maxX - point.x + margin
        context.saveGState()
        defer { context.restoreGState() }
        context.clip(to: visible)
        for run in runs where run.upper >= left && run.lower <= right {
            let count = CTRunGetGlyphCount(run.value)
            let glyphRange: CFRange
            if let positions = CTRunGetPositionsPtr(run.value), !CTRunGetStatus(run.value).contains(.nonMonotonic) {
                let ascending = positions[0].x <= positions[count - 1].x
                func bound(_ x: Double) -> Int {
                    var lower = 0, upper = count
                    while lower < upper {
                        let middle = (lower + upper) / 2
                        if ascending ? positions[middle].x < x : positions[middle].x > x { lower = middle + 1 }
                        else { upper = middle }
                    }
                    return lower
                }
                let start = max(0, bound(ascending ? left : right) - 1)
                let end = min(count, bound(ascending ? right : left) + 1)
                guard end > start else { continue }
                glyphRange = CFRange(location: start, length: end - start)
            } else { glyphRange = CFRange(location: 0, length: count) }
            context.saveGState()
            context.textMatrix = CTRunGetTextMatrix(run.value).concatenating(CGAffineTransform(scaleX: 1, y: -1))
            context.textPosition = CGPoint(x: point.x, y: point.y + baseline)
            CTRunDraw(run.value, context, glyphRange)
            context.restoreGState()
        }
    }
}
