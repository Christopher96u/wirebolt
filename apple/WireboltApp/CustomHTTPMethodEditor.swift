import SwiftUI

struct CustomHTTPMethodEditor: View {
    @Binding var method: HTTPMethod
    @State private var text: String
    @State private var problem: String?
    @Environment(\.dismiss) private var dismiss

    init(method: Binding<HTTPMethod>, text: String) {
        _method = method
        _text = State(initialValue: text)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: WireboltTheme.Spacing.large) {
            Text("Custom HTTP Method").font(.headline)
            Text("Use an HTTP token such as PROPFIND, REPORT or FOO_BAR.")
            TextField("METHOD", text: $text).textFieldStyle(.roundedBorder)
                .accessibilityLabel("Custom HTTP method")
            if let problem { Text(problem).foregroundStyle(.red).accessibilityLabel(problem) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Set", action: setMethod).keyboardShortcut(.defaultAction)
            }
        }
        .padding(WireboltTheme.Spacing.section).frame(width: 420)
    }

    private func setMethod() {
        guard let value = HTTPMethod(rawValue: text) else {
            problem = "Enter a valid HTTP method token without spaces or separators."
            return
        }
        method = value
        dismiss()
    }
}
