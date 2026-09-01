import SwiftUI

struct EnvironmentEditor: View {
    @Bindable var model: WireboltModel
    @Environment(\.dismiss) private var dismiss
    @State private var environment: EnvironmentDraft
    @State private var secretMaterialByRowID: [String: String] = [:]

    init(model: WireboltModel, environment: EnvironmentDraft) {
        self.model = model
        _environment = State(initialValue: environment)
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                TextField("Name", text: $environment.name)
                LabeledContent("Variables") {
                    Button("Add Variable", systemImage: "plus") {
                        environment.variables.append(EnvironmentVariableDraft(
                            order: environment.variables.count
                        ))
                    }
                }

                ForEach($environment.variables) { $variable in
                    EnvironmentVariableRow(
                        variable: $variable,
                        secretMaterial: Binding(
                            get: { secretMaterialByRowID[variable.id, default: ""] },
                            set: { secretMaterialByRowID[variable.id] = $0 }
                        ),
                        canMoveUp: variable.order > 0,
                        canMoveDown: variable.order < environment.variables.count - 1,
                        moveUp: { move(variable.id, offset: -1) },
                        moveDown: { move(variable.id, offset: 1) },
                        remove: {
                            environment.variables.removeAll { $0.id == variable.id }
                            normalizeOrder()
                        }
                    )
                }
            }
            .formStyle(.grouped)

            Divider()
            HStack {
                Text("Secret values are stored in Keychain; only their reference is saved.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Cancel", role: .cancel, action: dismiss.callAsFunction)
                    .keyboardShortcut(.cancelAction)
                Button("Save") { Task { await save() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(hasInvalidRows)
            }
            .padding(12)
        }
        .frame(minWidth: 760, minHeight: 420)
    }

    private func move(_ id: String, offset: Int) {
        guard let source = environment.variables.firstIndex(where: { $0.id == id }) else { return }
        let destination = source + offset
        guard environment.variables.indices.contains(destination) else { return }
        environment.variables.swapAt(source, destination)
        normalizeOrder()
    }

    private func normalizeOrder() {
        for index in environment.variables.indices {
            environment.variables[index].order = index
        }
    }

    private func save() async {
        for variable in environment.variables {
            guard case let .secret(reference) = variable.value,
                  let material = secretMaterialByRowID[variable.id],
                  material.isEmpty == false
            else { continue }
            await model.saveSecret(name: reference, value: material)
        }
        normalizeOrder()
        await model.saveEnvironment(environment)
        dismiss()
    }

    private var hasInvalidRows: Bool {
        let enabledKeys = environment.variables.filter(\.enabled).map(\.key)
        return environment.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            || environment.variables.contains { variable in
                variable.key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    || variable.value.editableValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            }
            || Set(enabledKeys).count != enabledKeys.count
    }
}

private struct EnvironmentVariableRow: View {
    @Binding var variable: EnvironmentVariableDraft
    @Binding var secretMaterial: String
    let canMoveUp: Bool
    let canMoveDown: Bool
    let moveUp: () -> Void
    let moveDown: () -> Void
    let remove: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Toggle("Enabled", isOn: $variable.enabled)
                .labelsHidden()
            TextField("Name", text: $variable.key)
            Picker("Storage", selection: isSecret) {
                Text("Literal").tag(false)
                Text("Keychain").tag(true)
            }
            .labelsHidden()
            .frame(width: 100)
            if isSecret.wrappedValue {
                TextField("Keychain reference", text: editableValue)
                SecureField("New secret value (optional)", text: $secretMaterial)
            } else {
                TextField("Value", text: editableValue)
            }
            Button("Move Up", systemImage: "chevron.up", action: moveUp)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .disabled(!canMoveUp)
            Button("Move Down", systemImage: "chevron.down", action: moveDown)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .disabled(!canMoveDown)
            Button("Remove", systemImage: "minus.circle", action: remove)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
        }
    }

    private var isSecret: Binding<Bool> {
        Binding(
            get: {
                if case .secret = variable.value { true } else { false }
            },
            set: { secret in
                variable.value = secret
                    ? .secret(variable.value.editableValue)
                    : .literal(variable.value.editableValue)
            }
        )
    }

    private var editableValue: Binding<String> {
        Binding(
            get: { variable.value.editableValue },
            set: { value in
                variable.value = isSecret.wrappedValue ? .secret(value) : .literal(value)
            }
        )
    }
}
