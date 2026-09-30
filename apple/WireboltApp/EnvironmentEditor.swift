import SwiftUI

struct EnvironmentEditor: View {
    @Bindable var model: WireboltModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @State private var environments: [EnvironmentDraft]
    /// The state the sheet opened with; Cancel returns to it without touching the workspace.
    private let original: [EnvironmentDraft]
    @State private var selectedID: String
    @State private var newKey = ""
    @State private var newValue = ""
    @State private var deletedIDs: Set<String> = []
    @State private var editingNameID: String?
    @FocusState private var newKeyFocused: Bool
    @FocusState private var environmentListFocused: Bool
    @State private var isSaving = false
    @State private var validationMessage: String?
    @State private var isConfirmingDiscard = false
    /// Secret values typed in this sheet, by Keychain name. Save writes them to Keychain;
    /// Cancel drops them. The workspace stores only the names.
    @State private var stagedSecrets: [String: String] = [:]
    @State private var revealedSecretIDs: Set<String> = []

    init(model: WireboltModel) {
        self.model = model
        var values = model.workspace.environments
        if !values.contains(where: { $0.id == WorkspaceDraft.globalEnvironmentID }) {
            values.insert(EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Global Environment"), at: 0)
        }
        _environments = State(initialValue: values)
        original = values
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
                            .background(selectedID == environment.id ? WireboltTheme.primaryAccent : .clear, in: .rect(cornerRadius: WireboltTheme.Radius.row))
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
                            // Same shortcut as Request ▸ Add Key.
                            .keyboardShortcut("k", modifiers: [.command, .shift])
                            .help("New Entry (⇧⌘K)")
                        Menu("Variable Actions", systemImage: "ellipsis.circle") {
                            Button("New Entry", action: addRow)
                            Button("Clear All") { selected.variables.wrappedValue = [] }
                        }.menuStyle(.borderlessButton).menuIndicator(.hidden).labelStyle(.iconOnly)
                        .help("Variable Actions")
                    }.padding(.leading, 12).frame(height: 31)
                    Divider()
                    HStack(spacing: 0) {
                        Color.clear.frame(width: 27)
                        Divider().frame(height: 14)
                        Text("Key").padding(.leading, 6).frame(width: 183, alignment: .leading)
                        Divider().frame(height: 14)
                        Text("Value").padding(.leading, 6).frame(maxWidth: .infinity, alignment: .leading)
                        Divider().frame(height: 14)
                        Image(systemName: "lock").frame(width: 52)
                            .help("Secret values are stored in this Mac’s Keychain, not in the workspace.")
                            .accessibilityLabel("Secret")
                    }.font(.system(size: 11)).frame(height: 27)
                    Divider()
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(selected.variables) { $variable in
                                let height = FieldEditorMetrics.height(
                                    key: variable.key,
                                    value: variable.isSecret ? "" : variable.value.editableValue,
                                    valueWidth: 340
                                )
                                HStack(alignment: .top, spacing: 0) {
                                    FieldCheckbox(isOn: $variable.enabled)
                                        .accessibilityLabel("Enable variable \(variable.key)")
                                    FieldTextInput("Key", text: $variable.key, height: height).frame(width: 175).padding(.horizontal, 4)
                                    Color.clear.frame(width: 1)
                                    valueField($variable, height: height)
                                        .padding(.horizontal, 4).padding(.trailing, 5).frame(maxWidth: .infinity)
                                    Toggle(isOn: Binding(
                                        get: { variable.isSecret },
                                        set: { setSecret($0, variable: $variable) }
                                    )) {
                                        Image(systemName: variable.isSecret ? "lock.fill" : "lock.open")
                                    }
                                    .toggleStyle(.button).buttonStyle(.borderless)
                                    .frame(width: 52, height: 20)
                                    .accessibilityLabel("Secret value for \(variable.key)")
                                    .help(variable.isSecret
                                        ? "Stored in Keychain. Click to store the value in the workspace file instead."
                                        : "Store the value in this Mac’s Keychain instead of the workspace file.")
                                }.frame(height: height).padding(.vertical, 4)
                                    .contextMenu {
                                        Button(variable.isSecret ? "Store in Workspace File" : "Make Secret") {
                                            setSecret(!variable.isSecret, variable: $variable)
                                        }
                                        Divider()
                                        Button("Delete") { selected.variables.wrappedValue.removeAll { $0.id == variable.id } }
                                    }
                                    .task(id: variable.value) {
                                        if variable.isSecret { await model.loadSecret(variable.value) }
                                    }
                            }
                            HStack(spacing: 0) {
                                Color.clear.frame(width: 28)
                                FieldTextInput("New Key", text: $newKey, height: newFieldHeight).frame(width: 175).padding(.horizontal, 4)
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
                if isSaving { ProgressView().controlSize(.small) }
                Button { cancel() } label: { Text("Cancel").frame(minWidth: 66) }
                    .keyboardShortcut(.cancelAction)
                Button { Task { await saveAndClose() } } label: { Text("Save").frame(minWidth: 66) }
                    .keyboardShortcut(.defaultAction)
                    .disabled(isSaving)
            }.controlSize(.regular).frame(height: 24)
        }
        .font(WireboltTheme.Typography.body)
        .padding(.horizontal, WireboltTheme.Spacing.xxLarge).padding(.top, WireboltTheme.Spacing.xxLarge)
        .padding(.bottom, WireboltTheme.Spacing.large)
        .frame(width: 857, height: 480)
        .onChange(of: selectedID) { previous, _ in commitNewRow(environmentID: previous) }
        .interactiveDismissDisabled()
        .alert("Environment could not be saved", isPresented: Binding(
            get: { validationMessage != nil }, set: { if !$0 { validationMessage = nil } }
        )) { Button("OK", role: .cancel) {} } message: { Text(validationMessage ?? "") }
        .alert("Discard changes?", isPresented: $isConfirmingDiscard) {
            Button("Discard Changes", role: .destructive) { dismiss() }
            Button("Keep Editing", role: .cancel) {}
        } message: {
            Text("Your edits to environments and variables haven’t been saved.")
        }
    }

    private var newFieldHeight: Double { FieldEditorMetrics.height(key: newKey, value: newValue, valueWidth: 392) }

    @ViewBuilder
    private func valueField(_ variable: Binding<EnvironmentVariableDraft>, height: Double) -> some View {
        let row = variable.wrappedValue
        if case let .secret(name) = row.value {
            let material = Binding(
                get: { stagedSecrets[name] ?? model.secretMaterial(for: row.value) },
                set: { stagedSecrets[name] = $0 }
            )
            HStack(spacing: 2) {
                Group {
                    if revealedSecretIDs.contains(row.id) {
                        TextField("Secret Value", text: material)
                    } else {
                        SecureField("Secret Value", text: material)
                    }
                }
                .textFieldStyle(.plain)
                .font(.system(size: 11, design: .monospaced))
                .accessibilityLabel("Secret value for \(row.key)")
                let revealed = revealedSecretIDs.contains(row.id)
                Button(revealed ? "Hide Value" : "Show Value", systemImage: revealed ? "eye.slash" : "eye") {
                    if revealed { revealedSecretIDs.remove(row.id) } else { revealedSecretIDs.insert(row.id) }
                }
                .labelStyle(.iconOnly).buttonStyle(.borderless)
                .help(revealed ? "Hide Value" : "Show Value")
            }
            .frame(height: height, alignment: .top)
        } else {
            FieldTextInput("Value", text: Binding(
                get: { row.value.editableValue },
                set: { variable.wrappedValue.value = .literal($0) }
            ), height: height)
        }
    }

    /// Moves a value between the workspace file and Keychain. Nothing is written until Save.
    private func setSecret(_ secret: Bool, variable: Binding<EnvironmentVariableDraft>) {
        let row = variable.wrappedValue
        guard secret != row.isSecret else { return }
        if secret {
            let name = row.makeSecretReference(environmentID: selectedID)
            stagedSecrets[name] = row.value.editableValue
            variable.wrappedValue.value = .secret(name)
        } else if case let .secret(name) = row.value {
            variable.wrappedValue.value = .literal(stagedSecrets[name] ?? model.secretMaterial(for: row.value))
            stagedSecrets.removeValue(forKey: name)
            revealedSecretIDs.remove(row.id)
        }
    }

    private var selected: Binding<EnvironmentDraft> {
        Binding(
            get: { environments.first { $0.id == selectedID } ?? environments[0] },
            set: { value in
                guard let index = environments.firstIndex(where: { $0.id == value.id }) else { return }
                environments[index] = value
            }
        )
    }

    private var hasChanges: Bool {
        environments != original || !deletedIDs.isEmpty || !stagedSecrets.isEmpty
            || !newKey.trimmingCharacters(in: .whitespaces).isEmpty || !newValue.isEmpty
    }

    /// Staged edits and deletions are only written by Save.
    private func cancel() {
        if hasChanges { isConfirmingDiscard = true } else { dismiss() }
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
        // Values first: a saved reference must never point at a value that was not stored.
        let referenced = Set(environments.flatMap(\.variables).compactMap { row -> String? in
            if case let .secret(name) = row.value { name } else { nil }
        })
        guard await model.saveSecrets(stagedSecrets.filter { referenced.contains($0.key) }) else {
            validationMessage = "The secret values could not be saved in Keychain. Your edits are still open."
            return
        }
        stagedSecrets = [:]
        for id in deletedIDs {
            guard await model.deleteEnvironment(id: id) else { validationMessage = "The environment could not be deleted."; return }
            deletedIDs.remove(id)
        }
        for environment in environments where !original.contains(environment) {
            guard await model.saveEnvironment(environment) else { validationMessage = "The workspace could not be saved. Your edits are still open."; return }
        }
        model.selectedEnvironmentID = selectedID == WorkspaceDraft.globalEnvironmentID ? nil : selectedID
        isSaving = false
        dismiss()
    }
}
