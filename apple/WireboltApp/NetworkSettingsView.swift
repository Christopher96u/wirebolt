import AppKit
import SwiftUI

struct WorkspaceNetworkSettings: View {
    @Bindable var model: WireboltModel
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Workspace Settings").font(.title3.weight(.semibold))
                    Text(model.workspace.name + " · Network").font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
            }.padding(WireboltTheme.Spacing.xxLarge)
            Divider()
            NetworkSettingsPage(model: model, scope: .workspace, onClose: { dismiss() })
        }
        .frame(width: 590, height: 580).background(Color(nsColor: .windowBackgroundColor))
        .interactiveDismissDisabled()
    }
}

/// Local edits are applied explicitly; a request editor never writes its parent policy.
/// In a sheet (`onClose` set) every edit, including transport settings, is staged
/// until Save; Cancel discards them after confirmation.
struct NetworkSettingsPage: View {
    @Bindable var model: WireboltModel
    let scope: ProxyScope
    var session: DocumentSession?
    var onClose: (() -> Void)?
    @Environment(\.openSettings) private var openSettings
    @State private var form: ProxyFormDraft
    @State private var baseline: ProxyFormDraft
    @State private var saving = false
    @State private var status: String?
    @State private var saveError: String?
    @State private var showsTransport = false
    @State private var test: ProxyConnectionTest
    @State private var testURL = ""
    @State private var showsTest = false
    @State private var transport: TransportSettings
    @State private var transportBaseline: TransportSettings
    @State private var isConfirmingDiscard = false

    init(model: WireboltModel, scope: ProxyScope, session: DocumentSession? = nil, onClose: (() -> Void)? = nil) {
        self.model = model; self.scope = scope; self.session = session; self.onClose = onClose
        _transport = State(initialValue: model.workspace.transport)
        _transportBaseline = State(initialValue: model.workspace.transport)
        _test = State(initialValue: model.makeProxyConnectionTest())
        let configuration: ProxyDocument? = switch scope {
        case .app: model.proxyPreferences.configuration
        case .workspace: model.workspace.proxy
        case .request: session?.draft.proxy.document
        }
        let draft = ProxyFormDraft(configuration: configuration)
        _form = State(initialValue: draft)
        _baseline = State(initialValue: draft)
    }

    private var currentConfiguration: ProxyDocument? {
        switch scope {
        case .app: model.proxyPreferences.configuration
        case .workspace: model.workspace.proxy
        case .request: session?.draft.proxy.document
        }
    }
    private var changed: Bool { form != baseline }
    private var isSheet: Bool { onClose != nil }
    /// Only the workspace sheet stages transport edits; elsewhere they apply directly.
    private var stagesTransport: Bool { isSheet && scope == .workspace }
    private var transportChanged: Bool { stagesTransport && transport != transportBaseline }
    private var validation: String? {
        do { _ = try form.document(); return nil }
        catch { return error.localizedDescription }
    }
    private var effective: EffectiveProxy {
        let value = try? form.document()
        switch scope {
        case .app: return .resolve(request: nil, workspace: nil, app: value ?? .system)
        case .workspace: return .resolve(request: nil, workspace: value, app: model.proxyPreferences.configuration)
        case .request: return .resolve(request: value, workspace: model.workspace.proxy, app: model.proxyPreferences.configuration)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 5) {
                        Text("Proxy").font(.title2.weight(.semibold))
                        Text(scopeDescription).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                    Spacer(minLength: 8)
                    Text(scope.title).font(.caption.weight(.medium)).padding(.horizontal, 9).padding(.vertical, 5)
                        .background(.quaternary, in: Capsule())
                }
                VStack(alignment: .leading, spacing: 12) {
                    Picker("Mode", selection: $form.mode) {
                        if scope != .app { Text(scope == .workspace ? "Inherit from app default" : "Inherit from workspace").tag(ProxyFormDraft.Mode.inherit) }
                        Text("System").tag(ProxyFormDraft.Mode.system)
                        Text("Direct — No proxy").tag(ProxyFormDraft.Mode.direct)
                        Text("Manual").tag(ProxyFormDraft.Mode.manual)
                    }.accessibilityLabel("Proxy mode")
                    Text(modeDescription).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    if form.mode == .manual {
                        Divider()
                        ForEach($form.routes) { $route in
                            ProxyRouteFields(route: $route, canRemove: form.routes.count > 1) {
                                form.routes.removeAll { $0.id == route.id }
                            }
                            if route.id != form.routes.last?.id { Divider().padding(.vertical, 4) }
                        }
                        if form.routes.count < 2 {
                            Button("Add separate route", systemImage: "plus") {
                                if form.routes.first?.destination == "all" { form.routes[0].destination = "http" }
                                var route = ProxyRouteDraft(); route.destination = "https"
                                form.routes.append(route)
                            }.font(.caption)
                        }
                    }
                }.padding(16).frame(maxWidth: .infinity, alignment: .leading).background(.background, in: RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.quaternary))

                if let validation {
                    Label(validation, systemImage: "exclamationmark.circle").font(.callout).foregroundStyle(WireboltTheme.danger)
                        .fixedSize(horizontal: false, vertical: true)
                } else {
                    VStack(alignment: .leading, spacing: 9) {
                        Label(changed ? "Connection preview" : "Effective connection", systemImage: "network")
                            .font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                        Text(effective.summary(for: session?.draft.url)).font(.system(.body, design: .monospaced).weight(.medium))
                            .textSelection(.enabled).lineLimit(2)
                            .id(effective.summary(for: session?.draft.url))
                        Text(sourceDescription).font(.caption).foregroundStyle(.secondary)
                        if scope != .app {
                            HStack(spacing: 14) {
                                if scope == .request {
                                    Button("Edit workspace settings") { model.isShowingWorkspaceSettings = true }
                                }
                                Button("Edit app default") { model.settingsTab = "network"; openSettings() }
                            }.buttonStyle(.link).font(.caption)
                        }
                    }.padding(16).frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color.accentColor.opacity(0.055), in: RoundedRectangle(cornerRadius: 10))
                }
                if let saveError { Label(saveError, systemImage: "exclamationmark.circle").font(.caption).foregroundStyle(WireboltTheme.danger) }
                if let session, session.kind == .webSocket, session.socket.status != .disconnected {
                    Label("Reconnect to apply changes to this WebSocket.", systemImage: "arrow.triangle.2.circlepath")
                        .font(.caption).foregroundStyle(.secondary)
                }
                if scope == .request {
                    Text("Applies to the next send. Save the request (⌘S) to keep its override.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Divider()
                DisclosureGroup("Test connection", isExpanded: $showsTest) {
                    VStack(alignment: .leading, spacing: 10) {
                        TextField("https://example.com", text: $testURL).accessibilityLabel("Proxy test URL").textFieldStyle(.roundedBorder)
                        Text("Send a HEAD request to this URL without request headers, body or cookies. Timeout: 5 seconds.")
                            .font(.caption).foregroundStyle(.secondary)
                        if !form.secrets.isEmpty {
                            Text("Save or apply credentials before testing.").font(.caption).foregroundStyle(.secondary)
                        }
                        HStack {
                            if test.isRunning {
                                ProgressView().controlSize(.small)
                                Text("Connecting…").font(.caption)
                                Spacer()
                                Button("Cancel test") { test.cancel() }
                            } else {
                                Button("Test connection") { test.start(configuration: effective.configuration, url: testURL) }
                                    .disabled(validation != nil || !form.secrets.isEmpty || !ProxyConnectionTest.validTarget(testURL))
                            }
                        }
                        if let message = test.message {
                            Label(message, systemImage: test.succeeded ? "checkmark.circle" : "info.circle")
                                .font(.caption).foregroundStyle(test.succeeded ? WireboltTheme.success : Color.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }.padding(.top, 10)
                }.font(.callout.weight(.medium))
                if scope != .app {
                    Divider()
                    DisclosureGroup("Timeouts, redirects & TLS", isExpanded: $showsTransport) {
                        TransportSettingsFields(model: model, session: session, staged: stagesTransport ? $transport : nil).padding(.top, 12)
                    }.font(.callout.weight(.medium))
                }
            }.padding(20).frame(maxWidth: 670, alignment: .leading).frame(maxWidth: .infinity)
                .background(NetworkScrollStyle())
        }
        Divider()
        if isSheet { sheetFooter } else { inlineFooter }
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onChange(of: currentConfiguration) {
            if !changed { reload() }
        }
        .onChange(of: form) { saveError = nil; status = nil; test.reset() }
        .onChange(of: effective.configuration) { test.reset() }
        .onChange(of: testURL) { test.reset() }
        .onDisappear { test.cancel() }
        .alert("Discard changes?", isPresented: $isConfirmingDiscard) {
            Button("Discard Changes", role: .destructive) { onClose?() }
            Button("Keep Editing", role: .cancel) {}
        } message: {
            Text("Your network settings changes haven’t been saved.")
        }
    }

    private var inlineFooter: some View {
        HStack {
            if saving { ProgressView().controlSize(.small) }
            else if changed { Text("Unapplied changes").font(.caption).foregroundStyle(.secondary) }
            else if let status { Label(status, systemImage: "checkmark.circle.fill").font(.caption).foregroundStyle(.secondary) }
            Spacer()
            if changed { Button("Reset") { form = baseline; saveError = nil } }
            Button(scope == .request ? "Apply to request" : "Save") { Task { await save() } }
                .buttonStyle(.borderedProminent).disabled(!changed || validation != nil || saving)
        }
        .padding(.horizontal, WireboltTheme.Spacing.xxLarge).padding(.vertical, WireboltTheme.Spacing.large)
    }

    private var sheetFooter: some View {
        HStack {
            if saving { ProgressView().controlSize(.small) }
            Spacer()
            Button("Cancel") {
                if changed || transportChanged { isConfirmingDiscard = true } else { onClose?() }
            }
            .keyboardShortcut(.cancelAction)
            Button("Save") { Task { await saveAndClose() } }
                .keyboardShortcut(.defaultAction)
                .disabled(validation != nil || saving)
        }
        .controlSize(.regular)
        .padding(.horizontal, WireboltTheme.Spacing.xxLarge).padding(.vertical, WireboltTheme.Spacing.large)
    }

    private func saveAndClose() async {
        if changed {
            await save()
            guard saveError == nil else { return }
        }
        if transportChanged { model.updateWorkspaceTransport(transport) }
        onClose?()
    }

    private var scopeDescription: String {
        switch scope {
        case .app: "HTTP and WebSocket defaults for workspaces that inherit. Stored only on this Mac."
        case .workspace: "Default for requests in this workspace. Shared in wirebolt.toml; credentials stay in Keychain."
        case .request: "Override the connection for this request, or inherit your workspace’s configuration."
        }
    }
    private var modeDescription: String {
        switch form.mode {
        case .inherit: "Follows the parent configuration automatically."
        case .system: "Use macOS proxy settings, regardless of the parent configuration."
        case .direct: "Connect directly, even if the parent uses a proxy."
        case .manual: "Route through an explicit proxy. A failed proxy connection will not fall back to direct."
        }
    }
    private var sourceDescription: String {
        "Source: \(effective.source.title)" + (scope == .request && form.mode == .inherit && model.workspace.proxy == nil ? " · inherited through workspace" : "")
    }
    private func reload() {
        let value = ProxyFormDraft(configuration: currentConfiguration)
        form = value; baseline = value
    }
    private func save() async {
        saving = true; saveError = nil
        defer { saving = false }
        do {
            try await model.applyProxy(form.document(), scope: scope, session: session, secrets: form.secrets)
            reload()
            status = scope == .request ? "Applied to request" : "Saved"
        } catch let error as ProxyValidationError { saveError = error.message }
        catch { saveError = "Could not save. Check workspace and Keychain access, then retry." }
    }
}

private struct ProxyRouteFields: View {
    @Binding var route: ProxyRouteDraft
    let canRemove: Bool
    let remove: () -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Picker("Destination", selection: $route.destination) {
                    Text("All traffic").tag("all")
                    Text("HTTP only").tag("http")
                    Text("HTTPS only").tag("https")
                }
                if canRemove { Button("Remove route", systemImage: "minus.circle", action: remove).labelStyle(.iconOnly) }
            }
            HStack(alignment: .top, spacing: 10) {
                VStack(alignment: .leading, spacing: 5) {
                    Text("Protocol").font(.caption).foregroundStyle(.secondary)
                    Picker("Proxy protocol", selection: $route.scheme) {
                        ForEach(ProxyRouteDraft.protocols, id: \.self) { Text($0.uppercased()).tag($0) }
                    }.labelsHidden().frame(width: 100)
                }
                VStack(alignment: .leading, spacing: 5) {
                    Text("Host").font(.caption).foregroundStyle(.secondary)
                    TextField("localhost", text: $route.host).accessibilityLabel("Proxy host")
                }
                VStack(alignment: .leading, spacing: 5) {
                    Text("Port").font(.caption).foregroundStyle(.secondary)
                    TextField("8080", text: $route.port).accessibilityLabel("Proxy port").frame(width: 65)
                }
            }.textFieldStyle(.roundedBorder)
            Toggle("Proxy authentication", isOn: $route.authenticated)
            if route.authenticated {
                if route.credentials != nil && !route.replaceCredentials {
                    HStack {
                        Label("Authentication uses Keychain", systemImage: "lock.fill").font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        Button("Replace…") { route.replaceCredentials = true }.font(.caption)
                    }
                } else {
                    TextField("Username", text: $route.username).accessibilityLabel("Proxy username")
                    SecureField("Password", text: $route.password).accessibilityLabel("Proxy password")
                    Label("Stored in Keychain, never in workspace files.", systemImage: "lock")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }
}

struct ProxyConnectionIndicator: View {
    @Bindable var model: WireboltModel
    let session: DocumentSession
    let openRequestSettings: () -> Void
    @State private var showsDetails = false
    @Environment(\.openSettings) private var openSettings
    private var effective: EffectiveProxy { model.effectiveProxy(for: session.draft) }
    private var label: String {
        if (try? effective.configuration.validate()) == nil { return "Error" }
        return switch effective.configuration {
        case .system: "System"
        case .direct: "Direct"
        case .manual: effective.summary(for: session.draft.url).hasPrefix("Direct") ? "Direct" : "Proxy"
        }
    }
    var body: some View {
        Button { showsDetails.toggle() } label: {
            Label(label, systemImage: "network").font(.system(size: 10, weight: .medium)).lineLimit(1)
                .frame(width: 68, height: 24)
                .background(.quaternary.opacity(0.4), in: Capsule())
        }.buttonStyle(.plain).accessibilityLabel("Proxy connection: \(label)")
            .help(effective.summary(for: session.draft.url) + " · " + effective.source.title)
            .popover(isPresented: $showsDetails) {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Effective connection").font(.headline)
                    Text(effective.summary(for: session.draft.url)).font(.system(.body, design: .monospaced))
                    Text("Source: \(effective.source.title)").foregroundStyle(.secondary)
                    Divider()
                    Button("Request settings") { showsDetails = false; openRequestSettings() }
                    Button("Workspace settings") { showsDetails = false; model.isShowingWorkspaceSettings = true }
                    Button("App default") { showsDetails = false; model.settingsTab = "network"; openSettings() }
                    if let last = session.preparedRun?.proxy {
                        Divider()
                        Text("Last execution").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                        Text(last.summary(for: session.preparedRun?.url)).font(.caption)
                        Text("Source: \(last.source.title)").font(.caption).foregroundStyle(.secondary)
                    }
                }.buttonStyle(.link).padding(18).frame(minWidth: 300, alignment: .leading)
            }
    }
}

struct TransportSettingsFields: View {
    @Bindable var model: WireboltModel
    var session: DocumentSession?
    /// Edits a staged copy instead of the request or workspace.
    var staged: Binding<TransportSettings>?
    private var inherited: Bool { session?.draft.inheritsWorkspaceTransport == true }
    private var transport: Binding<TransportSettings> {
        if let staged { return staged }
        return Binding(get: { session.map { $0.draft.inheritsWorkspaceTransport ? model.workspace.transport : $0.draft.transport } ?? model.workspace.transport },
                set: { value in
                    if let session { session.draft.transport = value }
                    else { model.updateWorkspaceTransport(value) }
                })
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let session {
                Toggle("Inherit workspace transport settings", isOn: Binding(
                    get: { session.draft.inheritsWorkspaceTransport },
                    set: { value in
                        if !value { session.draft.transport = model.workspace.transport }
                        session.draft.inheritsWorkspaceTransport = value
                    }))
                if inherited {
                    Text("Using workspace defaults. Turn off inheritance to edit only this request.").font(.caption).foregroundStyle(.secondary)
                    Button("Edit workspace settings") { model.isShowingWorkspaceSettings = true }
                }
            }
            Group {
                Toggle("Validate TLS certificates", isOn: transport.validateTLS)
                Toggle("Follow redirects", isOn: transport.followRedirects)
                Stepper("Maximum redirects: \(transport.wrappedValue.maximumRedirects)", value: transport.maximumRedirects, in: 0...10)
                    .disabled(!transport.wrappedValue.followRedirects)
                LabeledContent("Total timeout (ms)") { TextField("0 = no limit", value: transport.totalTimeoutMS, format: .number).frame(width: 110) }
                LabeledContent("Read timeout (ms)") { TextField("0 = no limit", value: transport.readTimeoutMS, format: .number).frame(width: 110) }
                Text("A timeout of 0 disables that deadline.").font(.caption).foregroundStyle(.secondary)
            }.disabled(inherited)
        }.font(.callout)
    }
}

/// Match the editor’s slim scrollbar while retaining native scrolling and hit targets.
private struct NetworkScrollStyle: NSViewRepresentable {
    func makeNSView(context: Context) -> Probe { Probe() }
    func updateNSView(_ view: Probe, context: Context) { view.configure() }
    final class Probe: NSView {
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); configure() }
        func configure() {
            guard let scroll = enclosingScrollView, !(scroll.verticalScroller is CodeScroller) else { return }
            scroll.verticalScroller = CodeScroller()
            scroll.scrollerStyle = .legacy
        }
    }
}

struct WireboltSettingsView: View {
    @Bindable var model: WireboltModel
    @AppStorage("interfaceAppearance") private var interfaceAppearance = "system"
    @AppStorage("editor.fontSize") private var fontSize = 12.0
    @State private var showingPrivacyPolicy = false

    var body: some View {
        TabView(selection: $model.settingsTab) {
            Tab("General", systemImage: "gearshape", value: "general") {
                Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 13) {
                    GridRow(alignment: .top) {
                        Text("Font Size:").gridColumnAlignment(.trailing)
                        VStack(alignment: .leading, spacing: 6) {
                            Picker("Font Size", selection: $fontSize) {
                                ForEach(Array(10...20) + [24, 28], id: \.self) { size in
                                    Text(String(size)).tag(Double(size))
                                }
                            }.labelsHidden().frame(width: 60)
                            Text("Applies only to the Body tab.").font(WireboltTheme.Typography.detail).foregroundStyle(.secondary)
                        }
                    }
                    GridRow {
                        Text("App Theme:")
                        Picker("App Theme", selection: $interfaceAppearance) {
                            Text("System").tag("system")
                            Text("Light").tag("light")
                            Text("Dark").tag("dark")
                        }.labelsHidden().frame(width: 80)
                    }
                }
                .font(WireboltTheme.Typography.body).controlSize(.regular)
                .padding(WireboltTheme.Spacing.xxLarge)
                // The Settings window sizes itself to each tab's content.
                .frame(width: 440, alignment: .top)
                .fixedSize(horizontal: false, vertical: true)
            }
            Tab("Network", systemImage: "network", value: "network") {
                NetworkSettingsPage(model: model, scope: .app)
                    .frame(width: 600, height: 540)
            }
            Tab("Privacy", systemImage: "hand.raised", value: "privacy") {
                Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 18) {
                    GridRow(alignment: .top) {
                        Text("Analytics:").frame(width: 104, alignment: .trailing)
                        VStack(alignment: .leading, spacing: 6) {
                            Label("Not collected", systemImage: "checkmark.shield")
                                .frame(height: 18, alignment: .top).padding(.leading, 2)
                            Text("Wirebolt does not collect usage analytics or send identifiers automatically. Your collections, requests and environments stay in your workspace.")
                                .font(.system(size: 11)).foregroundStyle(.secondary)
                                .frame(width: 384, height: 44, alignment: .topLeading).padding(.leading, 2)
                        }
                    }
                    GridRow(alignment: .top) {
                        Text("Crash Report:").frame(width: 104, alignment: .trailing)
                        VStack(alignment: .leading, spacing: 6) {
                            Label("Not uploaded automatically", systemImage: "checkmark.shield")
                                .frame(height: 18, alignment: .top).padding(.leading, 2)
                            Text("Wirebolt does not upload crash reports or diagnostics automatically. You decide what information to include when reporting a problem.")
                                .font(.system(size: 11)).foregroundStyle(.secondary)
                                .frame(width: 384, height: 44, alignment: .topLeading).padding(.leading, 2)
                            Text("Your data stays under your control.")
                                .font(.system(size: 13)).foregroundStyle(.secondary).padding(.leading, 2)
                            Button { showingPrivacyPolicy = true } label: {
                                Text("Privacy Policy").frame(width: 86, height: 18)
                            }
                                .padding(.top, 12).padding(.leading, 2)
                        }
                    }
                }
                .font(WireboltTheme.Typography.body).controlSize(.regular)
                .padding(WireboltTheme.Spacing.xxLarge).padding(.trailing, WireboltTheme.Spacing.xSmall)
                .frame(width: 580, alignment: .top)
                .fixedSize(horizontal: false, vertical: true)
                .sheet(isPresented: $showingPrivacyPolicy) {
                    VStack(alignment: .leading, spacing: 16) {
                        Text("Privacy Policy").font(.title2.weight(.semibold))
                        Text("Wirebolt stores your workspace on this Mac. It does not automatically send analytics, identifiers, crash reports or diagnostics.")
                        Text("Requests connect to the addresses you choose and may use your configured proxy. Sign-in and Git sync connect to their services when you use those features.")
                        Text("You control which workspace files or diagnostic details you share with others.")
                        HStack { Spacer(); Button("Done") { showingPrivacyPolicy = false }.keyboardShortcut(.defaultAction) }
                    }.padding(24).frame(width: 480)
                }
            }
        }
    }

}
