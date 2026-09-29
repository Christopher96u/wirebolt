import AppKit
import SwiftUI

struct ResponseHexView: NSViewRepresentable {
    let store: ResponseBodyStore?
    let data: Data
    let byteCount: UInt64

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = HexScrollView()
        CodeScroller.configure(scroll)
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = true
        scroll.backgroundColor = .textBackgroundColor
        scroll.documentView = HexGridView()
        updateNSView(scroll, context: context)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? HexGridView else { return }
        let changed = view.store?.url != store?.url || view.byteCount != byteCount
        view.store = store
        view.preview = data
        view.byteCount = byteCount
        view.autoresizingMask = [.width]
        view.resizeViewport(scroll.contentSize)
        if changed { view.invalidate() }
    }
}

@MainActor
private final class HexScrollView: NSScrollView {
    override func tile() {
        super.tile()
        (documentView as? HexGridView)?.resizeViewport(contentSize)
    }
}

@MainActor
private final class HexGridView: NSView {
    var store: ResponseBodyStore?
    var preview = Data()
    var byteCount: UInt64 = 0
    var columns = 38
    private var bytes = Data()
    private var firstByte: UInt64 = 0
    private var loading: Range<UInt64>?
    private var task: Task<Void, Never>?
    private var anchor: UInt64?
    private var selection: Range<UInt64>?
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true)
        setAccessibilityRole(.textArea)
        setAccessibilityLabel("Hex response body")
    }
    required init?(coder: NSCoder) { nil }

    func resizeViewport(_ size: NSSize) {
        guard size.width > 0 else { return }
        let updated = max(1, Int((size.width - 8) / 25.4))
        if columns != updated { columns = updated; invalidate() }
        let rows = (Double(byteCount) / Double(columns)).rounded(.up)
        let proposed = NSSize(width: size.width, height: max(size.height, rows * 15))
        if frame.size != proposed { setFrameSize(proposed) }
    }

    func invalidate() {
        task?.cancel()
        loading = nil
        bytes = Data()
        selection = nil
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.textBackgroundColor.setFill()
        dirtyRect.fill()
        let firstRow = max(0, Int(visibleRect.minY / 15))
        let lastRow = max(firstRow, Int(visibleRect.maxY / 15) + 1)
        let start = min(byteCount, UInt64(firstRow * columns))
        let end = min(byteCount, UInt64((lastRow + 1) * columns))
        if firstByte > start || firstByte + UInt64(bytes.count) < end { load(start..<end) }
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedSystemFont(ofSize: 10, weight: .regular),
            .foregroundColor: NSColor.labelColor,
        ]
        for row in firstRow...lastRow {
            let y = Double(row * 15)
            if row % 2 == 1 {
                NSColor.alternatingContentBackgroundColors[1].setFill()
                NSRect(x: 0, y: y, width: bounds.width, height: 15).fill()
            }
            for column in 0..<columns {
                let offset = UInt64(row * columns + column)
                guard offset >= firstByte, offset < firstByte + UInt64(bytes.count), offset < byteCount else { continue }
                let byte = bytes[Int(offset - firstByte)]
                let x = 4 + Double(column) * 18
                if selection?.contains(offset) == true {
                    NSColor.selectedTextBackgroundColor.setFill()
                    NSRect(x: x - 2, y: y, width: 18, height: 15).fill()
                }
                (String(format: "%02X", byte) as NSString).draw(at: NSPoint(x: x, y: y + 1), withAttributes: attributes)
                let character = (32...126).contains(byte) ? String(UnicodeScalar(byte)) : "."
                (character as NSString).draw(at: NSPoint(x: 4 + Double(columns) * 18 + 22 + Double(column) * 7.4, y: y + 1), withAttributes: attributes)
            }
        }
    }

    private func load(_ range: Range<UInt64>) {
        guard !range.isEmpty, loading != range else { return }
        task?.cancel()
        loading = range
        let store = store
        let preview = preview
        task = Task { [weak self] in
            let bytes: Data
            if let store { bytes = (try? await store.viewport(offset: range.lowerBound, length: Int(range.count))) ?? Data() }
            else { bytes = Data(preview.dropFirst(Int(range.lowerBound)).prefix(Int(range.count))) }
            guard !Task.isCancelled, let self else { return }
            self.firstByte = range.lowerBound
            self.bytes = bytes
            self.setAccessibilityValue(bytes.map { String(format: "%02X", $0) }.joined(separator: " "))
            NSAccessibility.post(element: self, notification: .valueChanged)
            self.needsDisplay = true
        }
    }

    private func offset(at event: NSEvent) -> UInt64 {
        let point = convert(event.locationInWindow, from: nil)
        let ascii = 4 + Double(columns) * 18 + 22
        let column = point.x >= ascii ? Int((point.x - ascii) / 7.4) : Int((point.x - 2) / 18)
        return min(byteCount == 0 ? 0 : byteCount - 1, UInt64(max(0, Int(point.y / 15)) * columns + min(columns - 1, max(0, column))))
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let offset = offset(at: event)
        anchor = offset
        selection = offset..<min(byteCount, offset + 1)
        needsDisplay = true
    }
    override func mouseDragged(with event: NSEvent) {
        guard let anchor else { return }
        let offset = offset(at: event)
        selection = min(anchor, offset)..<min(byteCount, max(anchor, offset) + 1)
        _ = autoscroll(with: event)
        needsDisplay = true
    }
    override func mouseUp(with event: NSEvent) { selectionChanged() }
    override func selectAll(_ sender: Any?) { anchor = 0; selection = 0..<byteCount; selectionChanged() }
    @objc func copy(_ sender: Any?) {
        guard let selection else { return }
        let store = store
        let preview = preview
        Task {
            let data: Data
            if let store { data = (try? await store.viewport(offset: selection.lowerBound, length: Int(selection.count))) ?? Data() }
            else { data = Data(preview.dropFirst(Int(selection.lowerBound)).prefix(Int(selection.count))) }
            let formatted = await Task.detached(priority: .userInitiated) { data.map { String(format: "%02X", $0) }.joined(separator: " ") }.value
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(formatted, forType: .string)
        }
    }
    override func keyDown(with event: NSEvent) {
        if event.modifierFlags.contains(.command), event.charactersIgnoringModifiers == "a" { selectAll(nil) }
        else if event.modifierFlags.contains(.command), event.charactersIgnoringModifiers == "c" { copy(nil) }
        else if moveCursor(for: event) {}
        else if event.keyCode == 119 { scroll(NSPoint(x: 0, y: max(0, frame.height - visibleRect.height))) }
        else if event.keyCode == 115 { scroll(.zero) }
        else { super.keyDown(with: event) }
    }

    /// Arrow and Page keys move a byte cursor; with Shift they extend the selection
    /// from its anchor (Home/End extend to the first or last byte).
    private func moveCursor(for event: NSEvent) -> Bool {
        guard byteCount > 0 else { return false }
        let extending = event.modifierFlags.contains(.shift)
        let last = Int64(byteCount - 1)
        let row = Int64(columns)
        let page = row * Int64(max(1, Int(visibleRect.height / 15) - 1))
        let origin = selection.map { anchor == $0.lowerBound ? $0.upperBound - 1 : $0.lowerBound }
            ?? min(byteCount - 1, UInt64(max(0, Int(visibleRect.minY / 15)) * columns))
        var cursor = Int64(origin)
        switch event.keyCode {
        case 123: cursor -= 1
        case 124: cursor += 1
        case 126: cursor -= row
        case 125: cursor += row
        case 116: cursor -= page
        case 121: cursor += page
        case 115 where extending: cursor = 0
        case 119 where extending: cursor = last
        default: return false
        }
        let target = UInt64(min(last, max(0, cursor)))
        let fixed = extending && selection != nil ? (anchor ?? origin) : extending ? origin : target
        anchor = fixed
        selection = min(fixed, target)..<(max(fixed, target) + 1)
        let y = Double(Int(target) / columns) * 15
        if y < visibleRect.minY || y + 15 > visibleRect.maxY {
            scroll(NSPoint(x: 0, y: max(0, y < visibleRect.minY ? y : y + 15 - visibleRect.height)))
        }
        selectionChanged()
        return true
    }

    private func selectionChanged() {
        needsDisplay = true
        NSAccessibility.post(element: self, notification: .selectedTextChanged)
    }

    // MARK: Focus ring

    override var focusRingMaskBounds: NSRect { visibleRect.insetBy(dx: 3, dy: 3) }
    override func drawFocusRingMask() { visibleRect.insetBy(dx: 3, dy: 3).fill() }
    override func becomeFirstResponder() -> Bool {
        noteFocusRingMaskChanged()
        return super.becomeFirstResponder()
    }
    override func resignFirstResponder() -> Bool {
        noteFocusRingMaskChanged()
        return super.resignFirstResponder()
    }
    override func viewDidMoveToSuperview() {
        super.viewDidMoveToSuperview()
        NotificationCenter.default.removeObserver(self, name: NSView.boundsDidChangeNotification, object: nil)
        guard let clip = superview as? NSClipView else { return }
        clip.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(self, selector: #selector(visibleAreaChanged), name: NSView.boundsDidChangeNotification, object: clip)
    }
    @objc private func visibleAreaChanged() {
        if window?.firstResponder === self { noteFocusRingMaskChanged() }
    }

    // MARK: Accessibility

    /// The value lists the loaded bytes as "XX XX …"; selection maps onto it (3 characters per byte).
    override func accessibilityNumberOfCharacters() -> Int { max(0, bytes.count * 3 - 1) }
    override func accessibilitySelectedTextRange() -> NSRange {
        guard let selection, selection.lowerBound >= firstByte, selection.lowerBound < firstByte + UInt64(bytes.count) else {
            return NSRange(location: 0, length: 0)
        }
        let lower = Int(selection.lowerBound - firstByte)
        let upper = min(bytes.count, Int(selection.upperBound - firstByte))
        return NSRange(location: lower * 3, length: max(0, (upper - lower) * 3 - 1))
    }
    override func setAccessibilitySelectedTextRange(_ range: NSRange) {
        guard !bytes.isEmpty else { return }
        let lower = firstByte + UInt64(min(bytes.count - 1, range.location / 3))
        let upper = firstByte + UInt64(min(bytes.count - 1, NSMaxRange(range) / 3))
        anchor = lower
        selection = lower..<(max(lower, upper) + 1)
        selectionChanged()
    }
    override func accessibilitySelectedText() -> String? {
        let range = accessibilitySelectedTextRange()
        guard range.length > 0, let value = accessibilityValue() as? String else { return nil }
        let text = value as NSString
        return text.substring(with: NSIntersectionRange(range, NSRange(location: 0, length: text.length)))
    }
}
