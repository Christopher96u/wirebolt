import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    @AppStorage("interfaceAppearance") private var interfaceAppearance = "system"
    @State private var workspaceSurfacePhase = 0

    var body: some View {
        workspaceSurface
    }

    private var workspaceSurface: some View {
        HStack(spacing: 0) {
            if interface.columnVisibility != .detailOnly {
                WorkspaceSidebar(
                    model: model,
                    interface: interface,
                    showsMaterial: workspaceSurfacePhase >= 1
                )
                    .frame(width: 242)
                Divider()
            }
            if workspaceSurfacePhase >= 2 {
                WorkspaceDeck(model: model, interface: interface)
            } else {
                InitialDetailPane()
            }
        }
        .navigationTitle("")
        .frame(minWidth: 900, minHeight: 520)
        .tint(WireboltTheme.primaryAccent)
        .toolbar { workspaceToolbar }
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
        .sheet(isPresented: $interface.isShowingCurlImporter) {
            CurlImportSheet(model: model)
        }
        .sheet(item: Binding(
            get: { model.importPreview },
            set: { if $0 == nil { model.cancelPendingImport() } }
        )) { preview in
            ImportPreviewSheet(preview: preview, model: model)
        }
        .sheet(item: $interface.workspaceNamePrompt) { prompt in
            WorkspaceNameEditor(prompt: prompt, model: model)
        }
        .alert("Discard Unsaved Changes?", isPresented: $interface.isShowingDirtyClose) {
            Button("Cancel", role: .cancel) {}
            Button("Discard and Close", role: .destructive) {
                interface.confirmDirtyClose(model: model)
            }
        } message: {
            Text(interface.dirtyCloseRequest?.documentTitles.joined(separator: ", ") ?? "Unsaved request")
        }
        .alert(
            "Delete \(interface.workspaceDeleteRequest?.title ?? "Item")?",
            isPresented: Binding(
                get: { interface.workspaceDeleteRequest != nil },
                set: { if $0 == false { interface.workspaceDeleteRequest = nil } }
            )
        ) {
            Button("Cancel", role: .cancel) {
                interface.workspaceDeleteRequest = nil
            }
            Button("Delete", role: .destructive) {
                interface.confirmWorkspaceDelete(model: model)
            }
        } message: {
            Text("This removes the item from the workspace. This action cannot be undone.")
        }
        .onAppear {
            interface.synchronizeSelection(model: model)
            guard workspaceSurfacePhase == 0 else { return }
            PerformanceProbe.markReady()
            Task.detached(priority: .utility) {
                _ = RustCore().status()
            }
            Task { @MainActor in
                await Task.yield()
                workspaceSurfacePhase = 1
                try? await Task.sleep(for: .milliseconds(50))
                workspaceSurfacePhase = 2
            }
        }
        .task {
            try? await Task.sleep(for: .milliseconds(150))
            PerformanceProbe.beginWorkspaceLoad()
            let persistence = await Task.detached(priority: .userInitiated) {
                try? RustWorkspacePersistence()
            }.value
            if let persistence {
                model.configurePersistence(persistence, gitCollaboration: persistence)
            }
            await model.loadWorkspace()
            PerformanceProbe.endWorkspaceLoad()
            interface.synchronizeSelection(model: model)
        }
    }

    @ToolbarContentBuilder
    private var workspaceToolbar: some ToolbarContent {
        if #available(macOS 26.0, *) {
            ToolbarItem(placement: .navigation) {
                Color.clear.frame(width: 126, height: 1)
            }
            .sharedBackgroundVisibility(.hidden)
        }
        ToolbarItem(placement: .navigation) {
            EnvironmentPopup(model: model)
                .frame(width: 185, height: 34)
        }

        ToolbarItem(placement: .primaryAction) {
            Button(
                interface.responseOrientation == .bottom
                    ? "Place Response on Right"
                    : "Place Response on Bottom",
                systemImage: interface.responseOrientation == .bottom
                    ? "rectangle.split.2x1"
                    : "rectangle.split.1x2"
            ) {
                interface.responseOrientation = interface.responseOrientation == .bottom ? .right : .bottom
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .frame(width: 30, height: 30)
            .help(interface.responseOrientation == .bottom ? "Response on Right" : "Response on Bottom")
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
            let hasAccess = url.startAccessingSecurityScopedResource()
            Task {
                await model.previewImport(url: url, format: interface.importFormat)
                if hasAccess { url.stopAccessingSecurityScopedResource() }
            }
        case .failure:
            interface.reportImportFailure()
        }
    }
}

private struct InitialDetailPane: View {
    var body: some View {
        VStack(spacing: 0) {
            Rectangle()
                .fill(Color(nsColor: .controlBackgroundColor).opacity(0.5))
                .frame(height: 30)
            Divider()
            Color(nsColor: .textBackgroundColor)
        }
        .accessibilityHidden(true)
    }
}

private struct WorkspaceSidebar: View {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let showsMaterial: Bool

    @FocusState private var filterIsFocused: Bool

    var body: some View {
        List {
            if showsMaterial {
                WorkspaceSidebarOutline(model: model, interface: interface)
            }
        }
        .listStyle(.sidebar)
        .environment(\.defaultMinListRowHeight, 16)
        .contentMargins(.top, -8, for: .scrollContent)
        .controlSize(.small)
        .scrollContentBackground(.hidden)
        .background {
            if reduceTransparency {
                Color(nsColor: .windowBackgroundColor)
            } else if showsMaterial == false {
                Color(nsColor: .windowBackgroundColor)
            } else {
                SidebarMaterialView()
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if showsMaterial {
                SidebarFooter(
                    interface: interface,
                    filterIsFocused: $filterIsFocused
                )
            }
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

private struct WorkspaceNameEditor: View {
    let prompt: WorkspaceNamePrompt
    @Bindable var model: WireboltModel

    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @FocusState private var nameIsFocused: Bool

    init(prompt: WorkspaceNamePrompt, model: WireboltModel) {
        self.prompt = prompt
        self.model = model
        _name = State(initialValue: prompt.initialName)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(prompt.title)
                .font(.headline)
            TextField("Name", text: $name)
                .textFieldStyle(.roundedBorder)
                .focused($nameIsFocused)
                .onSubmit(save)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save", action: save)
                    .keyboardShortcut(.defaultAction)
                    .disabled(trimmedName.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 380)
        .onAppear { nameIsFocused = true }
    }

    private var trimmedName: String {
        name.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func save() {
        guard trimmedName.isEmpty == false else { return }
        let value = trimmedName
        dismiss()
        Task {
            switch prompt.target {
            case let .collection(id):
                if let id {
                    await model.renameCollection(id: id, name: value)
                } else {
                    await model.createCollection(name: value)
                }
            case let .group(collectionID, id):
                if let id {
                    await model.renameGroup(collectionID: collectionID, id: id, name: value)
                } else {
                    await model.createGroup(collectionID: collectionID, name: value)
                }
            }
        }
    }
}

private struct CurlImportSheet: View {
    @Bindable var model: WireboltModel
    @Environment(\.dismiss) private var dismiss
    @State private var source = "curl "

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Import cURL")
                .font(.headline)
            TextEditor(text: $source)
                .font(.body.monospaced())
                .frame(minHeight: 180)
                .overlay { RoundedRectangle(cornerRadius: 6).stroke(WireboltTheme.separator) }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Preview") {
                    let value = source
                    dismiss()
                    Task { await model.previewImport(source: value, format: .curl) }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(source.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("curl ") == false)
            }
        }
        .padding(20)
        .frame(width: 620, height: 300)
    }
}

private struct ImportPreviewSheet: View {
    let preview: ImportPreview
    @Bindable var model: WireboltModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Import \(preview.collectionName)")
                .font(.headline)
            LabeledContent("Requests", value: preview.requestCount.formatted())
            LabeledContent("Folders", value: preview.groupCount.formatted())
            if preview.warnings.isEmpty == false {
                GroupBox("Warnings") {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 6) {
                            ForEach(preview.warnings, id: \.self) { warning in
                                Label(warning, systemImage: "exclamationmark.triangle")
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) {
                    model.cancelPendingImport()
                    dismiss()
                }
                .keyboardShortcut(.cancelAction)
                Button("Import") {
                    dismiss()
                    Task { await model.commitPendingImport() }
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 460, height: preview.warnings.isEmpty ? 210 : 320)
    }
}

private struct CollectionActionMenu: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        Menu {
            Button("New Collection") { interface.promptForNewCollection() }
            Menu("New Request") {
                Button("HTTP") { interface.makeNewRequest(model: model) }
                    .keyboardShortcut("n", modifiers: [.command, .shift])
                Button("WebSocket") { interface.makeNewRequest(model: model, kind: .webSocket) }
            }
            Button("New Folder") {
                guard let collectionID = model.workspace.collections.first?.id else { return }
                interface.promptForNewGroup(collectionID: collectionID)
            }
                .keyboardShortcut("n", modifiers: [.command, .option])
                .disabled(model.workspace.collections.isEmpty)
            Menu("Import") {
                Button("cURL…") { interface.isShowingCurlImporter = true }
                Button("HAR…") {
                    interface.importFormat = .har
                    interface.isShowingImporter = true
                }
                Button("Legacy Workspace v1…") {
                    interface.importFormat = .legacyWorkspaceV1
                    interface.isShowingImporter = true
                }
                Button("Postman Collection v2…") {
                    interface.importFormat = .postmanV2
                    interface.isShowingImporter = true
                }
            }
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

private struct WorkspaceSidebarOutline: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        ForEach(visibleCollections) { collection in
            SavedCollectionDisclosure(
                collection: collection,
                selectedID: model.selectedRequestID,
                model: model,
                interface: interface,
                onSelect: { interface.activateSavedRequest($0, model: model) },
                onSplit: { location in
                    interface.activateSavedRequest(location, model: model)
                    if let tabID = model.sessions.activeSession?.id {
                        interface.openInNewSplit(tabID: tabID, model: model)
                    }
                }
            )
        }
    }

    private var visibleCollections: [CollectionDraft] {
        let rawQuery = interface.sidebarFilter.trimmingCharacters(in: .whitespacesAndNewlines)
        guard rawQuery.isEmpty == false else { return model.workspace.collections }
        let query = model.normalizedSearchQuery(rawQuery)
        return model.workspace.collections.compactMap { collection in
            let requests = collection.requests.filter {
                model.requestMatches($0, normalizedQuery: query)
            }
            guard model.normalizedSearchQuery(collection.name).contains(query) || requests.isEmpty == false else {
                return nil
            }
            return CollectionDraft(
                id: collection.id,
                name: collection.name,
                order: collection.order,
                groups: collection.groups,
                requests: requests
            )
        }
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

private struct SavedCollectionDisclosure: View {
    let collection: CollectionDraft
    let selectedID: String?
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let onSelect: (RequestLocation) -> Void
    let onSplit: (RequestLocation) -> Void

    @State private var isExpanded = true

    var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            ForEach(rootGroups) { group in
                SavedGroupDisclosure(
                    collection: collection,
                    group: group,
                    selectedID: selectedID,
                    model: model,
                    interface: interface,
                    onSelect: onSelect,
                    onSplit: onSplit
                )
            }
            ForEach(rootRequests) { location in requestRow(location) }
        } label: {
            Label(collection.name, systemImage: "folder")
                .contextMenu {
                    Button("New HTTP Request") {
                        interface.makeNewRequest(model: model, collectionID: collection.id)
                    }
                    Button("New WebSocket Request") {
                        interface.makeNewRequest(
                            model: model,
                            kind: .webSocket,
                            collectionID: collection.id
                        )
                    }
                    Button("New Folder") {
                        interface.promptForNewGroup(collectionID: collection.id)
                    }
                    Divider()
                    Button("Export Collection…") {
                        Task {
                            if let document = await model.exportCollection(id: collection.id) {
                                saveExportedDocument(named: collection.name, content: document)
                            }
                        }
                    }
                    Button("Rename…") { interface.promptForCollectionRename(collection) }
                    Button("Delete", role: .destructive) {
                        interface.requestDelete(
                            .collection(id: collection.id),
                            title: collection.name
                        )
                    }
                }
        }
        .dropDestination(for: String.self) { identifiers, _ in
            handleDrop(identifiers.first, parentID: nil)
        }
    }

    private var rootGroups: [GroupDraft] {
        collection.groups
            .filter { $0.parentID == nil }
            .sorted { ($0.order, $0.name) < ($1.order, $1.name) }
    }

    private var rootRequests: [RequestLocation] {
        collection.requests
            .filter { $0.groupID == nil }
            .sorted { ($0.order, $0.request.name) < ($1.order, $1.request.name) }
    }

    private func requestRow(_ location: RequestLocation) -> some View {
        SidebarRequestButton(
            location: location,
            isSelected: selectedID == location.id,
            action: { onSelect(location) },
            onSplit: { onSplit(location) },
            onDuplicate: {
                Task {
                    await model.duplicateRequest(
                        collectionID: collection.id,
                        requestID: location.request.id
                    )
                }
            },
            onExport: {
                Task {
                    if let document = await model.exportRequest(
                        collectionID: collection.id,
                        id: location.request.id
                    ) {
                        saveExportedDocument(named: location.request.name, content: document)
                    }
                }
            },
            onDelete: {
                interface.requestDelete(
                    .request(collectionID: collection.id, id: location.request.id),
                    title: location.request.name
                )
            }
        )
    }

    private func handleDrop(_ identifier: String?, parentID: String?) -> Bool {
        guard let identifier else { return false }
        let parts = identifier.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3 else { return false }
        switch parts[0] {
        case "request":
            Task {
                await model.moveRequest(
                    fromCollectionID: parts[1],
                    requestID: parts[2],
                    toCollectionID: collection.id,
                    groupID: parentID,
                    order: rootRequests.count
                )
            }
        case "group" where parts[1] == collection.id:
            Task {
                await model.moveGroup(
                    collectionID: collection.id,
                    id: parts[2],
                    parentID: parentID,
                    order: rootGroups.count
                )
            }
        default:
            return false
        }
        return true
    }
}

private struct SavedGroupDisclosure: View {
    let collection: CollectionDraft
    let group: GroupDraft
    let selectedID: String?
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let onSelect: (RequestLocation) -> Void
    let onSplit: (RequestLocation) -> Void

    @State private var isExpanded = true

    var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            ForEach(childGroups) { child in
                SavedGroupDisclosure(
                    collection: collection,
                    group: child,
                    selectedID: selectedID,
                    model: model,
                    interface: interface,
                    onSelect: onSelect,
                    onSplit: onSplit
                )
            }
            ForEach(requests) { location in
                SidebarRequestButton(
                    location: location,
                    isSelected: selectedID == location.id,
                    action: { onSelect(location) },
                    onSplit: { onSplit(location) },
                    onDuplicate: {
                        Task {
                            await model.duplicateRequest(
                                collectionID: collection.id,
                                requestID: location.request.id
                            )
                        }
                    },
                    onExport: {
                        Task {
                            if let document = await model.exportRequest(
                                collectionID: collection.id,
                                id: location.request.id
                            ) {
                                saveExportedDocument(named: location.request.name, content: document)
                            }
                        }
                    },
                    onDelete: {
                        interface.requestDelete(
                            .request(collectionID: collection.id, id: location.request.id),
                            title: location.request.name
                        )
                    }
                )
            }
        } label: {
            SidebarFolderLabel(group.name)
                .draggable("group|\(collection.id)|\(group.id)")
                .contextMenu {
                    Button("New HTTP Request") {
                        interface.makeNewRequest(
                            model: model,
                            collectionID: collection.id,
                            groupID: group.id
                        )
                    }
                    Button("New Folder") {
                        Task {
                            await model.createGroup(
                                collectionID: collection.id,
                                parentID: group.id,
                                name: "New Folder"
                            )
                        }
                    }
                    Divider()
                    Button("Rename…") {
                        interface.promptForGroupRename(collectionID: collection.id, group: group)
                    }
                    Button("Delete", role: .destructive) {
                        interface.requestDelete(
                            .group(collectionID: collection.id, id: group.id),
                            title: group.name
                        )
                    }
                }
        }
        .dropDestination(for: String.self) { identifiers, _ in
            handleDrop(identifiers.first)
        }
    }

    private var childGroups: [GroupDraft] {
        collection.groups
            .filter { $0.parentID == group.id }
            .sorted { ($0.order, $0.name) < ($1.order, $1.name) }
    }

    private var requests: [RequestLocation] {
        collection.requests
            .filter { $0.groupID == group.id }
            .sorted { ($0.order, $0.request.name) < ($1.order, $1.request.name) }
    }

    private func handleDrop(_ identifier: String?) -> Bool {
        guard let identifier else { return false }
        let parts = identifier.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3 else { return false }
        switch parts[0] {
        case "request":
            Task {
                await model.moveRequest(
                    fromCollectionID: parts[1],
                    requestID: parts[2],
                    toCollectionID: collection.id,
                    groupID: group.id,
                    order: requests.count
                )
            }
        case "group" where parts[1] == collection.id && parts[2] != group.id:
            Task {
                await model.moveGroup(
                    collectionID: collection.id,
                    id: parts[2],
                    parentID: group.id,
                    order: childGroups.count
                )
            }
        default:
            return false
        }
        return true
    }
}

private struct SidebarRequestButton: View {
    @Environment(\.colorScheme) private var colorScheme

    let location: RequestLocation
    let isSelected: Bool
    let action: () -> Void
    let onSplit: () -> Void
    let onDuplicate: () -> Void
    let onExport: () -> Void
    let onDelete: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 7) {
                Text(location.request.method.rawValue)
                    .font(.caption2.monospaced().weight(.semibold))
                    .foregroundStyle(WireboltTheme.methodColor(location.request.method))
                    .frame(width: 46, alignment: .trailing)
                Text(location.request.name)
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
            Button("Open in New Split", action: onSplit)
            Divider()
            Button("Duplicate", action: onDuplicate)
            Button("Export Request…", action: onExport)
            Button("Delete", role: .destructive, action: onDelete)
        }
        .draggable("request|\(location.collectionID)|\(location.request.id)")
        .accessibilityLabel("\(location.request.method.rawValue) request, \(location.request.name)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    private var selectionBackground: Color {
        guard isSelected else { return .clear }
        return WireboltTheme.primaryAccent.opacity(colorScheme == .dark ? 0.82 : 0.90)
    }
}

@MainActor
private func saveExportedDocument(named name: String, content: String) {
    let panel = NSSavePanel()
    panel.nameFieldStringValue = "\(name).json"
    panel.allowedContentTypes = [.json]
    panel.canCreateDirectories = true
    guard panel.runModal() == .OK, let destination = panel.url else { return }
    do {
        try Data(content.utf8).write(to: destination, options: .atomic)
        NSWorkspace.shared.activateFileViewerSelecting([destination])
    } catch {
        NSSound.beep()
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

    var body: some View {
        Group {
            if model.sessions.groups.count > 1 {
                HSplitView {
                    ForEach(model.sessions.groups) { group in
                        EditorGroupDeck(
                            model: model,
                            interface: interface,
                            groupID: group.id
                        )
                        .frame(minWidth: 430)
                    }
                }
            } else if let groupID = model.sessions.groups.first?.id {
                EditorGroupDeck(model: model, interface: interface, groupID: groupID)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(WireboltTheme.paneBackground)
    }
}

private struct EditorGroupDeck: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let groupID: String

    var body: some View {
        VStack(spacing: 0) {
            DocumentTabBar(model: model, interface: interface, groupID: groupID)
            if let session {
                ResponseSplit(orientation: interface.responseOrientation) {
                    RequestWorkspace(
                        model: model,
                        interface: interface,
                        session: session,
                        groupID: groupID
                    )
                } response: {
                    if session.kind == .http {
                        ResponseViewer(
                            interface: interface,
                            session: session
                        )
                    } else {
                        LightweightPlaceholder(
                            title: "No Connection",
                            systemImage: "bolt.horizontal.circle",
                            description: "The WebSocket engine will arrive in its networking milestone."
                        )
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                LightweightPlaceholder(
                    title: "No Open Request",
                    systemImage: "doc",
                    description: "Create or open a request from the sidebar."
                )
            }
        }
    }

    private var session: DocumentSession? {
        guard let group = model.sessions.groups.first(where: { $0.id == groupID }),
              let selectedTabID = group.selectedTabID
        else { return nil }
        return model.sessions.session(id: selectedTabID)
    }
}

private struct DocumentTabBar: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let groupID: String
    @State private var isShowingSettings = false

    var body: some View {
        HStack(spacing: 0) {
            Button("Back", systemImage: "chevron.left", action: goBack)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.tertiary)
                .frame(width: 30)
                .disabled(group?.backwardTabIDs.isEmpty != false)
            Button("Forward", systemImage: "chevron.right", action: goForward)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.tertiary)
                .frame(width: 30)
                .disabled(group?.forwardTabIDs.isEmpty != false)

            HStack(spacing: 2) {
                ForEach(tabs) { tab in
                    DocumentTabButton(
                        tab: tab,
                        isSelected: group?.selectedTabID == tab.id,
                        onSelect: { interface.activateTab(id: tab.id, groupID: groupID, model: model) },
                        onClose: {
                            interface.activateTab(id: tab.id, groupID: groupID, model: model)
                            interface.closeTab(id: tab.id, model: model)
                        },
                        onCloseOthers: {
                            interface.activateTab(id: tab.id, groupID: groupID, model: model)
                            interface.close(.others(tab.id), model: model)
                        },
                        onCloseRight: {
                            interface.activateTab(id: tab.id, groupID: groupID, model: model)
                            interface.close(.rightOf(tab.id), model: model)
                        },
                        onCloseAll: {
                            interface.activateTab(id: tab.id, groupID: groupID, model: model)
                            interface.close(.all, model: model)
                        }
                    )
                        .frame(minWidth: 120, maxWidth: .infinity)
                }
            }
            .frame(maxWidth: .infinity)

            Button("Request Settings", systemImage: "sidebar.right") {
                isShowingSettings = true
            }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 30)
                .disabled(group?.selectedTabID == nil)
        }
        .frame(height: 30)
        .background(WireboltTheme.barBackground)
        .overlay(alignment: .bottom) { Divider() }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Request tabs")
        .sheet(isPresented: $isShowingSettings) {
            if let selected = group?.selectedTabID,
               let session = model.sessions.session(id: selected)
            {
                TransportSettingsEditor(model: model, session: session)
            }
        }
    }

    private var group: EditorGroup? {
        model.sessions.groups.first(where: { $0.id == groupID })
    }

    private var tabs: [DocumentSession] {
        (group?.tabIDs ?? []).compactMap { model.sessions.session(id: $0) }
    }

    private func goBack() {
        guard let selected = group?.selectedTabID else { return }
        model.sessions.select(tabID: selected, in: groupID)
        model.sessions.goBack()
        interface.synchronizeSelection(model: model)
    }

    private func goForward() {
        guard let selected = group?.selectedTabID else { return }
        model.sessions.select(tabID: selected, in: groupID)
        model.sessions.goForward()
        interface.synchronizeSelection(model: model)
    }
}

private struct TransportSettingsEditor: View {
    @Bindable var model: WireboltModel
    @Bindable var session: DocumentSession
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Toggle("Use workspace defaults", isOn: $session.draft.inheritsWorkspaceTransport)
                Text(session.draft.inheritsWorkspaceTransport ? "Editing Workspace Defaults" : "Editing Request Overrides")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                Toggle("Validate TLS certificates", isOn: transport.validateTLS)
                Toggle("Follow redirects", isOn: transport.followRedirects)
                Stepper(
                    "Maximum redirects: \(transport.wrappedValue.maximumRedirects)",
                    value: transport.maximumRedirects,
                    in: 0 ... 10
                )
                .disabled(!transport.wrappedValue.followRedirects)
                LabeledContent("Total timeout") {
                    TextField(
                        "Milliseconds",
                        value: transport.totalTimeoutMS,
                        format: .number
                    )
                    .frame(width: 120)
                    Text("ms")
                }
                LabeledContent("Read timeout") {
                    TextField(
                        "Milliseconds",
                        value: transport.readTimeoutMS,
                        format: .number
                    )
                    .frame(width: 120)
                    Text("ms")
                }
                TextField("Client certificate secret", text: Binding(
                    get: { transport.wrappedValue.clientCertificateReference ?? "" },
                    set: { transport.wrappedValue.clientCertificateReference = $0.isEmpty ? nil : $0 }
                ))
                Button("Import Client Identity PEM…", action: importClientIdentity)
                    .disabled(transport.wrappedValue.clientCertificateReference?.isEmpty != false)
                LabeledContent("Custom CA bundle") {
                    HStack {
                        TextField("System trust store", text: Binding(
                            get: { transport.wrappedValue.customCAPath ?? "" },
                            set: { transport.wrappedValue.customCAPath = $0.isEmpty ? nil : $0 }
                        ))
                        Button("Choose…", action: chooseCustomCA)
                    }
                }
                Text("Client identities are PEM bundles read only from Keychain; private keys are never written to the workspace.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text("A timeout of 0 disables that deadline. Redirects are capped at 10.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .formStyle(.grouped)
            Divider()
            HStack {
                Spacer()
                Button("Done") {
                    Task { await model.saveWorkspaceTransport() }
                    dismiss()
                }
                    .keyboardShortcut(.defaultAction)
            }
            .padding(12)
        }
        .frame(width: 520, height: 500)
    }

    private var transport: Binding<TransportSettings> {
        Binding(
            get: {
                session.draft.inheritsWorkspaceTransport
                    ? model.workspace.transport
                    : session.draft.transport
            },
            set: { value in
                if session.draft.inheritsWorkspaceTransport {
                    model.workspace.transport = value
                } else {
                    session.draft.transport = value
                }
            }
        )
    }

    private func chooseCustomCA() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [.data]
        if panel.runModal() == .OK {
            transport.wrappedValue.customCAPath = panel.url?.path
        }
    }

    private func importClientIdentity() {
        guard let reference = transport.wrappedValue.clientCertificateReference,
              reference.isEmpty == false
        else { return }
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [.data]
        guard panel.runModal() == .OK, let url = panel.url,
              let material = try? String(contentsOf: url, encoding: .utf8)
        else { return }
        Task { await model.saveSecret(name: reference, value: material) }
    }
}

private struct DocumentTabButton: View {
    let tab: DocumentSession
    let isSelected: Bool
    let onSelect: () -> Void
    let onClose: () -> Void
    let onCloseOthers: () -> Void
    let onCloseRight: () -> Void
    let onCloseAll: () -> Void

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
        .contextMenu {
            Button("Close Tab", action: onClose)
            Button("Close Other Tabs", action: onCloseOthers)
            Button("Close Tabs to Right", action: onCloseRight)
            Divider()
            Button("Close All Tabs", action: onCloseAll)
        }
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

private struct RequestWorkspace: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    @Bindable var session: DocumentSession
    let groupID: String

    var body: some View {
        if session.kind == .webSocket {
            WebSocketRequestWorkspace(model: model, interface: interface, session: session)
        } else {
            VStack(spacing: 0) {
                RequestURLBar(model: model, interface: interface, session: session, groupID: groupID)
                Divider()
                RequestSectionBar(interface: interface, session: session)
                Divider()
                requestContent
            }
            .background(WireboltTheme.paneBackground)
        }
    }

    @ViewBuilder
    private var requestContent: some View {
        switch interface.requestSection {
        case .params:
            FieldEditor(
                title: "Query Params",
                fields: $session.draft.query,
                kind: .query
            )
        case .headers:
            FieldEditor(
                title: "Header List",
                fields: $session.draft.headers,
                kind: .header
            )
        case .auth:
            AuthenticationEditor(model: model, session: session, authentication: $session.draft.authentication)
        case .body:
            BodyEditor(requestBody: $session.draft.body)
        case .note:
            BodyTextEditor(text: $session.note)
        }
    }
}

private enum WebSocketMessageKind: String, CaseIterable, Identifiable {
    case text = "Text"
    case json = "JSON"
    case binary = "Binary"
    case file = "File"

    var id: Self { self }
}

private struct WebSocketRequestWorkspace: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    @Bindable var session: DocumentSession

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 7) {
                Text("WS")
                    .font(.callout.monospaced().weight(.bold))
                    .foregroundStyle(WireboltTheme.primaryAccent)
                TextField("Enter ws:// or wss:// URL", text: $session.draft.url)
                    .textFieldStyle(.plain)
                    .font(.callout.monospaced())
                Button("CONNECT") {}
                    .buttonStyle(WorkspaceActionButtonStyle(color: WireboltTheme.primaryAccent))
                    .disabled(true)
                    .help("WebSocket networking is not installed yet")
            }
            .padding(.horizontal, 10)
            .frame(height: 44)
            .background(WireboltTheme.barBackground)
            Divider()

            HStack(spacing: 18) {
                ForEach(RequestPanelSection.allCases) { section in
                    PanelTabButton(
                        title: section == .body ? "Message" : section.rawValue,
                        badge: badge(for: section),
                        isSelected: interface.requestSection == section,
                        action: { interface.requestSection = section }
                    )
                }
                Spacer()
            }
            .padding(.horizontal, 11)
            .frame(height: 34)
            .background(WireboltTheme.barBackground)
            Divider()

            content
        }
        .background(WireboltTheme.paneBackground)
    }

    @ViewBuilder
    private var content: some View {
        switch interface.requestSection {
        case .body:
            VStack(spacing: 0) {
                Picker("Message type", selection: messageKind) {
                    ForEach(WebSocketMessageKind.allCases) { kind in
                        Text(kind.rawValue).tag(kind)
                    }
                }
                .labelsHidden()
                .frame(width: 110)
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                Divider()
                if messageKind.wrappedValue == .file {
                    LightweightPlaceholder(
                        title: "No File Selected",
                        systemImage: "doc",
                        description: "File messages will activate with the WebSocket engine."
                    )
                } else {
                    BodyTextEditor(text: messageText)
                }
            }
        case .params:
            FieldEditor(title: "Query Params", fields: $session.draft.query, kind: .query)
        case .headers:
            FieldEditor(title: "Header List", fields: $session.draft.headers, kind: .header)
        case .auth:
            AuthenticationEditor(model: model, session: session, authentication: $session.draft.authentication)
        case .note:
            BodyTextEditor(text: $session.note)
        }
    }

    private var messageKind: Binding<WebSocketMessageKind> {
        Binding(
            get: {
                switch session.draft.body {
                case .json: .json
                case let .text(contentType, _):
                    switch contentType {
                    case "application/octet-stream": .binary
                    case "application/x-wirebolt-file": .file
                    default: .text
                    }
                case .file: .file
                case .xml, .html, .raw, .multipart, .empty, .formURLEncoded: .text
                }
            },
            set: { kind in
                session.draft.body = switch kind {
                case .text: .text(contentType: "text/plain", value: "")
                case .json: .json(value: "{\n  \n}")
                case .binary: .text(contentType: "application/octet-stream", value: "")
                case .file: .text(contentType: "application/x-wirebolt-file", value: "")
                }
            }
        )
    }

    private var messageText: Binding<String> {
        Binding(
            get: {
                switch session.draft.body {
                case let .json(value), let .text(_, value), let .xml(value),
                     let .html(value), let .raw(_, value): value
                case .empty, .formURLEncoded, .multipart, .file: ""
                }
            },
            set: { value in
                switch messageKind.wrappedValue {
                case .json: session.draft.body = .json(value: value)
                case .binary: session.draft.body = .text(contentType: "application/octet-stream", value: value)
                case .file: session.draft.body = .text(contentType: "application/x-wirebolt-file", value: value)
                case .text: session.draft.body = .text(contentType: "text/plain", value: value)
                }
            }
        )
    }

    private func badge(for section: RequestPanelSection) -> Int? {
        switch section {
        case .params: session.draft.query.filter(\.enabled).count
        case .headers: session.draft.headers.filter(\.enabled).count
        case .body, .auth, .note: nil
        }
    }
}

private struct RequestURLBar: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    @Bindable var session: DocumentSession
    let groupID: String

    @FocusState private var urlIsFocused: Bool
    @State private var isEnteringCustomMethod = false
    @State private var customMethod = ""
    @State private var isEditingLongURL = false
    @State private var isSynchronizingQuery = false
    @State private var isShowingHistory = false

    var body: some View {
        HStack(spacing: 7) {
            Menu {
                ForEach(HTTPMethod.allCases, id: \.self) { method in
                    Button(method.rawValue) { session.draft.method = method }
                }
                Divider()
                Button("CUSTOM…") {
                    customMethod = HTTPMethod.allCases.contains(session.draft.method)
                        ? ""
                        : session.draft.method.rawValue
                    isEnteringCustomMethod = true
                }
            } label: {
                Text(session.draft.method.rawValue)
                    .font(.callout.monospaced().weight(.bold))
                    .foregroundStyle(WireboltTheme.requestBarMethodColor(session.draft.method))
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .accessibilityLabel("HTTP method, \(session.draft.method.rawValue)")

            TextField("Enter URL (Focus: ⌘L  |  Send: ⌘↩)", text: $session.draft.url)
                .textFieldStyle(.plain)
                .font(.callout.monospaced())
                .focused($urlIsFocused)
                .onSubmit(send)
                .accessibilityLabel("Request URL")

            InlineResponseStatus(session: session)

            Button("Edit Long URL", systemImage: "curlybraces.square") {
                isEditingLongURL = true
            }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 30)
                .help("Edit Long URL")

            Button("Request History", systemImage: "clock.arrow.circlepath") {
                Task {
                    await model.loadHistory(for: session)
                    isShowingHistory = true
                }
            }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 30)
                .help("Request History")

            if session.isRunning {
                Button("CANCEL", systemImage: "stop.fill", action: cancel)
                    .buttonStyle(WorkspaceActionButtonStyle(color: .red))
            } else {
                Button("SEND ⌘↩", action: send)
                .buttonStyle(WorkspaceActionButtonStyle(color: WireboltTheme.primaryAccent))
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(session.draft.url.isEmpty)
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
        .onChange(of: session.draft.url) { synchronizeQueryFromURL() }
        .onChange(of: session.draft.query) { synchronizeURLFromQuery() }
        .sheet(isPresented: $isEditingLongURL) {
            LongURLEditor(url: $session.draft.url)
        }
        .sheet(isPresented: $isShowingHistory) {
            RequestHistorySheet(model: model, session: session)
        }
        .alert("Custom HTTP Method", isPresented: $isEnteringCustomMethod) {
            TextField("METHOD", text: $customMethod)
            Button("Cancel", role: .cancel) {}
            Button("Set") {
                if let method = HTTPMethod(rawValue: customMethod) {
                    session.draft.method = method
                }
            }
        } message: {
            Text("Use an RFC token such as PROPFIND or REPORT.")
        }
    }

    private func send() {
        guard session.draft.url.isEmpty == false else { return }
        model.sessions.select(tabID: session.id, in: groupID)
        interface.synchronizeSelection(model: model)
        Task { await model.send() }
    }

    private func cancel() {
        model.sessions.select(tabID: session.id, in: groupID)
        model.cancel()
    }

    private func synchronizeQueryFromURL() {
        guard isSynchronizingQuery == false,
              var components = URLComponents(string: session.draft.url),
              let items = components.queryItems
        else { return }
        components.query = nil
        isSynchronizingQuery = true
        let existing = Dictionary(
            session.draft.query.enumerated().map { ("\($0.element.name)\u{0}\($0.offset)", $0.element.id) },
            uniquingKeysWith: { first, _ in first }
        )
        session.draft.query = items.enumerated().map { index, item in
            RequestField(
                id: existing["\(item.name)\u{0}\(index)"] ?? UUID().uuidString.lowercased(),
                name: item.name,
                value: .literal(item.value ?? ""),
                enabled: true
            )
        }
        isSynchronizingQuery = false
    }

    private func synchronizeURLFromQuery() {
        guard isSynchronizingQuery == false,
              var components = URLComponents(string: session.draft.url)
        else { return }
        isSynchronizingQuery = true
        components.queryItems = session.draft.query.filter(\.enabled).map {
            URLQueryItem(name: $0.name, value: $0.value.editableValue)
        }
        if let value = components.string { session.draft.url = value }
        isSynchronizingQuery = false
    }
}

private struct RequestHistorySheet: View {
    @Bindable var model: WireboltModel
    @Bindable var session: DocumentSession
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("History — \(session.title)")
                    .font(.headline)
                Spacer()
                Button("Clear", role: .destructive) {
                    Task { await model.clearHistory(for: session) }
                }
                .disabled(model.historyEntries.isEmpty)
            }
            .padding(12)
            Divider()
            if model.historyEntries.isEmpty {
                LightweightPlaceholder(
                    title: "No History",
                    systemImage: "clock",
                    description: "Completed runs for this request appear here."
                )
            } else {
                List(model.historyEntries) { entry in
                    HStack {
                        VStack(alignment: .leading, spacing: 3) {
                            Text("\(entry.prepared.method) \(entry.prepared.url)")
                                .lineLimit(1)
                            Text(entry.createdAt.formatted())
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        if let status = entry.responseHead?.status {
                            Text(status.formatted())
                                .foregroundStyle(WireboltTheme.statusColor(status))
                        } else if let failure = entry.failure {
                            Text(failure.kind.replacingOccurrences(of: "_", with: " ").capitalized)
                                .foregroundStyle(.red)
                        }
                        Button("Restore") {
                            Task {
                                await model.restoreHistory(entry, into: session)
                                dismiss()
                            }
                        }
                    }
                }
            }
            Divider()
            HStack {
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
            .padding(12)
        }
        .frame(width: 720, height: 430)
    }
}

private struct LongURLEditor: View {
    @Binding var url: String
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Edit Long URL")
                .font(.headline)
            TextEditor(text: $url)
                .font(.body.monospaced())
                .frame(minHeight: 150)
                .overlay { RoundedRectangle(cornerRadius: 6).stroke(WireboltTheme.separator) }
            HStack {
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 700, height: 250)
    }
}

private struct InlineResponseStatus: View {
    @Bindable var session: DocumentSession

    var body: some View {
        if session.isRunning {
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
        session.responseHead?.status
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

private struct WorkspaceActionButtonStyle: ButtonStyle {
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
    @Bindable var interface: WorkspaceUIState
    @Bindable var session: DocumentSession
    @State private var isShowingBulkEditor = false

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
                case .params: session.draft.query.append(RequestField())
                case .headers: session.draft.headers.append(RequestField())
                case .body, .auth, .note: break
                }
            }
            .labelStyle(.iconOnly)
            .buttonStyle(.borderless)
            .foregroundStyle(.secondary)
            .help("Add Field")

            Menu("Section Actions", systemImage: "ellipsis.circle") {
                Button("Enable All") {
                    updateAll(enabled: true)
                }
                Button("Disable All") { updateAll(enabled: false) }
                Button("Bulk Edit…") { isShowingBulkEditor = true }
                    .disabled(fieldsBinding == nil)
                Divider()
                Button("Clear", role: .destructive, action: clearFields)
                    .disabled(fieldsBinding?.wrappedValue.isEmpty != false)
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
        .sheet(isPresented: $isShowingBulkEditor) {
            if let fieldsBinding {
                BulkFieldEditor(fields: fieldsBinding)
            }
        }
    }

    private func badge(for section: RequestPanelSection) -> Int? {
        switch section {
        case .params: session.draft.query.filter(\.enabled).count
        case .headers: session.draft.headers.filter(\.enabled).count
        case .auth, .body, .note: nil
        }
    }

    private var fieldsBinding: Binding<[RequestField]>? {
        switch interface.requestSection {
        case .params: $session.draft.query
        case .headers: $session.draft.headers
        case .body, .auth, .note: nil
        }
    }

    private func updateAll(enabled: Bool) {
        guard var fields = fieldsBinding?.wrappedValue else { return }
        for index in fields.indices { fields[index].enabled = enabled }
        fieldsBinding?.wrappedValue = fields
    }

    private func clearFields() {
        fieldsBinding?.wrappedValue = []
    }
}

private struct BulkFieldEditor: View {
    @Binding var fields: [RequestField]
    @Environment(\.dismiss) private var dismiss
    @State private var text: String

    init(fields: Binding<[RequestField]>) {
        _fields = fields
        _text = State(initialValue: fields.wrappedValue.map {
            "\($0.enabled ? "" : "# ")\($0.name): \($0.value.editableValue)"
        }.joined(separator: "\n"))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Bulk Edit")
                .font(.headline)
            Text("One `Key: Value` per line. Prefix a line with `#` to disable it.")
                .font(.caption)
                .foregroundStyle(.secondary)
            TextEditor(text: $text)
                .font(.body.monospaced())
                .overlay { RoundedRectangle(cornerRadius: 6).stroke(WireboltTheme.separator) }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                Button("Apply") {
                    fields = parse(text)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 620, height: 360)
    }

    private func parse(_ source: String) -> [RequestField] {
        source.split(separator: "\n", omittingEmptySubsequences: true).compactMap { line in
            var value = String(line)
            let enabled = value.hasPrefix("#") == false
            if enabled == false { value.removeFirst(); value = value.trimmingCharacters(in: .whitespaces) }
            guard let separator = value.firstIndex(of: ":") else { return nil }
            return RequestField(
                name: String(value[..<separator]).trimmingCharacters(in: .whitespaces),
                value: .literal(String(value[value.index(after: separator)...]).trimmingCharacters(in: .whitespaces)),
                enabled: enabled
            )
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
                .fill(isSelected ? WireboltTheme.primaryAccent : .clear)
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
                        field.enabled ? WireboltTheme.primaryAccent : Color.clear
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
                                isEnabled ? WireboltTheme.primaryAccent : Color.clear
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
    @Bindable var model: WireboltModel
    @Bindable var session: DocumentSession
    @Binding var authentication: RequestAuthentication
    @State private var clientSecretMaterial = ""

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
            case let .oauth2(configuration):
                Picker("Grant", selection: oauthBinding(\.grant)) {
                    Text("Authorization Code + PKCE").tag(OAuth2Grant.authorizationCodePKCE)
                    Text("Client Credentials").tag(OAuth2Grant.clientCredentials)
                }
                if configuration.grant == .authorizationCodePKCE {
                    TextField("Authorization URL", text: oauthBinding(\.authorizationURL))
                    TextField("Redirect URI", text: oauthBinding(\.redirectURI))
                } else {
                    TextField("Client Secret Reference", text: oauthBinding(\.clientSecretReference))
                    SecureField("Client Secret (Keychain only)", text: $clientSecretMaterial)
                    Button("Store Client Secret in Keychain") {
                        guard clientSecretMaterial.isEmpty == false else { return }
                        let material = clientSecretMaterial
                        clientSecretMaterial = ""
                        Task {
                            await model.saveSecret(
                                name: configuration.clientSecretReference,
                                value: material
                            )
                        }
                    }
                    .disabled(clientSecretMaterial.isEmpty || configuration.clientSecretReference.isEmpty)
                }
                TextField("Token URL", text: oauthBinding(\.tokenURL))
                TextField("Client ID", text: oauthBinding(\.clientID))
                TextField("Scopes", text: oauthBinding(\.scopes))
                TextField("Audience", text: oauthBinding(\.audience))
                TextField("Access Token Reference", text: oauthBinding(\.accessTokenReference))
                LabeledContent("Token") {
                    HStack {
                        if model.isOAuthBusy {
                            ProgressView().controlSize(.small)
                        } else if let receipt = model.oauthReceipts[session.id] {
                            Label(
                                receipt.expiresAt.map { "Valid until \($0.formatted(date: .omitted, time: .shortened))" }
                                    ?? "Stored in Keychain",
                                systemImage: "checkmark.circle.fill"
                            )
                            .foregroundStyle(.green)
                        }
                        Button("Get New Access Token") {
                            Task { await model.acquireOAuthToken(for: session) }
                        }
                        .disabled(model.isOAuthBusy)
                    }
                }
                if let message = model.oauthFailureMessage {
                    Text(message).foregroundStyle(.red)
                }
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
                case .oauth2: .oauth2
                }
            },
            set: {
                authentication = switch $0 {
                case .none: .none
                case .basic: .basic(username: .literal(""), password: .secret("auth.password"))
                case .bearer: .bearer(token: .secret("auth.token"))
                case .apiKey: .apiKey(placement: .header, name: "X-API-Key", value: .secret("auth.api-key"))
                case .oauth2: .oauth2(configuration: OAuth2Configuration())
                }
            }
        )
    }

    private func oauthBinding<Value>(
        _ keyPath: WritableKeyPath<OAuth2Configuration, Value>
    ) -> Binding<Value> {
        Binding(
            get: {
                guard case let .oauth2(configuration) = authentication else {
                    preconditionFailure("OAuth binding used for another authentication kind")
                }
                return configuration[keyPath: keyPath]
            },
            set: { value in
                guard case var .oauth2(configuration) = authentication else { return }
                configuration[keyPath: keyPath] = value
                authentication = .oauth2(configuration: configuration)
            }
        )
    }
}

private enum AuthenticationKind: CaseIterable, Identifiable {
    case none
    case basic
    case bearer
    case apiKey
    case oauth2

    var id: Self { self }

    var title: String {
        switch self {
        case .none: "None"
        case .basic: "Basic Auth"
        case .bearer: "Bearer Token"
        case .apiKey: "API Key"
        case .oauth2: "OAuth 2.0"
        }
    }
}

private struct BodyEditor: View {
    @Binding var requestBody: RequestBody
    @State private var wrapsLines = true

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
                    Text("UTF-8")
                        .foregroundStyle(.secondary)
                }

                Spacer()
                Button("Format", systemImage: "text.alignleft", action: formatBody)
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .help("Format Body")
                    .disabled(!requestBody.isTextual)
                Button("Wrap Lines", systemImage: "arrow.turn.down.left") {
                    wrapsLines.toggle()
                }
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .help("Wrap Lines")
                    .foregroundStyle(wrapsLines ? WireboltTheme.primaryAccent : .secondary)
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
            case let .xml(value):
                BodyTextEditor(text: Binding(
                    get: { value },
                    set: { requestBody = .xml(value: $0) }
                ))
            case let .html(value):
                BodyTextEditor(text: Binding(
                    get: { value },
                    set: { requestBody = .html(value: $0) }
                ))
            case let .raw(contentType, value):
                VStack(spacing: 0) {
                    TextField("Content-Type (optional)", text: Binding(
                        get: { contentType ?? "" },
                        set: { requestBody = .raw(contentType: $0.isEmpty ? nil : $0, value: value) }
                    ))
                    .textFieldStyle(.plain)
                    .padding(10)
                    Divider()
                    BodyTextEditor(text: Binding(
                        get: { value },
                        set: { requestBody = .raw(contentType: contentType, value: $0) }
                    ))
                }
            case let .formURLEncoded(fields):
                FieldEditor(
                    title: "Form Fields",
                    fields: Binding(
                        get: { fields },
                        set: { requestBody = .formURLEncoded(fields: $0) }
                    ),
                    kind: .query
                )
            case let .multipart(parts):
                MultipartEditor(parts: Binding(
                    get: { parts },
                    set: { requestBody = .multipart(parts: $0) }
                ))
            case let .file(path, contentType):
                FileBodyEditor(
                    path: path,
                    contentType: contentType,
                    update: { requestBody = .file(path: $0, contentType: $1) }
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
                case .xml: .xml
                case .html: .html
                case .raw: .raw
                case .formURLEncoded: .form
                case .multipart: .multipart
                case .file: .file
                }
            },
            set: {
                requestBody = switch $0 {
                case .empty: .empty
                case .text: .text(contentType: nil, value: "")
                case .json: .json(value: "{\n  \n}")
                case .xml: .xml(value: "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root></root>")
                case .html: .html(value: "<!doctype html>\n<html>\n</html>")
                case .raw: .raw(contentType: nil, value: "")
                case .form: .formURLEncoded(fields: [])
                case .multipart: .multipart(parts: [])
                case .file: .file(path: "", contentType: nil)
                }
            }
        )
    }

    private func formatBody() {
        switch requestBody {
        case let .json(value):
            guard let data = value.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: data),
                  let formatted = try? JSONSerialization.data(
                      withJSONObject: object,
                      options: [.prettyPrinted, .sortedKeys]
                  )
            else { return }
            requestBody = .json(value: String(decoding: formatted, as: UTF8.self))
        case let .xml(value):
            requestBody = .xml(value: value.replacingOccurrences(of: "><", with: ">\n<"))
        case let .html(value):
            requestBody = .html(value: value.replacingOccurrences(of: "><", with: ">\n<"))
        case .empty, .text, .raw, .formURLEncoded, .multipart, .file:
            break
        }
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
    case xml
    case html
    case raw
    case form
    case multipart
    case file

    var id: Self { self }

    var title: String {
        switch self {
        case .empty: "None"
        case .json: "JSON"
        case .text: "Text"
        case .xml: "XML"
        case .html: "HTML"
        case .raw: "Raw"
        case .form: "Form"
        case .multipart: "Multipart"
        case .file: "File"
        }
    }
}

private extension RequestBody {
    var isTextual: Bool {
        switch self {
        case .json, .text, .xml, .html, .raw: true
        case .empty, .formURLEncoded, .multipart, .file: false
        }
    }
}

private struct MultipartEditor: View {
    @Binding var parts: [MultipartPart]

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Multipart Parts")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Add Part", systemImage: "plus") {
                    parts.append(MultipartPart())
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 10)
            .frame(height: 32)
            Divider()
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach($parts) { $part in
                        HStack(spacing: 7) {
                            Toggle("Enabled", isOn: $part.enabled)
                                .labelsHidden()
                            TextField("Name", text: $part.name)
                                .frame(width: 140)
                            Picker("Type", selection: $part.kind) {
                                Text("Text").tag(MultipartPartKind.text)
                                Text("File").tag(MultipartPartKind.file)
                            }
                            .labelsHidden()
                            .frame(width: 78)
                            if part.kind == .text {
                                TextField("Value", text: Binding(
                                    get: { part.value.editableValue },
                                    set: { part.value = .literal($0) }
                                ))
                            } else {
                                TextField("File", text: Binding(
                                    get: { part.filePath ?? "" },
                                    set: { part.filePath = $0.isEmpty ? nil : $0 }
                                ))
                                Button("Choose…") {
                                    if let path = chooseFile() { part.filePath = path }
                                }
                            }
                            Button("Remove", systemImage: "trash") {
                                parts.removeAll { $0.id == part.id }
                            }
                            .labelStyle(.iconOnly)
                            .buttonStyle(.borderless)
                        }
                        .padding(.horizontal, 10)
                        .frame(height: 34)
                        .overlay(alignment: .bottom) { Divider() }
                    }
                }
            }
        }
    }

    private func chooseFile() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        return panel.runModal() == .OK ? panel.url?.path : nil
    }
}

private struct FileBodyEditor: View {
    let path: String
    let contentType: String?
    let update: (String, String?) -> Void

    var body: some View {
        Form {
            LabeledContent("File") {
                HStack {
                    TextField("Choose a file", text: Binding(
                        get: { path },
                        set: { update($0, contentType) }
                    ))
                    Button("Choose…", action: choose)
                }
            }
            TextField("Content-Type", text: Binding(
                get: { contentType ?? "" },
                set: { update(path, $0.isEmpty ? nil : $0) }
            ))
            Text("The file path is passed to Rust; file contents never cross the Swift bridge.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .formStyle(.grouped)
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let selected = panel.url {
            update(selected.path, contentType)
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
        case let .json(value), let .text(_, value), let .xml(value),
             let .html(value), let .raw(_, value): value
        case .empty, .formURLEncoded, .multipart, .file: ""
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

private struct EnvironmentPopup: View {
    @Bindable var model: WireboltModel
    @State private var editingEnvironment: EnvironmentDraft?

    var body: some View {
        Menu {
            Button {
                model.selectedEnvironmentID = nil
            } label: {
                environmentLabel("Global Environment", selected: model.selectedEnvironmentID == nil)
            }
            ForEach(model.workspace.environments.sorted(by: { $0.name < $1.name })) { environment in
                Button {
                    model.selectedEnvironmentID = environment.id
                } label: {
                    environmentLabel(
                        environment.name,
                        selected: model.selectedEnvironmentID == environment.id
                    )
                }
            }
            Divider()
            Button("New Environment…", systemImage: "plus") {
                editingEnvironment = model.makeNewEnvironment()
            }
            if let selectedEnvironment {
                Button("Edit \(selectedEnvironment.name)…", systemImage: "slider.horizontal.3") {
                    editingEnvironment = selectedEnvironment
                }
                Button("Delete \(selectedEnvironment.name)", systemImage: "trash", role: .destructive) {
                    Task { await model.deleteEnvironment(id: selectedEnvironment.id) }
                }
            }
        } label: {
            HStack(spacing: 7) {
                Text(selectedEnvironment?.name ?? "Global Environment")
                    .lineLimit(1)
                Spacer(minLength: 4)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 9)
            .frame(maxWidth: .infinity, minHeight: 28)
            .contentShape(.rect)
        }
        .menuStyle(.button)
        .accessibilityLabel("Environment")
        .accessibilityValue(selectedEnvironment?.name ?? "Global Environment")
        .sheet(item: $editingEnvironment) { environment in
            EnvironmentEditor(model: model, environment: environment)
        }
    }

    private var selectedEnvironment: EnvironmentDraft? {
        model.workspace.environments.first { $0.id == model.selectedEnvironmentID }
    }

    private func environmentLabel(_ title: String, selected: Bool) -> some View {
        Label(title, systemImage: selected ? "checkmark" : "circle.dotted")
    }
}

private struct ResponseSplit<RequestContent: View, ResponseContent: View>: View {
    let orientation: ResponseOrientation
    @ViewBuilder let request: RequestContent
    @ViewBuilder let response: ResponseContent

    init(
        orientation: ResponseOrientation,
        @ViewBuilder request: () -> RequestContent,
        @ViewBuilder response: () -> ResponseContent
    ) {
        self.orientation = orientation
        self.request = request()
        self.response = response()
    }

    var body: some View {
        if orientation == .bottom {
            VerticalResponseSplit {
                request
            } response: {
                response
            }
        } else {
            HSplitView {
                request.frame(minWidth: 300)
                response.frame(minWidth: 300)
            }
        }
    }
}

private struct VerticalResponseSplit<RequestContent: View, ResponseContent: View>: View {
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
        synchronizeRequestMenuOrder()
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
            selector: #selector(synchronizeRequestMenuOrder),
            object: nil
        )
        perform(#selector(synchronizeRequestMenuOrder), with: nil, afterDelay: 0.5)
    }

    @objc private func synchronizeRequestMenuOrder() {
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
