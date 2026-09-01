import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let status: CoreStatus

    @AppStorage("interfaceAppearance") private var interfaceAppearance = "system"

    var body: some View {
        NavigationSplitView(columnVisibility: $interface.columnVisibility) {
            WorkspaceSidebar(model: model, interface: interface)
                .navigationSplitViewColumnWidth(min: 242, ideal: 242, max: 242)
        } detail: {
            WorkspaceDeck(model: model, interface: interface, status: status)
        }
        .navigationSplitViewStyle(.prominentDetail)
        .navigationTitle("")
        .frame(minWidth: 900, minHeight: 520)
        .tint(WireboltTheme.actionBlue)
        .toolbar { getAPIToolbar }
        .toolbar(removing: .sidebarToggle)
        .preferredColorScheme(preferredColorScheme)
        .background(WindowConfigurator())
        .fileImporter(
            isPresented: $interface.isShowingImporter,
            allowedContentTypes: [.json],
            allowsMultipleSelection: false,
            onCompletion: handleImport
        )
        .fileDialogMessage("Choose a Postman Collection v2 JSON document.")
        .fileDialogConfirmationLabel("Import")
        .sheet(isPresented: $model.isShowingGitCollaboration) {
            GitCollaborationView(model: model)
        }
        .onAppear {
            if let activeTab = interface.activeTab {
                model.draft = activeTab.draft
            }
            PerformanceProbe.markReady()
        }
        .onChange(of: model.draft) { _, draft in
            interface.captureActiveDraft(draft)
        }
        .task {
            await model.loadWorkspace()
        }
    }

    @ToolbarContentBuilder
    private var getAPIToolbar: some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            EnvironmentPopup()
                .frame(width: 185, height: 34)
        }

        ToolbarItem(placement: .primaryAction) {
            Button("Open Console", systemImage: "apple.terminal") {}
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .frame(width: 30, height: 30)
            .help("Console")
        }
    }

    private var preferredColorScheme: ColorScheme? {
        switch interfaceAppearance {
        case "light": .light
        case "dark": .dark
        default: nil
        }
    }

    private func handleImport(_ result: Result<[URL], any Error>) {
        switch result {
        case let .success(urls):
            guard let url = urls.first else { return }
            interface.selectImportedFile(url)
        case .failure:
            interface.reportImportFailure()
        }
    }
}

private struct WorkspaceSidebar: View {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    @FocusState private var filterIsFocused: Bool

    var body: some View {
        List {
            GetAPISidebarOutline(model: model, interface: interface)
        }
        .listStyle(.sidebar)
        .environment(\.defaultMinListRowHeight, 16)
        .contentMargins(.top, -8, for: .scrollContent)
        .controlSize(.small)
        .scrollContentBackground(.hidden)
        .background {
            if reduceTransparency {
                Color(nsColor: .windowBackgroundColor)
            } else {
                Color.clear
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            SidebarFooter(
                interface: interface,
                filterIsFocused: $filterIsFocused
            )
        }
        .onChange(of: interface.focusSearchTrigger) {
            filterIsFocused = true
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button("Toggle Sidebar", systemImage: "sidebar.left") {
                    interface.columnVisibility = interface.columnVisibility == .detailOnly ? .all : .detailOnly
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .help(interface.columnVisibility == .detailOnly ? "Show Sidebar" : "Hide Sidebar")

                CollectionActionMenu(model: model, interface: interface)
            }
        }
        .toolbar(removing: .sidebarToggle)
    }

}

private struct CollectionActionMenu: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        Menu {
            Button("New Request") { interface.makeNewRequest(model: model) }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            Button("New Folder") {}
                .keyboardShortcut("n", modifiers: [.command, .option])
            Menu("Import") {
                Button("cURL…") {}
                    .disabled(true)
                Button("Postman Collection v2…") {
                    interface.isShowingImporter = true
                }
            }
            Divider()
            Button("Rename…") {}
                .disabled(true)
            Button("Delete", role: .destructive) {}
                .keyboardShortcut(.delete, modifiers: .command)
                .disabled(true)
        } label: {
            Image(systemName: "plus")
                .frame(width: 18, height: 18)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .foregroundStyle(.secondary)
        .tint(.secondary)
        .fixedSize()
        .help("New or Import")
        .accessibilityLabel("Collection actions")
    }
}

private struct GetAPISidebarOutline: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    @State private var rootExpanded = true
    @State private var authExpanded = true
    @State private var utilitiesExpanded = true
    @State private var methodsExpanded = true
    @State private var draftExpanded = true

    var body: some View {
        DisclosureGroup(isExpanded: $rootExpanded) {
            DisclosureGroup(isExpanded: $authExpanded) {
                requestRow(.get, "DigestAuth Request", "digest-auth", "/digest-auth")
            } label: {
                SidebarFolderLabel("Auth: Digest")
            }

            CollapsedSidebarFolder("Utilities / Date and Time")

            DisclosureGroup(isExpanded: $utilitiesExpanded) {
                requestRow(.get, "IP address in JSON format", "ip-address", "/ip")
                requestRow(.get, "Deflate Compressed Response", "deflate", "/deflate")
                requestRow(.get, "GZip Compressed Response", "gzip", "/gzip")
                requestRow(.get, "Get UTF8 Encoded Response", "utf8", "/encoding/utf8")
                requestRow(.get, "Delay Response", "delay", "/delay/2")
                requestRow(.get, "Streamed Response", "streamed", "/stream/5")
                requestRow(.get, "Get Response Status Code", "status", "/status/200")
            } label: {
                SidebarFolderLabel("Utilities")
            }

            CollapsedSidebarFolder("Cookie Manipulation")
            CollapsedSidebarFolder("Authentication Methods")
            CollapsedSidebarFolder("Headers")

            DisclosureGroup(isExpanded: $methodsExpanded) {
                requestRow(.delete, "DELETE Request", "delete-request", "/delete")
                requestRow(.patch, "PATCH Request", "patch-request", "/patch")
                requestRow(.put, "PUT Request", "put-request", "/put")
                requestRow(.post, "POST Form Data", "post-form", "/post")
                requestRow(.post, "POST Raw Text", "post-raw", "/post")
                requestRow(.get, "GET Request", "get-request", "/get?foo1=bar1&foo2=bar2")
            } label: {
                SidebarFolderLabel("Request Methods")
            }
        } label: {
            SidebarFolderLabel("Postman Echo.postman_col…")
        }

        DisclosureGroup(isExpanded: $draftExpanded) {
            requestRow(.post, "Get License", "get-license", "https://www.google.com")
            requestRow(.get, "My First API", "my-first-api", "")
        } label: {
            SidebarFolderLabel("Draft")
        }
    }

    private func requestRow(
        _ method: HTTPMethod,
        _ name: String,
        _ id: String,
        _ path: String
    ) -> some View {
        let url = path.hasPrefix("http") || path.isEmpty
            ? path
            : "https://postman-echo.com\(path)"
        let headers = id == "patch-request" ? [
            RequestField(name: "Content-Type", value: .literal("application/json")),
            RequestField(name: "X-Data", value: .literal("GET API Token")),
            RequestField(name: "Early-Data", value: .literal("123")),
        ] : []
        let query = id == "get-request" ? [
            RequestField(name: "foo1", value: .literal("bar1")),
            RequestField(name: "foo2", value: .literal("bar2")),
        ] : []
        let request = DemoSidebarRequest(
            id: id,
            draft: RequestDraft(
                id: id,
                name: name,
                method: method,
                url: url,
                query: query,
                headers: headers
            ),
            status: 200
        )
        return DemoRequestRow(
            request: request,
            isSelected: interface.activeTabID == id,
            onSelect: { interface.activateDemoRequest(request, model: model) }
        )
    }
}

private struct SidebarFolderLabel: View {
    let title: String

    init(_ title: String) {
        self.title = title
    }

    var body: some View {
        HStack(spacing: 7) {
            Image(systemName: "folder.fill")
                .foregroundStyle(.orange)
            Text(title)
                .foregroundStyle(.primary)
                .lineLimit(1)
        }
        .padding(.vertical, -2)
    }
}

private struct CollapsedSidebarFolder: View {
    let title: String

    @State private var isExpanded = false

    init(_ title: String) {
        self.title = title
    }

    var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            EmptyView()
        } label: {
            SidebarFolderLabel(title)
        }
    }
}

private struct DemoCollectionDisclosure: View {
    let collection: DemoSidebarCollection
    let selectedID: String
    let onSelect: (DemoSidebarRequest) -> Void

    @State private var isExpanded = true

    var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            ForEach(collection.requests) { request in
                DemoRequestRow(
                    request: request,
                    isSelected: selectedID == request.id,
                    onSelect: { onSelect(request) }
                )
            }
        } label: {
            Label(collection.name, systemImage: "folder")
                .font(.body)
        }
    }
}

private struct SavedCollectionDisclosure: View {
    let collection: CollectionDraft
    let selectedID: String
    let onSelect: (RequestLocation) -> Void

    @State private var isExpanded = true

    var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            ForEach(collection.requests) { location in
                SidebarRequestButton(
                    method: location.request.method,
                    name: location.request.name,
                    isSelected: selectedID == location.id,
                    action: { onSelect(location) }
                )
            }
        } label: {
            Label(collection.name, systemImage: "folder")
        }
    }
}

private struct DemoRequestRow: View {
    let request: DemoSidebarRequest
    let isSelected: Bool
    let onSelect: () -> Void

    var body: some View {
        SidebarRequestButton(
            method: request.draft.method,
            name: request.draft.name,
            isSelected: isSelected,
            action: onSelect
        )
    }
}

private struct SidebarRequestButton: View {
    @Environment(\.colorScheme) private var colorScheme

    let method: HTTPMethod
    let name: String
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 7) {
                Text(method.rawValue)
                    .font(.caption2.monospaced().weight(.semibold))
                    .foregroundStyle(WireboltTheme.methodColor(method))
                    .frame(width: 46, alignment: .trailing)
                Text(name)
                    .lineLimit(1)
                    .foregroundStyle(isSelected ? Color.white : Color.primary)
                Spacer(minLength: 0)
            }
            .padding(.vertical, 1)
            .padding(.horizontal, 6)
            .contentShape(.rect)
            .background {
                GeometryReader { geometry in
                    RoundedRectangle(cornerRadius: 5)
                        .fill(selectionBackground)
                        .frame(width: geometry.size.width + 21)
                        .frame(height: 24)
                        .offset(x: -16, y: -1)
                }
            }
        }
        .buttonStyle(.plain)
        .padding(.leading, -22)
        .contextMenu {
            Button("Open") { action() }
            Button("Open in New Tab") { action() }
            Divider()
            Button("Rename…") {}
                .disabled(true)
            Button("Delete", role: .destructive) {}
                .disabled(true)
        }
        .accessibilityLabel("\(method.rawValue) request, \(name)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    private var selectionBackground: Color {
        guard isSelected else { return .clear }
        return WireboltTheme.actionBlue.opacity(colorScheme == .dark ? 0.82 : 0.90)
    }
}

private struct ImportedCollectionDisclosure: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    @State private var rootExpanded = true
    @State private var authExpanded = true
    @State private var utilitiesExpanded = true
    @State private var methodsExpanded = true

    var body: some View {
        DisclosureGroup(isExpanded: $rootExpanded) {
            DisclosureGroup(isExpanded: $authExpanded) {
                importedRow(.get, "Digest Auth Request", "digest-auth")
            } label: {
                Label("Auth: Digest", systemImage: "folder")
            }

            DisclosureGroup(isExpanded: $utilitiesExpanded) {
                importedRow(.get, "IP address in JSON", "ip-address")
                importedRow(.get, "GZip Response", "gzip")
                importedRow(.get, "Delay Response", "delay")
            } label: {
                Label("Utilities", systemImage: "folder")
            }

            Label("Cookie Manipulation", systemImage: "folder")
            Label("Authentication Methods", systemImage: "folder")

            DisclosureGroup(isExpanded: $methodsExpanded) {
                importedRow(.delete, "Delete Request", "import-delete")
                importedRow(.patch, "Patch Request", "import-patch")
                importedRow(.put, "Put Request", "import-put")
                importedRow(.post, "Post Form Data", "import-post")
                importedRow(.get, "Get Request", "import-get")
            } label: {
                Label("Request Methods", systemImage: "folder")
            }
        } label: {
            Label("Postman Echo", systemImage: "shippingbox")
        }
    }

    private func importedRow(_ method: HTTPMethod, _ name: String, _ id: String) -> some View {
        let request = DemoSidebarRequest(
            id: id,
            draft: RequestDraft(
                id: id,
                name: name,
                method: method,
                url: "https://postman-echo.com/\(method.rawValue.lowercased())",
                query: [
                    RequestField(name: "foo", value: .literal("bar")),
                    RequestField(name: "source", value: .literal("wirebolt")),
                ]
            ),
            status: 200
        )
        return DemoRequestRow(
            request: request,
            isSelected: interface.activeTabID == id,
            onSelect: { interface.activateDemoRequest(request, model: model) }
        )
    }
}

private struct SidebarFooter: View {
    @Bindable var interface: WorkspaceUIState
    var filterIsFocused: FocusState<Bool>.Binding

    var body: some View {
        HStack(spacing: 7) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.secondary)
            TextField("Filter (⌘⇧F)", text: $interface.sidebarFilter)
                .textFieldStyle(.plain)
                .focused(filterIsFocused)
        }
        .padding(.horizontal, 9)
        .frame(height: 26)
        .background(.thinMaterial, in: .capsule)
        .overlay {
            Capsule()
                .stroke(WireboltTheme.separator, lineWidth: 0.5)
        }
        .padding(.leading, 8)
        .padding(.trailing, 4)
        .padding(.top, 11)
        .padding(.bottom, 5)
    }
}

private struct ImportStatusBanner: View {
    let message: String
    let dismiss: () -> Void

    var body: some View {
        HStack(spacing: 7) {
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.green)
            Text(message)
                .lineLimit(1)
            Spacer(minLength: 4)
            Button("Dismiss", systemImage: "xmark", action: dismiss)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
        }
        .font(.caption)
        .padding(.horizontal, 10)
        .frame(height: 34)
        .background(.regularMaterial, in: .rect(cornerRadius: 7))
        .overlay {
            RoundedRectangle(cornerRadius: 7)
                .stroke(WireboltTheme.separator, lineWidth: 0.5)
        }
        .shadow(color: .black.opacity(0.16), radius: 8, y: 3)
    }
}

private struct WorkspaceDeck: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let status: CoreStatus

    var body: some View {
        VStack(spacing: 0) {
            DocumentTabBar(model: model, interface: interface)
            GetAPIVerticalSplit {
                RequestWorkspace(model: model, interface: interface)
            } response: {
                ResponseViewer(model: model, interface: interface)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(WireboltTheme.paneBackground)
    }
}

private struct DocumentTabBar: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        HStack(spacing: 0) {
            Button("Back", systemImage: "chevron.left") {}
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.tertiary)
                .frame(width: 30)
                .disabled(true)
            Button("Forward", systemImage: "chevron.right") {}
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.tertiary)
                .frame(width: 30)
                .disabled(true)

            HStack(spacing: 2) {
                ForEach(interface.openTabs) { tab in
                    DocumentTabButton(
                        tab: tab,
                        isSelected: interface.activeTabID == tab.id,
                        onSelect: { interface.activateTab(id: tab.id, model: model) },
                        onClose: { interface.closeTab(id: tab.id, model: model) }
                    )
                        .frame(minWidth: 120, maxWidth: .infinity)
                }
            }
            .frame(maxWidth: .infinity)

            Button("Toggle Inspector", systemImage: "sidebar.right") {}
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 30)
        }
        .frame(height: 30)
        .background(WireboltTheme.barBackground)
        .overlay(alignment: .bottom) { Divider() }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Request tabs")
    }
}

private struct DocumentTabButton: View {
    let tab: WireboltDocumentTab
    let isSelected: Bool
    let onSelect: () -> Void
    let onClose: () -> Void

    @State private var isHovered = false

    var body: some View {
        HStack(spacing: 5) {
            Button(action: onSelect) {
                Text(tab.title)
                    .font(.caption)
                    .fontWeight(isSelected ? .medium : .regular)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity)
                    .contentShape(.rect)
            }
            .buttonStyle(.plain)

            Button("Close \(tab.title)", systemImage: "xmark", action: onClose)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(.secondary)
                .opacity(isHovered ? 1 : 0)
                .accessibilityHidden(!isHovered)
        }
        .padding(.horizontal, 10)
        .frame(minWidth: 180, minHeight: 24)
        .background(isSelected ? Color.primary.opacity(0.055) : .clear, in: .rect(cornerRadius: 7))
        .overlay {
            RoundedRectangle(cornerRadius: 7)
                .stroke(isSelected ? WireboltTheme.separator.opacity(0.65) : .clear, lineWidth: 0.5)
        }
        .onHover { isHovered = $0 }
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

private struct RequestWorkspace: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        VStack(spacing: 0) {
            RequestURLBar(model: model, interface: interface)
            Divider()
            RequestSectionBar(model: model, interface: interface)
            Divider()
            requestContent
        }
        .background(WireboltTheme.paneBackground)
    }

    @ViewBuilder
    private var requestContent: some View {
        switch interface.requestSection {
        case .params:
            FieldEditor(
                title: "Query Params",
                fields: $model.draft.query,
                kind: .query
            )
        case .headers:
            FieldEditor(
                title: "Header List",
                fields: $model.draft.headers,
                kind: .header
            )
        case .auth:
            AuthenticationEditor(authentication: $model.draft.authentication)
        case .body:
            BodyEditor(requestBody: $model.draft.body)
        case .note:
            LightweightPlaceholder(
                title: "No Notes",
                systemImage: "note.text",
                description: "Add notes for this request."
            )
        }
    }
}

private struct RequestURLBar: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    @FocusState private var urlIsFocused: Bool

    var body: some View {
        HStack(spacing: 7) {
            Menu {
                ForEach(HTTPMethod.allCases, id: \.self) { method in
                    Button(method.rawValue) { model.draft.method = method }
                }
            } label: {
                Text(model.draft.method.rawValue)
                    .font(.callout.monospaced().weight(.bold))
                    .foregroundStyle(WireboltTheme.requestBarMethodColor(model.draft.method))
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .accessibilityLabel("HTTP method, \(model.draft.method.rawValue)")

            TextField("Enter URL (Focus: ⌘L  |  Send: ⌘↩)", text: $model.draft.url)
                .textFieldStyle(.plain)
                .font(.callout.monospaced())
                .focused($urlIsFocused)
                .onSubmit(send)
                .accessibilityLabel("Request URL")

            InlineResponseStatus(model: model, interface: interface)

            Button("URL Variables", systemImage: "curlybraces.square") {}
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 30)
                .help("Insert Environment Variable")

            Button("Request History", systemImage: "clock.arrow.circlepath") {}
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 30)
                .help("Request History")

            if model.isRunning {
                Button("CANCEL", systemImage: "stop.fill", action: model.cancel)
                    .buttonStyle(GetAPIActionButtonStyle(color: .red))
            } else {
                Button("SEND ⌘↩", action: send)
                .buttonStyle(GetAPIActionButtonStyle(color: WireboltTheme.actionBlue))
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(model.draft.url.isEmpty)
                .help("Send Request (⌘↩)")
            }
        }
        .padding(.leading, 10)
        .padding(.trailing, 11)
        .frame(height: 44)
        .background(WireboltTheme.barBackground)
        .onChange(of: interface.focusURLTrigger) {
            urlIsFocused = true
        }
    }

    private func send() {
        guard !model.draft.url.isEmpty else { return }
        Task { await model.send() }
    }
}

private struct InlineResponseStatus: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        if model.isRunning {
            ProgressView()
                .controlSize(.small)
        } else if let status {
            HStack(spacing: 6) {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(WireboltTheme.statusColor(status))
                Text(statusLabel(status))
                    .font(.system(size: 14.5, weight: .semibold).monospacedDigit())
                    .foregroundStyle(WireboltTheme.statusColor(status))
            }
            .fixedSize()
        }
    }

    private var status: UInt16? {
        model.responseHead?.status ?? interface.activeTab?.status
    }

    private func statusLabel(_ status: UInt16) -> String {
        let reason = switch status {
        case 200: "OK"
        case 201: "Created"
        case 202: "Accepted"
        case 204: "No Content"
        case 400: "Bad Request"
        case 401: "Unauthorized"
        case 403: "Forbidden"
        case 404: "Not Found"
        case 422: "Unprocessable Entity"
        case 500: "Server Error"
        default: "Response"
        }
        return "\(status) \(reason)"
    }
}

private struct GetAPIActionButtonStyle: ButtonStyle {
    let color: Color

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body.weight(.medium))
            .foregroundStyle(.white)
            .padding(.horizontal, 13)
            .frame(minWidth: 104, minHeight: 30)
            .background(color.opacity(configuration.isPressed ? 0.78 : 1), in: .capsule)
            .opacity(configuration.isPressed ? 0.9 : 1)
    }
}

private struct RequestSectionBar: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        HStack(spacing: 18) {
            ForEach(RequestPanelSection.allCases) { section in
                PanelTabButton(
                    title: section.rawValue,
                    badge: badge(for: section),
                    isSelected: interface.requestSection == section,
                    action: { interface.requestSection = section }
                )
            }
            Spacer(minLength: 0)
            Button("Add Field", systemImage: "plus") {
                switch interface.requestSection {
                case .params: model.draft.query.append(RequestField())
                case .headers: model.draft.headers.append(RequestField())
                case .body, .auth, .note: break
                }
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .foregroundStyle(.secondary)
            .help("Add Field")

            Menu("Section Actions", systemImage: "ellipsis.circle") {
                Button("Enable All") {
                    switch interface.requestSection {
                    case .params:
                        for index in model.draft.query.indices { model.draft.query[index].enabled = true }
                    case .headers:
                        for index in model.draft.headers.indices { model.draft.headers[index].enabled = true }
                    case .body, .auth, .note: break
                    }
                }
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .labelStyle(.iconOnly)
            .foregroundStyle(.secondary)
            .fixedSize()
        }
        .padding(.leading, 11)
        .padding(.trailing, 10)
        .frame(height: 34)
        .background(WireboltTheme.barBackground)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Request sections")
    }

    private func badge(for section: RequestPanelSection) -> Int? {
        switch section {
        case .params: model.draft.query.filter(\.enabled).count
        case .headers: model.draft.headers.filter(\.enabled).count
        case .auth, .body, .note: nil
        }
    }
}

struct PanelTabButton: View {
    let title: String
    var badge: Int?
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 5) {
                Text(title)
                if let badge, badge > 0 {
                    Text("\(badge)")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(WireboltTheme.success)
                }
            }
            .foregroundStyle(isSelected ? .primary : .secondary)
            .frame(height: 34)
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .overlay(alignment: .bottom) {
            Rectangle()
                .fill(isSelected ? WireboltTheme.actionBlue : .clear)
                .frame(height: 2)
        }
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

private enum FieldEditorKind {
    case query
    case header
}

private struct FieldEditor: View {
    let title: String
    @Binding var fields: [RequestField]
    let kind: FieldEditorKind

    var body: some View {
        VStack(spacing: 0) {
            FieldTableHeader()

            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach($fields) { $field in
                        FieldTableRow(
                            field: $field,
                            onRemove: { fields.removeAll { $0.id == field.id } }
                        )
                        .overlay(alignment: .bottom) { Divider() }
                    }
                    NewFieldTableRow(fields: $fields, kind: kind)
                }
            }
        }
        .background(WireboltTheme.paneBackground)
    }
}

private struct FieldTableHeader: View {
    var body: some View {
        HStack(spacing: 0) {
            Color.clear.frame(width: 27)
            Divider()
            Text("Key")
                .frame(width: 175, alignment: .leading)
                .padding(.leading, 4)
            Divider()
            Text("Value")
                .frame(minWidth: 145, maxWidth: .infinity, alignment: .leading)
                .padding(.leading, 4)
            Color.clear.frame(width: 42)
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .frame(height: 28)
        .background(WireboltTheme.barBackground)
        .overlay(alignment: .bottom) { Divider() }
    }
}

private struct FieldTableRow: View {
    @Binding var field: RequestField
    let onRemove: () -> Void

    @State private var isHovered = false

    var body: some View {
        HStack(spacing: 0) {
            Button {
                field.enabled.toggle()
            } label: {
                Image(systemName: field.enabled ? "checkmark.square.fill" : "square")
                    .symbolRenderingMode(.palette)
                    .foregroundStyle(
                        field.enabled ? Color.white : Color.secondary,
                        field.enabled ? WireboltTheme.actionBlue : Color.clear
                    )
                    .font(.system(size: 16))
            }
            .buttonStyle(.plain)
            .frame(width: 27)
            .accessibilityLabel("Enabled")
            .accessibilityValue(field.enabled ? "On" : "Off")
            Divider()
            TextField("Key", text: $field.name)
                .textFieldStyle(.plain)
                .font(.callout.monospaced())
                .frame(width: 175)
                .frame(height: 20)
                .padding(.horizontal, 4)
                .offset(y: -2)
            Divider()
            TextField("Value", text: literalBinding($field.value))
                .textFieldStyle(.plain)
                .font(.callout.monospaced())
                .frame(minWidth: 145, maxWidth: .infinity)
                .frame(height: 20)
                .padding(.horizontal, 4)
                .offset(y: -2)
            Button("Remove \(field.name)", systemImage: "trash", action: onRemove)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 42)
                .opacity(isHovered ? 1 : 0.55)
        }
        .frame(height: 32)
        .background(isHovered ? Color.primary.opacity(0.035) : .clear)
        .onHover { isHovered = $0 }
    }

    private func literalBinding(_ source: Binding<ValueSource>) -> Binding<String> {
        Binding(
            get: { source.wrappedValue.editableValue },
            set: { source.wrappedValue = .literal($0) }
        )
    }
}

private struct NewFieldTableRow: View {
    @Binding var fields: [RequestField]
    let kind: FieldEditorKind

    @State private var name = ""
    @State private var value = ""
    @State private var showingSuggestions = false
    @State private var showingValueSuggestions = false
    @State private var isEnabled = true
    @FocusState private var focusedField: NewFieldFocus?

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                if name.isEmpty {
                    Color.clear.frame(width: 27)
                } else {
                    Button {
                        isEnabled.toggle()
                    } label: {
                        Image(systemName: isEnabled ? "checkmark.square.fill" : "square")
                            .symbolRenderingMode(.palette)
                            .foregroundStyle(
                                isEnabled ? Color.white : Color.secondary,
                                isEnabled ? WireboltTheme.actionBlue : Color.clear
                            )
                            .font(.system(size: 16))
                    }
                    .buttonStyle(.plain)
                    .frame(width: 27)
                }
                Divider()
                TextField("New Key (⌘K)", text: $name)
                    .textFieldStyle(.plain)
                    .font(.callout.monospaced())
                    .focused($focusedField, equals: .key)
                    .frame(width: 175)
                    .frame(height: 20)
                    .padding(.horizontal, 4)
                    .offset(y: -2)
                    .onSubmit { focusedField = .value }
                    .popover(isPresented: $showingSuggestions, arrowEdge: .bottom) {
                        HeaderSuggestions(query: name) { suggestion in
                            name = suggestion
                            showingSuggestions = false
                            focusedField = .value
                        }
                    }
                Divider()
                TextField("New Value", text: $value)
                    .textFieldStyle(.plain)
                    .font(.callout.monospaced())
                    .focused($focusedField, equals: .value)
                    .frame(minWidth: 145, maxWidth: .infinity)
                    .frame(height: 20)
                    .padding(.horizontal, 4)
                    .offset(y: -2)
                    .onSubmit(commit)
                    .popover(isPresented: $showingValueSuggestions, arrowEdge: .bottom) {
                        HeaderValueSuggestions(query: value) { suggestion in
                            value = suggestion
                            showingValueSuggestions = false
                            commit()
                        }
                    }
                if name.isEmpty {
                    Color.clear.frame(width: 42)
                } else {
                    Button("Discard New Field", systemImage: "trash", action: discard)
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                        .foregroundStyle(.secondary)
                        .frame(width: 42)
                }
            }
            .frame(height: 32)

            if !name.isEmpty {
                Divider()
                EmptyNewFieldRow()
            }
        }
        .foregroundStyle(.secondary)
        .onChange(of: name) { updateSuggestions() }
        .onChange(of: value) { updateValueSuggestions() }
        .onChange(of: focusedField) {
            updateSuggestions()
            updateValueSuggestions()
        }
    }

    private func updateSuggestions() {
        showingSuggestions = kind == .header
            && focusedField == .key
            && !name.isEmpty
    }

    private func updateValueSuggestions() {
        showingValueSuggestions = kind == .header
            && focusedField == .value
            && name.caseInsensitiveCompare("Content-Type") == .orderedSame
            && !value.isEmpty
    }

    private func commit() {
        guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        fields.append(RequestField(name: name, value: .literal(value), enabled: isEnabled))
        discard()
        focusedField = .key
    }

    private func discard() {
        name = ""
        value = ""
        isEnabled = true
        showingSuggestions = false
        showingValueSuggestions = false
    }
}

private struct EmptyNewFieldRow: View {
    var body: some View {
        HStack(spacing: 0) {
            Color.clear.frame(width: 27)
            Divider()
            Text("New Key (⌘K)")
                .frame(width: 175, alignment: .leading)
                .padding(.horizontal, 4)
            Divider()
            Text("New Value")
                .frame(minWidth: 145, maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 4)
            Color.clear.frame(width: 42)
        }
        .font(.callout.monospaced())
        .foregroundStyle(.tertiary)
        .frame(height: 32)
    }
}

private enum NewFieldFocus: Hashable {
    case key
    case value
}

private struct HeaderSuggestions: View {
    let query: String
    let onSelect: (String) -> Void

    private let suggestions = [
        "Accept",
        "Accept-CH",
        "Accept-Charset",
        "Accept-Encoding",
        "Accept-Language",
        "Accept-Ranges",
        "Access-Control-Allow-Credentials",
        "Access-Control-Allow-Headers",
        "Access-Control-Allow-Methods",
        "Authorization",
        "Cache-Control",
        "Content-Type",
        "X-API-Key",
        "X-Correlation-ID",
        "X-Request-ID",
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(filteredSuggestions, id: \.self) { suggestion in
                Button(suggestion) { onSelect(suggestion) }
                    .buttonStyle(.plain)
                    .padding(.horizontal, 8)
                    .frame(maxWidth: .infinity, minHeight: 24, alignment: .leading)
            }
        }
        .padding(5)
        .frame(width: 280)
    }

    private var filteredSuggestions: [String] {
        let filtered = suggestions.filter { $0.localizedCaseInsensitiveContains(query) }
        return filtered.isEmpty ? suggestions : filtered
    }
}

private struct HeaderValueSuggestions: View {
    let query: String
    let onSelect: (String) -> Void

    private let suggestions = [
        "application/json",
        "application/json; charset=utf-8",
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(filteredSuggestions, id: \.self) { suggestion in
                Button(suggestion) { onSelect(suggestion) }
                    .buttonStyle(.plain)
                    .padding(.horizontal, 8)
                    .frame(maxWidth: .infinity, minHeight: 28, alignment: .leading)
            }
        }
        .padding(5)
        .frame(width: 280)
    }

    private var filteredSuggestions: [String] {
        let filtered = suggestions.filter { $0.localizedCaseInsensitiveContains(query) }
        return filtered.isEmpty ? suggestions : filtered
    }
}

private struct AuthenticationEditor: View {
    @Binding var authentication: RequestAuthentication

    var body: some View {
        Form {
            Picker("Authentication", selection: kindBinding) {
                ForEach(AuthenticationKind.allCases) { kind in
                    Text(kind.title).tag(kind)
                }
            }

            switch authentication {
            case .none:
                Text("No Authorization header will be added.")
                    .foregroundStyle(.secondary)
            case let .basic(username, password):
                TextField("Username", text: Binding(
                    get: { username.editableValue },
                    set: { newValue in
                        guard case let .basic(_, currentPassword) = authentication else { return }
                        authentication = .basic(username: .literal(newValue), password: currentPassword)
                    }
                ))
                TextField("Password Secret", text: Binding(
                    get: { password.editableValue },
                    set: { newValue in
                        guard case let .basic(currentUsername, _) = authentication else { return }
                        authentication = .basic(username: currentUsername, password: .secret(newValue))
                    }
                ))
            case let .bearer(token):
                TextField("Token Secret", text: Binding(
                    get: { token.editableValue },
                    set: { authentication = .bearer(token: .secret($0)) }
                ))
            case let .apiKey(placement, name, value):
                Picker("Placement", selection: Binding(
                    get: { placement },
                    set: { authentication = .apiKey(placement: $0, name: name, value: value) }
                )) {
                    ForEach(APIKeyPlacement.allCases, id: \.self) {
                        Text($0.rawValue.capitalized).tag($0)
                    }
                }
                TextField("Name", text: Binding(
                    get: { name },
                    set: { authentication = .apiKey(placement: placement, name: $0, value: value) }
                ))
                TextField("Value Secret", text: Binding(
                    get: { value.editableValue },
                    set: { authentication = .apiKey(placement: placement, name: name, value: .secret($0)) }
                ))
            }
        }
        .formStyle(.columns)
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
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
                case .apiKey: .apiKey(placement: .header, name: "X-API-Key", value: .secret("auth.api-key"))
                }
            }
        )
    }
}

private enum AuthenticationKind: CaseIterable, Identifiable {
    case none
    case basic
    case bearer
    case apiKey

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
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Picker("Body type", selection: kindBinding) {
                    ForEach(BodyKind.allCases) { kind in
                        Text(kind.title).tag(kind)
                    }
                }
                .labelsHidden()
                .frame(width: 92)

                if requestBody.isTextual {
                    Menu("Pretty") {
                        Button("Pretty") {}
                        Button("Minified") {}
                    }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                }

                Spacer()
                Button("Format", systemImage: "text.alignleft") {}
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .help("Format Body")
                Button("Wrap Lines", systemImage: "arrow.turn.down.left") {}
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .help("Wrap Lines")
            }
            .padding(.horizontal, 12)
            .frame(height: 43)
            .background(WireboltTheme.barBackground)
            Divider()

            switch requestBody {
            case .empty:
                LightweightPlaceholder(
                    title: "No Request Body",
                    systemImage: "doc",
                    description: "Choose JSON, Text, or Form to add a body."
                )
            case let .text(contentType, value):
                VStack(spacing: 0) {
                    TextField("Content-Type (optional)", text: Binding(
                        get: { contentType ?? "" },
                        set: { requestBody = .text(contentType: $0.isEmpty ? nil : $0, value: value) }
                    ))
                    .textFieldStyle(.plain)
                    .padding(10)
                    Divider()
                    BodyTextEditor(text: Binding(
                        get: { value },
                        set: { requestBody = .text(contentType: contentType, value: $0) }
                    ))
                }
            case let .json(value):
                BodyTextEditor(text: Binding(
                    get: { value },
                    set: { requestBody = .json(value: $0) }
                ))
            case let .formURLEncoded(fields):
                FieldEditor(
                    title: "Form Fields",
                    fields: Binding(
                        get: { fields },
                        set: { requestBody = .formURLEncoded(fields: $0) }
                    ),
                    kind: .query
                )
            }
        }
        .background(WireboltTheme.paneBackground)
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

private struct BodyTextEditor: View {
    @Binding var text: String

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            Text(lineNumbers)
                .font(.system(.body, design: .monospaced))
                .monospacedDigit()
                .foregroundStyle(.tertiary)
                .multilineTextAlignment(.trailing)
                .lineSpacing(3)
                .frame(width: 38, alignment: .trailing)
                .padding(.top, 9)
                .padding(.trailing, 8)
                .accessibilityHidden(true)

            Divider()

            TextEditor(text: $text)
                .font(.system(.body, design: .monospaced))
                .scrollContentBackground(.hidden)
                .background(WireboltTheme.paneBackground)
                .padding(.horizontal, 8)
                .accessibilityLabel("Request body")
        }
        .background(WireboltTheme.paneBackground)
    }

    private var lineNumbers: String {
        let count = max(text.components(separatedBy: .newlines).count, 1)
        return (1 ... count).map(String.init).joined(separator: "\n")
    }
}

private enum BodyKind: CaseIterable, Identifiable {
    case empty
    case json
    case text
    case form

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

private extension RequestBody {
    var isTextual: Bool {
        switch self {
        case .json, .text: true
        case .empty, .formURLEncoded: false
        }
    }
}

private struct WorkspaceStatusBar: View {
    @Bindable var model: WireboltModel
    let status: CoreStatus

    var body: some View {
        HStack {
            Text("UTF-8")
            Spacer()
            Text("\(requestLineCount) lines")
            Spacer()
            HStack(spacing: 6) {
                Image(systemName: "lock")
                Text("Local · No telemetry")
                Circle()
                    .fill(.green)
                    .frame(width: 8, height: 8)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Local mode. No telemetry.")
            Text("Core \(status.coreVersion)")
                .help("ABI \(status.streamABIVersion)")
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 12)
        .frame(height: 27)
        .background(WireboltTheme.barBackground)
        .overlay(alignment: .top) { Divider() }
    }

    private var requestLineCount: Int {
        let text = switch model.draft.body {
        case let .json(value), let .text(_, value): value
        case .empty, .formURLEncoded: ""
        }
        return max(text.components(separatedBy: .newlines).count, 1)
    }
}

struct LightweightPlaceholder: View {
    let title: String
    let systemImage: String
    var description: String?

    var body: some View {
        ContentUnavailableView(
            title,
            systemImage: systemImage,
            description: description.map(Text.init)
        )
        .accessibilityElement(children: .combine)
    }
}

private struct WindowConfigurator: NSViewRepresentable {
    func makeNSView(context _: Context) -> WindowConfigurationView {
        WindowConfigurationView()
    }

    func updateNSView(_ view: WindowConfigurationView, context _: Context) {
        view.scheduleMenuOrderMatch()
    }
}

private struct SidebarMaterialView: NSViewRepresentable {
    func makeNSView(context _: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .sidebar
        view.blendingMode = .behindWindow
        view.state = .followsWindowActiveState
        view.isEmphasized = true
        return view
    }

    func updateNSView(_ view: NSVisualEffectView, context _: Context) {
        view.state = .followsWindowActiveState
    }
}

private struct EnvironmentPopup: NSViewRepresentable {
    func makeNSView(context _: Context) -> NSPopUpButton {
        let button = NSPopUpButton(frame: .zero, pullsDown: false)
        button.controlSize = .regular
        button.font = .systemFont(ofSize: NSFont.systemFontSize)
        button.addItem(withTitle: "Global Environment")
        button.setAccessibilityLabel("Environment")
        button.setAccessibilityValue("Global Environment")
        return button
    }

    func updateNSView(_: NSPopUpButton, context _: Context) {}
}

private struct GetAPIVerticalSplit<RequestContent: View, ResponseContent: View>: View {
    @ViewBuilder let request: RequestContent
    @ViewBuilder let response: ResponseContent

    @State private var requestedHeight: CGFloat = 296
    @State private var dragStartHeight: CGFloat?

    init(
        @ViewBuilder request: () -> RequestContent,
        @ViewBuilder response: () -> ResponseContent
    ) {
        self.request = request()
        self.response = response()
    }

    var body: some View {
        GeometryReader { geometry in
            let usableHeight = max(geometry.size.height - 1, 0)
            let requestHeight = min(
                max(requestedHeight, 180),
                max(180, usableHeight - 140)
            )

            VStack(spacing: 0) {
                request
                    .frame(height: requestHeight)

                Rectangle()
                    .fill(WireboltTheme.separator)
                    .frame(height: 1)
                    .overlay {
                        Color.clear
                            .contentShape(.rect)
                            .frame(height: 7)
                            .gesture(splitDragGesture(totalHeight: usableHeight))
                            .onHover { hovering in
                                if hovering {
                                    NSCursor.resizeUpDown.set()
                                } else {
                                    NSCursor.arrow.set()
                                }
                            }
                    }

                response
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }

    private func splitDragGesture(totalHeight: CGFloat) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                let start = dragStartHeight ?? requestedHeight
                if dragStartHeight == nil { dragStartHeight = requestedHeight }
                requestedHeight = min(
                    max(start + value.translation.height, 180),
                    max(180, totalHeight - 140)
                )
            }
            .onEnded { _ in
                dragStartHeight = nil
            }
    }
}

@MainActor
private final class WindowConfigurationView: NSView {
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        window.titleVisibility = .hidden
        window.title = ""
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.toolbarStyle = .unified
        window.styleMask.insert(.fullSizeContentView)
        window.backgroundColor = .clear
        window.isOpaque = false
        window.setFrameAutosaveName("WireboltMainWindow")
        observeMainMenuChanges()
        matchGetAPIMenuOrder()
        scheduleMenuOrderMatch()
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }

    private func observeMainMenuChanges() {
        NotificationCenter.default.removeObserver(self)
        guard let menu = NSApp.mainMenu else { return }
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(mainMenuDidChange),
            name: NSMenu.didAddItemNotification,
            object: menu
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(mainMenuDidChange),
            name: NSMenu.didRemoveItemNotification,
            object: menu
        )
    }

    @objc private func mainMenuDidChange(_: Notification) {
        scheduleMenuOrderMatch()
    }

    func scheduleMenuOrderMatch() {
        NSObject.cancelPreviousPerformRequests(
            withTarget: self,
            selector: #selector(matchGetAPIMenuOrder),
            object: nil
        )
        perform(#selector(matchGetAPIMenuOrder), with: nil, afterDelay: 0.5)
    }

    @objc private func matchGetAPIMenuOrder() {
        guard let menu = NSApp.mainMenu,
              let viewItem = menu.items.first(where: { $0.title == "View" })
        else { return }

        let requestIndex = menu.indexOfItem(withTitle: "Request")
        let navigateIndex = menu.indexOfItem(withTitle: "Navigate")
        let viewIndexBeforeMove = menu.indexOfItem(withTitle: "View")
        if requestIndex >= 0,
           navigateIndex == requestIndex + 1,
           viewIndexBeforeMove == navigateIndex + 1
        {
            return
        }

        let movedItems = ["Request", "Navigate"].compactMap { title in
            menu.items.first(where: { $0.title == title })
        }
        for item in movedItems {
            menu.removeItem(item)
        }
        guard let viewIndex = menu.items.firstIndex(of: viewItem) else { return }
        for (offset, item) in movedItems.enumerated() {
            menu.insertItem(item, at: viewIndex + offset)
        }
    }
}
