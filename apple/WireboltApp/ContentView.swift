import AppKit
import SwiftUI

struct ContentView: View {
    @Bindable var model: WireboltModel
    let status: CoreStatus
    @State private var showingEnvironment = false

    var body: some View {
        NavigationSplitView {
            WorkspaceSidebar(model: model, showingEnvironment: $showingEnvironment)
                .navigationSplitViewColumnWidth(min: 190, ideal: 230, max: 320)
        } detail: {
            VSplitView {
                RequestComposer(model: model)
                    .frame(minHeight: 270)
                ResponseViewer(model: model)
                    .frame(minHeight: 220)
            }
        }
        .navigationTitle(model.draft.name)
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button("Save", systemImage: "square.and.arrow.down") {
                    guard let collectionID = model.workspace.collections.first?.id else { return }
                    Task { await model.saveCurrentRequest(collectionID: collectionID) }
                }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(model.draft.name.isEmpty)
                .help("Save Request (⌘S)")

                if model.isRunning {
                    Button("Cancel", systemImage: "stop.fill", action: model.cancel)
                        .help("Cancel Request")
                } else {
                    Button("Send", systemImage: "paperplane.fill") {
                        Task { await model.send() }
                    }
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(model.draft.url.isEmpty)
                    .help("Send Request (⌘↩)")
                }
            }
        }
        .sheet(isPresented: $showingEnvironment) {
            EnvironmentEditor(
                model: model,
                environment: model.workspace.environments
                    .first(where: { $0.id == model.selectedEnvironmentID })
                    ?? model.makeNewEnvironment()
            )
        }
        .onAppear {
            PerformanceProbe.markReady()
        }
        .task {
            await model.loadWorkspace()
        }
        .overlay(alignment: .bottomTrailing) {
            Text("Core \(status.coreVersion) · ABI \(status.streamABIVersion)")
                .font(.caption2.monospaced())
                .foregroundStyle(.tertiary)
                .padding(8)
                .accessibilityLabel("Rust core version \(status.coreVersion)")
        }
    }
}

private struct WorkspaceSidebar: View {
    @Bindable var model: WireboltModel
    @Binding var showingEnvironment: Bool

    var body: some View {
        List {
            Section("Workspace") {
                HStack {
                    Text(model.workspace.name).lineLimit(1)
                    Spacer()
                    Menu("Workspace actions", systemImage: "ellipsis.circle") {
                        Button("New Workspace…", action: createWorkspace)
                        Button("Open Workspace…", action: openWorkspace)
                    }
                    .menuStyle(.borderlessButton)
                    .labelStyle(.iconOnly)
                }
            }

            Section("Collections") {
                ForEach(model.workspace.collections) { collection in
                    DisclosureGroup(collection.name) {
                        ForEach(collection.requests) { location in
                            Button {
                                model.select(location)
                            } label: {
                                HStack(spacing: 7) {
                                    Text(location.request.method.rawValue)
                                        .font(.caption2.monospaced().bold())
                                        .foregroundStyle(methodColor(location.request.method))
                                        .frame(width: 38, alignment: .leading)
                                    Text(location.request.name).lineLimit(1)
                                    Spacer(minLength: 0)
                                }
                                .contentShape(.rect)
                            }
                            .buttonStyle(.plain)
                            .listRowBackground(
                                model.selectedRequestID == location.id
                                    ? Color.accentColor.opacity(0.16)
                                    : Color.clear
                            )
                        }
                    }
                }
            }

            Section("Environment") {
                Picker("Active", selection: $model.selectedEnvironmentID) {
                    Text("None").tag(String?.none)
                    ForEach(model.workspace.environments) { environment in
                        Text(environment.name).tag(Optional(environment.id))
                    }
                }
                .labelsHidden()

                Button("Edit Environments…", systemImage: "slider.horizontal.3") {
                    showingEnvironment = true
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            HStack {
                Button("New Request", systemImage: "plus", action: model.makeNewRequest)
                    .labelStyle(.iconOnly)
                    .help("New Request")
                Spacer()
            }
            .padding(8)
        }
    }

    private func methodColor(_ method: HTTPMethod) -> Color {
        switch method {
        case .get, .head, .options: .green
        case .post: .blue
        case .put, .patch: .orange
        case .delete: .red
        }
    }

    private func createWorkspace() {
        chooseWorkspace(prompt: "Create", message: "Choose an empty folder for the new Wirebolt workspace.") { url in
            guard let persistence = try? RustWorkspacePersistence(path: url, mode: .create) else { return }
            Task { await model.openWorkspace(using: persistence) }
        }
    }

    private func openWorkspace() {
        chooseWorkspace(prompt: "Open", message: "Choose a Wirebolt workspace folder.") { url in
            guard let persistence = try? RustWorkspacePersistence(path: url, mode: .open) else { return }
            Task { await model.openWorkspace(using: persistence) }
        }
    }

    private func chooseWorkspace(
        prompt: String,
        message: String,
        action: (URL) -> Void
    ) {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = prompt
        panel.message = message
        guard panel.runModal() == .OK, let url = panel.url else { return }
        action(url)
    }
}

private struct RequestComposer: View {
    @Bindable var model: WireboltModel
    @State private var tab = ComposerTab.params

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("Request Name", text: $model.draft.name)
                    .font(.headline)
                    .textFieldStyle(.plain)
                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.top, 10)

            HStack(spacing: 8) {
                Picker("Method", selection: $model.draft.method) {
                    ForEach(HTTPMethod.allCases, id: \.self) { method in
                        Text(method.rawValue).tag(method)
                    }
                }
                .labelsHidden()
                .frame(width: 94)

                TextField("https://api.example.com/v1/resource", text: $model.draft.url)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.body, design: .monospaced))
                    .accessibilityLabel("Request URL")

                Picker("Proxy", selection: $model.draft.proxy) {
                    Text("Inherit Proxy").tag(ProxySelection.inherit)
                    Text("System Proxy").tag(ProxySelection.system)
                    Text("Direct").tag(ProxySelection.direct)
                    if case .manual = model.draft.proxy {
                        Text("Manual Proxy").tag(model.draft.proxy)
                    }
                }
                .labelsHidden()
                .frame(width: 118)
                .help("Proxy policy for this request")
            }
            .padding(12)

            Picker("Request options", selection: $tab) {
                ForEach(ComposerTab.allCases) { tab in Text(tab.title).tag(tab) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, 12)
            .padding(.bottom, 8)

            Group {
                switch tab {
                case .params: FieldEditor(title: "Query parameters", fields: $model.draft.query)
                case .headers: FieldEditor(title: "Headers", fields: $model.draft.headers)
                case .auth: AuthenticationEditor(authentication: $model.draft.authentication)
                case .body: BodyEditor(requestBody: $model.draft.body)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

private enum ComposerTab: String, CaseIterable, Identifiable {
    case params, headers, auth, body
    var id: Self { self }
    var title: String { rawValue.capitalized }
}

private struct FieldEditor: View {
    let title: String
    @Binding var fields: [RequestField]

    var body: some View {
        VStack(spacing: 6) {
            HStack {
                Text(title).font(.headline)
                Spacer()
                Button("Add", systemImage: "plus") { fields.append(RequestField()) }
                    .labelStyle(.iconOnly)
                    .help("Add field")
            }

            if fields.isEmpty {
                LightweightPlaceholder(
                    title: "No Fields",
                    systemImage: "list.bullet.rectangle"
                )
            } else {
                ScrollView {
                    LazyVStack(spacing: 5) {
                        ForEach($fields) { $field in
                            HStack(spacing: 6) {
                                Toggle("Enabled", isOn: $field.enabled).labelsHidden()
                                TextField("Name", text: $field.name)
                                TextField("Value", text: literalBinding($field.value))
                                Button("Remove", systemImage: "minus.circle") {
                                    fields.removeAll { $0.id == field.id }
                                }
                                .labelStyle(.iconOnly)
                                .buttonStyle(.borderless)
                            }
                        }
                    }
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 10)
    }

    private func literalBinding(_ source: Binding<ValueSource>) -> Binding<String> {
        Binding(
            get: { source.wrappedValue.editableValue },
            set: { source.wrappedValue = .literal($0) }
        )
    }
}

private struct AuthenticationEditor: View {
    @Binding var authentication: RequestAuthentication

    var body: some View {
        Form {
            Picker("Authentication", selection: kindBinding) {
                ForEach(AuthenticationKind.allCases) { kind in Text(kind.title).tag(kind) }
            }

            switch authentication {
            case .none:
                Text("No Authorization header will be added.").foregroundStyle(.secondary)
            case let .basic(username, password):
                TextField("Username", text: Binding(
                    get: { username.editableValue },
                    set: { newValue in
                        guard case let .basic(_, currentPassword) = authentication else { return }
                        authentication = .basic(
                            username: .literal(newValue),
                            password: currentPassword
                        )
                    }
                ))
                TextField("Password Keychain reference", text: Binding(
                    get: { password.editableValue },
                    set: { newValue in
                        guard case let .basic(currentUsername, _) = authentication else { return }
                        authentication = .basic(
                            username: currentUsername,
                            password: .secret(newValue)
                        )
                    }
                ))
            case let .bearer(token):
                TextField("Token Keychain reference", text: Binding(
                    get: { token.editableValue },
                    set: { authentication = .bearer(token: .secret($0)) }
                ))
            case let .apiKey(placement, name, value):
                Picker("Placement", selection: Binding(get: { placement }, set: { authentication = .apiKey(placement: $0, name: name, value: value) })) {
                    ForEach(APIKeyPlacement.allCases, id: \.self) { Text($0.rawValue.capitalized).tag($0) }
                }
                TextField("Name", text: Binding(get: { name }, set: { authentication = .apiKey(placement: placement, name: $0, value: value) }))
                TextField("Value Keychain reference", text: Binding(
                    get: { value.editableValue },
                    set: { authentication = .apiKey(placement: placement, name: name, value: .secret($0)) }
                ))
            }
        }
        .formStyle(.grouped)
    }

    private var kindBinding: Binding<AuthenticationKind> {
        Binding(
            get: {
                switch authentication {
                case .none: .none
                case .basic: .basic
                case .bearer: .bearer
                case .apiKey: .apiKey
                }
            },
            set: {
                authentication = switch $0 {
                case .none: .none
                case .basic: .basic(username: .literal(""), password: .secret("auth.password"))
                case .bearer: .bearer(token: .secret("auth.token"))
                case .apiKey: .apiKey(placement: .header, name: "", value: .secret("auth.api-key"))
                }
            }
        )
    }

}

private enum AuthenticationKind: CaseIterable, Identifiable {
    case none, basic, bearer, apiKey
    var id: Self { self }
    var title: String {
        switch self {
        case .none: "None"
        case .basic: "Basic Auth"
        case .bearer: "Bearer Token"
        case .apiKey: "API Key"
        }
    }
}

private struct BodyEditor: View {
    @Binding var requestBody: RequestBody

    var body: some View {
        VStack(spacing: 8) {
            Picker("Body type", selection: kindBinding) {
                ForEach(BodyKind.allCases) { kind in Text(kind.title).tag(kind) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            switch requestBody {
            case .empty:
                LightweightPlaceholder(title: "No Body", systemImage: "doc")
            case let .text(contentType, value):
                TextField("Content-Type (optional)", text: Binding(get: { contentType ?? "" }, set: { requestBody = .text(contentType: $0.isEmpty ? nil : $0, value: value) }))
                TextEditor(text: Binding(get: { value }, set: { requestBody = .text(contentType: contentType, value: $0) }))
                    .font(.system(.body, design: .monospaced))
            case let .json(value):
                TextEditor(text: Binding(get: { value }, set: { requestBody = .json(value: $0) }))
                    .font(.system(.body, design: .monospaced))
            case let .formURLEncoded(fields):
                FieldEditor(title: "Form fields", fields: Binding(get: { fields }, set: { requestBody = .formURLEncoded(fields: $0) }))
            }
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 10)
    }

    private var kindBinding: Binding<BodyKind> {
        Binding(
            get: {
                switch requestBody {
                case .empty: .empty
                case .text: .text
                case .json: .json
                case .formURLEncoded: .form
                }
            },
            set: {
                requestBody = switch $0 {
                case .empty: .empty
                case .text: .text(contentType: nil, value: "")
                case .json: .json(value: "{\n  \n}")
                case .form: .formURLEncoded(fields: [])
                }
            }
        )
    }
}

struct LightweightPlaceholder: View {
    let title: String
    let systemImage: String
    var description: String?

    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: systemImage)
                .font(.title2)
                .foregroundStyle(.tertiary)
            Text(title)
                .font(.headline)
                .foregroundStyle(.secondary)
            if let description {
                Text(description)
                    .font(.callout)
                    .foregroundStyle(.tertiary)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
    }
}

private enum BodyKind: CaseIterable, Identifiable {
    case empty, json, text, form
    var id: Self { self }
    var title: String {
        switch self {
        case .empty: "None"
        case .json: "JSON"
        case .text: "Text"
        case .form: "Form"
        }
    }
}
