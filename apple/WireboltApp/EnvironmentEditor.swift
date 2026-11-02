import AppKit
import SwiftUI

/// Native field buffers survive failed commits. Rust owns validation, mutations,
/// secret storage and undo; this object only serializes end-editing callbacks.
@MainActor @Observable
final class EnvironmentInspectorEditing {
    enum Field: Hashable {
        case name(String)
        case key(String, String)
        case value(String, String)

        var environmentID: String {
            switch self {
            case .name(let id), .key(let id, _), .value(let id, _): id
            }
        }
        func commit(_ text: String) -> EnvironmentCommit {
            switch self {
            case .name: .rename(name: text)
            case .key(_, let id): .variableName(id: id, name: text)
            case .value(_, let id): .variableValue(id: id, value: text)
            }
        }
    }
    var drafts: [Field: String] = [:]
    var problems: [Field: String] = [:]
    var removedDrafts: [Field: String] = [:]
    var commandProblem: String?
    private var work: Task<Bool, Never>?
    private var pendingCommits = 0
    let model: WireboltModel
    var hasEdits: Bool { !drafts.isEmpty || !removedDrafts.isEmpty || pendingCommits > 0 }

    init(model: WireboltModel) { self.model = model }

    func commit(_ field: Field) {
        guard let text = drafts[field] else { return }
        let previous = work
        pendingCommits += 1
        work = Task { @MainActor in
            _ = await previous?.value
            defer { pendingCommits -= 1 }
            reconcile()
            guard drafts[field] != nil else { return removedDrafts[field] == nil }
            let problem = await model.commitEnvironment(id: field.environmentID, commit: field.commit(text))
            problems[field] = problem
            if problem == nil, drafts[field] == text { drafts.removeValue(forKey: field) }
            return problem == nil
        }
    }

    func perform(id: String, commit action: EnvironmentCommit, completion: @escaping () -> Void) {
        for field in Array(drafts.keys) { commit(field) }
        let previous = work
        pendingCommits += 1
        work = Task { @MainActor in
            _ = await previous?.value
            defer { pendingCommits -= 1 }
            guard drafts.isEmpty else { return false }
            commandProblem = await model.commitEnvironment(id: id, commit: action)
            if commandProblem == nil { completion() }
            return commandProblem == nil
        }
    }

    func reconcile() {
        let environments = workspaceEnvironmentInspector(environments: model.workspace.environments.map(\.domain))
        for field in Array(drafts.keys) {
            let environment = environments.first { $0.id == field.environmentID }
            let exists: Bool
            switch field {
            case .name: exists = environment != nil
            case .key(_, let id), .value(_, let id): exists = environment?.variables.contains { $0.id == id } == true
            }
            if !exists {
                removedDrafts[field] = drafts.removeValue(forKey: field)
                problems.removeValue(forKey: field)
            }
        }
    }

    func discardRemovedDrafts() { removedDrafts.removeAll() }

    func flush() async -> Bool {
        reconcile()
        _ = await work?.value
        for field in Array(drafts.keys) {
            commit(field)
            _ = await work?.value
        }
        reconcile()
        return drafts.isEmpty && removedDrafts.isEmpty
    }
}

struct EnvironmentEditor: View {
    @Bindable var model: WireboltModel
    let interface: WorkspaceUIState
    @Environment(\.wireboltTheme) private var theme
    @ScaledMetric(relativeTo: .body) private var fieldScale = 1.0
    let closeTrigger: Int
    let onClose: () -> Void
    @State private var editing: EnvironmentInspectorEditing
    @State private var inspector: AppEnvironmentInspector
    @State private var selectedID: String
    @State private var editingNameID: String?
    @State private var revealedSecretIDs: Set<String> = []

    init(model: WireboltModel, interface: WorkspaceUIState, closeTrigger: Int, onClose: @escaping () -> Void) {
        self.model = model
        self.interface = interface
        self.closeTrigger = closeTrigger
        self.onClose = onClose
        _editing = State(initialValue: EnvironmentInspectorEditing(model: model))
        _inspector = State(initialValue: AppEnvironmentInspector(active: model.selectedEnvironmentID))
        _selectedID = State(initialValue: model.selectedEnvironmentID ?? WorkspaceDraft.globalEnvironmentID)
    }

    private var environments: [EnvironmentDraft] {
        workspaceEnvironmentInspector(environments: model.workspace.environments.map(\.domain)).map(EnvironmentDraft.init(domain:))
    }
    private var selected: EnvironmentDraft? { environments.first { $0.id == selectedID } }

    var body: some View {
        VStack(alignment: .leading, spacing: WireboltTheme.Spacing.large) {
            HStack {
                Text("Environments").font(WireboltTheme.Typography.body.weight(.semibold))
                Spacer()
                Button("Close Inspector", systemImage: "xmark") { Task { _ = await close() } }
                    .labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
            }
            ScrollView {
                VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xLarge) {
                    environmentList
                    Button(selectedID == WorkspaceDraft.globalEnvironmentID ? "Use Globals Only" : "Use for Requests") {
                        Task { if await editing.flush() { model.selectedEnvironmentID = inspector.useForRequests() } }
                    }
                    .disabled(selectedID == activeID)
                    removedEdits
                    variablesCard
                    if let problem = editing.commandProblem { inlineProblem(problem) }
                    Label("Secrets stay in this Mac’s Keychain.", systemImage: "key")
                        .font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
                    Text("Changes save when you finish editing. Undo with ⌘Z.")
                        .font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
                }
            }
        }
        .font(WireboltTheme.Typography.tab)
        .padding(WireboltTheme.Spacing.xLarge)
        .background(theme.definition.overlay.color)
        .onChange(of: closeTrigger) { Task { _ = await close() } }
        .onChange(of: model.workspace.environments) {
            editing.reconcile()
            if !environments.contains(where: { $0.id == selectedID }) {
                selectedID = inspector.browse(id: environments.contains(where: { $0.id == activeID }) ? activeID : WorkspaceDraft.globalEnvironmentID)
                editingNameID = nil
            }
        }
        .onAppear {
            interface.closeEnvironmentInspector = close
            model.finishEnvironmentEditing = { await editing.flush() }
        }
        .onChange(of: editing.hasEdits, initial: true) { interface.hasEnvironmentEdits = editing.hasEdits }
        .onDisappear { interface.hasEnvironmentEdits = false; interface.closeEnvironmentInspector = nil; model.finishEnvironmentEditing = nil }
        .focusedValue(\.environmentAddRow, addRow)
        .onExitCommand { Task { _ = await close() } }
    }

    private var activeID: String { model.selectedEnvironmentID ?? WorkspaceDraft.globalEnvironmentID }

    @ViewBuilder
    private var removedEdits: some View {
        if !editing.removedDrafts.isEmpty {
            VStack(alignment: .leading, spacing: WireboltTheme.Spacing.medium) {
                inlineProblem("Edited fields were removed from the workspace. Discard their unsaved edits to continue.")
                Text("\(editing.removedDrafts.count) removed field edits")
                    .font(WireboltTheme.Typography.detail)
                Button("Discard Removed Edits") { editing.discardRemovedDrafts() }
            }
            .padding(WireboltTheme.Spacing.large)
            .background(theme.definition.card.color, in: .rect(cornerRadius: theme.geometry.card))
        }
    }

    private var environmentList: some View {
        VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xSmall) {
            HStack {
                Text("WORKSPACE").font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
                Spacer()
                Button("New Environment", systemImage: "plus") {
                    perform(.create) {
                        selectedID = inspector.browse(id: model.selectedEnvironmentID ?? WorkspaceDraft.globalEnvironmentID)
                        editingNameID = selectedID
                    }
                }.labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
            }
            ForEach(environments) { environment in
                VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xSmall) {
                    HStack(spacing: WireboltTheme.Spacing.medium) {
                        if editingNameID == environment.id {
                            field(.name(environment.id), text: environment.name, label: "Environment name")
                        } else {
                            Button {
                                Task { await select(environment.id) }
                            } label: {
                                HStack(spacing: WireboltTheme.Spacing.medium) {
                                    Circle().fill(environmentColor(environment.name)).frame(width: 7, height: 7)
                                        .accessibilityHidden(true)
                                    Text(environment.name)
                                        .lineLimit(1).truncationMode(.middle)
                                    Spacer(minLength: 0)
                                    if activeID == environment.id {
                                        Label("Active", systemImage: "checkmark.circle.fill")
                                            .font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
                                    }
                                }
                                .contentShape(.rect)
                            }
                            .buttonStyle(.plain)
                            .help(environment.name)
                            .accessibilityLabel(environment.name)
                            .accessibilityValue(activeID == environment.id ? "Active" : "")
                            .accessibilityAddTraits(selectedID == environment.id ? .isSelected : [])
                        }
                    }
                    if let problem = editing.problems[.name(environment.id)] { inlineProblem(problem) }
                }
                .padding(.horizontal, WireboltTheme.Spacing.large)
                .frame(minHeight: theme.density.controlHeight + WireboltTheme.Spacing.medium)
                .background(selectedID == environment.id ? theme.definition.selected.color : .clear, in: .rect(cornerRadius: theme.geometry.row))
                .contextMenu {
                    Button(environment.id == WorkspaceDraft.globalEnvironmentID ? "Use Globals Only" : "Use for Requests") { Task { if await select(environment.id) { model.selectedEnvironmentID = inspector.useForRequests() } } }
                    Button("Rename") { Task { if await select(environment.id) { editingNameID = environment.id } } }
                        .disabled(environment.id == WorkspaceDraft.globalEnvironmentID)
                    Button("Delete", role: .destructive) { perform(.delete, environmentID: environment.id) }
                        .disabled(environment.id == WorkspaceDraft.globalEnvironmentID)
                }
            }
        }
    }

    private var variablesCard: some View {
        VStack(alignment: .leading, spacing: WireboltTheme.Spacing.large) {
            HStack {
                Text("Variables").font(WireboltTheme.Typography.body.weight(.medium))
                Spacer()
                Menu("Variable Actions", systemImage: "ellipsis") {
                    Button("New Entry", action: addRow)
                    Button("Clear All") { perform(.clearVariables) }
                }.ghostMenuStyle().menuIndicator(.hidden).labelStyle(.iconOnly)
                Button("New Entry", systemImage: "plus", action: addRow)
                    .labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
            }
            if let selected {
                ForEach(selected.variables) { variable in
                    variableRow(variable)
                    Divider().overlay(theme.hairline.color)
                }
                if selected.variables.isEmpty {
                    Text("Add a variable to use {{name}} in your requests.")
                        .font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
                }
            }
        }
        .padding(WireboltTheme.Spacing.large)
        .background(theme.definition.card.color, in: .rect(cornerRadius: theme.geometry.card))
        .overlay { RoundedRectangle(cornerRadius: theme.geometry.card).stroke(theme.cardBorder.color, lineWidth: 1) }
    }

    private func variableRow(_ variable: EnvironmentVariableDraft) -> some View {
        VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xSmall) {
            HStack(alignment: .center, spacing: WireboltTheme.Spacing.xSmall) {
                FieldCheckbox(isOn: Binding(get: { variable.enabled }, set: { perform(.variableEnabled(id: variable.id, enabled: $0)) }), label: "Enable variable \(variable.key)")
                field(.key(selectedID, variable.id), text: variable.key, label: "Variable name")
                    .frame(minWidth: 64, idealWidth: 96, maxWidth: 112)
                field(.value(selectedID, variable.id), text: model.secretMaterial(for: variable.value), label: "Value for \(variable.key)", secret: variable.isSecret && !revealedSecretIDs.contains(variable.id))
                    .frame(maxWidth: .infinity)
                if variable.isSecret {
                    Button(revealedSecretIDs.contains(variable.id) ? "Hide Value" : "Show Value", systemImage: revealedSecretIDs.contains(variable.id) ? "eye.slash" : "eye") {
                        if revealedSecretIDs.contains(variable.id) { revealedSecretIDs.remove(variable.id) }
                        else { revealedSecretIDs.insert(variable.id) }
                    }.labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
                }
                Toggle(isOn: Binding(get: { variable.isSecret }, set: { perform(.variableSecret(id: variable.id, secret: $0)) })) {
                    Image(systemName: variable.isSecret ? "lock.fill" : "lock.open")
                }.toggleStyle(.button).buttonStyle(GhostIconStyle())
                    .accessibilityLabel("Store \(variable.key) in Keychain")
            }
            variableProblems(variable)
        }
        .padding(.vertical, WireboltTheme.Spacing.xSmall)
        .contextMenu {
            Button(variable.isSecret ? "Store in Workspace File" : "Make Secret") { perform(.variableSecret(id: variable.id, secret: !variable.isSecret)) }
            Button("Delete", role: .destructive) { perform(.deleteVariable(id: variable.id)) }
        }
        .task(id: variable.value) { if variable.isSecret { await model.loadSecret(variable.value) } }
    }

    @ViewBuilder
    private func variableProblems(_ variable: EnvironmentVariableDraft) -> some View {
        if let problem = editing.problems[.key(selectedID, variable.id)] { inlineProblem(problem) }
        if let problem = editing.problems[.value(selectedID, variable.id)] { inlineProblem(problem) }
    }

    private func field(_ id: EnvironmentInspectorEditing.Field, text: String, label: String, secret: Bool = false) -> some View {
        EnvironmentInspectorField(text: Binding(get: { editing.drafts[id] ?? text }, set: { editing.drafts[id] = $0 }), label: label, secret: secret,
            font: .monospacedSystemFont(ofSize: theme.typography.field * fieldScale, weight: .regular)) { editing.commit(id) }
            .id(secret)
            .frame(minHeight: max(theme.density.controlHeight, theme.typography.field * fieldScale + 8))
    }

    private func inlineProblem(_ message: String) -> some View {
        Label(message, systemImage: "exclamationmark.circle")
            .font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.danger)
            .fixedSize(horizontal: false, vertical: true)
    }

    @discardableResult
    private func select(_ id: String) async -> Bool {
        guard await editing.flush() else { return false }
        editingNameID = nil
        selectedID = inspector.browse(id: id)
        return true
    }

    private func close() async -> Bool {
        guard await editing.flush() else { return false }
        onClose()
        return true
    }

    private func perform(_ commit: EnvironmentCommit, environmentID: String? = nil, completion: @escaping () -> Void = {}) {
        let id = environmentID ?? selectedID
        editing.perform(id: id, commit: commit, completion: completion)
    }

    private func addRow() { perform(.addVariable) }

    private func environmentColor(_ name: String) -> Color {
        let name = name.lowercased()
        if name.contains("prod") { return WireboltTheme.danger }
        if name.contains("stag") { return WireboltTheme.warning }
        return WireboltTheme.success
    }
}

/// AppKit supplies precise end-editing notifications (including Tab and window
/// focus loss), middle truncation outside editing, and a horizontally scrolling
/// field editor with the complete value. No system events are synthesized.
struct EnvironmentInspectorField: NSViewRepresentable {
    @Binding var text: String
    let label: String
    let secret: Bool
    let font: NSFont
    let onCommit: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: Context) -> NSTextField {
        let field: NSTextField = secret ? NSSecureTextField() : NSTextField()
        field.isBordered = false
        field.drawsBackground = false
        field.cell?.usesSingleLineMode = true
        field.cell?.wraps = false
        field.cell?.isScrollable = true
        field.lineBreakMode = .byTruncatingMiddle
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        field.delegate = context.coordinator
        return field
    }
    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        if field.stringValue != text { field.stringValue = text }
        field.font = font
        field.textColor = .labelColor
        field.placeholderString = label
        field.setAccessibilityLabel(label)
        field.toolTip = secret ? nil : text
    }
    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: EnvironmentInspectorField
        init(_ parent: EnvironmentInspectorField) { self.parent = parent }
        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            parent.text = field.stringValue
        }
        func controlTextDidEndEditing(_ notification: Notification) { parent.onCommit() }
        func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            guard commandSelector == #selector(NSResponder.insertNewline(_:)) else { return false }
            control.window?.makeFirstResponder(nil)
            return true
        }
    }
}

private struct EnvironmentAddRowKey: FocusedValueKey {
    typealias Value = @MainActor () -> Void
}

extension FocusedValues {
    var environmentAddRow: (@MainActor () -> Void)? {
        get { self[EnvironmentAddRowKey.self] }
        set { self[EnvironmentAddRowKey.self] = newValue }
    }
}
