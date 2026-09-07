import AppKit
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
        verifyFontMetrics()
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
        let index = try ResponseTextIndex(url: url, columns: 120)
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
