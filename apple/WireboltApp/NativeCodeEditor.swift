import AppKit
import CoreText
import SwiftUI

enum SyntaxLanguage: Hashable {
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
    static func textTopInset(_ fontSize: Double) -> Double { floor(lineSpacing(fontSize) / 2) }
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
    var storageKey: String?
    @State private var localFind = EditorFindState()
    @Environment(\.editorIsActive) private var isActive
    var body: some View {
        NativeCodeEditor(text: .constant(text), editable: false, language: language, label: "Response body", search: search, find: find ?? localFind, storageKey: storageKey)
            .frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
            .editorFindOverlay(find ?? localFind, isActive: isActive)
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

private struct EditorActiveKey: EnvironmentKey {
    static let defaultValue = true
}

@MainActor
func setEditorVisibility(_ scroll: NSScrollView, active: Bool) {
    guard scroll.isHidden != !active else { return }
    if !active, let responder = scroll.window?.firstResponder as? NSView, responder.isDescendant(of: scroll) {
        scroll.window?.makeFirstResponder(nil)
    }
    scroll.isHidden = !active
}

private struct EditorStorageKey: EnvironmentKey {
    static let defaultValue: EditorPresentationStorage? = nil
}
extension EnvironmentValues {
    var editorIsActive: Bool {
        get { self[EditorActiveKey.self] }
        set { self[EditorActiveKey.self] = newValue }
    }
    var editorStorage: EditorPresentationStorage? {
        get { self[EditorStorageKey.self] }
        set { self[EditorStorageKey.self] = newValue }
    }
}

struct NativeCodeEditor: NSViewRepresentable {
    @Binding var text: String
    @Environment(\.editorStorage) private var editorStorage
    @Environment(\.editorIsActive) private var isActive
    var editable = true
    var language: SyntaxLanguage = .plain
    var label = "Request body"
    var search = ""
    var find: EditorFindState?
    var storageKey: String?
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
        view.findAction = { [weak coordinator = context.coordinator, weak view] action in
            guard let state = coordinator?.findState else { return false }
            switch action {
            case 1: state.isVisible = true
            case 2: state.move(1)
            case 3: state.move(-1)
            case 7:
                guard let view, view.selectedRange().length > 0 else { return false }
                state.query.text = (view.string as NSString).substring(with: view.selectedRange())
            case 11: state.isVisible = false
            default: return false
            }
            return true
        }
        view.isIncrementalSearchingEnabled = true
        view.isVerticallyResizable = true
        view.minSize = .zero
        view.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        view.textContainerInset = NSSize(width: 4, height: 3)
        view.textContainer?.lineFragmentPadding = 0
        view.delegate = context.coordinator
        view.prepareVisibleText = { [weak coordinator = context.coordinator, weak view] in
            if let view { coordinator?.highlightViewport(in: view) }
        }
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
        setEditorVisibility(scroll, active: isActive)
        context.coordinator.text = $text
        context.coordinator.findState = isActive ? find : nil
        (view as? FindableCodeTextView)?.findState = isActive ? find : nil
        guard isActive else { return }
        let configuration = Configuration(fontSize: fontSize, wraps: wrapsLines, invisibles: showInvisibles,
            scrollBeyond: scrollBeyond, editable: editable, language: language, appearance: view.effectiveAppearance.name)
        if context.coordinator.configuration == configuration,
           context.coordinator.projection.source == text, !context.coordinator.needsHighlight,
           context.coordinator.search == search {
            if let find, isActive { context.coordinator.refreshFind(in: scroll, state: find) }
            return
        }
        context.coordinator.configuration = configuration
        view.isEditable = editable
        view.textContainerInset = NSSize(width: 4, height: CodeEditorMetrics.textTopInset(fontSize))
        view.setAccessibilityLabel(label)
        let coordinator = context.coordinator
        if coordinator.presentation == nil {
            coordinator.presentation = editorStorage?.state(for: storageKey ?? label)
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
        if let find, isActive { coordinator.refreshFind(in: scroll, state: find) }
        if !coordinator.restored, let presentation = coordinator.presentation {
            coordinator.restored = true
            DispatchQueue.main.async { [weak scroll, weak view] in
                guard let scroll, let view else { return }
                coordinator.updating = true
                if NSMaxRange(presentation.selection) <= (view.string as NSString).length { view.setSelectedRange(presentation.selection) }
                (scroll.contentView as? EditorClipView)?.restorePresentationOrigin(presentation.origin)
                scroll.reflectScrolledClipView(scroll.contentView)
                coordinator.updating = false
            }
        }
    }

    struct Configuration: Equatable {
        let fontSize: Double
        let wraps: Bool
        let invisibles: Bool
        let scrollBeyond: Bool
        let editable: Bool
        let language: SyntaxLanguage
        let appearance: NSAppearance.Name
    }

    @MainActor
    final class Coordinator: NSObject, NSTextViewDelegate, @preconcurrency NSLayoutManagerDelegate {
        var text: Binding<String>
        var configuration: Configuration?
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
        private var highlightsViewport = false
        private var highlightedRange = NSRange(location: NSNotFound, length: 0)
        private var edit: (range: NSRange, replacement: String, removed: String)?
        private var lineIndex = TextLineIndex()
        private var foldTask: Task<Void, Never>?
        private var foldRevision = 0
        init(text: Binding<String>) { self.text = text }

        weak var findState: EditorFindState?
        private var findQuery = TextSearchQuery()
        private var findSource = ""
        private var findScope: NSRange?
        private var findRanges: [NSRange] = []
        private var findIndex: Int?
        private var findTask: Task<Void, Never>?
        deinit { findTask?.cancel(); foldTask?.cancel() }

        func refreshFind(in scroll: NSScrollView, state: EditorFindState) {
            guard let view = scroll.documentView as? NSTextView else { return }
            let query = state.isVisible ? state.query : TextSearchQuery()
            let scope = state.selectionOnly ? state.selection : nil
            if query.text.isEmpty && findQuery.text.isEmpty {
                findSource = projection.text
                findScope = scope
                return
            }
            if !query.text.isEmpty, !collapsed.isEmpty {
                collapsed.removeAll()
                presentation?.collapsed = []
                render(in: scroll)
            }
            if query != findQuery || scope != findScope || projection.text != findSource {
                findQuery = query
                findScope = scope
                findSource = projection.text
                findIndex = nil
                findTask?.cancel()
                findRanges = []
                let whole = NSRange(location: 0, length: (projection.text as NSString).length)
                view.layoutManager?.removeTemporaryAttribute(.backgroundColor, forCharacterRange: whole)
                let source = projection.text
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
            foldTask?.cancel()
            foldTask = nil
            foldRevision += 1
            lineIndex = TextLineIndex(text.wrappedValue)
            projection = CodeProjection(source: text.wrappedValue, collapsed: collapsed, syntax: language.folding)
            highlightsViewport = projection.text.utf8.count > 64 * 1024 && (language == .json || language == .http)
            highlightedRange = NSRange(location: NSNotFound, length: 0)
            view.textStorage?.setAttributedString(SyntaxHighlighter.attributedString(text: projection.text,
                language: highlightsViewport ? .plain : language, fontSize: fontSize))
            view.setSelectedRange(NSRange(location: min(projection.displayOffset(sourceSelection), (projection.text as NSString).length), length: 0))
            needsHighlight = false
            (scroll.verticalRulerView as? CodeLineRuler)?.updateProjection(projection, folding: language.folding != .none, index: lineIndex)
            updating = false
        }

        func highlightViewport(in view: NSTextView) {
            guard highlightsViewport, !updating, let storage = view.textStorage,
                  let manager = view.layoutManager, let container = view.textContainer,
                  view.visibleRect.width > 0, view.visibleRect.height > 0 else { return }
            let viewport = view.visibleRect.offsetBy(dx: -view.textContainerOrigin.x, dy: -view.textContainerOrigin.y)
            let glyphs = manager.glyphRange(forBoundingRect: viewport, in: container)
            let characters = manager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
            let source = projection.text as NSString
            guard NSMaxRange(characters) <= source.length else { return }
            let range = source.lineRange(for: characters)
            guard range.length > 0, range != highlightedRange else { return }
            highlightedRange = range
            updating = true
            defer { updating = false }
            let styled = SyntaxHighlighter.attributedString(text: source.substring(with: range), language: language, fontSize: fontSize)
            storage.beginEditing()
            styled.enumerateAttributes(in: NSRange(location: 0, length: styled.length)) { attributes, local, _ in
                storage.setAttributes(attributes, range: NSRange(location: range.location + local.location, length: local.length))
            }
            storage.endEditing()
        }

        func textView(_ textView: NSTextView, shouldChangeTextIn affectedCharRange: NSRange, replacementString: String?) -> Bool {
            edit = replacementString.map { (affectedCharRange, $0, (textView.string as NSString).substring(with: affectedCharRange)) }
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
            highlightedRange = NSRange(location: NSNotFound, length: 0)
            // Apply the same UTF-16 edit to our immutable native snapshot. Bridging
            // the entire mutable TextKit string per keystroke is linear in body size.
            let source: String
            if let edit, collapsed.isEmpty, let range = Range(edit.range, in: projection.source) {
                var updated = projection.source
                updated.replaceSubrange(range, with: edit.replacement)
                source = updated
            } else {
                var snapshot = view.string
                snapshot.makeContiguousUTF8()
                source = snapshot
            }
            // JSON tokens cannot cross an unescaped newline. Recolor only the edited
            // paragraphs, preserving TextKit's layout and the user's selection elsewhere.
            // Other grammars keep a full recolor because a tag can span several lines.
            if let edit, let storage = view.textStorage, let scroll = view.enclosingScrollView,
               language == .json || language == .plain || language == .http {
                updating = true
                let string = source as NSString
                let start = min(edit.range.location, string.length)
                let range = string.lineRange(for: NSRange(location: start, length: min((edit.replacement as NSString).length, string.length - start)))
                let styled = SyntaxHighlighter.attributedString(text: string.substring(with: range), language: language, fontSize: fontSize)
                storage.beginEditing()
                storage.setAttributes([:], range: range)
                styled.enumerateAttributes(in: NSRange(location: 0, length: styled.length)) { attributes, local, _ in
                    storage.setAttributes(attributes, range: NSRange(location: range.location + local.location, length: local.length))
                }
                storage.endEditing()
                lineIndex.replace(edit.range, with: edit.replacement)
                if language != .json {
                    projection = CodeProjection(unfolded: source)
                } else if foldTask == nil, let updated = projection.updatingUnfoldedSource(source,
                    range: edit.range, replacement: edit.replacement, removed: edit.removed) {
                    projection = updated
                } else {
                    projection = CodeProjection(unfolded: source)
                    scheduleFolds(in: scroll)
                }
                (scroll.verticalRulerView as? CodeLineRuler)?.updateProjection(projection, folding: language == .json, index: lineIndex)
                updating = false
                needsHighlight = false
            } else { needsHighlight = true }
            edit = nil
            text.wrappedValue = source
        }
        private func scheduleFolds(in scroll: NSScrollView) {
            foldTask?.cancel()
            foldRevision += 1
            let revision = foldRevision
            let source = projection.source
            let syntax = language.folding
            foldTask = Task { [weak self, weak scroll] in
                let worker = Task.detached(priority: .userInitiated) { CodeProjection(source: source, syntax: syntax) }
                let result = await withTaskCancellationHandler { await worker.value } onCancel: { worker.cancel() }
                guard !Task.isCancelled, let self, let scroll, self.foldRevision == revision else { return }
                self.projection = result
                self.foldTask = nil
                (scroll.verticalRulerView as? CodeLineRuler)?.updateProjection(result, folding: true, index: self.lineIndex)
            }
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
    var prepareVisibleText: (() -> Void)?
    weak var findState: EditorFindState?

    override func viewWillDraw() {
        prepareVisibleText?()
        super.viewWillDraw()
    }

    override func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        if let state = findState, item.action == #selector(performFindPanelAction(_:)) || item.action == #selector(performTextFinderAction(_:)) {
            switch item.tag {
            case 1: return true
            case 2, 3: return state.matchCount > 0
            case 7: return selectedRange().length > 0
            case 11: return state.isVisible
            default: break
            }
        }
        return super.validateUserInterfaceItem(item)
    }
    override func performFindPanelAction(_ sender: Any?) {
        let action = (sender as? NSMenuItem)?.tag ?? 1
        if findAction?(action) != true { super.performFindPanelAction(sender) }
    }
    override func performTextFinderAction(_ sender: Any?) {
        let action = (sender as? NSMenuItem)?.tag ?? 1
        if findAction?(action) != true { super.performTextFinderAction(sender) }
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
        for index in chars.location ..< NSMaxRange(chars) where string.character(at: index) == 32 {
            let glyph = glyphIndexForCharacter(at: index)
            let bounds = boundingRect(forGlyphRange: NSRange(location: glyph, length: 1), in: container)
            let font = storage.attribute(.font, at: index, effectiveRange: nil) as? NSFont
                ?? .monospacedSystemFont(ofSize: 12, weight: .regular)
            let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.tertiaryLabelColor]
            let marker = "·" as NSString
            let width = marker.size(withAttributes: attributes).width
            let line = lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
            marker.draw(at: NSPoint(x: origin.x + bounds.midX - width / 2, y: origin.y + line.minY), withAttributes: attributes)
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
        guard !isHidden, !updatingDocumentSize, let text = documentView as? NSTextView,
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

    private var leadingOrigin: CGPoint {
        var target = bounds
        target.origin = CGPoint(x: documentRect.minX - bounds.width, y: documentRect.minY - bounds.height)
        return constrainBoundsRect(target).origin
    }

    func restorePresentationOrigin(_ origin: CGPoint) {
        let leading = leadingOrigin
        let target = NSRect(origin: CGPoint(x: leading.x + origin.x, y: leading.y + origin.y), size: bounds.size)
        scroll(to: constrainBoundsRect(target).origin)
    }

    override func scroll(to newOrigin: NSPoint) {
        super.scroll(to: newOrigin)
        // Rulers and AppKit content insets can make the resting origin negative.
        // Persist distance from that edge so native and indexed editors agree.
        let leading = leadingOrigin
        changed?(CGPoint(x: max(0, bounds.origin.x - leading.x), y: max(0, bounds.origin.y - leading.y)))
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

    func updateProjection(_ projection: CodeProjection, folding: Bool, index: TextLineIndex) {
        let starts = index.starts
        let foldLines = Set(folding ? projection.folds.map(\.line) : [])
        var line = 0
        let displayStarts = projection.collapsed.isEmpty ? starts : TextLineIndex(projection.text).starts
        lines = displayStarts.map { offset in
            let original = projection.collapsed.isEmpty ? offset : projection.sourceOffset(offset)
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

    override func draw(_ dirtyRect: NSRect) {
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        let gutter = NSRect(x: bounds.minX, y: bounds.minY, width: ruleThickness, height: bounds.height)
        NSBezierPath(rect: gutter).addClip()
        (scrollView?.backgroundColor ?? .textBackgroundColor).setFill()
        dirtyRect.intersection(gutter).fill()
        drawHashMarksAndLabels(in: dirtyRect)
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
    private var paragraph: (range: NSRange, lineStart: Int, text: NSString, font: NSFont, ascii: Bool, typesetter: CTTypesetter?)?

    func invalidateParagraph() { paragraph = nil }

    override var isSimpleRectangularTextContainer: Bool { true }

    override func lineFragmentRect(forProposedRect proposed: NSRect, at index: Int,
        writingDirection: NSWritingDirection, remaining: UnsafeMutablePointer<NSRect>?) -> NSRect {
        var rect = super.lineFragmentRect(forProposedRect: proposed, at: index,
            writingDirection: writingDirection, remaining: remaining)
        guard let storage = layoutManager?.textStorage, index < storage.length else { return rect }
        let font = storage.attribute(.font, at: index, effectiveRange: nil) as? NSFont
            ?? .monospacedSystemFont(ofSize: 12, weight: .regular)
        let advance = (" " as NSString).size(withAttributes: [.font: font]).width
        // Shape enough context for several visual lines, never the entire minified document.
        let contextLength = max(2048, Int(min(32768, rect.width / max(1, advance) * 8)))
        if paragraph == nil || !NSLocationInRange(index, paragraph!.range) || paragraph!.font != font
            || (NSMaxRange(paragraph!.range) < storage.length && NSMaxRange(paragraph!.range) - index < contextLength / 2) {
            let source = storage.string as NSString
            let logical = source.lineRange(for: NSRange(location: index, length: 0))
            let end = min(NSMaxRange(logical), index + contextLength)
            let range = source.rangeOfComposedCharacterSequences(for: NSRange(location: index, length: end - index))
            let text = source.substring(with: range) as NSString
            let ascii = (0..<text.length).allSatisfy { (32...126).contains(text.character(at: $0)) || text.character(at: $0) == 10 || text.character(at: $0) == 13 }
            let typesetter = ascii ? nil : CTTypesetterCreateWithAttributedStringAndOptions(NSAttributedString(string: text as String,
                attributes: [.font: font]) as CFAttributedString, [kCTTypesetterOptionAllowUnboundedLayout: true] as CFDictionary)
            paragraph = (range, logical.location, text, font, ascii, typesetter)
        }
        guard let paragraph else { return rect }
        let text = paragraph.text
        let start = index - paragraph.range.location
        let style = storage.attribute(.paragraphStyle, at: index, effectiveRange: nil) as? NSParagraphStyle
        let indent = index == paragraph.lineStart ? (style?.firstLineHeadIndent ?? 0) : (style?.headIndent ?? 0)
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
            guard let typesetter else { return rect }
            fit = max(1, CTTypesetterSuggestClusterBreak(typesetter, start, available))
        }
        guard start + fit < text.length else { return rect }
        let count = CodeTextWrapping.breakLength(in: text, start: start, fitting: fit)
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
        // Applying successive token passes to one large attributed string inserts
        // into its run array repeatedly (quadratic memmoves). These grammars have
        // no multiline tokens, so style bounded, whole-paragraph chunks and append.
        guard language == .json || language == .plain || language == .http,
              text.utf8.count > 16 * 1024 else {
            return attributedChunk(text: text, language: language, fontSize: fontSize)
        }
        let source = text as NSString
        let result = NSMutableAttributedString(string: "")
        var offset = 0
        while offset < source.length {
            let end = NSMaxRange(source.lineRange(for: NSRange(
                location: offset, length: min(16 * 1024, source.length - offset)
            )))
            result.append(attributedChunk(text: source.substring(with: NSRange(location: offset, length: end - offset)),
                language: language, fontSize: fontSize))
            offset = end
        }
        return result
    }

    private static func attributedChunk(text: String, language: SyntaxLanguage, fontSize: Double) -> NSAttributedString {
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
        var paragraphStyles: [Int: NSParagraphStyle] = [:]
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
                let indented: NSParagraphStyle
                if let cached = paragraphStyles[columns] { indented = cached }
                else {
                    let style = paragraph.mutableCopy() as! NSMutableParagraphStyle
                    style.headIndent = Double(columns) * spaceWidth
                    style.tabStops = []
                    style.defaultTabInterval = spaceWidth * 4
                    indented = style
                    paragraphStyles[columns] = style
                }
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
