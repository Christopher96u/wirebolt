import AppKit
import Observation
import SwiftUI

@MainActor
@Observable
final class EditorFindState {
    var isVisible = false
    var query = TextSearchQuery() {
        didSet { if query != oldValue { currentMatch = nil } }
    }
    var selectionOnly = false {
        didSet { if selectionOnly != oldValue { currentMatch = nil } }
    }
    var matchCount = 0
    var currentMatch: Int?
    var error: String?
    var selection: NSRange?
    var indexedSelection: (start: ResponseTextIndex.SearchPosition, end: ResponseTextIndex.SearchPosition)?
    var hasSelection: Bool { (selection?.length ?? 0) > 0 || indexedSelection != nil }

    var countLabel: String {
        guard matchCount > 0 else { return "No results" }
        let count = matchCount == TextSearchQuery.maximumMatches ? "19999+" : String(matchCount)
        return "\(currentMatch.map { String($0 + 1) } ?? "?") of \(count)"
    }

    func move(_ delta: Int) {
        guard matchCount > 0 else { return }
        currentMatch = currentMatch.map { ($0 + delta + matchCount) % matchCount } ?? (delta > 0 ? 0 : matchCount - 1)
    }

    func update(count: Int, error: String? = nil) {
        matchCount = count
        self.error = error
        if let currentMatch, currentMatch >= count { self.currentMatch = nil }
    }
}

private struct EditorFindBar: View {
    @Bindable var state: EditorFindState
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 3) {
            HStack(spacing: 2) {
                TextField("Find", text: $state.query.text)
                    .textFieldStyle(.plain)
                    .focused($focused)
                    .frame(minWidth: 30, maxWidth: .infinity)
                    .onSubmit { state.move(1) }
                    .onKeyPress(.return, phases: .down) { press in
                        guard press.modifiers.contains(.shift) else { return .ignored }
                        state.move(-1)
                        return .handled
                    }
                option("Match Case", text: "Aa", value: $state.query.matchCase)
                    .keyboardShortcut("c", modifiers: [.command, .option])
                option("Match Whole Word", text: "ab", value: $state.query.wholeWord, underline: true)
                    .keyboardShortcut("w", modifiers: [.command, .option])
                option("Use Regular Expression", text: ".*", value: $state.query.regularExpression)
                    .keyboardShortcut("r", modifiers: [.command, .option])
            }
            .padding(.leading, 5).padding(.trailing, 2)
            .frame(height: 24)
            .background(Color(nsColor: .textBackgroundColor))
            .overlay { Rectangle().stroke(state.error == nil ? (focused ? WireboltTheme.primaryAccent : Color(nsColor: .separatorColor)) : .red, lineWidth: 1) }
            .help(state.error ?? "Find")
            Text(state.countLabel).font(.system(size: 12)).lineLimit(1).frame(width: 69, alignment: .leading)
            Button("Previous Match", systemImage: "arrow.up") { state.move(-1) }.disabled(state.matchCount == 0)
            Button("Next Match", systemImage: "arrow.down") { state.move(1) }.disabled(state.matchCount == 0)
            Button("Find in Selection", systemImage: "line.3.horizontal.decrease") { state.selectionOnly.toggle() }
                .disabled(!state.hasSelection)
                .background(state.selectionOnly ? WireboltTheme.primaryAccent.opacity(0.25) : .clear)
                .keyboardShortcut("l", modifiers: [.command, .option])
                .accessibilityRepresentation { Toggle("Find in Selection", isOn: $state.selectionOnly).disabled(!state.hasSelection) }
            Button("Close (Escape)", systemImage: "xmark") { state.isVisible = false }
        }
        .font(.system(size: 13))
        .buttonStyle(FindIconButtonStyle())
        .labelStyle(.iconOnly)
        .padding(.leading, 20).padding(.trailing, 4)
        .frame(height: 34)
        .background(Color(nsColor: .windowBackgroundColor), in: .rect(cornerRadius: 3))
        .overlay { RoundedRectangle(cornerRadius: 3).stroke(Color(nsColor: .separatorColor), lineWidth: 0.5) }
        .shadow(color: .black.opacity(0.4), radius: 5, y: 2)
        .onExitCommand { state.isVisible = false }
        .task { await Task.yield(); focused = true }
    }

    private func option(_ label: String, text: String, value: Binding<Bool>, underline: Bool = false) -> some View {
        Button { value.wrappedValue.toggle() } label: {
            Text(text).font(.system(size: 11)).underline(underline)
                .frame(width: 20, height: 20)
                .background(value.wrappedValue ? WireboltTheme.primaryAccent.opacity(0.3) : .clear, in: .rect(cornerRadius: 2))
        }
        .buttonStyle(.plain)
        .help(label)
        .accessibilityRepresentation { Toggle(label, isOn: value) }
    }
}

private struct FindIconButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.frame(width: 22, height: 23)
            .contentShape(.rect)
            .background(configuration.isPressed ? Color.primary.opacity(0.12) : .clear)
    }
}

extension View {
    func editorFindOverlay(_ state: EditorFindState, isActive: Bool = true) -> some View {
        overlay(alignment: .topTrailing) {
            if state.isVisible && isActive {
                GeometryReader { geometry in
                    EditorFindBar(state: state)
                        .frame(width: min(419, max(250, geometry.size.width - 28)))
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
                        .padding(.trailing, 28)
                }
            }
        }
    }
}
