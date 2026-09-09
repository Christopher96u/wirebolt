import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    @AppStorage("interfaceAppearance") private var interfaceAppearance = "system"
    @State private var workspaceSurfacePhase = 0
    @AppStorage("workspace.sidebarWidth") private var sidebarWidth = 250.0
    @State private var sidebarDragOrigin: Double?
    private var toolbarGap: Double { max(0, sidebarWidth - 184) }

    var body: some View {
        workspaceWithDialogs
    }

    private var workspacePanels: some View {
        HStack(spacing: 0) {
            if interface.columnVisibility != .detailOnly {
                WorkspaceSidebar(
                    model: model,
                    interface: interface,
                    showsMaterial: workspaceSurfacePhase >= 1
                )
                    .frame(width: sidebarWidth - 1)
                Rectangle().fill(WireboltTheme.separator).frame(width: 1)
                    .overlay {
                        Color.clear.frame(width: 7).contentShape(.rect)
                            .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .global)
                                .onChanged { value in
                                    if sidebarDragOrigin == nil { sidebarDragOrigin = sidebarWidth }
                                    sidebarWidth = min(480, max(180, (sidebarDragOrigin ?? sidebarWidth) + value.translation.width))
                                }
                                .onEnded { _ in sidebarDragOrigin = nil })
                            .onHover { inside in
                                if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
                            }
                    }
            }
            if workspaceSurfacePhase >= 2 {
                WorkspaceDeck(model: model, interface: interface)
            } else {
                InitialDetailPane()
            }
        }
    }

    private var workspaceSurface: some View {
        workspacePanels
        .navigationTitle("")
        .frame(minWidth: model.sessions.groups.count > 1
            ? (interface.columnVisibility == .detailOnly ? 0 : 208) + Double(model.sessions.groups.count * 440 + model.sessions.groups.count - 1)
            : 720, minHeight: 411)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width in
            if model.sessions.groups.count > 1 {
                let detailMinimum = Double(model.sessions.groups.count * 440 + model.sessions.groups.count - 1)
                sidebarWidth = min(sidebarWidth, max(208, width - detailMinimum))
            }
        }
        .tint(WireboltTheme.primaryAccent)
        .toolbar(id: "workspace-toolbar") { workspaceToolbar }
        .toolbar(removing: .sidebarToggle)
        .preferredColorScheme(preferredColorScheme)
        .background(WindowConfigurator(sidebarWidth: interface.columnVisibility == .detailOnly ? 0 : sidebarWidth))
        .fileImporter(
            isPresented: $interface.isShowingImporter,
            allowedContentTypes: [.json, .data],
            allowsMultipleSelection: false,
            onCompletion: handleImport
        )
        .fileDialogMessage("Choose a collection or archive to import.")
        .fileDialogConfirmationLabel("Import")
        .sheet(isPresented: $model.isShowingGitCollaboration) {
            GitCollaborationView(model: model)
        }
        .sheet(isPresented: $interface.isShowingCurlImporter) {
            CurlImportSheet(model: model)
        }
    }

    private var workspaceWithDialogs: some View {
        workspaceSurface
        .alert("Import Failed", isPresented: Binding(
            get: { model.importFailureMessage != nil },
            set: { if !$0 { model.importFailureMessage = nil } }
        )) {
            Button("OK", role: .cancel) { model.importFailureMessage = nil }
        } message: { Text(model.importFailureMessage ?? "") }
        .alert("Operation Failed", isPresented: Binding(
            get: { model.operationFailure != nil && model.importFailureMessage == nil },
            set: { if !$0 { model.operationFailure = nil } }
        )) {
            Button("OK", role: .cancel) { model.operationFailure = nil }
        } message: {
            Text(model.operationFailure?.kind == "keychain"
                ? "The credentials could not be read or saved in Keychain. Check access and try again."
                : "The operation could not be completed. Check access to the workspace and try again.")
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
            "Are you sure you want to delete the selected item?",
            isPresented: Binding(
                get: { interface.workspaceDeleteRequest != nil },
                set: { if $0 == false { interface.workspaceDeleteRequest = nil } }
            )
        ) {
            Button("Cancel", role: .cancel) {
                interface.workspaceDeleteRequest = nil
            }
            Button("Yes", role: .destructive) {
                interface.confirmWorkspaceDelete(model: model)
            }
        } message: {
            Text("This action cannot be reverted.")
        }
        .onAppear {
            interface.reopenLastDocument(model: model)
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
    private var workspaceToolbar: some CustomizableToolbarContent {
        if #available(macOS 26.0, *) {
            ToolbarItem(id: "sidebar", placement: .navigation) { sidebarToggle }
                .sharedBackgroundVisibility(.hidden)
            ToolbarItem(id: "new", placement: .navigation) { CollectionActionMenu(model: model, interface: interface).frame(width: 26, height: 30) }
                .sharedBackgroundVisibility(.hidden)
        } else {
            ToolbarItem(id: "sidebar", placement: .navigation) { sidebarToggle }
            ToolbarItem(id: "new", placement: .navigation) { CollectionActionMenu(model: model, interface: interface).frame(width: 26, height: 30) }
        }
        if #available(macOS 26.0, *) {
            ToolbarItem(id: "sidebar-spacing", placement: .navigation) {
                ToolbarSpace(width: interface.columnVisibility == .detailOnly ? 0 : toolbarGap)
            }
            .sharedBackgroundVisibility(.hidden)
        }
        ToolbarItem(id: "environment", placement: .navigation) {
            EnvironmentPopup(model: model)
                .frame(width: 185, height: 34)

        }

        ToolbarItem(id: "response-placement", placement: .primaryAction) {
            Button {
                interface.responseOrientation = interface.responseOrientation == .bottom ? .right : .bottom
            } label: {
                ResponsePlacementIcon(right: interface.responseOrientation == .right).frame(width: 17, height: 13)
            }
            .accessibilityLabel(interface.responseOrientation == .bottom ? "Place Response on Right" : "Place Response on Bottom")
            .buttonStyle(.borderless)
            .frame(width: 30, height: 30)
            .help(interface.responseOrientation == .bottom ? "Response on Right" : "Response on Bottom")
        }
    }

    @ViewBuilder
    private var sidebarToggle: some View {
        Button("Toggle Sidebar", systemImage: "sidebar.left") {
            interface.columnVisibility = interface.columnVisibility == .detailOnly ? .all : .detailOnly
        }
        .labelStyle(.iconOnly).buttonStyle(.borderless).frame(width: 26, height: 30)
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
                await model.importDocument(url: url, format: interface.importFormat)
                if hasAccess { url.stopAccessingSecurityScopedResource() }
            }
        case let .failure(error):
            if (error as NSError).code != NSUserCancelledError { interface.reportImportFailure() }
        }
    }
}

private struct ResponsePlacementIcon: View {
    let right: Bool
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 2).stroke(lineWidth: 1)
            Path { path in
                path.move(to: right ? CGPoint(x: 11, y: 0) : CGPoint(x: 0, y: 7))
                path.addLine(to: right ? CGPoint(x: 11, y: 13) : CGPoint(x: 17, y: 7))
            }.stroke(lineWidth: 1)
            ForEach(0..<3) { index in
                Circle().frame(width: 1.5, height: 1.5)
                    .position(x: right ? 14 : 4 + CGFloat(index) * 4.5, y: right ? 3 + CGFloat(index) * 3.5 : 10)
            }
        }.foregroundStyle(.secondary).accessibilityHidden(true)
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
    @FocusState private var sidebarIsFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if showsMaterial { WorkspaceSidebarOutline(model: model, interface: interface) }
            }
            .padding(.leading, 18).padding(.trailing, 9)
            .font(.system(size: 13))
            .disclosureGroupStyle(SidebarDisclosureStyle())
        }
        .scrollIndicators(.never)
        .clipped()
        .focusable().focusEffectDisabled().focused($sidebarIsFocused)
        .onChange(of: interface.focusSidebarTrigger) {
            if interface.renamingRequestID == nil { sidebarIsFocused = true }
        }
        .onMoveCommand { direction in
            guard interface.renamingRequestID == nil else { return }
            switch direction {
            case .up: interface.moveSidebarSelection(-1, model: model)
            case .down: interface.moveSidebarSelection(1, model: model)
            default: break
            }
        }
        .onKeyPress(.return) {
            guard interface.renamingRequestID == nil else { return .ignored }
            interface.renamingRequestID = model.selectedRequestID
            return .handled
        }
        .contentMargins(.top, 0, for: .scrollContent)
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
                Button("Import") {
                    Task {
                        await model.importDocument(source: source, format: .curl)
                        if model.importFailureMessage == nil { dismiss() }
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(source.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("curl ") == false)
            }
        }
        .padding(20)
        .frame(width: 620, height: 300)
    }
}

private struct CollectionActionMenu: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        Menu {
            Menu("New Request") {
                Button("HTTP") { interface.makeNewRequest(model: model) }
                    .keyboardShortcut("n", modifiers: [.command, .shift])
                Button("WebSocket") { interface.makeNewRequest(model: model, kind: .webSocket) }
            }
            Button("New Folder") {
                interface.makeNewFolder(model: model)
            }
                .keyboardShortcut("n", modifiers: [.command, .option])
            Divider()
            WorkspaceImportMenu(interface: interface)
            Button("Export") {
                Task { if let document = await model.exportWorkspace() {
                    saveExportedDocument(named: model.workspace.name, content: document)
                } }
            }.disabled(model.workspace.collections.isEmpty)

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

private struct WorkspaceImportMenu: View {
    @Bindable var interface: WorkspaceUIState
    var body: some View {
        Menu("Import") {
            Button("cURL") { interface.isShowingCurlImporter = true }
            Button("HAR") { open(.har) }
            Divider()
            Button("Legacy Collection v1") { open(.legacyWorkspaceV1) }
            Button("Postman Collection v2") { open(.postmanV2) }
        }
    }
    private func open(_ format: ImportFormat) {
        interface.importFormat = format
        interface.isShowingImporter = true
    }
}

private struct NewRequestMenu: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let collectionID: String
    var groupID: String?
    var body: some View {
        Menu("New Request") {
            Button("HTTP") { interface.makeNewRequest(model: model, collectionID: collectionID, groupID: groupID) }
            Button("WebSocket") { interface.makeNewRequest(model: model, kind: .webSocket, collectionID: collectionID, groupID: groupID) }
        }
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
                onSelect: { interface.activateSavedRequest($0, model: model); interface.focusSidebarTrigger += 1 },
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
        let collections = model.workspace.collections.sorted { ($0.order, $0.name) < ($1.order, $1.name) }
        guard rawQuery.isEmpty == false else { return collections }
        let query = model.normalizedSearchQuery(rawQuery)
        return collections.compactMap { collection in
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

private struct SidebarDisclosureStyle: DisclosureGroupStyle {
    var isEditing = false

    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                Button {
                    configuration.isExpanded.toggle()
                } label: {
                    Image(systemName: configuration.isExpanded ? "chevron.down" : "chevron.right")
                        .font(.system(size: 9, weight: .semibold)).foregroundStyle(.secondary)
                        .frame(width: 12, height: 24).padding(.trailing, 6).contentShape(.rect)
                }.buttonStyle(.plain).accessibilityLabel(configuration.isExpanded ? "Collapse" : "Expand")
                if isEditing {
                    configuration.label.font(.system(size: 13)).frame(maxWidth: .infinity, alignment: .leading)
                } else {
                    Button { configuration.isExpanded.toggle() } label: {
                        configuration.label.font(.system(size: 13))
                            .frame(maxWidth: .infinity, minHeight: 24, alignment: .leading)
                            .contentShape(.rect)
                    }.buttonStyle(.plain)
                }
            }.frame(height: 24)
            if configuration.isExpanded {
                configuration.content.padding(.leading, 14)
            }
        }
    }
}

private enum SidebarItem: Identifiable {
    case group(GroupDraft)
    case request(RequestLocation)
    var id: String {
        switch self { case let .group(group): "group:" + group.id; case let .request(request): "request:" + request.id }
    }
    var order: Int {
        switch self { case let .group(group): group.order; case let .request(request): request.order }
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
                .font(.system(size: 13)).foregroundStyle(.primary)
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

struct InlineSidebarName: View {
    let title: String
    @Binding var isEditing: Bool
    var renameOnDoubleClick = false
    let save: (String) -> Void
    @State private var value = ""
    @FocusState private var focused: Bool

    var body: some View {
        Group {
            if isEditing {
                TextField("Name", text: $value)
                    .textFieldStyle(.plain).focused($focused)
                    .onSubmit(commit)
                    .onExitCommand { isEditing = false }
                    .onChange(of: focused) { _, focused in if !focused && isEditing { commit() } }
            } else if renameOnDoubleClick {
                Text(title).lineLimit(1)
                    .contentShape(.rect)
                    .onTapGesture(count: 2) { isEditing = true }
            } else { Text(title).lineLimit(1) }
        }
        .task(id: isEditing) {
            if isEditing {
                value = title
                await Task.yield()
                if isEditing { focused = true }
            }
        }
    }

    private func commit() {
        let name = value.trimmingCharacters(in: .whitespacesAndNewlines)
        isEditing = false
        if !name.isEmpty && name != title { save(name) }
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
    @State private var isRenaming = false

    var body: some View {
        Group {
        if collection.id == WorkspaceDraft.rootCollectionID { items }
        else {
        DisclosureGroup(isExpanded: $isExpanded) {
            items

        } label: {
            HStack(spacing: 6) {
                Image(systemName: "folder.fill")
                InlineSidebarName(title: collection.name, isEditing: $isRenaming) { name in
                    Task { await model.renameCollection(id: collection.id, name: name) }
                }
            }
                .contextMenu {
                    NewRequestMenu(model: model, interface: interface, collectionID: collection.id)
                    Button("New Folder") {
                        interface.makeNewFolder(model: model, collectionID: collection.id)
                    }
                    Divider()
                    WorkspaceImportMenu(interface: interface)
                    Button("Export") {
                        Task {
                            if let document = await model.exportCollection(id: collection.id) {
                                saveExportedDocument(named: collection.name, content: document)
                            }
                        }
                    }
                    Button("Rename") { isRenaming = true }
                    Button("Delete", role: .destructive) {
                        interface.requestDelete(
                            .collection(id: collection.id),
                            title: collection.name
                        )
                    }
                }
        }
        .disclosureGroupStyle(SidebarDisclosureStyle(isEditing: isRenaming))
        }
        }
        .onChange(of: isExpanded) { _, expanded in
            if expanded { interface.collapsedSidebarCollections.remove(collection.id) }
            else { interface.collapsedSidebarCollections.insert(collection.id) }
        }
        .dropDestination(for: String.self) { identifiers, _ in
            handleDrop(identifiers.first, parentID: nil)
        }
    }

    private var items: some View {
            ForEach((rootGroups.map(SidebarItem.group) + rootRequests.map(SidebarItem.request)).sorted { $0.order < $1.order }) { item in
                switch item {
                case let .group(group):
                    SavedGroupDisclosure(collection: collection, group: group, selectedID: selectedID,
                        model: model, interface: interface, onSelect: onSelect, onSplit: onSplit)
                case let .request(location): requestRow(location)
                }
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
            model: model, interface: interface,
            location: location,
            isSelected: selectedID == location.id,
            action: { onSelect(location) },
            onSplit: { onSplit(location) },
            onRename: { name in Task { await model.renameRequest(collectionID: collection.id, requestID: location.request.id, name: name) } },
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

    @State private var isExpanded = false
    @State private var isRenaming = false

    var body: some View {
        DisclosureGroup(isExpanded: Binding(
            get: { isExpanded || !interface.sidebarFilter.isEmpty }, set: { isExpanded = $0 }
        )) {
            ForEach((childGroups.map(SidebarItem.group) + requests.map(SidebarItem.request)).sorted { $0.order < $1.order }) { item in
                switch item {
                case let .group(child):
                    SavedGroupDisclosure(collection: collection, group: child, selectedID: selectedID,
                        model: model, interface: interface, onSelect: onSelect, onSplit: onSplit)
                case let .request(location):
                SidebarRequestButton(
                    model: model, interface: interface,
                    location: location,
                    isSelected: selectedID == location.id,
                    action: { onSelect(location) },
                    onSplit: { onSplit(location) },
                    onRename: { name in Task { await model.renameRequest(collectionID: collection.id, requestID: location.request.id, name: name) } },
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
            }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "folder.fill")
                InlineSidebarName(title: group.name, isEditing: Binding(
                    get: { isRenaming || interface.renamingGroupID == group.id },
                    set: { isRenaming = $0; if !$0 && interface.renamingGroupID == group.id { interface.renamingGroupID = nil } }
                )) { name in
                    Task { await model.renameGroup(collectionID: collection.id, id: group.id, name: name) }
                }
            }
                .draggable("group|\(collection.id)|\(group.id)")
                .contextMenu {
                    NewRequestMenu(model: model, interface: interface, collectionID: collection.id, groupID: group.id)
                    Button("New Folder") {
                        isExpanded = true
                        interface.makeNewFolder(model: model, collectionID: collection.id, parentID: group.id)
                    }
                    Divider()
                    Button("Rename") { isRenaming = true }
                    Button("Delete", role: .destructive) {
                        interface.requestDelete(
                            .group(collectionID: collection.id, id: group.id),
                            title: group.name
                        )
                    }
                }
        }
        .disclosureGroupStyle(SidebarDisclosureStyle(isEditing: isRenaming || interface.renamingGroupID == group.id))
        .onChange(of: isExpanded) { _, expanded in
            let id = collection.id + ":" + group.id
            if expanded { interface.expandedSidebarGroups.insert(id) }
            else { interface.expandedSidebarGroups.remove(id) }
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
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    @Environment(\.colorScheme) private var colorScheme

    let location: RequestLocation
    let isSelected: Bool
    let action: () -> Void
    let onSplit: () -> Void
    let onRename: (String) -> Void
    let onDuplicate: () -> Void
    let onExport: () -> Void
    let onDelete: () -> Void
    @State private var isRenaming = false

    var body: some View {
            HStack(spacing: 3) {
                Text(location.request.webSocket ? "WS" : location.request.method.rawValue)
                    .font(.system(size: 10))
                    .foregroundStyle(WireboltTheme.methodColor(location.request.method))
                    .frame(width: 40, alignment: .trailing)
                InlineSidebarName(title: location.request.name, isEditing: Binding(
                    get: { isRenaming || interface.renamingRequestID == location.id },
                    set: { isRenaming = $0; if !$0 && interface.renamingRequestID == location.id { interface.renamingRequestID = nil } }
                ), renameOnDoubleClick: true, save: onRename)
                    .lineLimit(1)
                    .foregroundStyle(isSelected ? Color.white : Color.primary)
                Spacer(minLength: 0)
            }
            .font(.system(size: 13))
            .padding(.trailing, 6)
            .frame(height: 24)
            .contentShape(.rect)
            .background {
                GeometryReader { geometry in
                    let inset: CGFloat = location.collectionID == WorkspaceDraft.rootCollectionID && location.groupID == nil ? 0 : 14
                    RoundedRectangle(cornerRadius: 5)
                        .fill(selectionBackground)
                        .frame(width: geometry.size.width + inset)
                        .frame(height: 24)
                        .offset(x: -inset)
                }
            }
        .buttonStyle(.plain)
        .onTapGesture {
            if !isRenaming && interface.renamingRequestID != location.id { action() }
        }
        .accessibilityElement(children: isRenaming || interface.renamingRequestID == location.id ? .contain : .combine)
        .accessibilityAction(.default, action)
        .frame(height: 24)
        .listRowInsets(EdgeInsets(top: 0, leading: 0, bottom: 0, trailing: 0))
        .contextMenu {
            NewRequestMenu(model: model, interface: interface, collectionID: location.collectionID, groupID: location.groupID)
            Button("New Folder") { interface.makeNewFolder(model: model, collectionID: location.collectionID, parentID: location.groupID) }
            Divider()
            Button("Open in new split", action: onSplit)
            Divider()
            WorkspaceImportMenu(interface: interface)
            Button("Export", action: onExport)
            Divider()
            Button("Copy cURL") { copyRequestAsCurl(location.request, model: model) }
            Divider()
            Button("Rename") { isRenaming = true }
            Button("Duplicate", action: onDuplicate)
            Divider()
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
func copyRequestAsCurl(_ request: RequestDraft, model: WireboltModel) {
    Task {
        for source in request.curlValueSources { await model.loadSecret(source) }
        let command = request.curlCommand { model.secretMaterial(for: $0) }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(command, forType: .string)
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
        HStack(spacing: 3) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.secondary)
                .font(.system(size: 12)).frame(width: 12)
            TextField("Filter (⌘⇧F)", text: $interface.sidebarFilter)
                .textFieldStyle(.plain)
                .focused(filterIsFocused)
        }
        .padding(.horizontal, 7)
        .frame(height: 24)
        .background(.thinMaterial, in: .capsule)
        .padding(.leading, 14)
        .padding(.trailing, 5)
        .padding(.top, 11)
        .padding(.bottom, 14)
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
                        .frame(minWidth: 440)
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
                let presentation = interface.presentation(for: session)
                RequestURLBar(model: model, interface: interface, session: session, groupID: groupID)
                    .id(session.id)
                ResponseSplit(layout: interface.responseLayout(for: groupID), orientation: interface.responseOrientation, minimumResponseWidth: session.kind == .http && session.responseHead == nil ? 330 : 349) {
                    RequestWorkspace(
                        model: model,
                        interface: interface,
                        presentation: presentation,
                        session: session,
                        groupID: groupID
                    )
                } response: {
                    if session.kind == .http {
                        ResponseViewer(
                            interface: presentation,
                            session: session
                        )
                    } else {
                        WebSocketResponseView(session: session)
                    }
                }
                .environment(\.editorStorage, presentation.editorStorage)
                .simultaneousGesture(TapGesture().onEnded {
                    if model.sessions.activeGroupID != groupID {
                        interface.activateTab(id: session.id, groupID: groupID, model: model)
                    }
                })
                .id(session.id)
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

    var body: some View {
        HStack(spacing: 0) {
            if model.sessions.groups.count > 1 {
                Button("Close Group", systemImage: "xmark") {
                    interface.close(.all, model: model, in: groupID)
                }
                .labelStyle(.iconOnly).buttonStyle(.borderless)
                .foregroundStyle(.secondary).frame(width: 30)
            }
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

            GeometryReader { geometry in
            ScrollViewReader { scroll in
            ScrollView(.horizontal) {
            HStack(spacing: 0) {
                ForEach(tabs) { tab in
                    DocumentTabButton(
                        tab: tab,
                        isSelected: group?.selectedTabID == tab.id,
                        onSelect: { interface.activateTab(id: tab.id, groupID: groupID, model: model) },
                        onRename: { name in
                            if let collectionID = tab.collectionID {
                                Task { await model.renameRequest(collectionID: collectionID, requestID: tab.requestID, name: name) }
                            } else { tab.draft.name = name }
                        },
                        onClose: {
                            interface.close(.one(tab.id), model: model, in: groupID)
                        },
                        onCloseOthers: {
                            interface.close(.others(tab.id), model: model, in: groupID)
                        },
                        onCloseRight: {
                            interface.close(.rightOf(tab.id), model: model, in: groupID)
                        },
                        onCloseAll: {
                            interface.close(.all, model: model, in: groupID)
                        }
                    )
                        .frame(width: max(80, geometry.size.width / Double(max(1, tabs.count))))
                        .id(tab.id)
                }
            }
            .padding(.top, 2)
            }
            .scrollIndicators(.never)
            .onChange(of: group?.selectedTabID) { _, selected in
                if let selected { scroll.scrollTo(selected, anchor: .center) }
            }
            }
            }
            .frame(maxWidth: .infinity)

            Button("Open in New Split", systemImage: "sidebar.right") {
                if let selected = group?.selectedTabID {
                    interface.openInNewSplit(tabID: selected, model: model)
                }
            }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 30)
                .disabled(group?.selectedTabID == nil)
        }
        .frame(height: 32)
        .background(WireboltTheme.barBackground)
        .overlay(alignment: .bottom) { Divider() }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Request tabs")

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
    let onRename: (String) -> Void
    let onClose: () -> Void
    let onCloseOthers: () -> Void
    let onCloseRight: () -> Void
    let onCloseAll: () -> Void

    @State private var isHovered = false
    @State private var isRenaming = false

    var body: some View {
        ZStack(alignment: .trailing) {
            if isRenaming {
                InlineSidebarName(title: tab.title, isEditing: $isRenaming, save: onRename)
                    .font(.system(size: 12))
                    .frame(height: 19)
                    .accessibilityLabel("Request Name")
            } else {
            Button(action: onSelect) {
                DocumentTabLabel(title: tab.title)
                    .frame(height: 19)
                    .frame(maxWidth: .infinity)
                    .offset(y: -1)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
                    .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(tab.title)
            .highPriorityGesture(TapGesture(count: 2).onEnded { beginRenaming() })
            }

            Button("Close \(tab.title)", systemImage: "xmark", action: onClose)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(.secondary)
                .opacity(isHovered && !isRenaming ? 1 : 0)
                .allowsHitTesting(!isRenaming)
                .accessibilityHidden(!isHovered || isRenaming)
        }
        .padding(.leading, 19)
        .padding(.trailing, 12)
        .frame(minWidth: 28, minHeight: 28)
        .background(isSelected ? Color.primary.opacity(0.055) : .clear, in: .rect(cornerRadius: 14))
        .overlay {
            RoundedRectangle(cornerRadius: 14)
                .stroke(isSelected ? WireboltTheme.separator.opacity(0.65) : .clear, lineWidth: 0.5)
        }
        .onHover { isHovered = $0 }
        .contextMenu {
            Button("Rename", action: beginRenaming)
            Divider()
            Button("Close Tab", action: onClose)
            Button("Close Other Tabs", action: onCloseOthers)
            Button("Close Tabs to Right", action: onCloseRight)
            Divider()
            Button("Close All Tabs", action: onCloseAll)
        }
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    private func beginRenaming() {
        onSelect()
        isRenaming = true
    }
}

private struct DocumentTabLabel: NSViewRepresentable {
    let title: String

    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField(labelWithString: title)
        field.font = .systemFont(ofSize: 12)
        field.alignment = .center
        field.lineBreakMode = .byTruncatingTail
        field.maximumNumberOfLines = 1
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) { field.stringValue = title }
}

private struct RequestWorkspace: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    @Bindable var presentation: DocumentPresentationState
    @Bindable var session: DocumentSession
    let groupID: String

    var body: some View {
        if session.kind == .webSocket {
            WebSocketRequestWorkspace(model: model, interface: presentation, session: session)
        } else {
            VStack(spacing: 0) {
                RequestSectionBar(interface: presentation, session: session, isBulkEditing: $presentation.isBulkEditing)
                Divider()
                requestContent
            }
            .background(WireboltTheme.paneBackground)
        }
    }

    @ViewBuilder
    private var requestContent: some View {
        switch presentation.requestSection {
        case .params:
            if presentation.isBulkEditing {
                BulkFieldEditor(fields: $session.draft.query)
            } else {
                FieldEditor(title: "Query Params", fields: $session.draft.query, kind: .query, focusTrigger: presentation.focusNewKeyTrigger)
            }
        case .headers:
            if presentation.isBulkEditing {
                BulkFieldEditor(fields: $session.draft.headers)
            } else {
                FieldEditor(title: "Header List", fields: $session.draft.headers, kind: .header, focusTrigger: presentation.focusNewKeyTrigger)
            }
        case .auth:
            AuthenticationEditor(model: model, session: session, authentication: $session.draft.authentication)
        case .body:
            BodyEditor(requestBody: $session.draft.body, headers: $session.draft.headers)
        case .note:
            BodyTextEditor(text: $session.note, label: "Note")
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
    @Bindable var interface: DocumentPresentationState
    @Bindable var session: DocumentSession

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                ForEach([RequestPanelSection.body, .params, .headers, .auth, .note]) { section in
                    PanelTabButton(
                        title: section == .body ? "Message" : section.rawValue,
                        badge: badge(for: section),
                        isSelected: interface.requestSection == section,
                        action: { interface.requestSection = section }
                    )
                }
                Spacer(minLength: 4)
                if interface.requestSection == .body {
                    Picker("Content Type", selection: messageKind) {
                        ForEach(WebSocketMessageKind.allCases) { Text($0.rawValue).tag($0) }
                    }.font(.system(size: 13)).controlSize(.small).frame(width: 170)
                    if messageKind.wrappedValue == .binary {
                        Picker("Binary Encoding", selection: binaryEncoding) {
                            ForEach(WebSocketBinaryEncoding.allCases, id: \.self) { Text($0.rawValue).tag($0) }
                        }.labelsHidden().controlSize(.small).frame(width: 80)
                    }
                } else if interface.requestSection == .auth {
                    AuthenticationTypePicker(authentication: $session.draft.authentication)
                }
                Menu("Message Actions", systemImage: "ellipsis.circle") { EditorPreferencesMenu() }
                    .menuStyle(.borderlessButton).menuIndicator(.hidden).labelStyle(.iconOnly).fixedSize()
            }
            .padding(.horizontal, 11)
            .frame(height: 32)
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
            if case let .file(path, contentType) = session.draft.body {
                FileBodyEditor(path: path, contentType: contentType) { session.draft.body = .file(path: $0, contentType: $1) }
            } else { BodyTextEditor(text: messageText, language: messageKind.wrappedValue == .json ? .json : .plain) }
        case .params:
            FieldEditor(title: "Query Params", fields: $session.draft.query, kind: .query)
        case .headers:
            FieldEditor(title: "Header List", fields: $session.draft.headers, kind: .header)
        case .auth:
            AuthenticationEditor(model: model, session: session, authentication: $session.draft.authentication)
        case .note:
            BodyTextEditor(text: $session.note, label: "Note")
        }
    }

    private var messageKind: Binding<WebSocketMessageKind> {
        Binding(
            get: {
                switch session.draft.body {
                case .json: .json
                case let .text(contentType, _):
                    switch contentType {
                    case "application/octet-stream", "application/octet-stream; encoding=hex": .binary
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
                case .file: .file(path: "", contentType: nil)
                }
            }
        )
    }

    private var binaryEncoding: Binding<WebSocketBinaryEncoding> {
        Binding(
            get: {
                if case let .text(contentType, _) = session.draft.body,
                   contentType == WebSocketBinaryEncoding.hex.contentType { return .hex }
                return .base64
            },
            set: { encoding in
                let old = messageText.wrappedValue
                let bytes = try? self.binaryEncoding.wrappedValue.decode(old)
                let value = bytes.map { data in
                    encoding == .base64 ? data.base64EncodedString() : data.map { String(format: "%02x", $0) }.joined(separator: " ")
                } ?? old
                session.draft.body = .text(contentType: encoding.contentType, value: value)
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
                case .binary: session.draft.body = .text(contentType: binaryEncoding.wrappedValue.contentType, value: value)
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
    @State private var urlIsFocused = false

    @State private var isEnteringCustomMethod = false
    @State private var customMethod = ""
    @State private var isEditingLongURL = false
    @State private var urlText = ""

    var body: some View {
        HStack(spacing: 7) {
            if session.kind == .webSocket {
                Text("WS").font(.system(size: 14, weight: .bold)).foregroundStyle(WireboltTheme.primaryAccent)
            } else {
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
                    .font(.system(size: 15, weight: .bold))
                    .foregroundStyle(WireboltTheme.requestBarMethodColor(session.draft.method))
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .frame(width: ceil((session.draft.method.rawValue as NSString).size(withAttributes: [.font: NSFont.systemFont(ofSize: 15, weight: .bold)]).width) + 1)
            .fixedSize()
            .offset(x: 2, y: -1)
            .accessibilityLabel("HTTP method, \(session.draft.method.rawValue)")

            }

            NativeRequestURLField(text: Binding(
                get: { urlText },
                set: { urlText = $0; session.draft.editURL($0) }
            ), isEditing: $urlIsFocused, focusTrigger: interface.focusURLTrigger, active: model.sessions.activeGroupID == groupID, submit: send)
                .onSubmit(send)
                .accessibilityLabel("Request URL")

            InlineResponseStatus(session: session)
            if session.kind == .webSocket && session.socket.status == .connected {
                Label("101 Switching Protocols", systemImage: "info.circle.fill")
                    .font(.system(size: 14)).foregroundStyle(WireboltTheme.primaryAccent).fixedSize()
            }

            Button("Edit Long URL", systemImage: "rectangle.expand.vertical") {
                isEditingLongURL = true
            }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 30)
                .help("Edit Long URL")

            RequestHistoryMenu(model: model, session: session)
                .frame(width: 24, height: 30)

            if session.kind == .webSocket {
                Button(session.socket.status == .disconnected ? "CONNECT ⌘⌃⏎" : "DISCONNECT ⌘⌃⏎") {
                    if session.socket.status == .disconnected { Task { await model.connectWebSocket(session) } }
                    else { session.socket.disconnect() }
                }.buttonStyle(WorkspaceActionButtonStyle(color: WireboltTheme.primaryAccent))
                    .disabled(session.draft.url.isEmpty)
            } else if session.isRunning {
                Button("CANCEL", systemImage: "stop.fill", action: cancel)
                    .buttonStyle(WorkspaceActionButtonStyle(color: .red))
            } else {
                Button("SEND ⌘⏎", action: send)
                .buttonStyle(WorkspaceActionButtonStyle(color: WireboltTheme.primaryAccent))
                .disabled(session.draft.url.isEmpty)
                .help("Send Request (⌘↩)")
            }
        }
        .padding(.leading, 10)
        .padding(.trailing, 11)
        .frame(height: 44)
        .background(WireboltTheme.barBackground)
        .onAppear { urlText = session.draft.displayURL }
        .onChange(of: session.draft.query) {
            if !urlIsFocused { urlText = session.draft.displayURL }
        }
        .onChange(of: session.draft.url) {
            if !urlIsFocused { urlText = session.draft.displayURL }
        }
        .onChange(of: session.id) { urlText = session.draft.displayURL }
        .sheet(isPresented: $isEditingLongURL) {
            LongURLEditor(url: Binding(
                get: { session.draft.displayURL },
                set: { session.draft.editURL($0); urlText = $0 }
            ))
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
        NSApp.keyWindow?.makeFirstResponder(nil)
        model.sessions.select(tabID: session.id, in: groupID)
        interface.synchronizeSelection(model: model)
        if session.kind == .webSocket {
            if session.socket.status == .connected { Task { await session.socket.send(body: session.draft.body) } }
            else { Task { await model.connectWebSocket(session) } }
        } else { Task { await model.send(session) } }
    }

    private func cancel() {
        model.sessions.select(tabID: session.id, in: groupID)
        model.cancel()
    }


}

private struct WebSocketResponseView: View {
    @Bindable var session: DocumentSession
    @State private var selectedMessage: UUID?
    @State private var showsHeaders = false
    @State private var filter = "All"
    @State private var hex = false
    @State private var hideControl = false
    @State private var isSearching = false
    @State private var query = ""
    @State private var split = ResponseLayoutState()
    private var socket: WebSocketDocumentState { session.socket }
    private var selected: WebSocketMessage? { socket.messages.first { $0.id == selectedMessage } }
    private var messages: [WebSocketMessage] {
        socket.messages.filter {
            (!hideControl || $0.control == nil)
                && (filter == "All" || (filter == "Sent" ? $0.outgoing : !$0.outgoing && !$0.system))
                && (query.isEmpty || String(decoding: $0.data, as: UTF8.self).localizedCaseInsensitiveContains(query))
        }
    }
    var body: some View {
        Group {
            if socket.status == .connecting {
                VStack(spacing: 10) { ProgressView().controlSize(.small); Text("Connecting…") }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if socket.messages.isEmpty && socket.status == .disconnected {
                if let error = socket.errorMessage {
                    LightweightPlaceholder(title: "", systemImage: "exclamationmark.circle", description: error)
                } else {
                    VStack(spacing: 10) {
                        Image(systemName: "paperplane").font(.system(size: 49, weight: .light))
                        Text("No Connection").font(.system(size: 16, weight: .semibold))
                    }.foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            } else {
                VStack(spacing: 0) {
                    HStack(spacing: 10) {
                        PanelTabButton(title: "WebSocket", isSelected: !showsHeaders) { showsHeaders = false }
                        PanelTabButton(title: "Headers", isSelected: showsHeaders) { showsHeaders = true }
                        Spacer()
                    }.padding(.horizontal, 11).frame(height: 34).background(WireboltTheme.barBackground)
                    Divider()
                    if showsHeaders { ResponseHeadersTable(headers: socket.headers) }
                    else {
                        ResponseSplit(layout: split) {
                            VStack(spacing: 0) {
                                messageToolbar
                                Divider()
                                messageTable
                            }
                        } response: {
                            SyntaxTextView(text: preview, language: .plain)
                        }
                    }
                }
            }
        }.onAppear { split.requestHeight = 148 }
    }
    private var messageTable: some View {
        Table(messages, selection: $selectedMessage) {
                                    TableColumn("Data") { message in
                                        HStack(spacing: 8) {
                                            Image(systemName: message.system ? "exclamationmark.circle.fill" : message.outgoing ? "arrow.up" : "arrow.down")
                                                .foregroundStyle(message.system ? .orange : WireboltTheme.primaryAccent)
                                            Text(message.control ?? String(decoding: message.data.prefix(300), as: UTF8.self)).lineLimit(1)
                                        }
                                    }.width(200)
                                    TableColumn("Time") { message in Text(Self.time.string(from: message.timestamp)) }
                                }.font(.system(size: 11)).tableStyle(.bordered(alternatesRowBackgrounds: false))
    }
    private var messageToolbar: some View {
        HStack(spacing: 6) {
            Text("Messages").foregroundStyle(.secondary)
            Spacer(minLength: 4)
            if isSearching { TextField("Find", text: $query).frame(width: 120) }
            Button("Search", systemImage: "magnifyingglass") { isSearching.toggle() }
                .labelStyle(.iconOnly).buttonStyle(.borderless)
            Menu {
                ForEach(["All", "Sent", "Receive"], id: \.self) { option in
                    Button { filter = option } label: { Label(option, systemImage: filter == option ? "checkmark" : "") }
                }
                Divider()
                Menu("Option") { Toggle("Hide Ping/Pong", isOn: $hideControl) }
            } label: { Text(filter) }.menuStyle(.borderlessButton).fixedSize()
            Picker("Preview", selection: $hex) { Text("Previewer").tag(false); Text("Hex").tag(true) }
                .labelsHidden().pickerStyle(.menu).controlSize(.small).frame(width: 82)
            Button("Copy", systemImage: "doc.on.doc") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(preview, forType: .string)
            }.labelStyle(.iconOnly).buttonStyle(.borderless)
            Button { Task { await socket.send(body: session.draft.body) } } label: {
                Text("SEND ⌘↩").font(.system(size: 12, weight: .semibold)).frame(width: 80, height: 18)
            }.buttonStyle(.borderedProminent).controlSize(.regular)
                .disabled(socket.status != .connected)
        }.font(.system(size: 13)).padding(.horizontal, 12).frame(height: 32).background(WireboltTheme.barBackground)
    }
    private var preview: String {
        guard let selected else { return "" }
        let data = selected.data.prefix(DocumentSession.previewByteLimit)
        return hex ? data.map { String(format: "%02X", $0) }.joined(separator: " ") : String(decoding: data, as: UTF8.self)
    }
    private static let time: DateFormatter = {
        let formatter = DateFormatter(); formatter.dateFormat = "HH:mm:ss.SSS"; return formatter
    }()
}

private struct RequestHistoryMenu: View {
    @Bindable var model: WireboltModel
    @Bindable var session: DocumentSession
    @State private var entries: [RunHistoryEntry] = []
    @State private var isClearing = false

    var body: some View {
        Menu("Request History", systemImage: "clock.arrow.circlepath") {
            Button("Clear History") { isClearing = true }.disabled(entries.isEmpty)
            Divider()
            if entries.isEmpty { Text("No History") }
            ForEach(entries) { entry in
                Button {
                    Task { await model.restoreHistory(entry, into: session) }
                } label: {
                    HStack(spacing: 8) {
                        Text(entry.createdAt, style: .relative)
                        Text(entry.responseHead.map { String($0.status) } ?? "Error")
                        if let completion = entry.completion {
                            Text("\(completion.totalTimeNS / 1_000_000) ms")
                            Text(String(format: "%.3f KB", Double(completion.bytesReceived) / 1000))
                        }
                    }
                }
            }
        }
        .menuStyle(.borderlessButton).menuIndicator(.hidden).labelStyle(.iconOnly)
        .foregroundStyle(.secondary).help("Request History")
        .task(id: model.historyRevision) { entries = await model.historyEntries(for: session) }
        .alert("Clear History?", isPresented: $isClearing) {
            Button("Cancel", role: .cancel) {}
            Button("Clear", role: .destructive) {
                Task { await model.clearHistory(for: session); entries = [] }
            }
        }
    }
}

private struct LongURLEditor: View {
    @Binding var url: String
    @Environment(\.dismiss) private var dismiss
    @State private var editedURL: String

    init(url: Binding<String>) {
        _url = url
        _editedURL = State(initialValue: url.wrappedValue)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Edit Long URL").font(.system(size: 11))
            TextEditor(text: $editedURL)
                .font(.system(size: 12, design: .monospaced))
                .border(WireboltTheme.separator, width: 0.5)
            HStack {
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("Done (⌘↩)") { url = editedURL; dismiss() }
                    .keyboardShortcut(.return, modifiers: .command)
            }.controlSize(.small)
        }.padding(16).frame(width: 566, height: 362)
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
                Image(systemName: status >= 400 ? "exclamationmark.circle.fill" : "checkmark.circle.fill")
                    .foregroundStyle(WireboltTheme.statusColor(status))
                Text(statusLabel(status))
                    .font(.system(size: 15))
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
        case 500: "Internal Server Error"
        default: "Response"
        }
        return "\(status) \(reason)"
    }
}

private struct WorkspaceActionButtonStyle: ButtonStyle {
    let color: Color

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 12, weight: .semibold))
            .foregroundStyle(.white)
            .padding(.horizontal, 13)
            .frame(minWidth: 102, minHeight: 28)
            .background(color.opacity(configuration.isPressed ? 0.78 : 1), in: .capsule)
            .opacity(configuration.isPressed ? 0.9 : 1)
    }
}

private struct NativeRequestURLField: NSViewRepresentable {
    @Binding var text: String
    @Binding var isEditing: Bool
    let focusTrigger: Int
    let active: Bool
    let submit: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField()
        field.isBordered = false
        field.drawsBackground = false
        field.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        field.placeholderString = "Enter URL (⌘L)"
        field.cell?.usesSingleLineMode = true
        field.cell?.lineBreakMode = .byTruncatingTail
        field.cell?.isScrollable = true
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        field.delegate = context.coordinator
        field.target = context.coordinator
        field.action = #selector(Coordinator.submit)
        field.setAccessibilityLabel("Request URL")
        return field
    }
    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        field.cell?.isScrollable = isEditing
        field.cell?.lineBreakMode = isEditing ? .byClipping : .byTruncatingTail
        if field.stringValue != text { field.stringValue = text }
        if context.coordinator.focusTrigger != focusTrigger {
            context.coordinator.focusTrigger = focusTrigger
            if active { field.window?.makeFirstResponder(field); field.selectText(nil) }
        }
    }
    @MainActor
    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: NativeRequestURLField
        var focusTrigger: Int
        init(_ parent: NativeRequestURLField) { self.parent = parent; focusTrigger = parent.focusTrigger }
        func controlTextDidBeginEditing(_ notification: Notification) { parent.isEditing = true }
        func controlTextDidEndEditing(_ notification: Notification) { parent.isEditing = false }
        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            parent.text = field.stringValue
        }
        @objc func submit() { parent.submit() }
    }
}

private struct RequestSectionBar: View {
    @Bindable var interface: DocumentPresentationState
    @Bindable var session: DocumentSession
    @Binding var isBulkEditing: Bool

    var body: some View {
        ZStack(alignment: .leading) {
                HStack(spacing: 10) {
            ForEach(RequestPanelSection.allCases) { section in
                PanelTabButton(
                    title: section.rawValue,
                    badge: badge(for: section),
                    indicator: (section == .body && session.draft.body != .empty)
                        || (section == .auth && session.draft.authentication != .none),
                    isSelected: interface.requestSection == section,
                    action: { interface.requestSection = section }
                )
            }
                }
                .fixedSize(horizontal: true, vertical: false)
                .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
        }
        .overlay(alignment: .trailing) {
            HStack(spacing: 0) {
            if interface.requestSection == .body {
                BodyTools(requestBody: $session.draft.body)
            } else if interface.requestSection == .auth {
                AuthenticationTypePicker(authentication: $session.draft.authentication)
            } else if fieldsBinding != nil {
                Button("New Entry", systemImage: "plus") {
                    interface.isBulkEditing = false; interface.focusNewKeyTrigger += 1
                }
                .labelStyle(.iconOnly).buttonStyle(.borderless).foregroundStyle(.secondary)
                .frame(width: 30, height: 32)
                Menu("Section Actions", systemImage: "ellipsis.circle") {
                    Button("New Entry") { interface.isBulkEditing = false; interface.focusNewKeyTrigger += 1 }
                    Divider()
                    Button("Key-Value Edit") { isBulkEditing = false }
                    Button("Bulk Edit") { isBulkEditing = true }
                    Divider()
                    Button("Clear All", action: clearFields)
                }
                .menuStyle(.borderlessButton).menuIndicator(.hidden)
                .labelStyle(.iconOnly).foregroundStyle(.secondary).fixedSize()
                .frame(width: 30, height: 32)
            }
            }
        }
        .padding(.leading, 11)
        .padding(.trailing, 11)
        .frame(height: 32)
        .background(WireboltTheme.barBackground)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Request sections")
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
        BodyTextEditor(text: $text)
            .onChange(of: text) { fields = parse(text) }
    }

    private func parse(_ source: String) -> [RequestField] {
        var unused = fields
        return source.split(separator: "\n", omittingEmptySubsequences: true).compactMap { line in
            var value = String(line)
            let enabled = value.hasPrefix("#") == false
            if enabled == false { value.removeFirst(); value = value.trimmingCharacters(in: .whitespaces) }
            guard let separator = value.firstIndex(of: ":") else { return nil }
            let name = String(value[..<separator]).trimmingCharacters(in: .whitespaces)
            let index = unused.firstIndex { $0.name == name && $0.enabled == enabled }
            let id = index.map { unused.remove(at: $0).id } ?? UUID().uuidString.lowercased()
            return RequestField(
                id: id, name: name,
                value: .literal(String(value[value.index(after: separator)...]).trimmingCharacters(in: .whitespaces)),
                enabled: enabled
            )
        }
    }
}

struct PanelTabButton: View {
    let title: String
    var badge: Int?
    var indicator = false
    let isSelected: Bool
    var height: CGFloat = 34
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: title == "Headers" ? 4 : 5) {
                Text(title)
                if let badge, badge > 0 {
                    Text("(\(badge))")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(WireboltTheme.success)
                } else if indicator {
                    Text("•︎").foregroundStyle(WireboltTheme.success)
                }
            }
            .font(.system(size: 13))
            .lineLimit(1)
            .fixedSize(horizontal: true, vertical: false)
            .foregroundStyle(isSelected ? .primary : .secondary)
            .padding(.horizontal, 3)
            .frame(height: height)
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
    var focusTrigger = 0
    @State private var pendingID = UUID().uuidString.lowercased()
    @State private var committedRows = 0
    private var effectiveFocusTrigger: Int { focusTrigger + committedRows }

    var body: some View {
        GeometryReader { geometry in
        let valueWidth = max(1, geometry.size.width - 225)
        VStack(alignment: .leading, spacing: 0) {
            FieldTableHeader()
                .frame(width: geometry.size.width, alignment: .leading)

            ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach($fields) { $field in
                        if field.id != pendingID { FieldTableRow(
                            field: $field,
                            valueWidth: valueWidth,
                            onRemove: { fields.removeAll { $0.id == field.id } }
                        )
                        }
                    }
                    NewFieldTableRow(fields: $fields, kind: kind, pendingID: $pendingID, focusTrigger: effectiveFocusTrigger,
                        onCommit: { committedRows += 1 }, valueWidth: valueWidth)
                        .id("new-field")
                }
                .frame(width: geometry.size.width, alignment: .leading)
            }
            .task(id: effectiveFocusTrigger) {
                guard effectiveFocusTrigger > 0 else { return }
                await Task.yield()
                guard !Task.isCancelled else { return }
                proxy.scrollTo("new-field", anchor: .bottom)
            }
            }
        }
        .background(WireboltTheme.paneBackground)
        }
    }
}

private struct FieldTableHeader: View {
    var body: some View {
        HStack(spacing: 0) {
            Color.clear.frame(width: 27)
            Divider().frame(height: 14)
            Text("Key")
                .frame(width: 177, alignment: .leading)
                .padding(.leading, 6)
            Divider().frame(height: 14)
            Text("Value")
                .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
                .padding(.leading, 6)
        }
        .font(.system(size: 11, weight: .semibold))
        .foregroundStyle(.primary)
        .frame(height: 28)
        .background(WireboltTheme.barBackground)
        .overlay(alignment: .bottom) { Divider() }
    }
}

private struct FieldTableRow: View {
    @Binding var field: RequestField
    let valueWidth: Double
    let onRemove: () -> Void

    @State private var isHovered = false
    private var fieldHeight: Double { FieldEditorMetrics.height(key: field.name, value: field.value.editableValue, valueWidth: valueWidth) }

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            FieldCheckbox(isOn: $field.enabled)
            FieldTextInput("Key", text: $field.name, height: fieldHeight)
                .frame(width: 175)
                .padding(.horizontal, 4)

            Color.clear.frame(width: 1)
            FieldTextInput("Value", text: literalBinding($field.value), height: fieldHeight)
                .frame(width: valueWidth)
                .padding(.horizontal, 4)
                .padding(.trailing, 5)
        }
        .overlay(alignment: .topTrailing) {
            Button("Remove \(field.name)", systemImage: "trash", action: onRemove)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 42, height: 20)
                .opacity(isHovered ? 1 : 0)
                .allowsHitTesting(isHovered)
        }
        .frame(height: fieldHeight)
        .padding(.vertical, 4)
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
    @Binding var pendingID: String
    var focusTrigger = 0
    let onCommit: () -> Void
    let valueWidth: Double

    @State private var name = ""
    @State private var value = ""
    @State private var showingSuggestions = false
    @State private var showingValueSuggestions = false
    @State private var isEnabled = true
    private var fieldHeight: Double { FieldEditorMetrics.height(key: name, value: value, valueWidth: valueWidth) }
    @FocusState private var focusedField: NewFieldFocus?

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .top, spacing: 0) {
                if name.isEmpty {
                    Color.clear.frame(width: 28)
                } else {
                    FieldCheckbox(isOn: $isEnabled)
                }
                FieldTextInput("New Key (⌘K)", text: $name, height: fieldHeight)
                    .focused($focusedField, equals: .key)
                    .frame(width: 175)
                    .padding(.horizontal, 4)

                    .onSubmit { focusedField = .value }
                    .popover(isPresented: $showingSuggestions, arrowEdge: .bottom) {
                        HeaderSuggestions(query: name) { suggestion in
                            name = suggestion
                            showingSuggestions = false
                            focusedField = .value
                        }
                    }
                Color.clear.frame(width: 1)
                FieldTextInput("New Value", text: $value, height: fieldHeight)
                    .focused($focusedField, equals: .value)
                    .frame(width: valueWidth)
                    .padding(.horizontal, 4)
                    .padding(.trailing, 5)

                    .onSubmit(commit)
                    .popover(isPresented: $showingValueSuggestions, arrowEdge: .bottom) {
                        HeaderValueSuggestions(query: value) { suggestion in
                            value = suggestion
                            showingValueSuggestions = false
                            commit()
                        }
                    }
            }
            .overlay(alignment: .topTrailing) {
                if !name.isEmpty {
                    Button("Discard New Field", systemImage: "trash", action: discard)
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                        .foregroundStyle(.secondary)
                        .frame(width: 42, height: 20)
                }
            }
            .frame(height: fieldHeight)
            .padding(.vertical, 4)

            if !name.isEmpty {
                Color.clear.frame(width: 1)
                EmptyNewFieldRow()
            }
        }
        .foregroundStyle(.secondary)
        .onChange(of: fields) { _, fields in
            if (!name.isEmpty || !value.isEmpty) && !fields.contains(where: { $0.id == pendingID }) {
                name = ""; value = ""; isEnabled = true
            }
        }
        .task(id: focusTrigger) {
            guard focusTrigger > 0 else { return }
            if !name.isEmpty { commit() }
            await Task.yield()
            guard !Task.isCancelled else { return }
            focusedField = .key
        }
        .onChange(of: name) { synchronizePending(); updateSuggestions() }
        .onChange(of: value) { synchronizePending(); updateValueSuggestions() }
        .onChange(of: isEnabled) { synchronizePending() }
        .onChange(of: focusedField) {
            updateSuggestions()
            updateValueSuggestions()
        }
    }

    private func synchronizePending() {
        if name.isEmpty && value.isEmpty {
            fields.removeAll { $0.id == pendingID }
            return
        }
        let field = RequestField(id: pendingID, name: name, value: .literal(value), enabled: isEnabled)
        if let index = fields.firstIndex(where: { $0.id == pendingID }) { fields[index] = field }
        else { fields.append(field) }
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
        synchronizePending()
        pendingID = UUID().uuidString.lowercased()
        discard()
        onCommit()
    }

    private func discard() {
        fields.removeAll { $0.id == pendingID }
        pendingID = UUID().uuidString.lowercased()
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
            Color.clear.frame(width: 1)
            Text("New Key (⌘K)")
                .frame(width: 175, alignment: .leading)
                .padding(.horizontal, 4)
            Color.clear.frame(width: 1)
            Text("New Value")
                .frame(minWidth: 50, maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 4)
                .padding(.trailing, 5)
        }
        .font(.system(size: 11, design: .monospaced))
        .foregroundStyle(.tertiary)
        .frame(height: 28)
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
        Group {
            switch authentication {
            case .none:
                LightweightPlaceholder(title: "No Auth", systemImage: "lock.open")
            case let .basic(username, password):
                let user = model.secretMaterial(for: username)
                let secret = model.secretMaterial(for: password)
                Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 5) {
                    GridRow {
                        Text("Username").gridColumnAlignment(.trailing)
                        TextField("", text: credential(username, role: "username"))
                    }
                    GridRow {
                        Text("Password")
                        TextField("", text: credential(password, role: "password"))
                    }
                    GridRow(alignment: .top) {
                        Text("Generated Header").padding(.top, 3)
                        Text(user.isEmpty && secret.isEmpty ? "Basic" : "Basic " + Data("\(user):\(secret)".utf8).base64EncodedString())
                            .foregroundStyle(.secondary).padding(.top, 3)
                            .textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .font(.system(size: 13)).controlSize(.small)
                .textFieldStyle(.roundedBorder).padding(.horizontal, 20).padding(.top, 16)
                .task(id: password) { await model.loadSecret(password) }
                .task(id: username) { await model.loadSecret(username) }
            case let .bearer(token):
                HStack(alignment: .top, spacing: 10) {
                    Text("Bearer Token").frame(width: 80, alignment: .trailing).padding(.top, 5)
                    TextEditor(text: credential(token, role: "token"))
                        .font(.system(size: 13)).scrollContentBackground(.hidden)
                        .padding(4).frame(height: 162)
                        .background(Color(nsColor: .textBackgroundColor), in: .rect(cornerRadius: 5))
                        .overlay { RoundedRectangle(cornerRadius: 5).stroke(WireboltTheme.separator) }
                }.padding(20)
                    .task(id: token) { await model.loadSecret(token) }
            case .apiKey, .oauth2:
                advancedAuthentication
            }
        }
        .font(.system(size: 12))
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private func credential(_ source: ValueSource, role: String) -> Binding<String> {
        Binding(
            get: { model.secretMaterial(for: source) },
            set: { value in
                let name: String
                if case let .secret(existing) = source { name = existing }
                else { name = "request.\(session.requestID).\(role)" }
                model.editSecret(name: name, value: value)
                switch authentication {
                case let .basic(username, password):
                    authentication = role == "username"
                        ? .basic(username: .secret(name), password: password)
                        : .basic(username: username, password: .secret(name))
                case .bearer: authentication = .bearer(token: .secret(name))
                default: break
                }
            }
        )
    }

    private var advancedAuthentication: some View {
        Form {
            switch authentication {
            case .none, .basic, .bearer: EmptyView()
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

private struct AuthenticationTypePicker: View {
    @Binding var authentication: RequestAuthentication
    var body: some View {
        Picker("Auth Type", selection: kindBinding) {
            ForEach(AuthenticationKind.allCases.filter { [.none, .basic, .bearer, kindBinding.wrappedValue].contains($0) }) { kind in Text(kind.title).tag(kind) }
        }.font(.system(size: 13)).controlSize(.small).frame(width: 180)
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
                case .basic: .basic(username: .literal(""), password: .secret("auth.\(UUID().uuidString).password"))
                case .bearer: .bearer(token: .secret("auth.\(UUID().uuidString).token"))
                case .apiKey: .apiKey(placement: .header, name: "X-API-Key", value: .secret("auth.api-key"))
                case .oauth2: .oauth2(configuration: OAuth2Configuration())
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
    case oauth2

    var id: Self { self }

    var title: String {
        switch self {
        case .none: "None"
        case .basic: "Basic"
        case .bearer: "Bearer Token"
        case .apiKey: "API Key"
        case .oauth2: "OAuth 2.0"
        }
    }
}

private struct BodyEditor: View {
    @Binding var requestBody: RequestBody
    @Binding var headers: [RequestField]
    @State private var wrapsLines = true
    @State private var pendingContentType: String?

    var body: some View {
        VStack(spacing: 0) {
            switch requestBody {
            case .empty:
                LightweightPlaceholder(
                    title: "No Body",
                    systemImage: "nosign"
                )
            case let .text(contentType, value):
                BodyTextEditor(text: Binding(
                    get: { value },
                    set: { requestBody = .text(contentType: contentType, value: $0) }
                ))
            case let .json(value):
                BodyTextEditor(text: Binding(
                    get: { value },
                    set: { requestBody = .json(value: $0) }
                ), language: .json)
            case let .xml(value):
                BodyTextEditor(text: Binding(
                    get: { value },
                    set: { requestBody = .xml(value: $0) }
                ), language: .xml)
            case let .html(value):
                BodyTextEditor(text: Binding(
                    get: { value },
                    set: { requestBody = .html(value: $0) }
                ), language: .html)
            case let .raw(contentType, value):
                BodyTextEditor(text: Binding(
                    get: { value },
                    set: { requestBody = .raw(contentType: contentType, value: $0) }
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
            case let .multipart(parts):
                MultipartEditor(parts: Binding(
                    get: { parts },
                    set: { requestBody = .multipart(parts: $0) }
                ))
            case let .file(path, contentType):
                FileBodyEditor(
                    path: path,
                    contentType: contentType,
                    update: { path, mime in
                        requestBody = .file(path: path, contentType: mime)
                        if !path.isEmpty, let mime,
                           headers.first(where: { $0.enabled && $0.name.caseInsensitiveCompare("Content-Type") == .orderedSame })?.value != .literal(mime) {
                            pendingContentType = mime
                        }
                    }
                )
            }
        }
        .background(WireboltTheme.paneBackground)
        .alert("Change Content-Type Header", isPresented: Binding(
            get: { pendingContentType != nil }, set: { if !$0 { pendingContentType = nil } }
        )) {
            Button("Cancel", role: .cancel) { pendingContentType = nil }
            Button("Yes") {
                guard let mime = pendingContentType else { return }
                if let index = headers.firstIndex(where: { $0.name.caseInsensitiveCompare("Content-Type") == .orderedSame }) {
                    headers[index].value = .literal(mime)
                    headers[index].enabled = true
                } else { headers.append(RequestField(name: "Content-Type", value: .literal(mime))) }
                pendingContentType = nil
            }.keyboardShortcut(.defaultAction)
        } message: { Text("Do you want to set Content-Type: \(pendingContentType ?? "")") }
    }

}

private struct BodyTools: View {
    @Binding var requestBody: RequestBody
    @AppStorage("editor.wordWrap") private var wrapsLines = true
    var body: some View {
        HStack(spacing: 7) {
            Picker("Content Type", selection: kindBinding) {
                ForEach(BodyKind.allCases) { kind in
                    Text(kind.title).tag(kind)
                    if [.form, .html, .raw, .file].contains(kind) { Divider() }
                }
            }
            .font(.system(size: 13)).controlSize(.small).frame(width: 168)
            if case let .multipart(parts) = requestBody {
                Button("Add Part", systemImage: "plus") { requestBody = .multipart(parts: parts + [MultipartPart()]) }
                    .labelStyle(.iconOnly).buttonStyle(.borderless)
            } else {
                Button("Format Body", systemImage: "wand.and.stars", action: formatBody)
                    .labelStyle(.iconOnly).buttonStyle(.borderless).disabled(!requestBody.isTextual)
            }
            Menu("Body Actions", systemImage: "ellipsis.circle") {
                if case .multipart = requestBody {
                    Button("Add Part") {
                        if case let .multipart(parts) = requestBody { requestBody = .multipart(parts: parts + [MultipartPart()]) }
                    }
                    Button("Clear All") { requestBody = .multipart(parts: []) }
                }
                EditorPreferencesMenu()
            }.menuStyle(.borderlessButton).menuIndicator(.hidden).labelStyle(.iconOnly).fixedSize()
        }.fixedSize()
    }

    private var kindBinding: Binding<BodyKind> {
        Binding(
            get: {
                switch requestBody {
                case .empty: .empty
                case .text: .raw
                case .json: .json
                case .xml: .xml
                case .html: .html
                case .raw: .raw
                case .formURLEncoded: .form
                case .multipart: .multipart
                case .file: .file
                }
            },
            set: { kind in
                let previous: String = switch requestBody {
                case let .text(_, value), let .json(value), let .xml(value), let .html(value), let .raw(_, value): value
                default: ""
                }
                requestBody = switch kind {
                case .empty: .empty
                case .text: .text(contentType: nil, value: previous)
                case .json: .json(value: previous.isEmpty ? "{\n  \n}" : previous)
                case .xml: .xml(value: previous)
                case .html: .html(value: previous)
                case .raw: .raw(contentType: nil, value: previous)
                case .form: .formURLEncoded(fields: [])
                case .multipart: .multipart(parts: previous.isEmpty ? [] : [MultipartPart(name: "name", value: .literal(previous))])
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
    var language: SyntaxLanguage = .plain
    var label = "Request body"
    @State private var find = EditorFindState()
    var body: some View {
        NativeCodeEditor(text: $text, language: language, label: label, find: find)
            .frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
            .editorFindOverlay(find)
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

    static var allCases: [Self] { [.json, .form, .xml, .html, .raw, .multipart, .file, .empty] }
    var id: Self { self }

    var title: String {
        switch self {
        case .empty: "No Body"
        case .json: "JSON"
        case .text: "Text"
        case .xml: "XML"
        case .html: "HTML"
        case .raw: "Raw Text"
        case .form: "Form URLEncoded"
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
            HStack(spacing: 0) {
                Text("Part").frame(width: 99, alignment: .leading).padding(.leading, 10)
                Divider().frame(height: 16)
                Text("Content Type").frame(width: 118, alignment: .leading).padding(.leading, 6)
                Divider().frame(height: 16)
                Text("File Name").frame(width: 98, alignment: .leading).padding(.leading, 6)
                Divider().frame(height: 16)
                Text("Value").frame(width: 175, alignment: .leading).padding(.leading, 6)
                Spacer(minLength: 0)
            }.font(.system(size: 11)).frame(height: 28).background(WireboltTheme.barBackground)
            Divider()
            ScrollView([.horizontal, .vertical]) {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach($parts) { $part in
                        MultipartRow(part: $part) { parts.removeAll { $0.id == part.id } }
                    }
                }.frame(minWidth: 520, maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

private struct MultipartRow: View {
    @Binding var part: MultipartPart
    let remove: () -> Void
    @State private var isEditing = false
    var body: some View {
        HStack(spacing: 0) {
            Text(part.name).frame(width: 99, alignment: .leading).padding(.leading, 10)
            Text(part.contentType ?? "").frame(width: 119, alignment: .leading).padding(.leading, 6)
            Text(part.fileName ?? part.filePath.map { URL(fileURLWithPath: $0).lastPathComponent } ?? "")
                .frame(width: 99, alignment: .leading).padding(.leading, 6)
            Text(part.kind == .text ? String(part.value.editableValue.prefix(12)) : "<…")
                .lineLimit(1).frame(width: 31, alignment: .leading).padding(.leading, 6)
            Button("Export", systemImage: "square.and.arrow.up", action: export).controlSize(.mini).frame(width: 78)
            Button("Edit", systemImage: "pencil") { isEditing = true }.controlSize(.mini).frame(width: 63)
                .popover(isPresented: $isEditing, arrowEdge: .bottom) {
                    MultipartPartEditor(part: $part, remove: { isEditing = false; remove() })
                }
            Spacer(minLength: 0)
        }.font(.system(size: 11)).lineLimit(1).frame(height: 28)
            .onAppear { if part.name.isEmpty { isEditing = true } }
    }
    private func export() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = part.fileName ?? (part.name.isEmpty ? "part" : part.name)
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let data: Data
            switch part.kind {
            case .file: data = try Data(contentsOf: URL(fileURLWithPath: part.filePath ?? ""), options: .mappedIfSafe)
            case .binary:
                guard let decoded = Data(base64Encoded: part.value.editableValue) else { NSSound.beep(); return }
                data = decoded
            case .text: data = Data(part.value.editableValue.utf8)
            }
            try data.write(to: url, options: .atomic)
        } catch { NSSound.beep() }
    }
}

private struct MultipartPartEditor: View {
    @Binding var part: MultipartPart
    let remove: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var isEditingValue = false
    @State private var text = ""
    var body: some View {
        VStack(spacing: 0) {
            Grid(alignment: .leading, horizontalSpacing: 8, verticalSpacing: 2) {
                GridRow {
                    Text("Part:").gridColumnAlignment(.trailing)
                    TextField("", text: $part.name).frame(height: 26)
                }
                GridRow {
                    Text("Content Type:")
                    HStack(spacing: 0) {
                        TextField("", text: optional(\.contentType))
                        Menu("Content Type") {
                            ForEach(["text/plain", "application/json", "application/xml", "application/octet-stream", "image/png"], id: \.self) { type in
                                Button(type) { part.contentType = type }
                            }
                        }.labelsHidden().frame(width: 26)
                    }.frame(height: 26)
                }
                GridRow {
                    Text("File Name:")
                    TextField("", text: optional(\.fileName)).frame(height: 26)
                }
                GridRow(alignment: .top) {
                    Text("Value:").padding(.top, 4)
                    VStack(alignment: .leading, spacing: 8) {
                        Text("<Double-click to edit>").padding(4)
                            .frame(maxWidth: .infinity, minHeight: 82, maxHeight: 82, alignment: .topLeading)
                            .background(WireboltTheme.paneBackground).border(WireboltTheme.separator)
                            .onTapGesture(count: 2, perform: editValue)
                            .accessibilityAction(named: "Edit Value", editValue)
                        Button("Select File…", action: selectFile).controlSize(.small)
                    }.padding(.top, 6)
                }
            }.textFieldStyle(.roundedBorder)
            Spacer()
            HStack {
                Button("Delete this Part", systemImage: "trash", action: remove)
                Spacer()
                Button("OK") { dismiss() }.keyboardShortcut(.defaultAction)
            }.controlSize(.small)
        }.font(.system(size: 12)).padding(20).frame(width: 510, height: 298)
            .sheet(isPresented: $isEditingValue) {
                VStack(spacing: 12) {
                    BodyTextEditor(text: $text)
                    HStack {
                        Spacer()
                        Button("Cancel") { isEditingValue = false }.keyboardShortcut(.cancelAction)
                        Button("Done") {
                            if part.kind == .binary {
                                guard let data = try? WebSocketBinaryEncoding.hex.decode(text) else { NSSound.beep(); return }
                                part.value = .literal(data.base64EncodedString())
                            } else { part.kind = .text; part.filePath = nil; part.value = .literal(text) }
                            isEditingValue = false
                        }
                            .keyboardShortcut(.defaultAction)
                    }
                }.padding(16).frame(width: 566, height: 362)
            }
    }
    private func optional(_ keyPath: WritableKeyPath<MultipartPart, String?>) -> Binding<String> {
        Binding(get: { part[keyPath: keyPath] ?? "" }, set: { part[keyPath: keyPath] = $0.isEmpty ? nil : $0 })
    }
    private func editValue() {
        if part.kind == .binary, let data = Data(base64Encoded: part.value.editableValue) {
            text = data.map { String(format: "%02X", $0) }.joined(separator: " ")
        } else { text = part.value.editableValue }
        isEditingValue = true
    }
    private func selectFile() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        part.kind = .file
        part.filePath = url.path
        part.fileName = url.lastPathComponent
        part.contentType = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
    }
}

private struct FileBodyEditor: View {
    let path: String
    let contentType: String?
    let update: (String, String?) -> Void

    var body: some View {
        VStack(spacing: 0) {
            Image(systemName: "paperclip.circle.fill").font(.system(size: 42)).foregroundStyle(.secondary)
            Text(path.isEmpty ? "Select any file on your Mac" : path)
                .font(.system(size: 13)).foregroundStyle(.secondary)
                .lineLimit(1).truncationMode(.middle).padding(.horizontal, 20).padding(.top, 20)
            if !path.isEmpty {
                Button("Show In Finder…") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) }
                    .padding(.top, 6)
            }
            Divider().frame(width: 200).padding(.top, 14).padding(.bottom, 16)
            if path.isEmpty { Button("Select Local File…", action: choose) }
            else {
                HStack(spacing: 8) {
                    Button("Replace…", action: choose)
                    Button("Clear") { update("", contentType) }
                }
            }
        }
        .font(.system(size: 13)).controlSize(.regular)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .contentShape(.rect)
        .dropDestination(for: URL.self) { urls, _ in
            guard let file = urls.first, file.isFileURL else { return false }
            update(file.path, UTType(filenameExtension: file.pathExtension)?.preferredMIMEType ?? "application/octet-stream")
            return true
        }
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let selected = panel.url {
            update(selected.path, UTType(filenameExtension: selected.pathExtension)?.preferredMIMEType ?? "application/octet-stream")
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
        VStack(spacing: 10) {
            Image(systemName: systemImage).font(.system(size: 32, weight: .light))
            if !title.isEmpty { Text(title).font(.system(size: 13)) }
            if let description {
                Text(description).font(.system(size: 12)).multilineTextAlignment(.center).frame(maxWidth: 250)
            }
        }
        .foregroundStyle(.secondary)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
    }
}

private struct ToolbarSpace: NSViewRepresentable {
    var width: Double
    func makeNSView(context: Context) -> Space { Space() }
    func updateNSView(_ view: Space, context: Context) {
        view.width = width
        view.invalidateIntrinsicContentSize()
    }
    final class Space: NSView {
        var width = 0.0
        override var intrinsicContentSize: NSSize { NSSize(width: width, height: 1) }
    }
}

private struct WindowConfigurator: NSViewRepresentable {
    var sidebarWidth: Double
    func makeNSView(context _: Context) -> WindowConfigurationView {
        WindowConfigurationView()
    }

    func updateNSView(_ view: WindowConfigurationView, context _: Context) {
        view.sidebarWidth = sidebarWidth
        view.updateSidebarOutline()
        view.scheduleMenuOrderMatch()
    }
}

struct SidebarMaterialView: NSViewRepresentable {
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
    @State private var isEditingEnvironments = false

    var body: some View {
        Picker("Environment", selection: Binding(
            get: { model.selectedEnvironmentID ?? WorkspaceDraft.globalEnvironmentID },
            set: { selection in
                if selection == "configure" { isEditingEnvironments = true }
                else { model.selectedEnvironmentID = selection == WorkspaceDraft.globalEnvironmentID ? nil : selection }
            }
        )) {
            Text("Global Environment").tag(WorkspaceDraft.globalEnvironmentID)
            ForEach(model.workspace.environments.filter { $0.id != WorkspaceDraft.globalEnvironmentID }.sorted(by: { $0.name < $1.name })) { environment in
                Text(environment.name).tag(environment.id)
            }
            Divider()
            Text("Configure Environments…").tag("configure")
        }
        .labelsHidden().pickerStyle(.menu).controlSize(.large)
        .font(.system(size: 13))
        .accessibilityLabel("Environment")
        .accessibilityValue(selectedEnvironment?.name ?? "Global Environment")
        .sheet(isPresented: $isEditingEnvironments) {
            EnvironmentEditor(model: model)
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
    @Bindable var layout: ResponseLayoutState
    var orientation: ResponseOrientation = .bottom
    var minimumResponseWidth: CGFloat = 349
    @ViewBuilder let request: RequestContent
    @ViewBuilder let response: ResponseContent
    @State private var dragOrigin: CGFloat?

    var body: some View {
        GeometryReader { geometry in
            let vertical = orientation == .bottom
            let length = vertical ? geometry.size.height : geometry.size.width
            let minimum: CGFloat = 100
            let maximum = max(minimum, length - (vertical ? 200 : minimumResponseWidth) - 1)
            let position = min(maximum, max(minimum, (vertical ? layout.requestHeight : layout.requestWidth) ?? (length / 2).rounded(.down)))
            let arrangement = vertical ? AnyLayout(VStackLayout(spacing: 0)) : AnyLayout(HStackLayout(spacing: 0))
            arrangement {
                request.frame(width: vertical ? nil : position, height: vertical ? position : nil).clipped()
                Rectangle().fill(WireboltTheme.separator)
                    .frame(width: vertical ? nil : 1, height: vertical ? 1 : nil)
                    .overlay {
                        Color.clear.contentShape(.rect)
                            .frame(width: vertical ? nil : 7, height: vertical ? 7 : nil)
                            .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .global)
                                .onChanged { value in
                                    if dragOrigin == nil { dragOrigin = position }
                                    let offset = vertical ? value.translation.height : value.translation.width
                                    let updated = min(maximum, max(minimum, (dragOrigin ?? position) + offset))
                                    if vertical { layout.requestHeight = updated }
                                    else { layout.requestWidth = updated }
                                }
                                .onEnded { _ in dragOrigin = nil })
                            .onHover { hovering in
                                if hovering { (vertical ? NSCursor.resizeUpDown : NSCursor.resizeLeftRight).push() }
                                else { NSCursor.pop() }
                            }
                    }
                response.frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
            }
            .onAppear {
                if vertical, layout.requestHeight == nil { layout.requestHeight = position }
                if !vertical, layout.requestWidth == nil { layout.requestWidth = position }
            }
            .onChange(of: orientation) {
                if vertical { layout.requestHeight = (geometry.size.height / 2).rounded(.down) }
                else { layout.requestWidth = (geometry.size.width / 2).rounded(.down) }
            }
        }
    }
}

@MainActor
private final class WindowConfigurationView: NSView {
    var sidebarWidth = 250.0
    private let sidebarOutline = SidebarOutlineView()

    func updateSidebarOutline() {
        guard let content = window?.contentView?.superview else { return }
        if sidebarOutline.superview !== content {
            sidebarOutline.frame = content.bounds
            sidebarOutline.autoresizingMask = [.width, .height]
            content.addSubview(sidebarOutline, positioned: .above, relativeTo: nil)
        }
        sidebarOutline.sidebarWidth = sidebarWidth
        sidebarOutline.needsDisplay = true
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        window.titleVisibility = .hidden
        window.title = ""
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.toolbarStyle = .unified
        window.styleMask.insert(.fullSizeContentView)
        window.backgroundColor = .windowBackgroundColor
        window.isOpaque = false
        window.setFrameAutosaveName("WireboltMainWindow")
        updateSidebarOutline()
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
        for observed in [menu, menu.item(withTitle: "Edit")?.submenu].compactMap({ $0 }) {
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(mainMenuDidChange),
            name: NSMenu.didAddItemNotification,
            object: observed
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(mainMenuDidChange),
            name: NSMenu.didRemoveItemNotification,
            object: observed
        )
        }
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
        synchronizeEditingMenu()
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

    private func synchronizeEditingMenu() {
        guard let edit = NSApp.mainMenu?.item(withTitle: "Edit")?.submenu else { return }
        let pasteIndex = edit.indexOfItem(withTitle: "Paste")
        if pasteIndex >= 0, edit.item(withTitle: "Paste and Match Style") == nil {
            let item = NSMenuItem(title: "Paste and Match Style", action: #selector(NSTextView.pasteAsPlainText(_:)), keyEquivalent: "v")
            item.keyEquivalentModifierMask = [.command, .option, .shift]
            edit.insertItem(item, at: pasteIndex + 1)
        }
        if let item = edit.item(withTitle: "Delete") {
            item.keyEquivalent = "\u{8}"
            item.keyEquivalentModifierMask = .command
        }
    }
}

@MainActor
private final class SidebarOutlineView: NSView {
    var sidebarWidth = 250.0
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func draw(_ dirtyRect: NSRect) {
        guard sidebarWidth > 0 else { return }
        NSColor.separatorColor.setStroke()
        let outline = NSBezierPath(roundedRect: NSRect(x: 8.5, y: 8.5,
            width: sidebarWidth - 9, height: bounds.height - 17), xRadius: 18, yRadius: 18)
        outline.lineWidth = 1
        outline.stroke()
    }
}
