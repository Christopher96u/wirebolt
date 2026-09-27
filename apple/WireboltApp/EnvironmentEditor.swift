import SwiftUI

struct EnvironmentEditor: View {
    @Bindable var model: WireboltModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @State private var environments: [EnvironmentDraft]
    @State private var selectedID: String
    @State private var newKey = ""
    @State private var newValue = ""
    @State private var deletedIDs: Set<String> = []
    @State private var editingNameID: String?
    @FocusState private var newKeyFocused: Bool
    @FocusState private var environmentListFocused: Bool
    @State private var isSaving = false
    @State private var validationMessage: String?

    init(model: WireboltModel) {
        self.model = model
        var values = model.workspace.environments
        if !values.contains(where: { $0.id == WorkspaceDraft.globalEnvironmentID }) {
            values.insert(EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Global Environment"), at: 0)
        }
        _environments = State(initialValue: values)
        _selectedID = State(initialValue: model.selectedEnvironmentID ?? WorkspaceDraft.globalEnvironmentID)
    }

    var body: some View {
        VStack(spacing: 8) {
            HStack(alignment: .top, spacing: 0) {
                VStack(alignment: .leading, spacing: 0) {
                    Text("Environments").foregroundStyle(.secondary).padding(.leading, 12).frame(height: 31)
                    Divider()
                    ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach($environments) { $environment in
                            HStack(spacing: 4) {
                                if environment.id == WorkspaceDraft.globalEnvironmentID {
                                    Text("Global Environment").lineLimit(1)
                                    Spacer(minLength: 2)
                                    Image(systemName: "info.circle").help("Global variables are available in every environment.")
                                } else {
                                    InlineSidebarName(title: environment.name, isEditing: Binding(
                                        get: { editingNameID == environment.id },
                                        set: { editingNameID = $0 ? environment.id : nil }
                                    )) { environment.name = $0 }
                                    Spacer(minLength: 2)
                                }
                            }
                            .padding(.horizontal, 6)
                            .frame(height: 24)
                            .foregroundStyle(selectedID == environment.id ? Color.white : Color.primary)
                            .background(selectedID == environment.id ? WireboltTheme.primaryAccent : .clear, in: .rect(cornerRadius: 5))
                            .contentShape(.rect)
                            .simultaneousGesture(TapGesture(count: 2).onEnded {
                                selectedID = environment.id
                                if environment.id != WorkspaceDraft.globalEnvironmentID { editingNameID = environment.id }
                            })
                            .simultaneousGesture(TapGesture().onEnded {
                                selectedID = environment.id
                                if editingNameID == nil { environmentListFocused = true }
                            })
                            .accessibilityElement(children: .contain)
                            .accessibilityLabel(environment.name)
                            .accessibilityAction { selectedID = environment.id }
                            .accessibilityAddTraits(selectedID == environment.id ? .isSelected : [])
                            .contextMenu {
                                Button("Delete") { deleteEnvironment(id: environment.id) }
                                    .disabled(environment.id == WorkspaceDraft.globalEnvironmentID)
                            }
                        }
                    }.padding(10)
                    }
                    .focusable().focusEffectDisabled().focused($environmentListFocused)
                    .onMoveCommand { direction in
                        guard editingNameID == nil, let index = environments.firstIndex(where: { $0.id == selectedID }) else { return }
                        if direction == .up { selectedID = environments[max(0, index - 1)].id }
                        else if direction == .down { selectedID = environments[min(environments.count - 1, index + 1)].id }
                    }
                    .onKeyPress(.return) {
                        guard editingNameID == nil, selectedID != WorkspaceDraft.globalEnvironmentID else { return .ignored }
                        editingNameID = selectedID
                        return .handled
                    }
                    .background {
                        if reduceTransparency { Color(nsColor: .windowBackgroundColor) }
                        else { SidebarMaterialView() }
                    }
                }.frame(width: 200)
                VStack(spacing: 0) {
                    HStack {
                        Text("Variables").foregroundStyle(.secondary)
                        Spacer()
                        Button("New Entry", systemImage: "plus", action: addRow)
                            .labelStyle(.iconOnly).buttonStyle(.borderless)
                            .keyboardShortcut("k", modifiers: .command)
                        Menu("Variable Actions", systemImage: "ellipsis.circle") {
                            Button("New Entry", action: addRow)
                            Button("Clear All") { selected.variables.wrappedValue = [] }
                        }.menuStyle(.borderlessButton).menuIndicator(.hidden).labelStyle(.iconOnly)
                    }.padding(.leading, 12).frame(height: 31)
                    Divider()
                    HStack(spacing: 0) {
                        Color.clear.frame(width: 27)
                        Divider().frame(height: 14)
                        Text("Key").padding(.leading, 6).frame(width: 183, alignment: .leading)
                        Divider().frame(height: 14)
                        Text("Value").padding(.leading, 6).frame(maxWidth: .infinity, alignment: .leading)
                    }.font(.system(size: 11)).frame(height: 27)
                    Divider()
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(selected.variables) { $variable in
                                let height = FieldEditorMetrics.height(key: variable.key, value: variable.value.editableValue, valueWidth: 392)
                                HStack(alignment: .top, spacing: 0) {
                                    FieldCheckbox(isOn: $variable.enabled)
                                        .accessibilityLabel("Enable variable \(variable.key)")
                                    FieldTextInput("Key", text: $variable.key, height: height).frame(width: 175).padding(.horizontal, 4)
                                    Color.clear.frame(width: 1)
                                    FieldTextInput("Value", text: Binding(
                                        get: { variable.value.editableValue },
                                        set: { value in
                                            if case .secret = variable.value { variable.value = .secret(value) }
                                            else { variable.value = .literal(value) }
                                        }
                                    ), height: height).padding(.horizontal, 4).padding(.trailing, 5).frame(maxWidth: .infinity)
                                }.frame(height: height).padding(.vertical, 4)
                                    .contextMenu {
                                        Button("Delete") { selected.variables.wrappedValue.removeAll { $0.id == variable.id } }
                                    }
                            }
                            HStack(spacing: 0) {
                                Color.clear.frame(width: 28)
                                FieldTextInput("New Key (⌘K)", text: $newKey, height: newFieldHeight).frame(width: 175).padding(.horizontal, 4)
                                    .focused($newKeyFocused).onSubmit { commitNewRow(); newKeyFocused = true }
                                Color.clear.frame(width: 1)
                                FieldTextInput("New Value", text: $newValue, height: newFieldHeight).padding(.horizontal, 4).padding(.trailing, 5).onSubmit { commitNewRow(); newKeyFocused = true }
                            }.frame(height: newFieldHeight).padding(.vertical, 4)
                        }.font(.system(size: 11, design: .monospaced))
                    }.background(Color(nsColor: .textBackgroundColor))
                }
            }
            HStack {
                Button("New Environment") {
                    let environment = model.makeNewEnvironment()
                    environments.append(environment)
                    selectedID = environment.id
                    editingNameID = environment.id
                }
                Spacer()
                Button { Task { await saveAndClose() } } label: { Text("Close").frame(width: 66) }
                    .accessibilityLabel("Save environments and close")
                    .keyboardShortcut(.cancelAction)
                    .disabled(isSaving)
            }.controlSize(.regular).frame(height: 24)
        }
        .font(.system(size: 13)).padding(.horizontal, 20).padding(.top, 20).padding(.bottom, 8).frame(width: 857, height: 480)
        .onChange(of: selectedID) { previous, _ in commitNewRow(environmentID: previous) }
        .interactiveDismissDisabled()
        .alert("Environment could not be saved", isPresented: Binding(
            get: { validationMessage != nil }, set: { if !$0 { validationMessage = nil } }
        )) { Button("OK", role: .cancel) {} } message: { Text(validationMessage ?? "") }
    }

    private var newFieldHeight: Double { FieldEditorMetrics.height(key: newKey, value: newValue, valueWidth: 392) }

    private var selected: Binding<EnvironmentDraft> {
        Binding(
            get: { environments.first { $0.id == selectedID } ?? environments[0] },
            set: { value in
                guard let index = environments.firstIndex(where: { $0.id == value.id }) else { return }
                environments[index] = value
            }
        )
    }

    private func addRow() {
        commitNewRow()
        newKeyFocused = true
    }

    private func deleteEnvironment(id: String) {
        guard id != WorkspaceDraft.globalEnvironmentID else { return }
        if model.workspace.environments.contains(where: { $0.id == id }) { deletedIDs.insert(id) }
        environments.removeAll { $0.id == id }
        selectedID = WorkspaceDraft.globalEnvironmentID
    }

    private func commitNewRow(environmentID: String? = nil) {
        defer { newKey = ""; newValue = "" }
        guard !newKey.trimmingCharacters(in: .whitespaces).isEmpty,
              let index = environments.firstIndex(where: { $0.id == (environmentID ?? selectedID) }) else { return }
        environments[index].variables.append(EnvironmentVariableDraft(
            key: newKey, value: .literal(newValue), order: environments[index].variables.count
        ))
    }

    private var hasInvalidRows: Bool {
        environments.contains { environment in
            let keys = environment.variables.filter(\.enabled).map(\.key)
            return environment.name.trimmingCharacters(in: .whitespaces).isEmpty
                || keys.contains { $0.trimmingCharacters(in: .whitespaces).isEmpty }
                || Set(keys).count != keys.count
        }
    }

    private func saveAndClose() async {
        commitNewRow()
        guard !hasInvalidRows else {
            validationMessage = "Give each environment a name and each enabled variable a unique, non-empty key."
            return
        }
        isSaving = true
        defer { isSaving = false }
        for id in deletedIDs {
            guard await model.deleteEnvironment(id: id) else { validationMessage = "The environment could not be deleted."; return }
            deletedIDs.remove(id)
        }
        for environment in environments {
            guard await model.saveEnvironment(environment) else { validationMessage = "The workspace could not be saved. Your edits are still open."; return }
        }
        model.selectedEnvironmentID = selectedID == WorkspaceDraft.globalEnvironmentID ? nil : selectedID
        isSaving = false
        dismiss()
    }
}
