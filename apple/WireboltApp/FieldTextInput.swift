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

struct FieldTextInput: View {
    let placeholder: String
    @Binding var text: String
    let height: Double

    init(_ placeholder: String, text: Binding<String>, height: Double) {
        self.placeholder = placeholder
        _text = text
        self.height = height
    }

    var body: some View {
        TextField(placeholder, text: $text, axis: .vertical)
            .textFieldStyle(.plain)
            .font(.system(size: 11, design: .monospaced))
            .frame(height: max(14, height - 4), alignment: .top)
            .padding(.top, 1)
            .padding(.bottom, 3)
            .padding(.leading, 1)
    }
}

struct FieldCheckbox: View {
    @Binding var isOn: Bool

    var body: some View {
        NativeCheckbox(isOn: $isOn)
            .frame(width: 18, height: 18)
            .offset(x: 2)
            .frame(width: 28, height: 20)
    }

    private struct NativeCheckbox: NSViewRepresentable {
        @Binding var isOn: Bool
        @Environment(\.isEnabled) private var isEnabled

        func makeCoordinator() -> Coordinator { Coordinator(isOn: $isOn) }
        func makeNSView(context: Context) -> NSButton {
            let button = NSButton(checkboxWithTitle: "", target: context.coordinator, action: #selector(Coordinator.changed(_:)))
            button.controlSize = .regular
            button.imagePosition = .imageOnly
            button.setAccessibilityLabel("Enabled")
            return button
        }
        func updateNSView(_ button: NSButton, context: Context) {
            context.coordinator.isOn = $isOn
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
