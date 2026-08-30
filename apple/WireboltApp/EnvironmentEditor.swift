import SwiftUI

struct EnvironmentEditor: View {
    @Bindable var model: WireboltModel
    @Environment(\.dismiss) private var dismiss
    @State private var environment: EnvironmentDraft
    @State private var rows: [EnvironmentRow]

    init(model: WireboltModel, environment: EnvironmentDraft) {
        self.model = model
        _environment = State(initialValue: environment)
        _rows = State(initialValue: environment.variables.map {
            EnvironmentRow(name: $0.key, source: $0.value)
        }.sorted { $0.name < $1.name })
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                TextField("Name", text: $environment.name)
                LabeledContent("Variables") {
                    Button("Add Variable", systemImage: "plus") {
                        rows.append(EnvironmentRow())
                    }
                }

                ForEach($rows) { $row in
                    HStack {
                        TextField("Name", text: $row.name)
                        Picker("Storage", selection: $row.isSecret) {
                            Text("Literal").tag(false)
                            Text("Keychain").tag(true)
                        }
                        .labelsHidden()
                        .frame(width: 100)
                        if row.isSecret {
                            TextField("Reference", text: $row.value)
                            SecureField("Secret value", text: $row.secretMaterial)
                        } else {
                            TextField("Value", text: $row.value)
                        }
                        Button("Remove", systemImage: "minus.circle") {
                            rows.removeAll { $0.id == row.id }
                        }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                    }
                }
            }
            .formStyle(.grouped)

            Divider()
            HStack {
                Spacer()
                Button("Cancel", role: .cancel, action: dismiss.callAsFunction)
                    .keyboardShortcut(.cancelAction)
                Button("Save") { Task { await save() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(hasInvalidRows)
            }
            .padding(12)
        }
        .frame(minWidth: 680, minHeight: 360)
    }

    private func save() async {
        for row in rows where row.isSecret && !row.secretMaterial.isEmpty {
            await model.saveSecret(name: row.value, value: row.secretMaterial)
        }
        var variables: [String: ValueSource] = [:]
        for row in rows {
            variables[row.name] = row.isSecret ? .secret(row.value) : .literal(row.value)
        }
        environment.variables = variables
        await model.saveEnvironment(environment)
        dismiss()
    }

    private var hasInvalidRows: Bool {
        rows.contains { $0.name.isEmpty || $0.value.isEmpty }
            || Set(rows.map(\.name)).count != rows.count
    }
}

private struct EnvironmentRow: Identifiable {
    let id = UUID()
    var name = ""
    var value = ""
    var isSecret = false
    var secretMaterial = ""

    init(name: String = "", source: ValueSource = .literal("")) {
        self.name = name
        switch source {
        case let .literal(value): self.value = value
        case let .secret(name):
            value = name
            isSecret = true
        }
    }
}
