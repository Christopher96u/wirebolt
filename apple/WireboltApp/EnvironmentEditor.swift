import SwiftUI

struct EnvironmentEditor: View {
    @Bindable var model: WireboltModel
    let interface: WorkspaceUIState
    @Environment(\.wireboltTheme) private var theme
    let closeTrigger: Int
    let onClose: () -> Void
    @State private var environments: [EnvironmentDraft]
    /// Baseline including any successfully applied rows after a recoverable save failure.
    @State private var original: [EnvironmentDraft]
    @State private var workspaceLocation: URL?
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
    /// Secret values typed in this inspector, by Keychain name. Save writes them to Keychain;
    /// Cancel drops them. The workspace stores only the names.
    @State private var stagedSecrets: [String: String] = [:]
    @State private var revealedSecretIDs: Set<String> = []

    init(model: WireboltModel, interface: WorkspaceUIState, closeTrigger: Int, onClose: @escaping () -> Void) {
        self.model = model
        self.interface = interface
        self.closeTrigger = closeTrigger
        self.onClose = onClose
        var values = model.workspace.environments
        if !values.contains(where: { $0.id == WorkspaceDraft.globalEnvironmentID }) {
            values.insert(EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Global Environment"), at: 0)
        }
        _environments = State(initialValue: values)
        _original = State(initialValue: values)
        _workspaceLocation = State(initialValue: model.workspaceLocation)
        _selectedID = State(initialValue: model.selectedEnvironmentID ?? WorkspaceDraft.globalEnvironmentID)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: WireboltTheme.Spacing.large) {
            HStack {
                Text("Environments").font(WireboltTheme.Typography.body.weight(.semibold))
                Spacer()
                Button("Close Inspector", systemImage: "xmark", action: cancel)
                    .labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
            }
            ScrollView {
                VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xLarge) {
                    environmentList
                    variablesCard
                    if selectedID != WorkspaceDraft.globalEnvironmentID {
                        DisclosureGroup("Globals") {
                            let globals = environments.first { $0.id == WorkspaceDraft.globalEnvironmentID }?.variables ?? []
                            ForEach(globals) { variable in
                                HStack {
                                    Text(variable.key).font(WireboltTheme.Typography.field)
                                    Spacer()
                                    // A global secret remains redacted even when it has been loaded.
                                    Text(variable.isSecret ? "Keychain" : variable.value.editableValue)
                                        .font(WireboltTheme.Typography.field).foregroundStyle(WireboltTheme.muted).lineLimit(1)
                                }.frame(minHeight: theme.density.controlHeight)
                            }
                            Button("Edit Globals") { selectedID = WorkspaceDraft.globalEnvironmentID }
                        }
                        .foregroundStyle(WireboltTheme.muted)
                    }
                    Label("Secrets stay in this Mac’s Keychain.", systemImage: "key")
                        .font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
                }
            }
            if isSaving { ProgressView().controlSize(.small) }
            HStack {
                Button("Cancel", action: cancel)
                Spacer()
                Button("Apply") { Task { _ = await saveAndClose() } }.disabled(isSaving)
            }
            .controlSize(.regular)
            .frame(minHeight: theme.density.controlHeight)
        }
        .font(WireboltTheme.Typography.tab)
        .padding(WireboltTheme.Spacing.xLarge)
        .background(theme.definition.overlay.color)
        .onChange(of: selectedID) { previous, _ in commitNewRow(environmentID: previous) }
        .onChange(of: closeTrigger) { cancel() }
        .onChange(of: model.workspaceLocation) { onClose() }
        .onAppear { interface.saveEnvironmentEdits = saveAndClose }
        .onChange(of: hasChanges, initial: true) { interface.hasEnvironmentEdits = hasChanges }
        .onDisappear { interface.hasEnvironmentEdits = false; interface.saveEnvironmentEdits = nil }
        .focusedValue(\.environmentAddRow, addRow)
        .onSubmit { Task { _ = await saveAndClose() } }
        .onExitCommand(perform: cancel)
        .disabled(isSaving)
        .alert("Environment could not be saved", isPresented: Binding(
            get: { validationMessage != nil }, set: { if !$0 { validationMessage = nil } }
        )) { Button("OK", role: .cancel) {} } message: { Text(validationMessage ?? "") }
        .alert("Discard changes?", isPresented: $isConfirmingDiscard) {
            Button("Discard Changes", role: .destructive) { onClose() }
            Button("Keep Editing", role: .cancel) {}
        } message: { Text("Your edits to environments and variables haven’t been saved.") }
    }

    private var environmentList: some View {
        VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xSmall) {
            HStack {
                Text("WORKSPACE").font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
                Spacer()
                Button("New Environment", systemImage: "plus") {
                    let environment = model.makeNewEnvironment()
                    environments.append(environment)
                    selectedID = environment.id
                    editingNameID = environment.id
                }.labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
            }
            ForEach($environments) { $environment in
                HStack(spacing: WireboltTheme.Spacing.medium) {
                    Circle().fill(environmentColor(environment.name)).frame(width: 7, height: 7)
                        .accessibilityHidden(true)
                    InlineSidebarName(title: environment.id == WorkspaceDraft.globalEnvironmentID ? "Globals" : environment.name,
                        isEditing: Binding(get: { editingNameID == environment.id }, set: { editingNameID = $0 ? environment.id : nil })) {
                            environment.name = $0
                        }
                    Spacer(minLength: 0)
                    if selectedID == environment.id { Image(systemName: "checkmark").foregroundStyle(WireboltTheme.muted) }
                }
                .padding(.horizontal, WireboltTheme.Spacing.large)
                .frame(minHeight: theme.density.controlHeight + WireboltTheme.Spacing.medium)
                .background(selectedID == environment.id ? theme.definition.selected.color : .clear, in: .rect(cornerRadius: theme.geometry.row))
                .contentShape(.rect)
                .simultaneousGesture(TapGesture().onEnded { selectedID = environment.id; environmentListFocused = true })
                .accessibilityElement(children: .contain)
                .accessibilityLabel(environment.name)
                .accessibilityAction { selectedID = environment.id }
                .accessibilityAddTraits(selectedID == environment.id ? .isSelected : [])
                .contextMenu {
                    Button("Rename") { selectedID = environment.id; editingNameID = environment.id }
                        .disabled(environment.id == WorkspaceDraft.globalEnvironmentID)
                    Button("Delete", role: .destructive) { deleteEnvironment(id: environment.id) }
                        .disabled(environment.id == WorkspaceDraft.globalEnvironmentID)
                }
            }
        }
        .focusable().focused($environmentListFocused)
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
    }

    private func environmentColor(_ name: String) -> Color {
        let name = name.lowercased()
        if name.contains("prod") { return WireboltTheme.danger }
        if name.contains("stag") { return WireboltTheme.warning }
        return WireboltTheme.success
    }

    private var variablesCard: some View {
        VStack(alignment: .leading, spacing: WireboltTheme.Spacing.large) {
            HStack {
                Text("Variables").font(WireboltTheme.Typography.body.weight(.medium))
                Spacer()
                Menu("Variable Actions", systemImage: "ellipsis") {
                    Button("New Entry", action: addRow)
                    Button("Clear All") { selected.variables.wrappedValue = [] }
                }.ghostMenuStyle().menuIndicator(.hidden).labelStyle(.iconOnly)
                Button("New Entry", systemImage: "plus", action: addRow)
                    .labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
            }
            ForEach(selected.variables) { $variable in
                VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xSmall) {
                    HStack(alignment: .top, spacing: WireboltTheme.Spacing.xSmall) {
                        FieldCheckbox(isOn: $variable.enabled, label: "Enable variable \(variable.key)")
                        TextField("Key", text: $variable.key).textFieldStyle(.plain)
                            .font(WireboltTheme.Typography.field).accessibilityLabel("Variable key")
                            .frame(width: 96, height: 28)
                        let height = FieldEditorMetrics.height(for: variable.isSecret ? "" : variable.value.editableValue, width: 160)
                        valueField($variable, height: height).frame(maxWidth: .infinity, minHeight: 28)
                        Toggle(isOn: Binding(get: { variable.isSecret }, set: { setSecret($0, variable: $variable) })) {
                            Image(systemName: variable.isSecret ? "lock.fill" : "lock.open")
                        }.toggleStyle(.button).buttonStyle(GhostIconStyle())
                            .accessibilityLabel("Store \(variable.key) in Keychain")
                    }
                    if variable.isSecret {
                        Label("Keychain", systemImage: "key").font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
                            .padding(.leading, WireboltTheme.Spacing.canvas)
                    }
                }
                .padding(.vertical, WireboltTheme.Spacing.xSmall)
                .contextMenu {
                    Button(variable.isSecret ? "Store in Workspace File" : "Make Secret") { setSecret(!variable.isSecret, variable: $variable) }
                    Button("Delete", role: .destructive) { selected.variables.wrappedValue.removeAll { $0.id == variable.id } }
                }
                .task(id: variable.value) { if variable.isSecret { await model.loadSecret(variable.value) } }
                Divider().overlay(theme.hairline.color)
            }
            HStack(spacing: WireboltTheme.Spacing.medium) {
                TextField("New key", text: $newKey).textFieldStyle(.plain).focused($newKeyFocused)
                    .frame(width: 112)
                TextField("Value", text: $newValue).textFieldStyle(.plain)
            }.frame(minHeight: 28)

        }
        .padding(WireboltTheme.Spacing.large)
        .background(theme.definition.card.color, in: .rect(cornerRadius: theme.geometry.card))
        .overlay { RoundedRectangle(cornerRadius: theme.geometry.card).stroke(theme.cardBorder.color, lineWidth: 1) }
    }

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
                .font(WireboltTheme.Typography.field)
                .accessibilityLabel("Secret value for \(row.key)")
                let revealed = revealedSecretIDs.contains(row.id)
                Button(revealed ? "Hide Value" : "Show Value", systemImage: revealed ? "eye.slash" : "eye") {
                    if revealed { revealedSecretIDs.remove(row.id) } else { revealedSecretIDs.insert(row.id) }
                }
                .labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
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
        if hasChanges { isConfirmingDiscard = true } else { onClose() }
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

    private func saveAndClose() async -> Bool {
        guard model.workspaceLocation == workspaceLocation, !model.isLoadingWorkspace else { return false }
        var current = model.workspace.environments
        if !current.contains(where: { $0.id == WorkspaceDraft.globalEnvironmentID }) {
            current.insert(EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Global Environment"), at: 0)
        }
        guard current == original else {
            validationMessage = "Environments changed while you were editing. Cancel and reopen the inspector to use the latest values."
            return false
        }
        commitNewRow()
        guard !hasInvalidRows else {
            validationMessage = "Give each environment a name and each enabled variable a unique, non-empty key."
            return false
        }
        isSaving = true
        defer { isSaving = false }
        for id in deletedIDs {
            guard model.workspaceLocation == workspaceLocation else { return false }
            guard await model.deleteEnvironment(id: id) else { validationMessage = "The environment could not be deleted."; return false }
            deletedIDs.remove(id)
            original.removeAll { $0.id == id }
        }
        for index in environments.indices {
            let environment = environments[index]
            let references = Set(environment.variables.compactMap { $0.value.secretName })
            let values = stagedSecrets.filter { references.contains($0.key) }
            guard !original.contains(environment) || !values.isEmpty else { continue }
            guard model.workspaceLocation == workspaceLocation else { return false }
            guard await model.saveEnvironment(environment, secrets: values) else { validationMessage = "The environment or its Keychain values could not be saved. Your edits are still open."; return false }
            guard let saved = model.workspace.environments.first(where: { $0.id == environment.id }) else { return false }
            environments[index] = saved
            if let originalIndex = original.firstIndex(where: { $0.id == saved.id }) { original[originalIndex] = saved }
            else { original.append(saved) }
        }
        stagedSecrets = [:]
        model.selectedEnvironmentID = selectedID == WorkspaceDraft.globalEnvironmentID ? nil : selectedID
        isSaving = false
        onClose()
        return true
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
