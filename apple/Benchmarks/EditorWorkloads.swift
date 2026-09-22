import AppKit
import CoreText
import Foundation
import Observation
import SwiftUI

@MainActor @Observable
private final class EditorDocument {
    var text: String
    var language: SyntaxLanguage = .json
    init(_ text: String) { self.text = text }
}

private struct EditorFixture: View {
    @Bindable var document: EditorDocument
    var body: some View { NativeCodeEditor(text: $document.text, language: document.language) }
}

private final class ScrollFixtureView: NSView {
    override var isFlipped: Bool { true }
}

@main
private struct EditorWorkloads {
    @MainActor static func main() throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let fixture = "{\n  \"message\": \"café 東京 🚀\",\n  \"items\": [\n" + (0..<600).map { "    {\"id\":\($0),\"active\":true}" }.joined(separator: ",\n") + "\n  ]\n}"
        let document = EditorDocument(fixture)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 980, height: 600), styleMask: [.borderless], backing: .buffered, defer: false)
        let start = ContinuousClock.now
        let host = NSHostingView(rootView: EditorFixture(document: document))
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        host.displayIfNeeded()
        let cold = milliseconds(since: start)
        guard let editor = textView(in: host) else { throw CocoaError(.coderValueNotFound) }
        guard let scroll = editor.enclosingScrollView else { throw CocoaError(.coderValueNotFound) }
        precondition(scroll.contentView.bounds.height > 500)
        precondition(editor.frame.width + (scroll.verticalRulerView?.ruleThickness ?? 0) <= scroll.contentView.bounds.width + 1)
        var samples: [Double] = []
        for _ in 0..<30 {
            let start = ContinuousClock.now
            editor.insertText(" ", replacementRange: NSRange(location: 1, length: 0))
            host.layoutSubtreeIfNeeded()
            host.displayIfNeeded()
            samples.append(milliseconds(since: start))
        }
        precondition(document.text.utf16.count == fixture.utf16.count + 30)
        let p95 = samples.sorted()[Int(Double(samples.count - 1) * 0.95)]
        precondition(editor.textContainer is CodeTextContainer)
        verifyCodeWrapping()
        verifyLongUnicodeWrapping()
        verifyFontMetrics()
        verifyFontFallback()
        verifyIndexedLineClipping()
        verifyFieldHeights()
        verifySyntaxChunks()
        verifyScrollRestoration(scroll)
        verifyViewportHighlighting(document: document, editor: editor, host: host)
        // Switching a binary response to Raw must keep TextKit responsive, including
        // its idle layout pass. Control bytes previously trapped that pass in layout.
        let watchdog = DispatchWorkItem { exit(86) }
        DispatchQueue.global().asyncAfter(deadline: .now() + 10, execute: watchdog)
        let binaryStart = ContinuousClock.now
        let binary = Data((0..<400).flatMap { _ in (0...255).map(UInt8.init) })
        document.language = .plain
        document.text = String(decoding: binary, as: UTF8.self)
        host.layoutSubtreeIfNeeded()
        host.displayIfNeeded()
        RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        precondition(editor.string == document.text)
        document.text = fixture
        document.language = .json
        host.layoutSubtreeIfNeeded()
        host.displayIfNeeded()
        RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        let binaryTime = milliseconds(since: binaryStart)
        watchdog.cancel()
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("wirebolt-editor-benchmark-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: url) }
        FileManager.default.createFile(atPath: url.path, contents: nil)
        let file = try FileHandle(forWritingTo: url)
        let chunk = Data((String(repeating: "0123456789", count: 10) + "\n").utf8)
        let block = (0..<1000).reduce(into: Data()) { data, _ in data.append(chunk) }
        for _ in 0..<1040 { try file.write(contentsOf: block) }
        try file.close()
        let indexStart = ContinuousClock.now
        let font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        let index = try ResponseTextIndex(url: url, columns: 120,
            wrapping: CodeTextWrapping(fontName: font.fontName, fontSize: 12, width: 870))
        let indexTime = milliseconds(since: indexStart)
        var viewportSamples: [Double] = []
        for row in stride(from: 0, to: index.rowCount, by: max(1, index.rowCount / 30)) {
            let start = ContinuousClock.now
            let rows = try index.rows(start: row, count: 45)
            precondition(!rows.isEmpty)
            viewportSamples.append(milliseconds(since: start))
        }
        let viewportP95 = viewportSamples.sorted()[Int(Double(viewportSamples.count - 1) * 0.95)]
        let result: [String: Any] = ["nativeEditorColdMilliseconds": cold, "binaryRawTransitionMilliseconds": binaryTime, "nativeEditorTypingP95Milliseconds": p95,
            "responseBytes": block.count * 1040, "backgroundIndexMilliseconds": indexTime,
            "indexedViewportP95Milliseconds": viewportP95, "rows": index.rowCount]
        print(String(decoding: try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys]), as: UTF8.self))
        // Cold construction includes framework setup. The interactive budgets remain unchanged.
        if p95 > 16 || viewportP95 > 4 { exit(1) }
    }

    @MainActor private static func verifyCodeWrapping() {
        let source = "  \"path\": \"/json?limit=10&search=wirebolt\",\n  \"message\": \"Parity audit — Unicode: café, 東京, 🚀\","
        let storage = NSTextStorage(attributedString: SyntaxHighlighter.attributedString(text: source, language: .json))
        let manager = NSLayoutManager()
        storage.addLayoutManager(manager)
        let container = CodeTextContainer(containerSize: NSSize(width: 267, height: 10000))
        container.lineFragmentPadding = 0
        manager.addTextContainer(container)
        manager.ensureLayout(for: container)
        var lines: [String] = []
        manager.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: manager.numberOfGlyphs)) { _, _, _, range, _ in
            let characters = manager.characterRange(forGlyphRange: range, actualGlyphRange: nil)
            lines.append((source as NSString).substring(with: characters))
        }
        precondition(lines == ["  \"path\": \"/json?limit=10&", "search=wirebolt\",\n", "  \"message\": \"Parity audit — ", "Unicode: café, 東京, 🚀\","])
        precondition(storage.string == source)
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try! Data(source.utf8).write(to: url)
        let font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        let index = try! ResponseTextIndex(url: url, columns: 36,
            wrapping: CodeTextWrapping(fontName: font.fontName, fontSize: 12, width: 267))
        let indexed = try! index.rows(start: 0, count: index.rowCount).map(\.text)
        precondition(indexed == lines.map { $0.replacingOccurrences(of: "\n", with: "") })
    }

    @MainActor private static func verifyIndexedLineClipping() {
        let source = String(repeating: "ASCII café 東京 👨‍👩‍👧‍👦 שלום مرحبا /path?a=1&b=2; ", count: 80)
        for size in [10.0, 12.0, 28.0] {
            let font = NSFont.monospacedSystemFont(ofSize: size, weight: .regular)
            let styled = NSAttributedString(string: source, attributes: [.font: font, .foregroundColor: NSColor.black])
            let reference = NSAttributedString(string: source, attributes: [.font: font, .foregroundColor: NSColor.black.cgColor])
            let fullLine = CTLineCreateWithAttributedString(reference)
            let clippedLine = IndexedTextLine(styled, fontSize: size)
            var prepared: IndexedTextLine?
            Task { prepared = try? await IndexedTextLine.prepare(styled, fontSize: size) }
            let deadline = Date(timeIntervalSinceNow: 5)
            while prepared == nil && Date() < deadline { RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.001)) }
            precondition(prepared != nil, "Background shaping must finish")
            for candidate in [clippedLine, prepared!] {
                for origin in [0.0, 97.0, 733.0, 2200.0, 22000.0] {
                    func render(clipped: Bool) -> NSBitmapImageRep {
                        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 280, pixelsHigh: 80,
                            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
                        bitmap.bitmapData!.initialize(repeating: 0, count: bitmap.bytesPerRow * bitmap.pixelsHigh)
                        let context = NSGraphicsContext(bitmapImageRep: bitmap)!.cgContext
                        context.translateBy(x: -origin, y: 80)
                        context.scaleBy(x: 1, y: -1)
                        let prior = NSGraphicsContext.current
                        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
                        defer { NSGraphicsContext.current = prior }
                        let visible = CGRect(x: origin, y: 0, width: 280, height: 80)
                        if clipped { candidate.draw(at: CGPoint(x: 0, y: 3), visible: visible) }
                        else {
                            context.clip(to: visible)
                            context.textMatrix = CGAffineTransform(scaleX: 1, y: -1)
                            context.textPosition = CGPoint(x: 0, y: 3 + NSLayoutManager().defaultBaselineOffset(for: font))
                            CTLineDraw(fullLine, context)
                        }
                        return bitmap
                    }
                    let expected = render(clipped: false), actual = render(clipped: true)
                    let bytes = expected.bytesPerRow * expected.pixelsHigh
                    precondition(UnsafeBufferPointer(start: expected.bitmapData!, count: bytes).elementsEqual(
                        UnsafeBufferPointer(start: actual.bitmapData!, count: bytes)), "Synchronous and background shaping must preserve Unicode glyphs at every horizontal position")
                }
            }
        }
    }

    @MainActor private static func verifySyntaxChunks() {
        // Chunk boundaries must not split Unicode, tokens, indentation, or links.
        // A line-at-a-time oracle also catches missing/duplicated boundary text.
        for language in [SyntaxLanguage.json, .http, .plain] {
            let line = "    {\"message\":\"café 東京 👨‍👩‍👧‍👦 https://example.invalid/path\",\"active\":true,\"n\":123,\"v\":null}\n"
            let source = String(repeating: line, count: 700) + "  \"last\": false"
            let expected = NSMutableAttributedString(string: "")
            let styled = SyntaxHighlighter.attributedString(text: line, language: language)
            for _ in 0..<700 { expected.append(styled) }
            expected.append(SyntaxHighlighter.attributedString(text: "  \"last\": false", language: language))
            let actual = SyntaxHighlighter.attributedString(text: source, language: language)
            precondition(actual.isEqual(to: expected), "Chunked syntax must preserve every character and style")
        }
    }

    @MainActor private static func verifyViewportHighlighting(document: EditorDocument, editor: NSTextView, host: NSView) {
        let line = "    {\"message\":\"café 東京 👨‍👩‍👧‍👦\",\"url\":\"https://example.invalid\"},\n"
        document.text = "[\n" + String(repeating: line, count: 2000) + "{}\n]"
        RunLoop.current.run(until: Date().addingTimeInterval(0.02))
        host.layoutSubtreeIfNeeded()
        for fraction in [0, 1, 2] {
            let source = editor.string as NSString
            let offset = source.length * fraction / 3
            let key = source.range(of: "\"message\"", range: NSRange(location: offset, length: source.length - offset))
            editor.setSelectedRange(key)
            editor.scrollRangeToVisible(key)
            editor.display()
            let drawn = (editor.delegate as? NativeCodeEditor.Coordinator)?.layoutManager(editor.layoutManager!,
                shouldUseTemporaryAttributes: [:], forDrawingToScreen: true, atCharacterIndex: key.location, effectiveRange: nil)
            let color = (drawn?[.foregroundColor] ?? editor.textStorage?.attribute(.foregroundColor, at: key.location, effectiveRange: nil)) as? NSColor
            precondition(color == WireboltTheme.nsJSONKey, "Newly visible JSON keys must be highlighted")
            let link = source.range(of: "https://example.invalid", range: NSRange(location: key.location, length: source.length - key.location))
            precondition(editor.textStorage?.attribute(.link, at: link.location, effectiveRange: nil) as? String == "https://example.invalid",
                "Viewport highlighting must preserve clickable links")
            precondition(editor.selectedRange() == key, "Highlighting must not move the selection")
            precondition(editor.string == document.text, "Highlighting must preserve the entire Unicode document")
        }
    }

    @MainActor private static func verifyFieldHeights() {
        // Measured narrow and wide columns, including a soft-wrap boundary.
        for (value, narrow) in [("abcdef", 20.0), ("abcdefg", 34.0), ("abcdefgh", 34.0),
            ("abcdefghijkl", 34.0), ("abcdefghijklmnopqrstuvwxyz", 76.0)] {
            precondition(FieldEditorMetrics.height(for: value, width: 75) == narrow)
            precondition(FieldEditorMetrics.height(for: value, width: 274) == 20)
        }
        precondition(FieldEditorMetrics.height(for: "first\nsecond", width: 274) == 34)
        precondition(FieldEditorMetrics.height(for: "10", width: 1) == 34)
        precondition(FieldEditorMetrics.height(for: "wirebolt", width: 1) == 118)
    }

    @MainActor private static func verifyScrollRestoration(_ native: NSScrollView) {
        let plain = NSScrollView(frame: native.frame)
        plain.contentView = EditorClipView()
        plain.documentView = ScrollFixtureView(frame: NSRect(x: 0, y: 0, width: plain.contentSize.width, height: 4000))
        plain.tile()
        for scroll in [native, plain] {
            let clip = scroll.contentView as! EditorClipView
            let original = clip.bounds.origin
            let changed = clip.changed
            var saved = CGPoint.zero
            clip.changed = { saved = $0 }
            clip.restorePresentationOrigin(.zero)
            let leading = clip.bounds.origin
            precondition(saved == .zero)
            if scroll === native {
                precondition(abs(leading.x + (native.verticalRulerView?.ruleThickness ?? 0)) < 1)
            } else { precondition(leading.x == 0) }
            clip.restorePresentationOrigin(CGPoint(x: 0, y: 90))
            precondition(abs(clip.bounds.origin.y - leading.y - 90) < 1)
            precondition(abs(saved.y - 90) < 1 && saved.x == 0)
            clip.restorePresentationOrigin(.zero)
            precondition(clip.bounds.origin == leading)
            clip.changed = changed
            clip.scroll(to: original)
        }
    }

    @MainActor private static func verifyLongUnicodeWrapping() {
        let source = String(repeating: "café 東京 👨‍👩‍👧‍👦 /path?x=1&y=2; ", count: 1200)
        let storage = NSTextStorage(attributedString: SyntaxHighlighter.attributedString(text: source, language: .plain))
        let manager = NSLayoutManager()
        storage.addLayoutManager(manager)
        let container = CodeTextContainer(containerSize: NSSize(width: 267, height: CGFloat.greatestFiniteMagnitude))
        container.lineFragmentPadding = 0
        manager.addTextContainer(container)
        manager.ensureLayout(for: container)
        var lines: [String] = []
        manager.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: manager.numberOfGlyphs)) { _, _, _, range, _ in
            lines.append((source as NSString).substring(with: manager.characterRange(forGlyphRange: range, actualGlyphRange: nil)))
        }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: url) }
        try! Data(source.utf8).write(to: url)
        let font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        let index = try! ResponseTextIndex(url: url, columns: 36,
            wrapping: CodeTextWrapping(fontName: font.fontName, fontSize: 12, width: 267))
        let indexed = try! index.rows(start: 0, count: index.rowCount).map(\.text)
        if indexed != lines {
            let differences = zip(indexed, lines).enumerated().filter { $0.element.0 != $0.element.1 }.prefix(3)
            let report = "Unicode wrapping: indexed \(indexed.count), native \(lines.count), differences \(Array(differences))\n"
            FileHandle.standardError.write(Data(report.utf8))
            preconditionFailure("Native and indexed Unicode wrapping differ")
        }
    }

    @MainActor private static func verifyFontFallback() {
        let source = "ASCII café e\u{301} 東京 العربية עברית हिन्दी ไทย 👨‍👩‍👧‍👦 🚀\nsecond line"
        func layout(_ storage: NSTextStorage, source: String, edit: Bool) -> ([String], [NSRect]) {
            let probe = GlyphFontProbe()
            let manager = NSLayoutManager()
            manager.delegate = probe
            storage.setAttributedString(SyntaxHighlighter.attributedString(text: source, language: .plain))
            if edit {
                storage.ensureAttributesAreFixed(in: NSRange(location: 0, length: storage.length))
                storage.replaceCharacters(in: NSRange(location: 4093, length: 3), with: "e\u{301} 👨‍👩‍👧‍👦")
            }
            storage.addLayoutManager(manager)
            let container = CodeTextContainer(containerSize: NSSize(width: 267, height: CGFloat.greatestFiniteMagnitude))
            container.lineFragmentPadding = 0
            manager.addTextContainer(container)
            manager.ensureLayout(for: container)
            var rects: [NSRect] = []
            manager.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: manager.numberOfGlyphs)) { rect, _, _, _, _ in rects.append(rect) }
            return (probe.glyphs, rects)
        }
        for (fixture, edit) in [(source, false), (String(repeating: "a", count: 4093) + source + String(repeating: "b", count: 5000) + source, false),
                                (String(repeating: "a", count: 4093) + source + source, true)] {
            let native = layout(NSTextStorage(), source: fixture, edit: edit)
            let code = layout(CodeTextStorage(), source: fixture, edit: edit)
            precondition(!native.0.isEmpty && native.0 == code.0, "Font fallback must generate the same glyphs and fonts for Unicode scripts, chunk boundaries and edits")
            precondition(native.1 == code.1, "Font fallback must preserve line metrics and wrapping")
        }
    }

    @MainActor private static func verifyFontMetrics() {
        // Measured reference sizes: 10/12/28 pt use 15/18/42 pt lines and
        // start their content 57/63/113 pt from the response panel's left edge.
        for (size, height, contentLeft) in [(10.0, 15.0, 57.0), (12.0, 18.0, 63.0), (28.0, 42.0, 113.0)] {
            let storage = NSTextStorage(attributedString: SyntaxHighlighter.attributedString(text: "first\nsecond", language: .plain, fontSize: size))
            let manager = NSLayoutManager()
            storage.addLayoutManager(manager)
            let container = CodeTextContainer(containerSize: NSSize(width: 900, height: 1000))
            manager.addTextContainer(container)
            manager.ensureLayout(for: container)
            let first = manager.lineFragmentRect(forGlyphAt: 0, effectiveRange: nil)
            let second = manager.lineFragmentRect(forGlyphAt: 6, effectiveRange: nil)
            precondition(abs(second.minY - first.minY - height) < 0.1)
            precondition(CodeEditorMetrics.gutterWidth(size, lineCount: 100) + 4 == contentLeft)
        }
    }

    @MainActor private static func textView(in view: NSView) -> NSTextView? {
        if let text = view as? NSTextView { return text }
        return view.subviews.lazy.compactMap { textView(in: $0) }.first
    }
    private static func milliseconds(since start: ContinuousClock.Instant) -> Double {
        let duration = start.duration(to: .now).components
        return Double(duration.seconds) * 1000 + Double(duration.attoseconds) / 1e15
    }
}

@MainActor private final class GlyphFontProbe: NSObject, @preconcurrency NSLayoutManagerDelegate {
    var glyphs: [String] = []
    func layoutManager(_ layoutManager: NSLayoutManager, shouldGenerateGlyphs glyphs: UnsafePointer<CGGlyph>,
        properties props: UnsafePointer<NSLayoutManager.GlyphProperty>, characterIndexes charIndexes: UnsafePointer<Int>,
        font: NSFont, forGlyphRange glyphRange: NSRange) -> Int {
        for i in 0..<glyphRange.length { self.glyphs.append("\(charIndexes[i]):\(font.fontName):\(glyphs[i])") }
        return 0
    }
}
