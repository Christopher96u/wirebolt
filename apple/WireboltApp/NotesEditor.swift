import AppKit
import SwiftUI

struct NotesEditor: View {
    @Binding var text: String
    @Binding var preview: Bool
    @State private var find = EditorFindState()

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Markdown").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Picker("Notes mode", selection: $preview) {
                    Text("Edit").tag(false)
                    Text("Preview").tag(true)
                }.pickerStyle(.segmented).frame(width: 150)
            }.frame(height: 24).padding(.horizontal, 12).padding(.vertical, 6)
            Divider()
            if preview {
                NotesPreview(source: text)
            } else {
                NativeCodeEditor(text: $text, language: .plain, label: "Note", find: find)
                    .editorFindOverlay(find)
            }
        }
    }
}

private struct NotesPreview: View {
    let source: String
    @State private var rendered: RenderedMarkdown?
    @State private var failed = false

    var body: some View {
        Group {
            if let rendered {
                if rendered.text.length == 0 {
                    Text("No notes yet. Switch to Edit to add Markdown.")
                        .foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
                } else { MarkdownTextView(rendered: rendered) }
            } else if failed {
                Text("Could not preview this note. Your text is available in Edit.")
                    .foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else { ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity) }
        }
        .task(id: source) {
            rendered = nil; failed = false
            do {
                let result = try await MarkdownPreviewCache.shared.render(source)
                try Task.checkCancellation()
                rendered = result
            } catch is CancellationError {
                // A hidden note or a previous document must never publish its result.
            } catch { if !Task.isCancelled { failed = true } }
        }
    }
}

struct MarkdownTextView: NSViewRepresentable {
    let rendered: RenderedMarkdown
    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.verticalScroller = CodeScroller()
        scroll.drawsBackground = false
        let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 500, height: 300))
        text.isEditable = false
        text.isSelectable = true
        text.isRichText = true
        text.drawsBackground = false
        text.usesFindBar = true
        text.textContainerInset = NSSize(width: 16, height: 12)
        text.minSize = .zero
        text.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        text.isVerticallyResizable = true
        text.isHorizontallyResizable = false
        text.autoresizingMask = [.width]
        text.textContainer?.widthTracksTextView = true
        text.textContainer?.containerSize = NSSize(width: 500, height: CGFloat.greatestFiniteMagnitude)
        text.layoutManager?.allowsNonContiguousLayout = true
        text.layoutManager?.backgroundLayoutEnabled = false
        text.delegate = context.coordinator
        text.setAccessibilityLabel("Markdown preview")
        scroll.documentView = text
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard context.coordinator.rendered !== rendered, let text = scroll.documentView as? NSTextView else { return }
        context.coordinator.rendered = rendered
        text.textStorage?.setAttributedString(rendered.text)
        text.scrollToBeginningOfDocument(nil)
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var rendered: RenderedMarkdown?
        func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
            guard let url = link as? URL, ["http", "https", "mailto"].contains(url.scheme?.lowercased() ?? "") else { return true }
            NSWorkspace.shared.open(url)
            return true
        }
    }
}
