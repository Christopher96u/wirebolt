import AppKit
import SwiftUI

@MainActor
enum FieldEditorMetrics {
    static func height(for text: String, width: Double) -> Double {
        let cell = NSTextFieldCell(textCell: text.isEmpty ? " " : text)
        let font = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
        cell.font = font
        cell.wraps = true
        cell.isScrollable = false
        cell.usesSingleLineMode = false
        // Reserve the inline action while measuring, even when it is hidden.
        // Below one glyph, NSTextFieldCell collapses its measured height to a
        // single line. Keep measuring wrapped rows when the value is clipped.
        let minimumWidth = (" " as NSString).size(withAttributes: [.font: font]).width.rounded(.up)
        let bounds = NSRect(x: 0, y: 0, width: max(minimumWidth, width - 24), height: .greatestFiniteMagnitude)
        return max(20, cell.cellSize(forBounds: bounds).height.rounded(.up) + 6)
    }

    static func height(key: String, value: String, valueWidth: Double) -> Double {
        max(height(for: key, width: 175), height(for: value, width: valueWidth))
    }
}

extension EnvironmentValues {
    /// Variables of the active environments for `{{name}}` highlighting and completion.
    /// Nil (the default) turns both off, for example inside the environment editor.
    @Entry var variableCatalog: VariableCatalog? = nil
}

/// Key and value cells. Values containing `{{name}}` show defined variables in the accent
/// color and undefined ones in red while not editing, list their values in the help tag,
/// and typing `{{` offers the active variables (↑↓ to choose, Return or Tab to insert,
/// Esc to dismiss).
struct FieldTextInput: View {
    let placeholder: String
    @Binding var text: String
    let height: Double
    let accessibilityLabel: String?

    @Environment(\.variableCatalog) private var variables
    @FocusState private var isFocused: Bool
    @State private var selection: TextSelection?
    @State private var completionIndex = 0
    /// The text for which Esc dismissed completion; typing shows it again.
    @State private var dismissedText: String?

    init(_ placeholder: String, text: Binding<String>, height: Double, accessibilityLabel: String? = nil) {
        self.placeholder = placeholder
        _text = text
        self.height = height
        self.accessibilityLabel = accessibilityLabel
    }

    var body: some View {
        let highlights = variables != nil && !isFocused && text.contains("{{")
        let completions = completionEntries
        TextField(placeholder, text: $text, selection: $selection, axis: .vertical)
            .textFieldStyle(.plain)
            .font(.system(size: 11, design: .monospaced))
            // Clear (not zero opacity) so the field still takes clicks under the overlay.
            .foregroundStyle(highlights ? AnyShapeStyle(Color.clear) : AnyShapeStyle(HierarchicalShapeStyle.primary))
            .focused($isFocused)
            .overlay(alignment: .topLeading) {
                if highlights, let variables {
                    Text(VariableHighlighting.attributed(text, catalog: variables))
                        .font(.system(size: 11, design: .monospaced))
                        .frame(maxWidth: .infinity, alignment: .topLeading)
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
            }
            .help(variables.flatMap { text.contains("{{") ? $0.summary(for: text) : nil } ?? "")
            .accessibilityLabel(accessibilityLabel.map { Text($0) } ?? Text(placeholder))
            .onKeyPress(keys: [.downArrow, .upArrow, .return, .tab, .escape]) { press in
                guard !completions.isEmpty else { return .ignored }
                switch press.key {
                case .downArrow: completionIndex = (completionIndex + 1) % completions.count
                case .upArrow: completionIndex = (completionIndex + completions.count - 1) % completions.count
                case .escape: dismissedText = text
                default: insert(completions[min(completionIndex, completions.count - 1)])
                }
                return .handled
            }
            .onChange(of: text) { completionIndex = 0 }
            .popover(isPresented: Binding(
                get: { !completions.isEmpty },
                set: { if !$0 { dismissedText = text } }
            ), arrowEdge: .bottom) {
                VariableCompletionList(entries: completions, selectedIndex: completionIndex) { insert($0) }
            }
            .frame(height: max(14, height - 4), alignment: .top)
            .padding(.top, 1)
            .padding(.bottom, 3)
            .padding(.leading, 1)
    }

    private var openReference: OpenVariableReference? {
        guard variables != nil, isFocused, dismissedText != text, text.contains("{{"),
              case let .selection(range)? = selection?.indices, range.isEmpty,
              // The selection can briefly refer to the previous text while typing.
              range.lowerBound <= text.endIndex, let caret = range.lowerBound.samePosition(in: text.utf16)
        else { return nil }
        return VariableTemplate.openReference(in: text, caret: text.utf16.distance(from: text.utf16.startIndex, to: caret))
    }

    private var completionEntries: [VariableCatalog.Entry] {
        guard let variables, let openReference else { return [] }
        return Array(variables.completions(matching: openReference.prefix).prefix(VariableCompletionList.limit))
    }

    private func insert(_ entry: VariableCatalog.Entry) {
        guard let openReference else { return }
        let (updated, caret) = VariableHighlighting.completing(text, reference: openReference, with: entry.name)
        text = updated
        selection = TextSelection(insertionPoint: String.Index(utf16Offset: caret, in: updated))
        completionIndex = 0
    }
}

/// Highlighting and completion edits shared by the URL field and key/value cells.
@MainActor
enum VariableHighlighting {
    static let resolvedColor = NSColor(WireboltTheme.primaryAccent)
    static let unresolvedColor = NSColor(WireboltTheme.danger)

    static func attributed(_ text: String, catalog: VariableCatalog) -> AttributedString {
        var attributed = AttributedString(text)
        for reference in VariableTemplate.references(in: text) {
            guard let stringRange = Range(reference.range, in: text),
                  let range = Range(stringRange, in: attributed) else { continue }
            attributed[range].foregroundColor = catalog.entry(named: reference.name) == nil ? WireboltTheme.danger : WireboltTheme.primaryAccent
        }
        return attributed
    }

    /// Colors references in place; used on the field editor while the URL is edited.
    static func apply(to storage: NSTextStorage, catalog: VariableCatalog, baseColor: NSColor) {
        let text = storage.string
        let full = NSRange(location: 0, length: storage.length)
        storage.beginEditing()
        storage.addAttribute(.foregroundColor, value: baseColor, range: full)
        for reference in VariableTemplate.references(in: text) where NSMaxRange(reference.range) <= storage.length {
            storage.addAttribute(.foregroundColor, value: color(for: reference, catalog: catalog), range: reference.range)
        }
        storage.endEditing()
    }

    static func attributedString(_ text: String, font: NSFont, catalog: VariableCatalog) -> NSAttributedString {
        let result = NSMutableAttributedString(string: text, attributes: [.font: font, .foregroundColor: NSColor.labelColor])
        for reference in VariableTemplate.references(in: text) {
            result.addAttribute(.foregroundColor, value: color(for: reference, catalog: catalog), range: reference.range)
        }
        return result
    }

    /// Replaces the partial name with `name}}` (or `name` when `}}` already follows) and
    /// returns the new text with the UTF-16 caret after the closing braces.
    nonisolated static func completing(_ text: String, reference: OpenVariableReference, with name: String) -> (String, Int) {
        let source = text as NSString
        let updated = source.replacingCharacters(in: reference.replacementRange, with: reference.isClosed ? name : name + "}}")
        return (updated, reference.replacementRange.location + (name as NSString).length + 2)
    }

    private static func color(for reference: VariableReference, catalog: VariableCatalog) -> NSColor {
        catalog.entry(named: reference.name) == nil ? unresolvedColor : resolvedColor
    }
}

/// Completion rows: variable name, its display value (secrets masked) and environment.
struct VariableCompletionList: View {
    static let limit = 12
    let entries: [VariableCatalog.Entry]
    let selectedIndex: Int
    let onSelect: (VariableCatalog.Entry) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(entries.enumerated()), id: \.element.name) { index, entry in
                Button { onSelect(entry) } label: {
                    HStack(spacing: WireboltTheme.Spacing.medium) {
                        Text(entry.name).font(.system(size: 12, design: .monospaced))
                        Spacer(minLength: WireboltTheme.Spacing.large)
                        Text(entry.displayValue).lineLimit(1).truncationMode(.middle)
                            .foregroundStyle(index == selectedIndex ? Color.white.opacity(0.85) : .secondary)
                            .frame(maxWidth: 180, alignment: .trailing)
                        Text(entry.environmentName).font(.system(size: 10))
                            .foregroundStyle(index == selectedIndex ? Color.white.opacity(0.85) : Color.secondary)
                    }
                    .font(.system(size: 11))
                    .foregroundStyle(index == selectedIndex ? Color.white : Color.primary)
                    .padding(.horizontal, WireboltTheme.Spacing.medium)
                    .frame(maxWidth: .infinity, minHeight: 22, alignment: .leading)
                    .background(index == selectedIndex ? Color.accentColor : .clear, in: .rect(cornerRadius: WireboltTheme.Radius.small))
                    .contentShape(.rect)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(entry.name), \(entry.displayValue), \(entry.environmentName)")
                .accessibilityAddTraits(index == selectedIndex ? .isSelected : [])
            }
        }
        .padding(WireboltTheme.Spacing.xSmall)
        .frame(width: 360)
    }
}

struct FieldCheckbox: View {
    @Binding var isOn: Bool
    /// Names the row for VoiceOver, for example "Enable header Accept".
    var label = "Enabled"

    var body: some View {
        NativeCheckbox(isOn: $isOn, label: label)
            .frame(width: 18, height: 18)
            .offset(x: 2)
            .frame(width: 28, height: 20)
    }

    private struct NativeCheckbox: NSViewRepresentable {
        @Binding var isOn: Bool
        let label: String
        @Environment(\.isEnabled) private var isEnabled

        func makeCoordinator() -> Coordinator { Coordinator(isOn: $isOn) }
        func makeNSView(context: Context) -> NSButton {
            let button = NSButton(checkboxWithTitle: "", target: context.coordinator, action: #selector(Coordinator.changed(_:)))
            button.controlSize = .regular
            button.imagePosition = .imageOnly
            button.setAccessibilityLabel(label)
            return button
        }
        func updateNSView(_ button: NSButton, context: Context) {
            context.coordinator.isOn = $isOn
            if button.accessibilityLabel() != label { button.setAccessibilityLabel(label) }
            button.state = isOn ? .on : .off
            button.isEnabled = isEnabled
            button.contentTintColor = NSColor(WireboltTheme.primaryAccent)
        }

        @MainActor final class Coordinator: NSObject {
            var isOn: Binding<Bool>
            init(isOn: Binding<Bool>) { self.isOn = isOn }
            @objc func changed(_ sender: NSButton) { isOn.wrappedValue = sender.state == .on }
        }
    }
}
