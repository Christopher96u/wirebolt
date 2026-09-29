import AppKit
import SwiftUI

struct IndexedResponseEditor: View {
    let url: URL
    let preview: String
    let language: SyntaxLanguage
    let search: String
    var prefix = ""
    var find: EditorFindState?
    @State private var localFind = EditorFindState()
    @Environment(\.editorIsActive) private var isActive
    @AppStorage("editor.fontSize") private var fontSize = 12.0
    @AppStorage("editor.wordWrap") private var wraps = true
    @State private var index: ResponseTextIndex?
    @State private var failure = false

    var body: some View {
        GeometryReader { geometry in
            let gutter = CodeEditorMetrics.gutterWidth(fontSize, lineCount: index?.lineCount ?? 1)
            let font = NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)
            let width = max(1, geometry.size.width - gutter - 22)
            let advance = (" " as NSString).size(withAttributes: [.font: font]).width
            let columns = wraps ? max(1, Int(width / advance)) : Int.max / 4
            let wrapping = wraps ? CodeTextWrapping(fontName: font.fontName, fontSize: fontSize, columns: columns) : nil
            Group {
                if let index {
                    IndexedCodeScrollView(index: index, fontSize: fontSize, language: language, search: search, wraps: wraps,
                        storageKey: prefix.isEmpty ? "Response body \(language)" : "Raw response", find: find ?? localFind)
                } else if failure {
                    Text("The response could not be opened.").foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .opacity(index == nil || index?.url == url ? 1 : 0)
            .overlay { if let index, index.url != url { ProgressView() } }
            .overlay(alignment: .bottomTrailing) {
                if index?.isComplete == false {
                    ProgressView().controlSize(.small).padding(8).help("Loading response…")
                }
            }
            // Keyed by column count, not pixels: a live resize only re-wraps when a column is gained or lost.
            .task(id: "\(url.path):\(columns):\(fontSize):\(prefix):\(isActive)") {
                guard isActive else { return }
                failure = false
                do {
                    if let index, index.url == url, index.columns != columns || index.wrapping != wrapping {
                        // Debounce re-wrapping while the width keeps changing; cancellation restarts the wait.
                        try await Task.sleep(for: .milliseconds(120))
                    }
                    if index == nil || index?.url != url {
                        let first = try await ResponseIndexCache.shared.firstViewport(url: url, columns: columns, prefix: prefix, wrapping: wrapping)
                        try Task.checkCancellation()
                        index = first
                        if first.isComplete { return }
                        // Give SwiftUI a turn to draw the first viewport before publishing the full layout.
                        await Task.yield()
                    }
                    let result = try await ResponseIndexCache.shared.index(url: url, columns: columns, prefix: prefix, wrapping: wrapping)
                    try Task.checkCancellation()
                    index = result
                } catch is CancellationError {} catch { failure = true }
            }
        }
        .editorFindOverlay(find ?? localFind, isActive: isActive)
    }
}

private struct IndexedCodeScrollView: NSViewRepresentable {
    @Environment(\.editorStorage) private var editorStorage
    @Environment(\.editorIsActive) private var isActive
    let index: ResponseTextIndex
    let fontSize: Double
    let language: SyntaxLanguage
    let search: String
    let wraps: Bool
    let storageKey: String
    let find: EditorFindState
    @AppStorage("editor.scrollBeyondLastLine") private var beyond = true

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        CodeScroller.configure(scroll)
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = true
        scroll.backgroundColor = .textBackgroundColor
        scroll.contentView = EditorClipView()
        scroll.documentView = IndexedCodeView()
        updateNSView(scroll, context: context)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? IndexedCodeView else { return }
        setEditorVisibility(scroll, active: isActive)
        guard isActive else { return }
        let restore = view.presentation == nil
        if restore { view.presentation = editorStorage?.state(for: storageKey) }
        (scroll.contentView as? EditorClipView)?.changed = { [weak view] point in
            view?.presentation?.origin = point
            view?.visibleAreaChanged()
        }
        let changed = view.index?.rowCount != index.rowCount || view.index?.url != index.url || view.index?.columns != index.columns || view.index?.wrapping != index.wrapping || view.fontSize != fontSize || view.language != language
        let oldIndex = view.index
        let oldOrigin = scroll.contentView.bounds.origin
        let oldRow = max(0, Int(oldOrigin.y / view.lineHeight))
        let fraction = oldOrigin.y / view.lineHeight - Double(oldRow)
        let anchor = changed && oldIndex?.url == index.url ? try? oldIndex?.rows(start: oldRow, count: 1).first : nil
        view.index = index
        view.fontSize = fontSize
        view.language = language
        scroll.hasHorizontalScroller = !wraps
        view.autoresizingMask = wraps ? [.width] : []
        let width = wraps ? scroll.contentSize.width : max(scroll.contentSize.width, Double(index.maximumColumns) * (" " as NSString).size(withAttributes: [.font: NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)]).width + view.gutterWidth + 22)
        view.setFrameSize(NSSize(width: width, height: max(scroll.contentSize.height, Double(index.rowCount) * view.lineHeight + 2 * CodeEditorMetrics.textTopInset(fontSize) + (beyond ? max(0, scroll.contentSize.height - view.lineHeight) : 0))))
        scroll.contentInsets = NSEdgeInsets(top: 0, left: 0, bottom: 0, right: 0)
        if changed {
            view.invalidateRows()
            if let anchor, let mapped = try? index.row(preserving: anchor) {
                let origin = NSPoint(x: oldOrigin.x, y: (Double(mapped) + fraction) * view.lineHeight)
                scroll.contentView.scroll(to: origin)
                scroll.reflectScrolledClipView(scroll.contentView)
            }
        }
        if restore, let presentation = view.presentation {
            let origin = presentation.origin
            view.restoreSelection()
            DispatchQueue.main.async { [weak scroll] in
                guard let scroll else { return }
                (scroll.contentView as? EditorClipView)?.restorePresentationOrigin(origin)
                scroll.reflectScrolledClipView(scroll.contentView)
            }
        }
        if view.query != search { view.find(search) }
        if isActive { view.updateFind(find) }
    }
}

@MainActor
final class IndexedCodeView: NSView, NSUserInterfaceValidations {
    var presentation: EditorPresentationStorage.State?
    var index: ResponseTextIndex?
    var fontSize = 12.0
    var language = SyntaxLanguage.plain
    private(set) var query = ""
    var lineHeight: Double { CodeEditorMetrics.lineHeight(fontSize) }
    var gutterWidth: Double { CodeEditorMetrics.gutterWidth(fontSize, lineCount: index?.lineCount ?? 1) }
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    private(set) var hasDrawnViewport = false
    private struct ShapedLine {
        let text: String
        let language: SyntaxLanguage
        let fontSize: Double
        let appearance: NSAppearance.Name
        let line: IndexedTextLine
        var complete = true
    }
    private var shapedLines: [ShapedLine] = []
    private var shapingTask: Task<Void, Never>?
    private var shapingText: String?
    private var cachedRows: [ResponseTextIndex.Row] = []
    private var loadingRange: Range<Int>?
    private var loadTask: Task<Void, Never>?
    private var findTask: Task<Void, Never>?
    private var selectionStart: (row: Int, column: Int)?
    private var selectionEnd: (row: Int, column: Int)?
    deinit { loadTask?.cancel(); findTask?.cancel(); shapingTask?.cancel() }

    func restoreSelection() {
        guard let selection = presentation?.indexedSelection else { return }
        selectionStart = (selection.startRow, selection.startColumn)
        selectionEnd = (selection.endRow, selection.endColumn)
    }
    private func saveSelection() {
        guard let start = selectionStart, let end = selectionEnd else { return }
        presentation?.indexedSelection = (start.row, start.column, end.row, end.column)
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(true)
        setAccessibilityRole(.textArea)
        setAccessibilityLabel("Response body")
    }
    required init?(coder: NSCoder) { nil }

    private weak var findState: EditorFindState?
    private var findQuery = TextSearchQuery()
    private var findScopeOnly = false
    private var findSource = ""
    private var findMatches: [ResponseTextIndex.SearchMatch] = []
    private var findIndex: Int?

    func updateFind(_ state: EditorFindState) {
        findState = state
        guard let index else { return }
        let query = state.isVisible ? state.query : TextSearchQuery()
        let source = "\(index.url.path):\(index.rowCount):\(index.columns):\(String(describing: index.wrapping))"
        if query != findQuery || state.selectionOnly != findScopeOnly || source != findSource {
            findQuery = query
            findScopeOnly = state.selectionOnly
            findSource = source
            findTask?.cancel()
            findMatches = []
            findIndex = nil
            needsDisplay = true
            let scope = state.selectionOnly ? state.indexedSelection : nil
            findTask = Task { [weak self, weak state] in
                do {
                    if !query.text.isEmpty { try await Task.sleep(for: .milliseconds(100)) }
                    let worker = Task.detached(priority: .userInitiated) { try index.search(query, selection: scope) }
                    let matches = try await withTaskCancellationHandler { try await worker.value } onCancel: { worker.cancel() }
                    try Task.checkCancellation()
                    guard let self, let state else { return }
                    self.findMatches = matches
                    state.update(count: matches.count)
                    self.applyFindSelection(state)
                    self.needsDisplay = true
                } catch is CancellationError {} catch {
                    state?.update(count: 0, error: "Invalid regular expression")
                }
            }
        }
        applyFindSelection(state)
    }

    private func applyFindSelection(_ state: EditorFindState) {
        guard state.isVisible, let current = state.currentMatch, findMatches.indices.contains(current), findIndex != current else { return }
        findIndex = current
        let match = findMatches[current]
        selectionStart = (match.start.row, match.start.column)
        selectionEnd = (match.end.row, match.end.column)
        scroll(NSPoint(x: 0, y: Double(match.start.row) * lineHeight))
        needsDisplay = true
    }

    private func findHighlights(row: Int, length: Int) -> [NSRange] {
        var lower = 0
        var upper = findMatches.count
        while lower < upper {
            let middle = (lower + upper) / 2
            if findMatches[middle].end.row < row { lower = middle + 1 } else { upper = middle }
        }
        var ranges: [NSRange] = []
        while lower < findMatches.count, findMatches[lower].start.row <= row {
            let match = findMatches[lower]
            let start = match.start.row == row ? min(length, match.start.column) : 0
            let end = match.end.row == row ? min(length, match.end.column) : length
            if end > start { ranges.append(NSRange(location: start, length: end - start)) }
            lower += 1
        }
        return ranges
    }

    func invalidateRows() {
        shapingTask?.cancel()
        shapingText = nil
        loadTask?.cancel()
        hasDrawnViewport = false
        cachedRows = []
        loadingRange = nil
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.textBackgroundColor.setFill()
        dirtyRect.fill()
        guard let index else { return }
        let first = max(0, Int(visibleRect.minY / lineHeight) - 4)
        let end = min(index.rowCount, Int(visibleRect.maxY / lineHeight) + 5)
        let range = first..<max(first, end)
        if cachedRows.first?.number ?? Int.max > first || cachedRows.last?.number ?? -1 < end - 1 {
            load(range)
        }
        let numberAttributes: [NSAttributedString.Key: Any] = [.font: NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular), .foregroundColor: NSColor.secondaryLabelColor]
        let selection = orderedSelection
        let singleLongLine = cachedRows.filter { $0.text.utf16.count > 2048 }.count == 1
        for row in cachedRows where range.contains(row.number) {
            let y = Double(row.number) * lineHeight + CodeEditorMetrics.textTopInset(fontSize)
            if !row.continuation {
                let number = String(row.line + 1) as NSString
                number.draw(at: NSPoint(x: gutterWidth - 22 - number.size(withAttributes: numberAttributes).width, y: y), withAttributes: numberAttributes)
            }
            if row.text.utf16.count > 2048, selection == nil, query.isEmpty, findMatches.isEmpty {
                let cached: ShapedLine
                if let found = shapedLines.firstIndex(where: { $0.text == row.text && $0.language == language && $0.fontSize == fontSize && $0.appearance == effectiveAppearance.name }) {
                    cached = shapedLines.remove(at: found)
                } else {
                    let source = row.text as NSString
                    let progressive = singleLongLine && source.length <= 2 * 1024 * 1024
                    let prefix = progressive ? source.substring(with: source.rangeOfComposedCharacterSequences(for: NSRange(location: 0, length: min(2048, source.length)))) : row.text
                    cached = ShapedLine(text: row.text, language: language, fontSize: fontSize, appearance: effectiveAppearance.name,
                        line: IndexedTextLine(SyntaxHighlighter.attributedString(text: prefix, language: language, fontSize: fontSize), fontSize: fontSize), complete: !progressive)
                }
                shapedLines.append(cached)
                if !cached.complete { prepareLongLine(row.text) }
                while shapedLines.count > 2 || shapedLines.reduce(0, { $0 + $1.text.utf16.count }) > 2 * 1024 * 1024 { shapedLines.removeFirst() }
                cached.line.draw(at: CGPoint(x: gutterWidth + 4 + row.indent, y: y), visible: visibleRect)
                hasDrawnViewport = true
                continue
            }
            let attributed = NSMutableAttributedString(attributedString: SyntaxHighlighter.attributedString(text: row.text, language: language, fontSize: fontSize))
            for range in findHighlights(row: row.number, length: attributed.length) {
                attributed.addAttribute(.backgroundColor, value: NSColor.findHighlightColor.withAlphaComponent(0.25), range: range)
            }
            if let (start, end) = selection, row.number >= start.row && row.number <= end.row {
                let lower = row.number == start.row ? min(start.column, attributed.length) : 0
                let upper = row.number == end.row ? min(end.column, attributed.length) : attributed.length
                if upper > lower { attributed.addAttribute(.backgroundColor, value: NSColor.selectedTextBackgroundColor, range: NSRange(location: lower, length: upper - lower)) }
            }
            if !query.isEmpty {
                let match = (row.text as NSString).range(of: query)
                if match.location != NSNotFound { attributed.addAttribute(.backgroundColor, value: NSColor.findHighlightColor, range: match) }
            }
            attributed.draw(at: NSPoint(x: gutterWidth + 4 + row.indent, y: y))
            hasDrawnViewport = true
        }
    }

    private func prepareLongLine(_ text: String) {
        guard shapingText == nil else { return }
        shapingTask?.cancel()
        shapingText = text
        let fontSize = fontSize, language = language, appearance = effectiveAppearance.name
        shapingTask = Task { [weak self] in
            do {
                let styled = SyntaxHighlighter.attributedString(text: text, language: language, fontSize: fontSize)
                let line = try await IndexedTextLine.prepare(styled, fontSize: fontSize)
                try Task.checkCancellation()
                guard let self, self.fontSize == fontSize, self.language == language, self.effectiveAppearance.name == appearance else { return }
                self.shapedLines.removeAll { $0.text == text }
                self.shapedLines.append(ShapedLine(text: text, language: language, fontSize: fontSize, appearance: appearance, line: line))
                while self.shapedLines.count > 2 { self.shapedLines.removeFirst() }
                self.shapingText = nil
                self.needsDisplay = true
            } catch {
                if !Task.isCancelled { self?.shapingText = nil }
            }
        }
    }

    private func load(_ range: Range<Int>) {
        guard loadingRange != range, let index else { return }
        loadTask?.cancel()
        loadingRange = range
        loadTask = Task { [weak self] in
            let reader = Task.detached(priority: .userInitiated) { try index.rows(start: range.lowerBound, count: range.count) }
            let rows = try? await withTaskCancellationHandler { try await reader.value } onCancel: { reader.cancel() }
            guard !Task.isCancelled, let self, let rows else { return }
            self.cachedRows = rows
            self.updateAccessibilityWindow()
            self.needsDisplay = true
        }
    }

    func find(_ query: String) {
        self.query = query
        findTask?.cancel()
        needsDisplay = true
        guard !query.isEmpty, let index else { return }
        findTask = Task { [weak self] in
            let search = Task.detached(priority: .userInitiated) { try index.firstMatch(query) }
            let row = try? await withTaskCancellationHandler { try await search.value } onCancel: { search.cancel() }
            guard !Task.isCancelled, let self, let row else { return }
            self.scroll(NSPoint(x: 0, y: Double(row) * self.lineHeight))
        }
    }

    private func position(_ event: NSEvent) -> (row: Int, column: Int) {
        let point = convert(event.locationInWindow, from: nil)
        let row = min(max(0, Int((point.y - CodeEditorMetrics.textTopInset(fontSize)) / lineHeight)), max(0, (index?.rowCount ?? 1) - 1))
        guard let content = cachedRows.first(where: { $0.number == row }) else { return (row, 0) }
        let text = content.text
        let target = max(0, point.x - gutterWidth - 4 - content.indent)
        let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)]
        var prefix = ""
        var previousWidth = 0.0
        for character in text {
            let previous = prefix.utf16.count
            prefix.append(character)
            let width = (prefix as NSString).size(withAttributes: attributes).width
            if target < (previousWidth + width) / 2 { return (row, previous) }
            previousWidth = width
        }
        return (row, prefix.utf16.count)
    }
    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        selectionStart = position(event)
        selectionEnd = selectionStart
        saveSelection()
        needsDisplay = true
    }
    override func mouseDragged(with event: NSEvent) {
        selectionEnd = position(event)
        saveSelection()
        _ = autoscroll(with: event)
        needsDisplay = true
    }
    override func mouseUp(with event: NSEvent) {
        selectionChanged()
    }
    @objc func performFindPanelAction(_ sender: Any?) { performTextFinderAction(sender) }
    func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        if item.action == #selector(performFindPanelAction(_:)) || item.action == #selector(performTextFinderAction(_:)) {
            switch item.tag {
            case 1: return true
            case 2, 3: return !findMatches.isEmpty
            case 11: return findState?.isVisible == true
            default: return false
            }
        }
        return true
    }
    override func performTextFinderAction(_ sender: Any?) {
        switch (sender as? NSMenuItem)?.tag ?? 1 {
        case 1: findState?.isVisible = true
        case 2: findState?.move(1)
        case 3: findState?.move(-1)
        case 11: findState?.isVisible = false
        default: super.performTextFinderAction(sender)
        }
    }
    override func keyDown(with event: NSEvent) {
        if event.modifierFlags.contains(.shift), extendSelection(for: event) { return }
        if event.modifierFlags.contains(.command), event.charactersIgnoringModifiers == "f" { findState?.isVisible = true }
        else if event.modifierFlags.contains(.command), event.charactersIgnoringModifiers == "a" { selectAll(nil) }
        else if event.modifierFlags.contains(.command), event.charactersIgnoringModifiers == "c" { copy(nil) }
        else {
            let origin = visibleRect.origin
            let target: Double? = switch event.keyCode {
            case 115: 0
            case 119: max(0, frame.height - visibleRect.height)
            case 116: max(0, origin.y - visibleRect.height)
            case 121: min(frame.height - visibleRect.height, origin.y + visibleRect.height)
            case 125: event.modifierFlags.contains(.command) ? max(0, frame.height - visibleRect.height) : origin.y + lineHeight
            case 126: event.modifierFlags.contains(.command) ? 0 : max(0, origin.y - lineHeight)
            default: nil
            }
            if let target { scroll(NSPoint(x: origin.x, y: target)) }
            else { super.keyDown(with: event) }
        }
    }
    override func selectAll(_ sender: Any?) {
        selectionStart = (0, 0)
        selectionEnd = (max(0, (index?.rowCount ?? 1) - 1), Int.max)
        selectionChanged()
    }

    private func selectionChanged() {
        saveSelection()
        if let (start, end) = orderedSelection, start != end {
            findState?.indexedSelection = (.init(row: start.row, column: start.column), .init(row: end.row, column: end.column))
        } else { findState?.indexedSelection = nil }
        needsDisplay = true
        NSAccessibility.post(element: self, notification: .selectedTextChanged)
    }

    private func rowLength(_ row: Int) -> Int? {
        cachedRows.first(where: { $0.number == row }).map { ($0.text as NSString).length }
    }

    /// Shift + arrow, Page Up/Down, Home/End extend the selection from its anchor,
    /// matching NSTextView so the response is selectable without a mouse.
    private func extendSelection(for event: NSEvent) -> Bool {
        guard let index, index.rowCount > 0 else { return false }
        let lastRow = index.rowCount - 1
        let page = max(1, Int(visibleRect.height / lineHeight) - 1)
        let command = event.modifierFlags.contains(.command)
        let anchor = selectionStart ?? (min(lastRow, max(0, Int(visibleRect.minY / lineHeight))), 0)
        var end = selectionEnd ?? anchor
        end.column = min(end.column, rowLength(end.row) ?? end.column)
        switch event.keyCode {
        case 126: end = command ? (0, 0) : (max(0, end.row - 1), end.column)
        case 125: end = command ? (lastRow, Int.max) : (min(lastRow, end.row + 1), end.column)
        case 123:
            if end.column > 0 { end.column -= 1 }
            else if end.row > 0 { end = (end.row - 1, rowLength(end.row - 1) ?? 0) }
        case 124:
            if let length = rowLength(end.row), end.column >= length, end.row < lastRow { end = (end.row + 1, 0) }
            else { end.column += 1 }
        case 116: end.row = max(0, end.row - page)
        case 121: end.row = min(lastRow, end.row + page)
        case 115: end = (0, 0)
        case 119: end = (lastRow, Int.max)
        default: return false
        }
        selectionStart = anchor
        selectionEnd = end
        let y = Double(end.row) * lineHeight
        if y < visibleRect.minY || y + lineHeight > visibleRect.maxY {
            scroll(NSPoint(x: visibleRect.minX, y: max(0, y - (end.row > anchor.row ? visibleRect.height - 2 * lineHeight : lineHeight))))
        }
        selectionChanged()
        return true
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
    func visibleAreaChanged() {
        if window?.firstResponder === self { noteFocusRingMaskChanged() }
    }

    // MARK: Accessibility

    /// Assistive technologies see the loaded viewport (the visible rows plus a
    /// small margin) as a text area with lines, selection and insertion point.
    /// Moving the selection past either edge scrolls and loads the next rows.
    private var accessibilityRows: [ResponseTextIndex.Row] = []
    private var accessibilityLineStarts: [Int] = []
    private var accessibilityText: NSString = ""

    private func updateAccessibilityWindow() {
        accessibilityRows = cachedRows
        var starts: [Int] = []
        var location = 0
        for row in cachedRows {
            starts.append(location)
            location += (row.text as NSString).length + 1
        }
        accessibilityLineStarts = starts
        accessibilityText = cachedRows.map(\.text).joined(separator: "\n") as NSString
        setAccessibilityValue(accessibilityText as String)
        NSAccessibility.post(element: self, notification: .valueChanged)
    }

    private func accessibilityOffset(row: Int, column: Int) -> Int? {
        guard let line = accessibilityRows.firstIndex(where: { $0.number == row }) else { return nil }
        return accessibilityLineStarts[line] + min(column, (accessibilityRows[line].text as NSString).length)
    }

    private func accessibilityPosition(_ offset: Int) -> (row: Int, column: Int)? {
        guard !accessibilityRows.isEmpty else { return nil }
        let line = accessibilityLine(for: offset)
        return (accessibilityRows[line].number, offset - accessibilityLineStarts[line])
    }

    override func accessibilityNumberOfCharacters() -> Int { accessibilityText.length }
    override func accessibilityVisibleCharacterRange() -> NSRange { NSRange(location: 0, length: accessibilityText.length) }
    override func accessibilityString(for range: NSRange) -> String? {
        let clamped = NSIntersectionRange(range, NSRange(location: 0, length: accessibilityText.length))
        return accessibilityText.substring(with: clamped)
    }
    override func accessibilityLine(for index: Int) -> Int {
        var lower = 0, upper = accessibilityLineStarts.count
        while lower + 1 < upper {
            let middle = (lower + upper) / 2
            if accessibilityLineStarts[middle] <= index { lower = middle } else { upper = middle }
        }
        return lower
    }
    override func accessibilityRange(forLine line: Int) -> NSRange {
        guard accessibilityRows.indices.contains(line) else { return NSRange(location: NSNotFound, length: 0) }
        return NSRange(location: accessibilityLineStarts[line], length: (accessibilityRows[line].text as NSString).length)
    }
    override func accessibilitySelectedTextRange() -> NSRange {
        guard let (start, end) = orderedSelection,
              let lower = accessibilityOffset(row: start.row, column: start.column) else {
            return NSRange(location: accessibilityLineStarts.first ?? 0, length: 0)
        }
        let upper = accessibilityOffset(row: end.row, column: end.column) ?? accessibilityText.length
        return NSRange(location: lower, length: max(0, upper - lower))
    }
    override func setAccessibilitySelectedTextRange(_ range: NSRange) {
        guard let start = accessibilityPosition(range.location),
              let end = accessibilityPosition(NSMaxRange(range)) else { return }
        selectionStart = start
        selectionEnd = end
        // Keep the assistive window moving with the reading position.
        let y = Double(end.row) * lineHeight
        if y < visibleRect.minY || y + lineHeight > visibleRect.maxY {
            scroll(NSPoint(x: visibleRect.minX, y: max(0, y - visibleRect.height / 2)))
        }
        selectionChanged()
    }
    override func accessibilitySelectedText() -> String? {
        accessibilityString(for: accessibilitySelectedTextRange())
    }
    override func accessibilityInsertionPointLineNumber() -> Int {
        accessibilityLine(for: accessibilitySelectedTextRange().location)
    }
    private var orderedSelection: ((row: Int, column: Int), (row: Int, column: Int))? {
        guard let start = selectionStart, let end = selectionEnd else { return nil }
        return start.row < end.row || (start.row == end.row && start.column <= end.column) ? (start, end) : (end, start)
    }
    @objc func copy(_ sender: Any?) {
        guard let index, let (start, end) = orderedSelection else { return }
        Task {
            let result = await Task.detached(priority: .userInitiated) {
                guard let rows = try? index.rows(start: start.row, count: end.row - start.row + 1) else { return "" }
                return rows.map { row in
                    let string = row.text as NSString
                    let lower = row.number == start.row ? min(start.column, string.length) : 0
                    let upper = row.number == end.row ? min(end.column, string.length) : string.length
                    return (row.number > start.row && !row.continuation ? "\n" : "") + string.substring(with: NSRange(location: lower, length: max(0, upper - lower)))
                }.joined()
            }.value
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(result, forType: .string)
        }
    }
}
