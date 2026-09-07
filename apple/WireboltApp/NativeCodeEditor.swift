import AppKit
import CoreText
import SwiftUI

enum SyntaxLanguage: Equatable {
    case json
    case xml
    case html
    case http
    case plain

    var folding: CodeProjection.Syntax {
        switch self {
        case .json: .json
        case .xml: .xml
        case .html: .html
        case .http, .plain: .none
        }
    }
}

@MainActor
enum CodeEditorMetrics {
    static func lineHeight(_ fontSize: Double) -> Double { (fontSize * 1.5).rounded() }
    static func lineSpacing(_ fontSize: Double) -> Double {
        let font = NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)
        return max(0, lineHeight(fontSize) - NSLayoutManager().defaultLineHeight(for: font))
    }
    static func gutterWidth(_ fontSize: Double, lineCount: Int) -> Double {
        let font = NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)
        let digitWidth = ("0" as NSString).size(withAttributes: [.font: font]).width
        return (Double(max(5, String(lineCount).count)) * digitWidth).rounded() + 22
    }
}

struct SyntaxTextView: View {
    let text: String
    let language: SyntaxLanguage
    var search = ""
    var find: EditorFindState?
    @State private var localFind = EditorFindState()
    var body: some View {
        NativeCodeEditor(text: .constant(text), editable: false, language: language, label: "Response body", search: search, find: find ?? localFind)
            .frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
            .editorFindOverlay(find ?? localFind)
    }
}

@MainActor
final class EditorPresentationStorage {
    final class State {
        var origin = CGPoint.zero
        var selection = NSRange(location: 0, length: 0)
        var collapsed: Set<Int> = []
        var indexedSelection: (startRow: Int, startColumn: Int, endRow: Int, endColumn: Int)?
    }
    private var states: [String: State] = [:]
    func state(for label: String) -> State {
        if let state = states[label] { return state }
        let state = State()
        states[label] = state
        return state
    }
}

private struct EditorStorageKey: EnvironmentKey {
    static let defaultValue: EditorPresentationStorage? = nil
}
extension EnvironmentValues {
    var editorStorage: EditorPresentationStorage? {
        get { self[EditorStorageKey.self] }
        set { self[EditorStorageKey.self] = newValue }
    }
}

struct NativeCodeEditor: NSViewRepresentable {
    @Binding var text: String
    @Environment(\.editorStorage) private var editorStorage
    var editable = true
    var language: SyntaxLanguage = .plain
    var label = "Request body"
    var search = ""
    var find: EditorFindState?
    @AppStorage("editor.fontSize") private var fontSize = 12.0
    @AppStorage("editor.wordWrap") private var wrapsLines = true
    @AppStorage("editor.showInvisibles") private var showInvisibles = true
    @AppStorage("editor.scrollBeyondLastLine") private var scrollBeyond = true

    func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: NSScrollView, context: Context) -> CGSize? {
        CGSize(width: proposal.width ?? 300, height: proposal.height ?? 200)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NativeEditorScrollView()
        CodeScroller.configure(scroll)
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = true
        scroll.backgroundColor = .textBackgroundColor
        let clip = EditorClipView()
        scroll.contentView = clip
        let view = FindableCodeTextView(frame: NSRect(origin: .zero, size: scroll.contentSize))
        view.replaceTextContainer(CodeTextContainer(containerSize: scroll.contentSize))
        clip.changed = { [weak coordinator = context.coordinator] origin in
            guard let coordinator, !coordinator.updating else { return }
            coordinator.presentation?.origin = origin
        }
        let layoutManager = CodeLayoutManager()
        layoutManager.allowsNonContiguousLayout = true
        layoutManager.backgroundLayoutEnabled = false
        layoutManager.delegate = context.coordinator
        view.textContainer?.replaceLayoutManager(layoutManager)
        view.isRichText = false
        view.isAutomaticQuoteSubstitutionEnabled = false
        view.isAutomaticDashSubstitutionEnabled = false
        view.isAutomaticTextReplacementEnabled = false
        view.isAutomaticSpellingCorrectionEnabled = false
        view.isContinuousSpellCheckingEnabled = false
        view.allowsUndo = true
        view.usesFindBar = find == nil
        view.findAction = { [weak coordinator = context.coordinator] action in
            guard let state = coordinator?.findState else { return false }
            if action == 2 { state.move(1) }
            else if action == 3 { state.move(-1) }
            else { state.isVisible = true }
            return true
        }
        view.isIncrementalSearchingEnabled = true
        view.isVerticallyResizable = true
        view.minSize = .zero
        view.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        view.textContainerInset = NSSize(width: 4, height: 3)
        view.textContainer?.lineFragmentPadding = 0
        view.delegate = context.coordinator
        scroll.documentView = view
        scroll.verticalRulerView = CodeLineRuler(scrollView: scroll, orientation: .verticalRuler)
        scroll.hasVerticalRuler = true
        scroll.rulersVisible = true
        (scroll.verticalRulerView as? CodeLineRuler)?.toggle = { [weak coordinator = context.coordinator, weak scroll] line in
            guard let coordinator, let scroll else { return }
            if coordinator.collapsed.contains(line) { coordinator.collapsed.remove(line) }
            else { coordinator.collapsed.insert(line) }
            coordinator.presentation?.collapsed = coordinator.collapsed
            coordinator.render(in: scroll)
        }
        updateNSView(scroll, context: context)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? NSTextView else { return }
        context.coordinator.text = $text
        context.coordinator.findState = find
        view.isEditable = editable
        view.setAccessibilityLabel(label)
        let coordinator = context.coordinator
        if coordinator.presentation == nil {
            coordinator.presentation = editorStorage?.state(for: label)
            coordinator.collapsed = coordinator.presentation?.collapsed ?? []
        }
        let needsStyle = coordinator.fontSize != fontSize || coordinator.language != language
            || coordinator.appearance != view.effectiveAppearance.name
        coordinator.fontSize = fontSize
        coordinator.language = language
        coordinator.appearance = view.effectiveAppearance.name
        if coordinator.projection.source != text || needsStyle || coordinator.needsHighlight {
            coordinator.render(in: scroll)
        }
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = CodeEditorMetrics.lineSpacing(fontSize)
        paragraph.lineBreakMode = .byCharWrapping
        view.typingAttributes = [.paragraphStyle: paragraph, .font: NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular), .foregroundColor: NSColor.labelColor]
        (view.layoutManager as? CodeLayoutManager)?.drawInvisibles = showInvisibles
        scroll.contentInsets = NSEdgeInsets(top: 0, left: 0, bottom: 0, right: 0)
        (scroll as? NativeEditorScrollView)?.scrollBeyond = scrollBeyond
        if context.coordinator.search != search {
            context.coordinator.search = search
            if !search.isEmpty {
                let range = (view.string as NSString).range(of: search, options: .caseInsensitive)
                if range.location != NSNotFound { view.setSelectedRange(range); view.scrollRangeToVisible(range) }
            }
        }
        scroll.hasHorizontalScroller = !wrapsLines
        (scroll.verticalRulerView as? CodeLineRuler)?.updateMetrics(fontSize: fontSize)
        view.isHorizontallyResizable = !wrapsLines
        view.autoresizingMask = wrapsLines ? [.width] : []
        view.textContainer?.widthTracksTextView = wrapsLines
        let viewport = (scroll as? NativeEditorScrollView)?.textViewportSize ?? scroll.contentSize
        let width = wrapsLines ? max(1, viewport.width) : CGFloat.greatestFiniteMagnitude
        view.textContainer?.containerSize = NSSize(width: width, height: CGFloat.greatestFiniteMagnitude)
        if wrapsLines { view.setFrameSize(NSSize(width: viewport.width, height: max(view.frame.height, viewport.height))) }
        else { view.sizeToFit() }
        (scroll as? NativeEditorScrollView)?.updateDocumentSize()
        scroll.verticalRulerView?.needsDisplay = true
        if let find { coordinator.refreshFind(in: scroll, state: find) }
        if !coordinator.restored, let presentation = coordinator.presentation {
            coordinator.restored = true
            DispatchQueue.main.async { [weak scroll, weak view] in
                guard let scroll, let view else { return }
                coordinator.updating = true
                if NSMaxRange(presentation.selection) <= (view.string as NSString).length { view.setSelectedRange(presentation.selection) }
                scroll.contentView.scroll(to: presentation.origin)
                scroll.reflectScrolledClipView(scroll.contentView)
                coordinator.updating = false
            }
        }
    }

    @MainActor
    final class Coordinator: NSObject, NSTextViewDelegate, @preconcurrency NSLayoutManagerDelegate {
        var text: Binding<String>
        var search = ""
        var updating = false
        var needsHighlight = false
        var fontSize = 0.0
        var language: SyntaxLanguage = .plain
        var appearance: NSAppearance.Name?
        var presentation: EditorPresentationStorage.State?
        var restored = false
        var collapsed: Set<Int> = []
        var projection = CodeProjection(source: "")
        private var editedRange: NSRange?
        init(text: Binding<String>) { self.text = text }

        weak var findState: EditorFindState?
        private var findQuery = TextSearchQuery()
        private var findSource = ""
        private var findScope: NSRange?
        private var findRanges: [NSRange] = []
        private var findIndex: Int?
        private var findTask: Task<Void, Never>?
        deinit { findTask?.cancel() }

        func refreshFind(in scroll: NSScrollView, state: EditorFindState) {
            guard let view = scroll.documentView as? NSTextView else { return }
            let query = state.isVisible ? state.query : TextSearchQuery()
            let scope = state.selectionOnly ? state.selection : nil
            if !query.text.isEmpty, !collapsed.isEmpty {
                collapsed.removeAll()
                presentation?.collapsed = []
                render(in: scroll)
            }
            if query != findQuery || scope != findScope || view.string != findSource {
                findQuery = query
                findScope = scope
                findSource = view.string
                findIndex = nil
                findTask?.cancel()
                findRanges = []
                let whole = NSRange(location: 0, length: (view.string as NSString).length)
                view.layoutManager?.removeTemporaryAttribute(.backgroundColor, forCharacterRange: whole)
                let source = view.string
                findTask = Task { [weak self, weak view, weak state] in
                    do {
                        let worker = Task.detached(priority: .userInitiated) { try query.matches(in: source, range: scope) }
                        let ranges = try await withTaskCancellationHandler { try await worker.value } onCancel: { worker.cancel() }
                        try Task.checkCancellation()
                        guard let self, let view, let state else { return }
                        self.findRanges = ranges
                        for range in ranges where range.length > 0 {
                            view.layoutManager?.addTemporaryAttribute(.backgroundColor, value: NSColor.findHighlightColor.withAlphaComponent(0.25), forCharacterRange: range)
                        }
                        state.update(count: ranges.count)
                        self.applyFindSelection(in: view, state: state)
                    } catch is CancellationError {} catch {
                        state?.update(count: 0, error: "Invalid regular expression")
                    }
                }
            }
            applyFindSelection(in: view, state: state)
        }

        private func applyFindSelection(in view: NSTextView, state: EditorFindState) {
            if state.isVisible, let index = state.currentMatch, findRanges.indices.contains(index), findIndex != index {
                findIndex = index
                view.setSelectedRange(findRanges[index])
                view.scrollRangeToVisible(findRanges[index])
            }
        }

        func layoutManager(_ layoutManager: NSLayoutManager, didCompleteLayoutFor textContainer: NSTextContainer?, atEnd layoutFinishedFlag: Bool) {
            guard layoutFinishedFlag else { return }
            (textContainer?.textView?.enclosingScrollView as? NativeEditorScrollView)?.updateDocumentSize()
        }

        func render(in scroll: NSScrollView) {
            guard let view = scroll.documentView as? NSTextView else { return }
            let selection = view.selectedRange()
            let sourceSelection = projection.sourceOffset(selection.location)
            updating = true
            projection = CodeProjection(source: text.wrappedValue, collapsed: collapsed, syntax: language.folding)
            view.textStorage?.setAttributedString(SyntaxHighlighter.attributedString(text: projection.text, language: language, fontSize: fontSize))
            view.setSelectedRange(NSRange(location: min(projection.displayOffset(sourceSelection), (projection.text as NSString).length), length: 0))
            needsHighlight = false
            (scroll.verticalRulerView as? CodeLineRuler)?.updateProjection(projection, folding: language.folding != .none)
            updating = false
        }

        func textView(_ textView: NSTextView, shouldChangeTextIn affectedCharRange: NSRange, replacementString: String?) -> Bool {
            editedRange = NSRange(location: affectedCharRange.location, length: (replacementString as NSString?)?.length ?? 0)
            guard !updating, !collapsed.isEmpty, let replacementString, let scroll = textView.enclosingScrollView else { return true }
            let offset = projection.sourceOffset(affectedCharRange.location)
            let updated = projection.replacing(displayRange: affectedCharRange, with: replacementString)
            let previous = text.wrappedValue
            textView.undoManager?.registerUndo(withTarget: self) { target in
                MainActor.assumeIsolated { target.text.wrappedValue = previous; target.collapsed.removeAll() }
            }
            collapsed.removeAll()
            presentation?.collapsed = []
            text.wrappedValue = updated
            render(in: scroll)
            textView.setSelectedRange(NSRange(location: offset + (replacementString as NSString).length, length: 0))
            return false
        }

        func textDidChange(_ notification: Notification) {
            guard !updating, let view = notification.object as? NSTextView else { return }
            // JSON tokens cannot cross an unescaped newline. Recolor only the edited
            // paragraphs, preserving TextKit's layout and the user's selection elsewhere.
            // Other grammars keep a full recolor because a tag can span several lines.
            if let editedRange, let storage = view.textStorage, let scroll = view.enclosingScrollView,
               language == .json || language == .plain || language == .http {
                updating = true
                let string = view.string as NSString
                let start = min(editedRange.location, string.length)
                let range = string.lineRange(for: NSRange(location: start, length: min(editedRange.length, string.length - start)))
                let styled = SyntaxHighlighter.attributedString(text: string.substring(with: range), language: language, fontSize: fontSize)
                storage.beginEditing()
                storage.setAttributes([:], range: range)
                styled.enumerateAttributes(in: NSRange(location: 0, length: styled.length)) { attributes, local, _ in
                    storage.setAttributes(attributes, range: NSRange(location: range.location + local.location, length: local.length))
                }
                storage.endEditing()
                projection = CodeProjection(source: view.string, syntax: language.folding)
                (scroll.verticalRulerView as? CodeLineRuler)?.updateProjection(projection, folding: language == .json)
                updating = false
                needsHighlight = false
            } else { needsHighlight = true }
            editedRange = nil
            text.wrappedValue = view.string
        }
        func textViewDidChangeSelection(_ notification: Notification) {
            if let view = notification.object as? NSTextView, let findState, !findState.isVisible {
                let range = view.selectedRange()
                Task { @MainActor [weak findState] in
                    guard findState?.isVisible == false else { return }
                    findState?.selection = range
                }
            }
            guard !updating, let view = notification.object as? NSTextView else { return }
            presentation?.selection = view.selectedRange()
        }

    }
}

private final class FindableCodeTextView: NSTextView {
    var findAction: ((Int) -> Bool)?
    override func performFindPanelAction(_ sender: Any?) {
        let action = (sender as? NSMenuItem)?.tag ?? 1
        if findAction?(action) != true { super.performFindPanelAction(sender) }
    }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
           event.charactersIgnoringModifiers == "f", findAction?(1) == true { return true }
        return super.performKeyEquivalent(with: event)
    }
}

private final class CodeLayoutManager: NSLayoutManager {
    var drawInvisibles = false
    override func processEditing(for textStorage: NSTextStorage, edited editMask: NSTextStorageEditActions,
        range newCharRange: NSRange, changeInLength delta: Int, invalidatedRange invalidatedCharRange: NSRange) {
        for container in textContainers { (container as? CodeTextContainer)?.invalidateParagraph() }
        super.processEditing(for: textStorage, edited: editMask, range: newCharRange,
            changeInLength: delta, invalidatedRange: invalidatedCharRange)
    }
    override func drawGlyphs(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        super.drawGlyphs(forGlyphRange: glyphsToShow, at: origin)
        guard drawInvisibles, let storage = textStorage, let container = textContainers.first else { return }
        let string = storage.string as NSString
        let chars = characterRange(forGlyphRange: glyphsToShow, actualGlyphRange: nil)
        let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 8), .foregroundColor: NSColor.tertiaryLabelColor]
        for index in chars.location ..< NSMaxRange(chars) where string.character(at: index) == 32 {
            let glyph = glyphIndexForCharacter(at: index)
            let bounds = boundingRect(forGlyphRange: NSRange(location: glyph, length: 1), in: container)
            ("·" as NSString).draw(at: NSPoint(x: origin.x + bounds.midX - 1, y: origin.y + bounds.midY - 4), withAttributes: attributes)
        }
    }
}

@MainActor
final class CodeScroller: NSScroller {
    override class func scrollerWidth(for controlSize: NSControl.ControlSize, scrollerStyle: NSScroller.Style) -> CGFloat { 14 }

    static func configure(_ scroll: NSScrollView) {
        scroll.verticalScroller = CodeScroller()
        scroll.horizontalScroller = CodeScroller()
        scroll.scrollerStyle = .legacy
        scroll.autohidesScrollers = false
    }

    override func drawKnobSlot(in slotRect: NSRect, highlight flag: Bool) {
        NSColor.textBackgroundColor.setFill()
        slotRect.fill()
    }

    override func drawKnob() {
        var knob = rect(for: .knob)
        guard knobProportion < 1 else { return }
        if bounds.height > bounds.width { knob.origin.x = 0; knob.size.width = bounds.width }
        else { knob.origin.y = 0; knob.size.height = bounds.height }
        NSColor.secondaryLabelColor.withAlphaComponent(0.4).setFill()
        knob.fill()
    }
}

@MainActor
private final class NativeEditorScrollView: NSScrollView {
    var scrollBeyond = true
    private var updatingDocumentSize = false

    var textViewportSize: NSSize {
        // AppKit reserves ruler space inside the clip view using a negative bounds
        // origin. That space is not available for document text or soft wrapping.
        var size = contentView.bounds.size
        if rulersVisible, hasVerticalRuler { size.width -= verticalRulerView?.ruleThickness ?? 0 }
        if rulersVisible, hasHorizontalRuler { size.height -= horizontalRulerView?.ruleThickness ?? 0 }
        return NSSize(width: max(1, size.width), height: max(1, size.height))
    }

    override func tile() {
        super.tile()
        updateDocumentSize()
    }

    func updateDocumentSize() {
        guard !updatingDocumentSize, let text = documentView as? NSTextView,
              let manager = text.layoutManager, let container = text.textContainer else { return }
        updatingDocumentSize = true
        defer { updatingDocumentSize = false }
        let viewport = textViewportSize
        let extra = scrollBeyond ? max(0, viewport.height - (text.font?.pointSize ?? 12) - 6) : 0
        let height = max(viewport.height, manager.usedRect(for: container).maxY + text.textContainerInset.height * 2 + extra)
        let width = text.isHorizontallyResizable ? max(viewport.width, text.frame.width) : viewport.width
        text.minSize = NSSize(width: 0, height: height)
        let proposed = NSSize(width: width, height: height)
        if text.frame.size != proposed { text.setFrameSize(proposed) }
    }
}

@MainActor
final class EditorClipView: NSClipView {
    var changed: ((CGPoint) -> Void)?
    override func scroll(to newOrigin: NSPoint) {
        super.scroll(to: newOrigin)
        changed?(bounds.origin)
    }
}

@MainActor
private final class CodeLineRuler: NSRulerView {
    private var lines: [(offset: Int, number: Int, fold: Bool)] = []
    private var hitAreas: [(NSRect, Int)] = []
    private var collapsed: Set<Int> = []
    var toggle: ((Int) -> Void)?

    override init(scrollView: NSScrollView?, orientation: NSRulerView.Orientation) {
        super.init(scrollView: scrollView, orientation: orientation)
        ruleThickness = 60
        clientView = scrollView?.documentView
    }
    required init(coder: NSCoder) { fatalError("init(coder:) is unavailable") }

    func updateMetrics(fontSize: Double) {
        let width = CodeEditorMetrics.gutterWidth(fontSize, lineCount: (lines.last?.number ?? 0) + 1)
        if ruleThickness != width { ruleThickness = width }
    }

    func updateProjection(_ projection: CodeProjection, folding: Bool) {
        let source = projection.source
        var starts = [0]
        for (offset, unit) in source.utf16.enumerated() where unit == 10 { starts.append(offset + 1) }
        let foldLines = Set(folding ? projection.folds.map(\.line) : [])
        var line = 0
        let displayStarts = [0] + projection.text.utf16.enumerated().compactMap { $0.element == 10 ? $0.offset + 1 : nil }
        lines = displayStarts.map { offset in
            let original = projection.sourceOffset(offset)
            while line + 1 < starts.count && starts[line + 1] <= original { line += 1 }
            return (offset, line, foldLines.contains(line))
        }
        collapsed = projection.collapsed
        needsDisplay = true
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        if let hit = hitAreas.first(where: { $0.0.contains(point) }) { toggle?(hit.1) }
        else { super.mouseDown(with: event) }
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let view = clientView as? NSTextView, let manager = view.layoutManager,
              let container = view.textContainer else { return }
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSBezierPath(rect: bounds).addClip()
        let attributes: [NSAttributedString.Key: Any] = [
            .font: view.font ?? NSFont.monospacedSystemFont(ofSize: 12, weight: .regular),
            .foregroundColor: NSColor.secondaryLabelColor
        ]
        let glyphs = manager.glyphRange(forBoundingRect: view.visibleRect, in: container)
        let chars = manager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
        hitAreas.removeAll(keepingCapacity: true)
        for line in lines where line.offset >= chars.location && line.offset <= NSMaxRange(chars) {
            let point: NSPoint
            if line.offset < (view.string as NSString).length {
                let glyph = manager.glyphIndexForCharacter(at: line.offset)
                point = manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil).origin
            } else { point = manager.extraLineFragmentRect.origin }
            let origin = convert(NSPoint(x: 0, y: point.y + view.textContainerOrigin.y), from: view)
            let label = String(line.number + 1) as NSString
            let size = label.size(withAttributes: attributes)
            label.draw(at: NSPoint(x: ruleThickness - size.width - 22, y: origin.y), withAttributes: attributes)
            if line.fold {
                let hit = NSRect(x: ruleThickness - 17, y: origin.y, width: 16, height: max(18, size.height))
                hitAreas.append((hit, line.number))
                let path = NSBezierPath()
                let x = hit.midX, y = hit.midY
                let scale = (view.font?.pointSize ?? 12) / 12
                if collapsed.contains(line.number) {
                    path.move(to: NSPoint(x: x - 2 * scale, y: y - 4 * scale)); path.line(to: NSPoint(x: x + 2 * scale, y: y)); path.line(to: NSPoint(x: x - 2 * scale, y: y + 4 * scale))
                } else {
                    path.move(to: NSPoint(x: x - 4 * scale, y: y - 2 * scale)); path.line(to: NSPoint(x: x, y: y + 2 * scale)); path.line(to: NSPoint(x: x + 4 * scale, y: y - 2 * scale))
                }
                NSColor.secondaryLabelColor.setStroke()
                path.lineWidth = max(1, scale)
                path.stroke()
            }
        }
    }
}

final class CodeTextContainer: NSTextContainer {
    // Code punctuation defines wrapping opportunities independently of natural
    // language word boundaries. The storage remains the original editable text.
    nonisolated private static let breakAfter = Set(" \t})]?|/&.,;¢°′″‰℃、。｡､￠，．：；？！％・･ゝゞヽヾーァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゕゖㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ々〻ｧｨｩｪｫｬｭｮｯｰ”〉》」』】〕）］｝｣".utf16)
    nonisolated private static let breakBefore = Set("([{‘“〈《「『【〔（［｛｢£¥＄￡￥+＋".utf16)
    private var paragraph: (range: NSRange, text: NSString, font: NSFont, ascii: Bool, typesetter: CTTypesetter?)?

    func invalidateParagraph() { paragraph = nil }

    override var isSimpleRectangularTextContainer: Bool { true }

    override func lineFragmentRect(forProposedRect proposed: NSRect, at index: Int,
        writingDirection: NSWritingDirection, remaining: UnsafeMutablePointer<NSRect>?) -> NSRect {
        var rect = super.lineFragmentRect(forProposedRect: proposed, at: index,
            writingDirection: writingDirection, remaining: remaining)
        guard let storage = layoutManager?.textStorage, index < storage.length else { return rect }
        let font = storage.attribute(.font, at: index, effectiveRange: nil) as? NSFont
            ?? .monospacedSystemFont(ofSize: 12, weight: .regular)
        if paragraph == nil || !NSLocationInRange(index, paragraph!.range) || paragraph!.font != font {
            let source = storage.string as NSString
            let range = source.lineRange(for: NSRange(location: index, length: 0))
            let text = source.substring(with: range) as NSString
            let ascii = (0..<text.length).allSatisfy { (32...126).contains(text.character(at: $0)) || text.character(at: $0) == 10 || text.character(at: $0) == 13 }
            let typesetter = ascii ? nil : CTTypesetterCreateWithAttributedString(NSAttributedString(string: text as String,
                attributes: [.font: font]) as CFAttributedString)
            paragraph = (range, text, font, ascii, typesetter)
        }
        guard let paragraph else { return rect }
        let text = paragraph.text
        let start = index - paragraph.range.location
        let style = storage.attribute(.paragraphStyle, at: index, effectiveRange: nil) as? NSParagraphStyle
        let indent = start == 0 ? (style?.firstLineHeadIndent ?? 0) : (style?.headIndent ?? 0)
        let advance = (" " as NSString).size(withAttributes: [.font: font]).width
        let available = max(1, rect.width - indent)
        var contentEnd = text.length
        while contentEnd > start && (text.character(at: contentEnd - 1) == 10 || text.character(at: contentEnd - 1) == 13) { contentEnd -= 1 }
        // Most editor paragraphs are short ASCII lines. Avoid shaping them twice.
        if paragraph.ascii && Double(contentEnd - start) * advance <= available { return rect }
        let typesetter = paragraph.typesetter
        let fit: Int
        if paragraph.ascii {
            fit = min(text.length - start, max(1, Int(available / max(1, advance))))
        } else {
            fit = max(1, CTTypesetterSuggestClusterBreak(typesetter!, start, available))
        }
        guard start + fit < text.length else { return rect }
        var count = fit
        let firstContent = (start..<text.length).first { text.character(at: $0) != 32 && text.character(at: $0) != 9 } ?? text.length
        for end in stride(from: start + fit, through: start + 1, by: -1) {
            if end > firstContent && (Self.breakAfter.contains(text.character(at: end - 1)) || Self.breakBefore.contains(text.character(at: end))) {
                count = end - start
                break
            }
        }
        let width: Double
        if let typesetter {
            let line = CTTypesetterCreateLine(typesetter, CFRange(location: start, length: count))
            width = CTLineGetTypographicBounds(line, nil, nil, nil)
        } else { width = Double(count) * advance }
        rect.size.width = min(rect.width, ceil(width + indent) + 0.1)
        return rect
    }
}

@MainActor
enum SyntaxHighlighter {
    private static var expressions: [String: NSRegularExpression] = [:]

    private static func expression(_ pattern: String) -> NSRegularExpression? {
        if let expression = expressions[pattern] { return expression }
        guard let expression = try? NSRegularExpression(pattern: pattern) else { return nil }
        expressions[pattern] = expression
        return expression
    }
    static func attributedString(text: String, language: SyntaxLanguage, fontSize: Double = 12) -> NSAttributedString {
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = CodeEditorMetrics.lineSpacing(fontSize)
        paragraph.lineBreakMode = .byCharWrapping
        let result = NSMutableAttributedString(
            string: text,
            attributes: [
                .font: NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular),
                .foregroundColor: NSColor.labelColor,
                .paragraphStyle: paragraph,
            ]
        )

        let source = text as NSString
        let spaceWidth = (" " as NSString).size(withAttributes: [.font: NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)]).width
        var cursor = 0
        while cursor < source.length {
            let range = source.lineRange(for: NSRange(location: cursor, length: 0))
            var start = cursor, columns = 0
            while start < NSMaxRange(range) {
                if source.character(at: start) == 32 { columns += 1 }
                else if source.character(at: start) == 9 { columns += 4 - columns % 4 }
                else { break }
                start += 1
            }
            if columns > 0 {
                let indented = paragraph.mutableCopy() as! NSMutableParagraphStyle
                indented.headIndent = Double(columns) * spaceWidth
                indented.tabStops = []
                indented.defaultTabInterval = spaceWidth * 4
                result.addAttribute(.paragraphStyle, value: indented, range: range)
            }
            cursor = NSMaxRange(range)
        }

        switch language {
        case .json:
            apply(#"\b(true|false)\b"#, color: WireboltTheme.nsJSONBoolean, to: result)
            apply(#"\bnull\b"#, color: WireboltTheme.nsJSONNull, to: result)
            apply(#"-?\b\d+(?:\.\d+)?\b"#, color: WireboltTheme.nsJSONNumber, to: result)
            apply(#"\"(?:\\.|[^\"\\])*\""#, color: WireboltTheme.nsJSONString, to: result)
            apply(#"\"(?:\\.|[^\"\\])*\"(?=\s*:)"#, color: WireboltTheme.nsJSONKey, to: result)
            applyURL(to: result)
        case .xml, .html:
            apply(#"</?[A-Za-z][^>]*>"#, color: .systemTeal, to: result)
            apply(#"\"[^\"]*\""#, color: .systemOrange, to: result)
        case .http:
            apply(#"(?m)^[A-Z]+\s+\S+\s+HTTP/\d(?:\.\d)?$"#, color: .systemGreen, to: result)
            apply(#"(?m)^[A-Za-z0-9-]+(?=:)"#, color: .systemTeal, to: result)
            apply(#"•+"#, color: .systemOrange, to: result)
        case .plain:
            break
        }
        return result
    }

    private static func apply(
        _ pattern: String,
        color: NSColor,
        to text: NSMutableAttributedString
    ) {
        guard let expression = expression(pattern) else { return }
        let range = NSRange(location: 0, length: text.length)
        expression.enumerateMatches(in: text.string, range: range) { match, _, _ in
            guard let match else { return }
            text.addAttribute(.foregroundColor, value: color, range: match.range)
        }
    }

    private static func applyURL(to text: NSMutableAttributedString) {
        guard let expression = expression(#"https?://[^\"\s]+"#) else { return }
        let range = NSRange(location: 0, length: text.length)
        expression.enumerateMatches(in: text.string, range: range) { match, _, _ in
            guard let match else { return }
            text.addAttributes([
                .underlineStyle: NSUnderlineStyle.single.rawValue,
                .link: text.attributedSubstring(from: match.range).string,
            ], range: match.range)
        }
    }
}
