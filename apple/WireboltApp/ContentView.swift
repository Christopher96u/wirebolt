import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var loadsWorkspace = true
    @AppStorage("workspace.sidebarWidth") private var storedSidebarWidth = 250.0
    /// Width while the divider is dragged; stored once the drag ends.
    @State private var liveSidebarWidth: Double?
    @State private var sidebarDragOrigin: Double?
    @State private var isDropTargeted = false
    /// The window's undo manager; workspace mutations register their inverses on it.
    @Environment(\.undoManager) private var undoManager
    private var sidebarWidth: Double { liveSidebarWidth ?? storedSidebarWidth }
    private var toolbarGap: Double { max(0, sidebarWidth - 184) }
    /// Until the first workspace load finishes, show placeholders instead of an empty sidebar
    /// and "No Open Request".
    private var showsLaunchSkeleton: Bool { loadsWorkspace && !model.hasLoadedWorkspace }

    var body: some View {
        workspaceWithDialogs
    }

    private var workspacePanels: some View {
        HStack(spacing: 0) {
            if interface.columnVisibility != .detailOnly {
                Group {
                    if showsLaunchSkeleton {
                        SidebarLoadingSkeleton()
                    } else {
                        WorkspaceSidebar(
                            model: model,
                            interface: interface,
                            showsMaterial: true
                        )
                    }
                }
                    .frame(width: sidebarWidth - 1)
                Rectangle().fill(WireboltTheme.separator).frame(width: 1)
                    .overlay {
                        Color.clear.frame(width: 7).contentShape(.rect)
                            .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .global)
                                .onChanged { value in
                                    if sidebarDragOrigin == nil { sidebarDragOrigin = sidebarWidth }
                                    liveSidebarWidth = min(480, max(180, (sidebarDragOrigin ?? sidebarWidth) + value.translation.width))
                                }
                                .onEnded { _ in
                                    if let liveSidebarWidth { storedSidebarWidth = liveSidebarWidth }
                                    liveSidebarWidth = nil
                                    sidebarDragOrigin = nil
                                })
                            .onHover { inside in
                                if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
                            }
                    }
            }
            if showsLaunchSkeleton {
                InitialDetailPane()
            } else {
                WorkspaceDeck(model: model, interface: interface)
            }
        }
    }

    private var workspaceSurface: some View {
        let groupCount = model.sessions.groupCount
        return workspacePanels
        .disabled(model.isLoadingWorkspace)
        .background { WorkspaceWindowTitle(model: model) }
        .frame(minWidth: groupCount > 1
            ? (interface.columnVisibility == .detailOnly ? 0 : 208) + Double(groupCount * 440 + groupCount - 1)
            : 720, minHeight: 411)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width in
            let groupCount = model.sessions.groupCount
            if groupCount > 1 {
                let detailMinimum = Double(groupCount * 440 + groupCount - 1)
                let fitted = min(storedSidebarWidth, max(208, width - detailMinimum))
                if fitted != storedSidebarWidth { storedSidebarWidth = fitted }
            }
        }
        .tint(WireboltTheme.primaryAccent)
        .toolbar(id: "workspace-toolbar") { workspaceToolbar }
        .toolbar(removing: .sidebarToggle)
        .background(WindowConfigurator(
            sidebarWidth: interface.columnVisibility == .detailOnly ? 0 : sidebarWidth,
            isWorkspaceWindow: loadsWorkspace,
            requestClose: { window in requestWorkspaceWindowClose(window, model: model, interface: interface) }
        ))
        .onDrop(of: [.fileURL], isTargeted: $isDropTargeted) { providers in
            guard loadsWorkspace else { return false }
            for provider in providers {
                _ = provider.loadObject(ofClass: URL.self) { url, _ in
                    guard let url else { return }
                    Task { @MainActor in ExternalFileQueue.shared.enqueue([url]) }
                }
            }
            return !providers.isEmpty
        }
        .overlay {
            if isDropTargeted {
                RoundedRectangle(cornerRadius: 10)
                    .strokeBorder(Color.accentColor, lineWidth: 3)
                    .padding(4)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
        }
        .fileImporter(
            isPresented: $interface.isShowingImporter,
            allowedContentTypes: [.json, .data],
            allowsMultipleSelection: false,
            onCompletion: handleImport
        )
        .fileDialogMessage("Choose a collection or archive to import.")
        .fileDialogConfirmationLabel("Import")
        .sheet(isPresented: $model.isShowingWorkspaceSettings) {
            WorkspaceNetworkSettings(model: model)
        }
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
                : model.operationFailure?.kind == "export"
                    ? (model.exportFailureMessage ?? "The saved request could not be exported.")
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
            "Delete “\(interface.workspaceDeleteRequest?.title ?? "")”?",
            isPresented: Binding(
                get: { interface.workspaceDeleteRequest != nil },
                set: { if $0 == false { interface.workspaceDeleteRequest = nil } }
            ),
            presenting: interface.workspaceDeleteRequest
        ) { _ in
            Button("Cancel", role: .cancel) {
                interface.workspaceDeleteRequest = nil
            }
            Button("Delete", role: .destructive) {
                interface.confirmWorkspaceDelete(model: model)
            }
        } message: { request in
            Text(request.detail)
        }
        .onAppear {
            model.undoManager = undoManager
            interface.reopenLastDocument(model: model)
            interface.synchronizeSelection(model: model)
        }
        .onChange(of: undoManager) { model.undoManager = undoManager }
        .task {
            // The model outlives the window; reopening it must not reload the workspace.
            guard loadsWorkspace, !model.hasLoadedWorkspace else { return }
            PerformanceProbe.beginWorkspaceLoad()
            let url = RustWorkspacePersistence.defaultWorkspaceURL
            let persistence = await Task.detached(priority: .userInitiated) {
                try? RustWorkspacePersistence(path: url)
            }.value
            if let persistence {
                model.configurePersistence(persistence, gitCollaboration: persistence)
                interface.workspaceURL = url
                RecentWorkspaces.shared.note(url)
            }
            await model.loadWorkspace(restoring: interface.savedSessionLayout())
            PerformanceProbe.endWorkspaceLoad()
            interface.synchronizeSelection(model: model)
            await Task.yield()
            // Ready means the loaded sidebar and restored tabs are on screen, not the skeleton.
            PerformanceProbe.markReady()
            await importPendingFiles()
        }
        .onChange(of: ExternalFileQueue.shared.pending) {
            Task { await importPendingFiles() }
        }
    }

    private func importPendingFiles() async {
        guard loadsWorkspace, model.hasLoadedWorkspace else { return }
        for url in ExternalFileQueue.shared.drain() {
            await importExternalFile(url, model: model)
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

        if #available(macOS 26.0, *) {
            // Pins the window actions to the trailing edge. Without it they follow the
            // environment picker whenever the window has no visible title to push them over.
            ToolbarSpacer(.flexible, placement: .primaryAction)
        }
        ToolbarItem(id: "workspace-settings", placement: .primaryAction) {
            Button("Workspace Settings", systemImage: "gearshape") { model.isShowingWorkspaceSettings = true }
                .labelStyle(.iconOnly).buttonStyle(.borderless).help("Workspace Settings")
        }
        ToolbarItem(id: "response-placement", placement: .primaryAction) {
            Button {
                interface.responseOrientation = interface.responseOrientation == .bottom ? .right : .bottom
            } label: {
                // Shows the current layout, like Xcode's area toggles.
                Image(systemName: interface.responseOrientation == .right
                    ? "rectangle.righthalf.inset.filled" : "rectangle.bottomthird.inset.filled")
            }
            .accessibilityLabel(interface.responseOrientation == .bottom ? "Place Response on Right" : "Place Response on Bottom")
            .buttonStyle(.borderless)
            .frame(width: 30, height: 30)
            .help(interface.responseOrientation == .bottom ? "Place Response on Right" : "Place Response on Bottom")
        }
    }

    @ViewBuilder
    private var sidebarToggle: some View {
        Button("Toggle Sidebar", systemImage: "sidebar.left") {
            interface.columnVisibility = interface.columnVisibility == .detailOnly ? .all : .detailOnly
        }
        .labelStyle(.iconOnly).buttonStyle(.borderless).frame(width: 26, height: 30)
        .help(interface.columnVisibility == .detailOnly ? "Show Sidebar (⌃⌘S)" : "Hide Sidebar (⌃⌘S)")
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

/// Names the window "<Workspace> — <Request>" for the Window menu, Mission Control and
/// VoiceOver. Isolated so title changes do not re-render the workspace.
private struct WorkspaceWindowTitle: View {
    let model: WireboltModel

    var body: some View {
        // Never empty: an untitled window has no name in the Window menu or Mission Control.
        let workspace = model.workspace.name.isEmpty ? "Wirebolt" : model.workspace.name
        let request = model.sessions.activeSession?.title ?? ""
        Color.clear
            .navigationTitle(request.isEmpty ? workspace : "\(workspace) — \(request)")
            .accessibilityHidden(true)
    }
}

private struct SidebarLoadingSkeleton: View {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array([96.0, 150, 128, 162, 112, 140].enumerated()), id: \.offset) { _, width in
                RoundedRectangle(cornerRadius: 4)
                    .fill(Color.secondary.opacity(0.14))
                    .frame(width: width, height: 11)
            }
        }
        .padding(.leading, 28).padding(.top, 62)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background {
            if reduceTransparency { Color(nsColor: .windowBackgroundColor) } else { SidebarMaterialView() }
        }
        .accessibilityElement()
        .accessibilityLabel("Loading workspace")
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
    @Environment(\.controlActiveState) private var controlActiveState

    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let showsMaterial: Bool

    @FocusState private var filterIsFocused: Bool
    @FocusState private var sidebarIsFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if showsMaterial { WorkspaceSidebarOutline(model: model, interface: interface) }
                }
                .padding(.leading, 18).padding(.trailing, 9)
                .font(.system(size: 13))
            }
            .onChange(of: interface.sidebarScrollTarget) { _, target in
                guard let target else { return }
                proxy.scrollTo(target)
                interface.sidebarScrollTarget = nil
            }
        }
        .scrollIndicators(.never)
        .overlay {
            if showsMaterial {
                SidebarSearchEmptyState(model: model, interface: interface)
                SidebarEmptyWorkspaceState(model: model, interface: interface)
            }
        }
        .clipped()
        // Selection is drawn emphasized (accent) only while the sidebar has keyboard focus,
        // like a native source list, so focus stays visible without a ring around the list.
        .environment(\.sidebarSelectionIsEmphasized, sidebarIsFocused && controlActiveState != .inactive)
        .overlay {
            if sidebarIsFocused { SidebarFocusRing(model: model, interface: interface) }
        }
        .focusable().focusEffectDisabled().focused($sidebarIsFocused)
        .modifier(SidebarMoveFocusedValue(model: model, interface: interface, isFocused: sidebarIsFocused))
        .onChange(of: interface.focusSidebarTrigger) {
            if !interface.isRenamingInSidebar { sidebarIsFocused = true }
        }
        .task {
            // Focus Sidebar while the sidebar was hidden: take focus once it is on screen.
            guard interface.focusesSidebarOnAppear else { return }
            interface.focusesSidebarOnAppear = false
            await Task.yield()
            sidebarIsFocused = true
        }
        .background { SidebarCursorReset(model: model, interface: interface) }
        .onDeleteCommand {
            if !interface.isRenamingInSidebar { interface.requestDeleteOfSelection(model: model) }
        }
        .onMoveCommand { direction in
            guard !interface.isRenamingInSidebar else { return }
            switch direction {
            case .up: interface.moveSidebarSelection(-1, model: model)
            case .down: interface.moveSidebarSelection(1, model: model)
            case .left: interface.moveSidebarSelectionHorizontally(right: false, model: model)
            case .right: interface.moveSidebarSelectionHorizontally(right: true, model: model)
            @unknown default: break
            }
        }
        .onKeyPress(.return) {
            guard !interface.isRenamingInSidebar else { return .ignored }
            interface.renameSidebarSelection(model: model)
            return .handled
        }
        .onKeyPress(.downArrow, phases: .down) { press in
            guard press.modifiers == .command, !interface.isRenamingInSidebar else { return .ignored }
            interface.openSidebarSelection(model: model)
            return .handled
        }
        .onKeyPress(phases: .down) { press in
            guard !interface.isRenamingInSidebar, press.modifiers.isDisjoint(with: [.command, .control]),
                  !press.characters.isEmpty,
                  press.characters.unicodeScalars.allSatisfy({ scalar in
                      !CharacterSet.controlCharacters.contains(scalar) && !(0xF700...0xF8FF).contains(scalar.value)
                  })
            else { return .ignored }
            return interface.typeSelectInSidebar(press.characters, model: model) ? .handled : .ignored
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Requests")
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

/// The focus ring of a focused sidebar without a selected row. Isolated (like the modifier
/// below) so selection changes don't re-render the whole sidebar.
private struct SidebarFocusRing: View {
    let model: WireboltModel
    let interface: WorkspaceUIState

    var body: some View {
        if interface.sidebarSelectionRowID(model: model) == nil {
            RoundedRectangle(cornerRadius: WireboltTheme.Radius.control)
                .strokeBorder(Color(nsColor: .keyboardFocusIndicatorColor), lineWidth: 3)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
        }
    }
}

/// Publishes Move Up/Down for the selection while the sidebar has focus.
private struct SidebarMoveFocusedValue: ViewModifier {
    let model: WireboltModel
    let interface: WorkspaceUIState
    let isFocused: Bool

    func body(content: Content) -> some View {
        content.focusedValue(\.sidebarMove, isFocused ? interface.sidebarMoveCommands(model: model) : nil)
    }
}

/// Opening a request elsewhere (tabs, history) moves the highlight back to it. Isolated so
/// tab switches don't re-render the whole sidebar.
private struct SidebarCursorReset: View {
    let model: WireboltModel
    let interface: WorkspaceUIState

    var body: some View {
        Color.clear
            .onChange(of: model.selectedRequestID) { interface.sidebarCursor = nil }
            .accessibilityHidden(true)
    }
}

private extension EnvironmentValues {
    /// True while the sidebar has keyboard focus in the key window.
    @Entry var sidebarSelectionIsEmphasized = false
}

/// Accent fill while the sidebar has focus, a neutral gray otherwise.
private struct SidebarSelectionBackground: View {
    let isSelected: Bool
    var leadingInset: CGFloat = 0
    @Environment(\.sidebarSelectionIsEmphasized) private var emphasized
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        GeometryReader { geometry in
            RoundedRectangle(cornerRadius: WireboltTheme.Radius.row)
                .fill(fill)
                .frame(width: geometry.size.width + leadingInset, height: 24)
                .offset(x: -leadingInset)
        }
    }

    private var fill: Color {
        guard isSelected else { return .clear }
        guard emphasized else { return WireboltTheme.unemphasizedSidebarSelection }
        // Opaque in light mode so white labels keep 4.5:1 over the sidebar.
        return WireboltTheme.primaryAccent.opacity(colorScheme == .dark ? 0.82 : 1)
    }
}

private struct SidebarSearchEmptyState: View {
    let model: WireboltModel
    let interface: WorkspaceUIState

    var body: some View {
        let query = interface.sidebarFilter.trimmingCharacters(in: .whitespacesAndNewlines)
        if !query.isEmpty, interface.visibleSidebarRows(model: model).isEmpty {
            ContentUnavailableView.search(text: query)
                .padding(.horizontal, WireboltTheme.Spacing.small)
        }
    }
}

/// With no collection left, the sidebar offers the ways to start instead of staying blank.
private struct SidebarEmptyWorkspaceState: View {
    let model: WireboltModel
    let interface: WorkspaceUIState

    var body: some View {
        if model.hasLoadedWorkspace, model.workspace.collections.isEmpty, !interface.isFilteringSidebar {
            ContentUnavailableView {
                Label("No Requests", systemImage: "tray")
            } description: {
                Text("Requests you create or import appear here.")
            } actions: {
                Button("New Request") { interface.makeNewRequest(model: model) }
                    .help("New Request (⌘N)")
                WorkspaceImportMenu(interface: interface)
                    .fixedSize()
            }
            .padding(.horizontal, WireboltTheme.Spacing.small)
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
            // Shortcuts are declared once, on the File menu commands.
            Menu("New Request") {
                Button("HTTP") { interface.makeNewRequest(model: model) }
                Button("WebSocket") { interface.makeNewRequest(model: model, kind: .webSocket) }
            }
            Button("New Folder") {
                interface.makeNewFolder(model: model)
            }
            Divider()
            Button("New Collection…") { interface.promptForNewCollection() }
            Button("Open Workspace…") { chooseWorkspace(model: model, interface: interface, create: false) }
                .disabled(model.isLoadingWorkspace || model.isGitBusy || model.isOAuthBusy)
            Button("New Workspace…") { chooseWorkspace(model: model, interface: interface, create: true) }
                .disabled(model.isLoadingWorkspace || model.isGitBusy || model.isOAuthBusy)
            Button("Workspace Settings…") { model.isShowingWorkspaceSettings = true }
            Button("Git Collaboration…") { model.isShowingGitCollaboration = true }
            Divider()
            WorkspaceImportMenu(interface: interface)
            Button("Export Wirebolt JSON…") {
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
            Button("Wirebolt / Legacy Collection v1 JSON") { open(.legacyWorkspaceV1) }
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
        // Exactly one subview per row: a conditional sibling would make the lazy stack keep
        // off-screen rows alive and resolve every element to count its views.
        ForEach(interface.visibleSidebarRows(model: model)) { row in
            SidebarOutlineRow(row: row, model: model, interface: interface)
                .id(row.id)
        }
    }
}

private struct SidebarOutlineRow: View {
    let row: SidebarSnapshot.Row
    let model: WireboltModel
    let interface: WorkspaceUIState

    var body: some View {
        if case .collection(let collection) = row.content, collection.groups.isEmpty, collection.requests.isEmpty {
            VStack(alignment: .leading, spacing: 0) {
                content
                if interface.isSidebarRowExpanded(row) {
                    EmptyCollectionRow(collection: collection, model: model, interface: interface)
                }
            }
        } else {
            content
        }
    }

    private var content: some View {
        Group {
            switch row.content {
            case .collection(let collection):
                SavedCollectionRow(row: row, collection: collection, model: model, interface: interface)
            case .group(let collection, let group):
                SavedGroupRow(row: row, collection: collection, group: group, model: model, interface: interface)
            case .request(let location):
                SavedRequestRow(model: model, interface: interface, location: location, depth: row.depth)
            }
        }
        .padding(.leading, Double(row.depth) * 14)
        .frame(height: 24)
        .modifier(SidebarReorderTarget(row: row, model: model, interface: interface))
    }
}

/// An empty collection offers its next step inline instead of looking like a dead end.
private struct EmptyCollectionRow: View {
    let collection: CollectionDraft
    let model: WireboltModel
    let interface: WorkspaceUIState

    var body: some View {
        Button {
            interface.makeNewRequest(model: model, collectionID: collection.id)
        } label: {
            Label("New Request", systemImage: "plus")
                .frame(maxWidth: .infinity, minHeight: 24, alignment: .leading)
                .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .foregroundStyle(.secondary)
        .padding(.leading, 32)
        .frame(height: 24)
        .accessibilityLabel("New Request in \(collection.name)")
        .help("Create a request in “\(collection.name)”")
    }
}

@MainActor
private func sidebarDragProvider(_ identifier: String, interface: WorkspaceUIState) -> NSItemProvider {
    interface.sidebarDragIdentifier = identifier
    return NSItemProvider(object: identifier as NSString)
}

private enum SidebarDropPosition: Sendable { case before, after, inside }

private struct SidebarReorderTarget: ViewModifier {
    let row: SidebarSnapshot.Row
    let model: WireboltModel
    let interface: WorkspaceUIState
    @State private var position: SidebarDropPosition?

    func body(content: Content) -> some View {
        switch row.content {
        case .collection:
            content
        default:
            content
                .onDrag { sidebarDragProvider(row.moveIdentifier ?? "", interface: interface) }
                .overlay(alignment: position == .after ? .bottom : .top) {
                    if let position {
                        if position == .inside {
                            RoundedRectangle(cornerRadius: 4).stroke(Color.accentColor, lineWidth: 2)
                                .allowsHitTesting(false)
                        } else {
                            Rectangle().fill(Color.accentColor).frame(height: 2)
                                .allowsHitTesting(false)
                        }
                    }
                }
                .background { SidebarDropZone(row: row, model: model, interface: interface, position: $position) }
        }
    }
}

/// A row's drop target. Rows only accept drags that started in the sidebar, so the target
/// (an AppKit view per row) exists only while one is in progress; creating it for every row
/// made opening and filtering the sidebar markedly slower.
private struct SidebarDropZone: View {
    let row: SidebarSnapshot.Row
    let model: WireboltModel
    let interface: WorkspaceUIState
    @Binding var position: SidebarDropPosition?

    var body: some View {
        if interface.sidebarDragIdentifier != nil {
            Color.clear
                .contentShape(.rect)
                .onDrop(of: [UTType.text], delegate: SidebarReorderDrop(
                    row: row, model: model, interface: interface, position: $position))
        }
    }
}

private struct SidebarReorderDrop: DropDelegate {
    let row: SidebarSnapshot.Row
    let model: WireboltModel
    let interface: WorkspaceUIState
    @Binding var position: SidebarDropPosition?

    private var target: (collection: String, parent: String?, item: String, folder: String?) {
        switch row.content {
        case .group(let collection, let group):
            return (collection.id, group.parentID, "group:" + group.id, group.id)
        case .request(let location):
            return (location.collectionID, location.groupID, "request:" + location.request.id, nil)
        case .collection(let collection):
            return (collection.id, nil, "", nil)
        }
    }

    private func destination(_ info: DropInfo) -> SidebarDropPosition? {
        guard let identifier = interface.sidebarDragIdentifier else { return nil }
        let target = target
        if let folder = target.folder, info.location.y >= 6, info.location.y <= 18 {
            let parts = identifier.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
            guard parts.count == 3 else { return nil }
            if parts[0] == "request" { return .inside }
            if parts[0] == "group", parts[1] == target.collection,
               let collection = model.workspace.collections.first(where: { $0.id == target.collection }),
               !collection.descendantGroupIDs(of: parts[2]).contains(folder) { return .inside }
            return nil
        }
        guard model.canReorderSidebar(identifier, relativeTo: target.item,
            collectionID: target.collection, parentID: target.parent) else { return nil }
        return info.location.y < 12 ? .before : .after
    }

    func validateDrop(info: DropInfo) -> Bool { destination(info) != nil }
    func dropEntered(info: DropInfo) { position = destination(info) }
    func dropExited(info: DropInfo) { position = nil }
    func dropUpdated(info: DropInfo) -> DropProposal? {
        position = destination(info)
        return DropProposal(operation: position == nil ? .forbidden : .move)
    }
    func performDrop(info: DropInfo) -> Bool {
        guard let destination = destination(info), let expected = interface.sidebarDragIdentifier,
              let provider = info.itemProviders(for: [UTType.text]).first else { return false }
        position = nil
        interface.sidebarDragIdentifier = nil
        let target = target
        let model = model
        // Verify the actual pasteboard payload, rather than trusting a previous drag's UI state.
        provider.loadObject(ofClass: NSString.self) { value, _ in
            guard let identifier = value as? String, identifier == expected else { return }
            Task { @MainActor in
                if destination == .inside, let folder = target.folder {
                    await model.moveSidebarItem(identifier, toCollectionID: target.collection, parentID: folder)
                } else {
                    await model.reorderSidebar(identifier, relativeTo: target.item, after: destination == .after,
                                               collectionID: target.collection, parentID: target.parent)
                }
            }
        }
        return true
    }

}

private struct SavedRequestRow: View {
    let model: WireboltModel
    let interface: WorkspaceUIState
    let location: RequestLocation
    let depth: Int

    var body: some View {
        SidebarRequestButton(model: model, interface: interface, location: location, depth: depth,
            isSelected: model.selectedRequestID == location.id && interface.sidebarCursor == nil,
            action: {
                interface.sidebarCursor = nil
                interface.activateSavedRequest(location, model: model)
                interface.focusSidebarTrigger += 1
            },
            onSplit: {
                interface.activateSavedRequest(location, model: model)
                if let tabID = model.sessions.activeSession?.id { interface.openInNewSplit(tabID: tabID, model: model) }
            },
            onRename: { name in Task { await model.renameRequest(collectionID: location.collectionID, requestID: location.request.id, name: name) } },
            onDuplicate: { Task { await model.duplicateRequest(collectionID: location.collectionID, requestID: location.request.id) } },
            onExport: { Task {
                if let document = await model.exportRequest(collectionID: location.collectionID, id: location.request.id) {
                    saveExportedDocument(named: location.request.name, content: document)
                }
            } },
            onDelete: {
                interface.requestDelete(.request(collectionID: location.collectionID, id: location.request.id),
                                        title: location.request.name, model: model)
            }
        ).equatable()
    }
}

/// A collection or folder row: disclosure chevron, name, selection and outline accessibility.
private struct SidebarContainerRow<Name: View, Actions: View>: View {
    let kind: String
    let title: String
    let depth: Int
    let isExpanded: Bool
    let isSelected: Bool
    let isEditing: Bool
    var isHeader = false
    let toggle: () -> Void
    let select: () -> Void
    let actions: SidebarRowActions
    @ViewBuilder let name: Name
    @ViewBuilder let menu: Actions
    @Environment(\.sidebarSelectionIsEmphasized) private var emphasized

    var body: some View {
        let highlighted = isSelected && emphasized
        HStack(spacing: 0) {
            Button(action: toggle) {
                Image(systemName: isExpanded ? "chevron.down" : "chevron.forward")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(highlighted ? AnyShapeStyle(Color.white) : AnyShapeStyle(.secondary))
                    .frame(width: 12, height: 24).padding(.trailing, 6).contentShape(.rect)
            }
            .buttonStyle(.plain)
            .help(isExpanded ? "Collapse" : "Expand")
            let label = HStack(spacing: 6) {
                Image(systemName: "folder.fill")
                name
            }
            .foregroundStyle(highlighted ? Color.white : Color.primary)
            .frame(maxWidth: .infinity, minHeight: 24, alignment: .leading)
            .contentShape(.rect)
            if isEditing {
                label
            } else {
                Button { select(); toggle() } label: { label }.buttonStyle(.plain)
            }
        }
        .frame(height: 24)
        .background { SidebarSelectionBackground(isSelected: isSelected, leadingInset: 4) }
        .contextMenu { menu }
        // One outline row for VoiceOver: kind, name, state and level, with its actions.
        .accessibilityElement(children: isEditing ? .contain : .ignore)
        .accessibilityLabel("\(kind), \(title)")
        .accessibilityValue("\(isExpanded ? "Expanded" : "Collapsed"), level \(depth + 1)")
        .accessibilityAddTraits(isHeader ? [.isHeader, .isButton] : .isButton)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .accessibilityAction { select(); toggle() }
        .accessibilityAction(named: isExpanded ? "Collapse" : "Expand", toggle)
        .modifier(SidebarAccessibilityActions(actions: actions))
    }
}

/// Custom VoiceOver actions shared by every sidebar row.
private struct SidebarRowActions {
    let newRequest: () -> Void
    let rename: () -> Void
    let delete: () -> Void
    var move: ((Int) -> Void)?
}

private struct SidebarAccessibilityActions: ViewModifier {
    let actions: SidebarRowActions

    // Plain named actions rather than an `accessibilityActions` builder: rows are created
    // on every filter keystroke, and the builder's proxy modifier costs more per row.
    func body(content: Content) -> some View {
        if let move = actions.move {
            content
                .accessibilityAction(named: "New Request", actions.newRequest)
                .accessibilityAction(named: "Rename", actions.rename)
                .accessibilityAction(named: "Move Up") { move(-1) }
                .accessibilityAction(named: "Move Down") { move(1) }
                .accessibilityAction(named: "Delete", actions.delete)
        } else {
            content
                .accessibilityAction(named: "New Request", actions.newRequest)
                .accessibilityAction(named: "Rename", actions.rename)
                .accessibilityAction(named: "Delete", actions.delete)
        }
    }
}

/// Move Up/Down and Move To for a request or folder. Built only when its menu is shown,
/// so rows don't pay for sibling and destination lookups while rendering.
private struct SidebarMoveMenu: View {
    let model: WireboltModel
    let interface: WorkspaceUIState
    let identifier: String

    var body: some View {
        Button("Move Up") { interface.moveSidebarItem(identifier, by: -1, model: model) }
            .keyboardShortcut(.upArrow, modifiers: [.command, .option])
            .disabled(!model.canMoveSidebarItem(identifier, by: -1))
        Button("Move Down") { interface.moveSidebarItem(identifier, by: 1, model: model) }
            .keyboardShortcut(.downArrow, modifiers: [.command, .option])
            .disabled(!model.canMoveSidebarItem(identifier, by: 1))
        Menu("Move To") {
            let isFolder = identifier.hasPrefix("group|")
            let sourceCollection = identifier.split(separator: "|", omittingEmptySubsequences: false).dropFirst().first.map(String.init)
            let collections = model.workspace.collections
                .filter { !isFolder || $0.id == sourceCollection }
                .sorted { ($0.order, $0.name, $0.id) < ($1.order, $1.name, $1.id) }
            ForEach(collections) { collection in
                if collection.id != collections.first?.id { Divider() }
                // The root collection has no sidebar header; its items sit at the top level.
                destination(collection.id == WorkspaceDraft.rootCollectionID ? "Top Level" : collection.name,
                            collectionID: collection.id, parentID: nil)
                ForEach(collection.folderOutline(), id: \.group.id) { entry in
                    destination(String(repeating: "    ", count: entry.depth + 1) + entry.group.name,
                                collectionID: collection.id, parentID: entry.group.id)
                }
            }
        }
    }

    private func destination(_ title: String, collectionID: String, parentID: String?) -> some View {
        Button(title) {
            Task { await model.moveSidebarItem(identifier, toCollectionID: collectionID, parentID: parentID) }
        }
        .disabled(!model.canMoveSidebarItem(identifier, toCollectionID: collectionID, parentID: parentID))
    }
}

struct InlineSidebarName: View {
    let title: String
    /// A value rather than a binding: a closure-built binding counts as changed on every
    /// update, which re-rendered each sidebar row's name several times per update.
    let isEditing: Bool
    var renameOnDoubleClick = false
    let setEditing: (Bool) -> Void
    let save: (String) -> Void
    @State private var value = ""
    @FocusState private var focused: Bool

    init(title: String, isEditing: Binding<Bool>, renameOnDoubleClick: Bool = false, save: @escaping (String) -> Void) {
        self.title = title
        self.isEditing = isEditing.wrappedValue
        self.renameOnDoubleClick = renameOnDoubleClick
        setEditing = { isEditing.wrappedValue = $0 }
        self.save = save
    }

    var body: some View {
        Group {
            if isEditing {
                TextField("Name", text: $value)
                    .textFieldStyle(.plain).focused($focused)
                    .onSubmit(commit)
                    .onExitCommand { setEditing(false) }
                    .onChange(of: focused) { _, focused in if !focused && isEditing { commit() } }
            } else if renameOnDoubleClick {
                Text(title).lineLimit(1)
                    .contentShape(.rect)
                    .onTapGesture(count: 2) { setEditing(true) }
            } else { Text(title).lineLimit(1) }
        }
        // Only a rename does work here; rows are created on every filter change, so they
        // don't start a task each.
        .onChange(of: isEditing, initial: true) {
            guard isEditing else { return }
            value = title
            Task {
                await Task.yield()
                if isEditing { focused = true }
            }
        }
    }

    private func commit() {
        let name = value.trimmingCharacters(in: .whitespacesAndNewlines)
        setEditing(false)
        if !name.isEmpty && name != title { save(name) }
    }
}

private struct SavedCollectionRow: View {
    let row: SidebarSnapshot.Row
    let collection: CollectionDraft
    let model: WireboltModel
    let interface: WorkspaceUIState

    var body: some View {
        let isExpanded = interface.isSidebarRowExpanded(row)
        SidebarContainerRow(
            kind: "Collection", title: collection.name, depth: row.depth,
            isExpanded: isExpanded,
            isSelected: interface.sidebarCursor == row.id,
            isEditing: interface.renamingCollectionID == collection.id,
            isHeader: true,
            toggle: { interface.setSidebarRow(row, expanded: !interface.isSidebarRowExpanded(row)) },
            select: { interface.sidebarCursor = row.id; interface.focusSidebarTrigger += 1 },
            actions: SidebarRowActions(
                newRequest: { interface.makeNewRequest(model: model, collectionID: collection.id) },
                rename: { interface.beginRename(row) },
                delete: { interface.requestDelete(.collection(id: collection.id), title: collection.name, model: model) }
            )
        ) {
            InlineSidebarName(title: collection.name, isEditing: Binding(
                get: { interface.renamingCollectionID == collection.id },
                set: { editing in
                    if editing { interface.renamingCollectionID = collection.id }
                    else if interface.renamingCollectionID == collection.id { interface.renamingCollectionID = nil }
                }
            )) { name in
                Task { await model.renameCollection(id: collection.id, name: name) }
            }
        } menu: {
            NewRequestMenu(model: model, interface: interface, collectionID: collection.id)
            Button("New Folder") {
                interface.makeNewFolder(model: model, collectionID: collection.id)
            }
            Divider()
            WorkspaceImportMenu(interface: interface)
            Button("Export Wirebolt JSON…") {
                Task {
                    if let document = await model.exportCollection(id: collection.id) {
                        saveExportedDocument(named: collection.name, content: document)
                    }
                }
            }
            Divider()
            Button("Rename") { interface.beginRename(row) }
            Button("Delete", role: .destructive) {
                interface.requestDelete(.collection(id: collection.id), title: collection.name, model: model)
            }
        }
        .dropDestination(for: String.self) { identifiers, _ in
            guard let identifier = identifiers.first,
                  model.canMoveSidebarItem(identifier, toCollectionID: collection.id, parentID: nil) else { return false }
            Task { await model.moveSidebarItem(identifier, toCollectionID: collection.id, parentID: nil) }
            return true
        }
    }
}

private struct SavedGroupRow: View {
    let row: SidebarSnapshot.Row
    let collection: CollectionDraft
    let group: GroupDraft
    let model: WireboltModel
    let interface: WorkspaceUIState

    var body: some View {
        let identifier = "group|\(collection.id)|\(group.id)"
        SidebarContainerRow(
            kind: "Folder", title: group.name, depth: row.depth,
            isExpanded: interface.isSidebarRowExpanded(row),
            isSelected: interface.sidebarCursor == row.id,
            isEditing: interface.renamingGroupID == group.id,
            toggle: { interface.setSidebarRow(row, expanded: !interface.isSidebarRowExpanded(row)) },
            select: { interface.sidebarCursor = row.id; interface.focusSidebarTrigger += 1 },
            actions: SidebarRowActions(
                newRequest: { interface.makeNewRequest(model: model, collectionID: collection.id, groupID: group.id) },
                rename: { interface.beginRename(row) },
                delete: { delete() },
                move: { delta in interface.moveSidebarItem(identifier, by: delta, model: model) }
            )
        ) {
            InlineSidebarName(title: group.name, isEditing: Binding(
                get: { interface.renamingGroupID == group.id },
                set: { editing in
                    if editing { interface.renamingGroupID = group.id }
                    else if interface.renamingGroupID == group.id { interface.renamingGroupID = nil }
                }
            )) { name in
                Task { await model.renameGroup(collectionID: collection.id, id: group.id, name: name) }
            }
        } menu: {
            NewRequestMenu(model: model, interface: interface, collectionID: collection.id, groupID: group.id)
            Button("New Folder") {
                interface.setSidebarRow(row, expanded: true)
                interface.makeNewFolder(model: model, collectionID: collection.id, parentID: group.id)
            }
            Divider()
            Button("Rename") { interface.beginRename(row) }
            SidebarMoveMenu(model: model, interface: interface, identifier: identifier)
            Divider()
            Button("Delete", role: .destructive, action: delete)
        }
    }

    private func delete() {
        interface.requestDelete(.group(collectionID: collection.id, id: group.id), title: group.name, model: model)
    }
}

private struct SidebarRequestButton: View, @MainActor Equatable {
    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.model === rhs.model && lhs.interface === rhs.interface && lhs.location == rhs.location
            && lhs.isSelected == rhs.isSelected && lhs.depth == rhs.depth
    }

    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    @Environment(\.sidebarSelectionIsEmphasized) private var emphasized

    let location: RequestLocation
    let depth: Int
    let isSelected: Bool
    let action: () -> Void
    let onSplit: () -> Void
    let onRename: (String) -> Void
    let onDuplicate: () -> Void
    let onExport: () -> Void
    let onDelete: () -> Void

    private var isRenaming: Bool { interface.renamingRequestID == location.id }
    private var identifier: String { "request|\(location.collectionID)|\(location.request.id)" }

    var body: some View {
        let highlighted = isSelected && emphasized
            HStack(spacing: 3) {
                Text(location.request.webSocket ? "WS" : location.request.method.rawValue)
                    .font(WireboltTheme.Typography.methodLabel)
                    .lineLimit(1).minimumScaleFactor(0.8)
                    // Method hues can't keep contrast on the accent selection fill.
                    .foregroundStyle(highlighted ? Color.white : isSelected ? Color.primary
                        : WireboltTheme.methodColor(location.request.method, webSocket: location.request.webSocket))
                    .frame(width: 40, alignment: .trailing)
                InlineSidebarName(title: location.request.name, isEditing: Binding(
                    get: { interface.renamingRequestID == location.id },
                    set: { editing in
                    if editing { interface.renamingRequestID = location.id }
                    else if interface.renamingRequestID == location.id { interface.renamingRequestID = nil }
                }
                ), renameOnDoubleClick: true, save: onRename)
                    .lineLimit(1)
                    .foregroundStyle(highlighted ? Color.white : Color.primary)
                Spacer(minLength: 0)
            }
            .font(.system(size: 13))
            .padding(.trailing, 6)
            .frame(height: 24)
            .contentShape(.rect)
            .background {
                SidebarSelectionBackground(
                    isSelected: isSelected,
                    leadingInset: location.collectionID == WorkspaceDraft.rootCollectionID && location.groupID == nil ? 0 : 14
                )
            }
        .buttonStyle(.plain)
        // Selection must not wait for the name's double-click rename gesture to fail.
        .simultaneousGesture(TapGesture().onEnded {
            if !isRenaming { action() }
        })
        .accessibilityElement(children: isRenaming ? .contain : .combine)
        .accessibilityAction(.default, action)
        .frame(height: 24)
        .listRowInsets(EdgeInsets(top: 0, leading: 0, bottom: 0, trailing: 0))
        .contextMenu {
            NewRequestMenu(model: model, interface: interface, collectionID: location.collectionID, groupID: location.groupID)
            Button("New Folder") { interface.makeNewFolder(model: model, collectionID: location.collectionID, parentID: location.groupID) }
            Divider()
            Button("Open in New Split", action: onSplit)
            Divider()
            WorkspaceImportMenu(interface: interface)
            Button("Export Wirebolt JSON…", action: onExport)
            Divider()
            Button("Copy cURL") { copyRequestAsCurl(location.request, model: model) }
            Divider()
            Button("Rename") { interface.renamingRequestID = location.id }
            Button("Duplicate", action: onDuplicate)
            SidebarMoveMenu(model: model, interface: interface, identifier: identifier)
            Divider()
            Button("Delete", role: .destructive, action: onDelete)
        }
        .accessibilityLabel("\(location.request.webSocket ? "WebSocket" : location.request.method.rawValue) request, \(location.request.name)")
        .accessibilityValue("level \(depth + 1)")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .modifier(SidebarAccessibilityActions(actions: SidebarRowActions(
            newRequest: { interface.makeNewRequest(model: model, collectionID: location.collectionID, groupID: location.groupID) },
            rename: { interface.renamingRequestID = location.id },
            delete: onDelete,
            move: { delta in interface.moveSidebarItem(identifier, by: delta, model: model) }
        )))
    }
}

@MainActor
func copyRequestAsCurl(_ request: RequestDraft, model: WireboltModel) {
    Task {
        guard let command = await model.curlCommand(for: request) else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(command, forType: .string)
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
            TextField("Filter", text: $interface.sidebarFilter)
                .textFieldStyle(.plain)
                .focused(filterIsFocused)
                .help("Filter Requests (⇧⌘F)")
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
                .help("Dismiss")
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
        VariableCatalogScope(model: model) {
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
        .background { WindowEditedIndicator(model: model) }
    }
}

/// Provides the active environments' variables to URL and key/value fields. Isolated so
/// only workspace or environment changes rebuild the catalog.
private struct VariableCatalogScope<Content: View>: View {
    let model: WireboltModel
    @ViewBuilder let content: Content

    var body: some View {
        content.environment(\.variableCatalog, VariableCatalog(
            environments: model.workspace.environments,
            selectedEnvironmentID: model.selectedEnvironmentID
        ))
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
                // Stays mounted across tab switches; the URL field inside is per document.
                RequestURLBar(model: model, interface: interface, session: session, groupID: groupID)
                ResponseSplit(layout: interface.responseLayout(for: groupID), orientation: interface.responseOrientation, minimumResponseWidth: 349) {
                    RequestWorkspace(
                        model: model,
                        interface: interface,
                        presentation: presentation,
                        session: session,
                        groupID: groupID
                    )
                } response: {
                    Group {
                    if session.kind == .http {
                        ResponseViewer(
                            interface: presentation,
                            session: session,
                            send: {
                                model.sessions.select(tabID: session.id, in: groupID)
                                interface.synchronizeSelection(model: model)
                                Task { await model.send(session) }
                            },
                            cancel: { model.cancel(session) }
                        )
                    } else {
                        WebSocketResponseView(session: session)
                    }
                    }
                    // Each document gets fresh response views (scroll, find, renderer state).
                    .id(session.id)
                    .background { ResponseFocusAnchor(model: model, interface: interface, groupID: groupID) }
                }
                .environment(\.editorStorage, presentation.editorStorage)
                .simultaneousGesture(TapGesture().onEnded {
                    if model.sessions.activeGroupID != groupID {
                        interface.activateTab(id: session.id, groupID: groupID, model: model)
                    }
                })
                // The split and the request section bar hold no per-document state, so they
                // stay mounted across tab switches; only the editors below them are rebuilt.
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                // Fill the detail area so the empty state is centered below the tab bar.
                NoOpenRequestPlaceholder(model: model, interface: interface)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
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

/// Which edges of the tab strip have tabs scrolled out of view.
private struct TabStripOverflow: Equatable {
    var leading = false
    var trailing = false
}

private struct DocumentTabBar: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let groupID: String
    // No @State here: this bar re-renders on every tab switch, and a stateful body makes
    // that update measurably slower. The strip's overflow state lives in TabStripScroller.

    /// Tabs shrink to this width before the strip starts scrolling.
    private static let minimumTabWidth = 96.0

    var body: some View {
        HStack(spacing: 0) {
            if model.sessions.groups.count > 1 {
                Button("Close Split", systemImage: "xmark") {
                    interface.close(.all, model: model, in: groupID)
                }
                .labelStyle(.iconOnly).buttonStyle(.borderless)
                .foregroundStyle(.secondary).frame(width: 30)
                .help("Close Split")
            }
            Button("Back", systemImage: "chevron.backward", action: goBack)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.tertiary)
                .frame(width: 30)
                .disabled(group?.backwardTabIDs.isEmpty != false)
                .help("Back (⌃⌘←)")
            Button("Forward", systemImage: "chevron.forward", action: goForward)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.tertiary)
                .frame(width: 30)
                .disabled(group?.forwardTabIDs.isEmpty != false)
                .help("Forward (⌃⌘→)")

            GeometryReader { geometry in
            ScrollViewReader { scroll in
            TabStripScroller {
            HStack(spacing: 0) {
                ForEach(tabs) { tab in
                    DocumentTabButton(
                        tab: tab,
                        isSelected: group?.selectedTabID == tab.id,
                        isPreview: model.sessions.isPreview(tabID: tab.id),
                        onSelect: { interface.activateTab(id: tab.id, groupID: groupID, model: model) },
                        onPin: { interface.pinTab(id: tab.id, model: model) },
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
                        .frame(width: max(Self.minimumTabWidth, geometry.size.width / Double(max(1, tabs.count))))
                        .id(tab.id)
                }
            }
            .padding(.top, 2)
            }
            .onChange(of: group?.selectedTabID) { _, selected in
                if let selected { scroll.scrollTo(selected, anchor: .center) }
            }
            }
            }
            .padding(.trailing, WireboltTheme.Spacing.xSmall)
            .frame(maxWidth: .infinity)

            Button("Open in New Split", systemImage: "rectangle.split.2x1") {
                if let selected = group?.selectedTabID {
                    interface.openInNewSplit(tabID: selected, model: model)
                }
            }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 30)
                .disabled(group?.selectedTabID == nil)
                .help("Open in New Split (⇧⌘D)")
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

/// The horizontally scrolling tab strip. Tabs cut off at an edge end at a divider, so they
/// never look like they run underneath the navigation or split buttons.
private struct TabStripScroller<Content: View>: View {
    @ViewBuilder let content: Content
    @State private var overflow = TabStripOverflow()

    var body: some View {
        ScrollView(.horizontal) { content }
            .scrollIndicators(.never)
            .onScrollGeometryChange(for: TabStripOverflow.self) { geometry in
                TabStripOverflow(
                    leading: geometry.contentOffset.x > 0.5,
                    trailing: geometry.contentOffset.x + geometry.containerSize.width < geometry.contentSize.width - 0.5
                )
            } action: { _, overflow in
                self.overflow = overflow
            }
            .clipped()
            .overlay(alignment: .leading) { if overflow.leading { divider } }
            .overlay(alignment: .trailing) { if overflow.trailing { divider } }
    }

    private var divider: some View {
        Rectangle().fill(WireboltTheme.separator).frame(width: 1, height: 16)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

private struct DocumentTabButton: View {
    let tab: DocumentSession
    let isSelected: Bool
    /// Preview tabs are reused by the next sidebar click; their title is italic.
    let isPreview: Bool
    let onSelect: () -> Void
    let onPin: () -> Void
    let onRename: (String) -> Void
    let onClose: () -> Void
    let onCloseOthers: () -> Void
    let onCloseRight: () -> Void
    let onCloseAll: () -> Void

    @State private var isHovered = false
    @State private var isRenaming = false

    var body: some View {
        let isDirty = tab.isDirty
        HStack(spacing: WireboltTheme.Spacing.xxSmall) {
            DocumentTabCloseSlot(
                title: tab.title,
                isDirty: isDirty,
                isRevealed: isHovered || (isSelected && !isDirty),
                isRenaming: isRenaming,
                onClose: onClose
            )

            if isRenaming {
                InlineSidebarName(title: tab.title, isEditing: $isRenaming, save: onRename)
                    .font(.system(size: 12))
                    .frame(height: 19)
                    .accessibilityLabel("Request Name")
            } else {
                Button(action: onSelect) {
                    DocumentTabLabel(title: tab.title, italic: isPreview)
                        .frame(height: 19)
                        .frame(maxWidth: .infinity)
                        .offset(y: -1)
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                        .contentShape(.rect)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(tab.title)
                .accessibilityValue([isDirty ? "Edited" : nil, isPreview ? "Preview" : nil].compactMap(\.self).joined(separator: ", "))
                .accessibilityAction(named: "Close Tab", onClose)
                .help(isPreview ? "\(tab.title) — Preview. Double-click to keep open." : tab.title)
                // Select immediately while still recognizing a double-click, which keeps a
                // preview tab open (Xcode, Finder) or renames a tab that is already kept.
                .simultaneousGesture(TapGesture(count: 2).onEnded {
                    if isPreview { onPin() } else { beginRenaming() }
                })
            }
            // Balances the close slot so the title stays centered.
            Color.clear.frame(width: 20, height: 20)
        }
        .padding(.horizontal, WireboltTheme.Spacing.small)
        .frame(minWidth: 28, minHeight: 28)
        // Only the selected tab draws a capsule; the others add no shapes.
        .background { if isSelected { RoundedRectangle(cornerRadius: 14).fill(Color.primary.opacity(0.055)) } }
        .overlay {
            if isSelected {
                RoundedRectangle(cornerRadius: 14).stroke(WireboltTheme.separator.opacity(0.65), lineWidth: 0.5)
            }
        }
        .onHover { isHovered = $0 }
        .contextMenu {
            if isPreview {
                Button("Keep Open", action: onPin)
                Divider()
            }
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

/// The close button, or the unsaved-edit dot in the same slot so the title never moves.
/// The button shows on hover, keyboard focus and the clean active tab. It owns its focus
/// state so the tab button itself stays cheap to update on every tab switch.
private struct DocumentTabCloseSlot: View {
    let title: String
    let isDirty: Bool
    /// Hovered, or the clean active tab.
    let isRevealed: Bool
    let isRenaming: Bool
    let onClose: () -> Void
    @FocusState private var isFocused: Bool

    var body: some View {
        let showsClose = !isRenaming && (isRevealed || isFocused)
        ZStack {
            if isDirty && !showsClose {
                Circle().fill(.secondary).frame(width: 7, height: 7)
                    .accessibilityHidden(true)
            }
            Button(action: onClose) {
                Image(systemName: "xmark")
                    .font(.system(size: 9, weight: .semibold))
                    .frame(width: 20, height: 20)
                    .contentShape(.rect)
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            .focused($isFocused)
            .opacity(showsClose ? 1 : 0)
            .allowsHitTesting(showsClose)
            .accessibilityLabel("Close \(title)")
            .accessibilityHidden(isRenaming)
            .help("Close Tab (⌘W)")
        }
        .frame(width: 20, height: 20)
    }
}

/// A tab's title, truncated at the end. Plain text rather than an AppKit label, so opening
/// a workspace with many tabs does not create a platform view per tab.
private struct DocumentTabLabel: View {
    let title: String
    var italic = false

    var body: some View {
        Text(title)
            .font(.system(size: 12))
            .italic(italic)
            .lineLimit(1)
            .truncationMode(.tail)
    }
}

/// Navigate ▸ Focus Response: moves keyboard focus to the largest focusable view inside the
/// active group's response pane (the body editor, hex view, JSON tree or message table).
/// Isolated so only this view observes the trigger and the active group.
private struct ResponseFocusAnchor: View {
    let model: WireboltModel
    let interface: WorkspaceUIState
    let groupID: String
    /// The trigger when this pane appeared: a pane that appears later must not steal focus
    /// for an earlier request.
    @State private var appearedTrigger: Int

    init(model: WireboltModel, interface: WorkspaceUIState, groupID: String) {
        self.model = model
        self.interface = interface
        self.groupID = groupID
        _appearedTrigger = State(initialValue: interface.focusResponseTrigger)
    }

    var body: some View {
        // The anchor view is added on the first Focus Response for this pane, so switching
        // tabs (which remounts the pane) does not create an AppKit view each time.
        if interface.focusResponseTrigger != appearedTrigger {
            ResponseFocusBridge(
                trigger: interface.focusResponseTrigger,
                handledTrigger: appearedTrigger,
                isActive: model.sessions.activeGroupID == groupID
            )
            .accessibilityHidden(true)
        }
    }
}

private struct ResponseFocusBridge: NSViewRepresentable {
    let trigger: Int
    /// The last trigger this pane already handled when the anchor is created.
    let handledTrigger: Int
    let isActive: Bool

    func makeNSView(context: Context) -> AnchorView {
        let view = AnchorView()
        view.handledTrigger = handledTrigger
        return view
    }

    func updateNSView(_ view: AnchorView, context _: Context) {
        guard trigger != view.handledTrigger else { return }
        view.handledTrigger = trigger
        guard isActive else { return }
        DispatchQueue.main.async { view.focusLargestResponder() }
    }

    final class AnchorView: NSView {
        var handledTrigger = 0

        override func hitTest(_: NSPoint) -> NSView? { nil }

        func focusLargestResponder() {
            guard let window, let root = window.contentView else { return }
            let pane = convert(bounds, to: nil).insetBy(dx: -1, dy: -1)
            var best: (view: NSView, area: CGFloat)?
            func visit(_ view: NSView) {
                for subview in view.subviews where !subview.isHidden {
                    // The visible part: a text view inside a scroll view is taller than the pane.
                    let frame = subview.convert(subview.visibleRect, to: nil)
                    guard !frame.isEmpty, frame.intersects(pane) else { continue }
                    if subview !== self, subview.acceptsFirstResponder, pane.contains(frame) {
                        let area = frame.width * frame.height
                        if area > (best?.area ?? 0) { best = (subview, area) }
                    }
                    visit(subview)
                }
            }
            visit(root)
            if let target = best?.view { window.makeFirstResponder(target) } else { NSSound.beep() }
        }
    }
}

/// Mirrors unsaved edits in any tab to the window's close button (NSWindow.isDocumentEdited).
private struct WindowEditedIndicator: View {
    let model: WireboltModel

    var body: some View {
        WindowEditedBridge(isEdited: model.hasUnsavedRequestChanges)
            .frame(width: 0, height: 0)
            .accessibilityHidden(true)
    }
}

private struct WindowEditedBridge: NSViewRepresentable {
    let isEdited: Bool

    func makeNSView(context _: Context) -> NSView { NSView() }

    func updateNSView(_ view: NSView, context _: Context) {
        let isEdited = isEdited
        // The view may not be in a window during its first update.
        DispatchQueue.main.async {
            if let window = view.window, window.isDocumentEdited != isEdited { window.isDocumentEdited = isEdited }
        }
    }
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
                .id(session.id)
        } else {
            VStack(spacing: 0) {
                RequestSectionBar(interface: presentation, session: session)
                Divider()
                // Each document gets fresh editors (pending rows, focus, scroll position).
                requestContent
                    .id(session.id)
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
        case .settings:
            NetworkSettingsPage(model: model, scope: .request, session: session).id(session.id)
        case .note:
            NotesEditor(text: $session.note, preview: $presentation.previewsNotes).id(session.id)
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
                // Tabs scroll instead of being clipped when the pane is narrow; the section
                // controls keep their size at the trailing edge.
                ScrollView(.horizontal) {
                    HStack(spacing: 10) {
                        ForEach([RequestPanelSection.body, .params, .headers, .auth, .note, .settings]) { section in
                            PanelTabButton(
                                title: section == .body ? "Message" : section.rawValue,
                                badge: badge(for: section),
                                isSelected: interface.requestSection == section,
                                action: { interface.requestSection = section }
                            )
                        }
                    }
                }
                .scrollIndicators(.never)
                .scrollBounceBehavior(.basedOnSize, axes: .horizontal)
                Spacer(minLength: 4)
                if interface.requestSection == .body {
                    // A borderless menu showing only the chosen kind, like the HTTP body type,
                    // so it reads as a value rather than another section tab.
                    Menu {
                        Picker("Content Type", selection: messageKind) {
                            ForEach(WebSocketMessageKind.allCases) { Text($0.rawValue).tag($0) }
                        }
                        .pickerStyle(.inline).labelsHidden()
                    } label: {
                        Text(messageKind.wrappedValue.rawValue)
                    }
                    .menuStyle(.borderlessButton).controlSize(.small).fixedSize()
                    .accessibilityLabel("Content Type")
                    .accessibilityValue(messageKind.wrappedValue.rawValue)
                    .help("Content Type")
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
                    .help("Message Actions")
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
            FieldEditor(title: "Query Params", fields: $session.draft.query, kind: .query, focusTrigger: interface.focusNewKeyTrigger)
        case .headers:
            FieldEditor(title: "Header List", fields: $session.draft.headers, kind: .header, focusTrigger: interface.focusNewKeyTrigger)
        case .auth:
            AuthenticationEditor(model: model, session: session, authentication: $session.draft.authentication)
        case .settings:
            NetworkSettingsPage(model: model, scope: .request, session: session).id(session.id)
        case .note:
            NotesEditor(text: $session.note, preview: $interface.previewsNotes).id(session.id)
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
        case .body, .auth, .note, .settings: nil
        }
    }
}

private struct RequestURLBar: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    @Bindable var session: DocumentSession
    let groupID: String

    @State private var isEnteringCustomMethod = false
    @State private var customMethod = ""
    @State private var isEditingLongURL = false
    /// Narrow groups (for example a split editor) collapse the proxy pill and status to
    /// icons so the URL keeps most of the width.
    @State private var isCompact = false

    var body: some View {
        HStack(spacing: 7) {
            if session.kind == .webSocket {
                Text("WS").font(.system(size: 14, weight: .bold)).foregroundStyle(WireboltTheme.webSocketColor)
                    // The URL field has layout priority; keep the label from compressing to nothing.
                    .fixedSize()
            } else {
            Menu {
                ForEach(HTTPMethod.allCases, id: \.self) { method in
                    Button(method.rawValue) { session.draft.method = method }
                }
                Divider()
                Button("Custom…") {
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

            RequestURLField(session: session, focusTrigger: interface.focusURLTrigger,
                active: model.sessions.activeGroupID == groupID, submit: send)
                .id(session.id)
                .frame(minWidth: 96)
                .layoutPriority(1)

            ProxyConnectionIndicator(model: model, session: session, compact: isCompact) {
                interface.presentation(for: session).requestSection = .settings
            }
            // A fixed slot: the URL field never resizes when a status appears or changes.
            InlineResponseStatus(session: session, compact: isCompact)
                // A tab switch shows the other document's status without animating.
                .id(session.id)
                .frame(width: isCompact ? 20 : 148, alignment: .leading)

            Button("Edit Long URL", systemImage: "rectangle.expand.vertical") {
                isEditingLongURL = true
            }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 30)
                .accessibilityLabel("Edit Long URL")
                .help("Edit Long URL")

            RequestHistoryMenu(model: model, session: session)
                .frame(width: 24, height: 30)

            primaryAction
        }
        .padding(.leading, 10)
        .padding(.trailing, 11)
        .frame(height: 44)
        .background(WireboltTheme.barBackground)
        .onGeometryChange(for: Bool.self) { $0.size.width < Self.compactWidth } action: { isCompact = $0 }
        .onChange(of: session.id) {
            // The bar stays mounted across tab switches: close what belonged to the previous
            // document, as it closed when each document had its own bar.
            if isEnteringCustomMethod { isEnteringCustomMethod = false }
            if isEditingLongURL { isEditingLongURL = false }
        }
        .sheet(isPresented: $isEditingLongURL) {
            // The URL field shows the edited URL once the draft changes.
            LongURLEditor(url: Binding(
                get: { session.draft.displayURL },
                set: { session.draft.editURL($0) }
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

    /// Below this bar width the proxy pill and status collapse to icons.
    nonisolated private static let compactWidth: CGFloat = 760

    /// Native bordered buttons provide the disabled, hover, pressed, and focus-ring states.
    /// Send and Connect are prominent; Cancel and Disconnect are secondary. Shortcuts live in
    /// the Request menu and in the help tag, not in the title.
    @ViewBuilder private var primaryAction: some View {
        let hasURL = !session.draft.url.isEmpty
        Group {
            if session.kind == .webSocket {
                if session.socket.status == .disconnected {
                    prominentAction("Connect", enabled: hasURL, action: toggleConnection)
                        .help(hasURL ? "Connect (⌃⌘↩)" : "Enter a URL to connect")
                } else {
                    Button(action: toggleConnection) { actionTitle("Disconnect") }
                        .buttonStyle(.bordered)
                        .help("Disconnect (⌃⌘↩)")
                }
            } else if session.isRunning {
                Button(action: cancel) { actionTitle("Cancel") }
                    .buttonStyle(.bordered)
                    .accessibilityLabel("Cancel Request")
                    .help("Cancel Request (⌘.)")
            } else {
                prominentAction("Send", enabled: hasURL, action: send)
                    .accessibilityLabel("Send Request")
                    .help(hasURL ? "Send Request (⌘↩)" : "Enter a URL to send the request")
            }
        }
        .controlSize(.large)
        .buttonBorderShape(.capsule)
        .tint(WireboltTheme.primaryAccent)
        .fixedSize()
    }

    /// Equal minimum widths keep Send and Cancel from shifting the URL field when they swap.
    private func actionTitle(_ title: String) -> some View {
        Text(title).frame(minWidth: 52)
    }

    /// A disabled prominent button keeps a tinted fill that reads as enabled, especially in
    /// Dark Mode, so an unavailable Send or Connect falls back to the neutral bordered style
    /// with the system's dimmed label.
    @ViewBuilder
    private func prominentAction(_ title: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        if enabled {
            Button(action: action) { actionTitle(title) }
                .buttonStyle(.borderedProminent)
        } else {
            Button(action: action) { actionTitle(title) }
                .buttonStyle(.bordered)
                .disabled(true)
        }
    }

    private func toggleConnection() {
        if session.socket.status == .disconnected { Task { await model.connectWebSocket(session) } }
        else { session.socket.disconnect() }
    }

    private func send() {
        guard session.draft.url.isEmpty == false else { return }
        commitPendingEdits(in: NSApp.keyWindow)
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

private enum WebSocketMessageFilter: String, CaseIterable, Identifiable {
    case all = "All"
    case sent = "Sent"
    case received = "Received"

    var id: Self { self }

    func includes(_ message: WebSocketMessage) -> Bool {
        switch self {
        case .all: true
        case .sent: message.outgoing
        case .received: !message.outgoing && !message.system
        }
    }
}

private struct WebSocketResponseView: View {
    @Bindable var session: DocumentSession
    @State private var selectedMessage: UUID?
    @State private var showsHeaders = false
    @State private var filter = WebSocketMessageFilter.all
    @State private var hex = false
    @State private var hideControl = false
    @State private var isSearching = false
    @State private var query = ""
    /// The debounced query that actually filters the list.
    @State private var appliedQuery = ""
    @State private var split = ResponseLayoutState()
    private var socket: WebSocketDocumentState { session.socket }
    private var selected: WebSocketMessage? { socket.messages.first { $0.id == selectedMessage } }
    private var messages: [WebSocketMessage] {
        socket.messages.filter {
            (!hideControl || $0.control == nil) && filter.includes($0) && $0.matches(appliedQuery)
        }
    }
    var body: some View {
        Group {
            if socket.status == .connecting {
                VStack(spacing: WireboltTheme.Spacing.medium) {
                    ProgressView().controlSize(.small)
                    Text("Connecting…").foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if socket.messages.isEmpty && socket.status == .disconnected {
                if let error = socket.errorMessage {
                    ContentUnavailableView("Connection Failed", systemImage: "exclamationmark.triangle", description: Text(error))
                } else {
                    ContentUnavailableView(
                        "Not Connected",
                        systemImage: "bolt.horizontal",
                        description: Text("Choose Connect (⌃⌘↩) to start exchanging messages.")
                    )
                }
            } else {
                VStack(spacing: 0) {
                    HStack(spacing: WireboltTheme.Spacing.medium) {
                        PanelTabButton(title: "Messages", isSelected: !showsHeaders) { showsHeaders = false }
                        PanelTabButton(title: "Headers", badge: socket.headers.count, isSelected: showsHeaders) { showsHeaders = true }
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
                            if selected == nil {
                                ContentUnavailableView(
                                    "No Message Selected",
                                    systemImage: "text.bubble",
                                    description: Text("Select a message to see its contents.")
                                )
                            } else {
                                SyntaxTextView(text: preview, language: .plain)
                            }
                        }
                    }
                }
            }
        }
        .onAppear { split.requestHeight = 148 }
        .task(id: query) {
            // Filtering runs over up to 1,000 messages; wait for a typing pause.
            if !query.isEmpty { try? await Task.sleep(for: .milliseconds(150)) }
            guard !Task.isCancelled else { return }
            appliedQuery = query
        }
    }
    private var messageTable: some View {
        Table(messages, selection: $selectedMessage) {
            TableColumn("Data") { message in
                HStack(spacing: WireboltTheme.Spacing.medium) {
                    let icon = Self.icon(for: message)
                    Image(systemName: icon.symbol)
                        .foregroundStyle(icon.color)
                        .accessibilityLabel(icon.label)
                    Text(message.control ?? String(message.text.prefix(300))).lineLimit(1)
                }
            }.width(200)
            TableColumn("Time") { message in Text(Self.time.string(from: message.timestamp)) }
        }
        .font(WireboltTheme.Typography.detail)
        .tableStyle(.bordered(alternatesRowBackgrounds: false))
    }
    private var messageToolbar: some View {
        HStack(spacing: WireboltTheme.Spacing.small) {
            Picker("Show", selection: $filter) {
                ForEach(WebSocketMessageFilter.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented).labelsHidden().controlSize(.small).fixedSize()
            .help("Show all, sent, or received messages")
            Spacer(minLength: WireboltTheme.Spacing.xSmall)
            if isSearching {
                TextField("Find", text: $query)
                    .textFieldStyle(.roundedBorder).controlSize(.small).frame(width: 120)
                    .accessibilityLabel("Find messages")
            }
            Button("Find Messages", systemImage: "magnifyingglass") {
                isSearching.toggle()
                if !isSearching { query = "" }
            }
            .labelStyle(.iconOnly).buttonStyle(.borderless).help("Find Messages")
            Menu("View Options", systemImage: "line.3.horizontal.decrease.circle") {
                Toggle("Hide Ping/Pong", isOn: $hideControl)
                Divider()
                Picker("Show Selected Message As", selection: $hex) {
                    Text("Text").tag(false)
                    Text("Hex").tag(true)
                }.pickerStyle(.inline)
            }
            .menuStyle(.borderlessButton).menuIndicator(.hidden).labelStyle(.iconOnly).fixedSize()
            .help("View Options")
            Button("Copy Message", systemImage: "doc.on.doc") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(preview, forType: .string)
            }
            .labelStyle(.iconOnly).buttonStyle(.borderless)
            .disabled(selected == nil).help("Copy Selected Message")
            Button("Send", systemImage: "paperplane") { Task { await socket.send(body: session.draft.body) } }
                .buttonStyle(.bordered).controlSize(.small)
                .disabled(socket.status != .connected)
                .help("Send Message (⌘↩)")
        }
        .font(WireboltTheme.Typography.body)
        .padding(.horizontal, WireboltTheme.Spacing.large).frame(height: 32).background(WireboltTheme.barBackground)
    }
    private var preview: String {
        guard let selected else { return "" }
        if !hex && selected.data.count <= DocumentSession.previewByteLimit { return selected.text }
        let data = selected.data.prefix(DocumentSession.previewByteLimit)
        return hex ? data.map { String(format: "%02X", $0) }.joined(separator: " ") : String(decoding: data, as: UTF8.self)
    }
    private static func icon(for message: WebSocketMessage) -> (symbol: String, color: Color, label: String) {
        switch message.notice {
        case .connected: ("link", WireboltTheme.success, "Connected")
        case .disconnected: ("minus.circle", Color.secondary, "Disconnected")
        case .failed: ("exclamationmark.triangle.fill", WireboltTheme.statusColor(500), "Connection error")
        case nil: message.outgoing
            ? ("arrow.up", WireboltTheme.webSocketColor, "Sent")
            : ("arrow.down", WireboltTheme.success, "Received")
        }
    }
    private static let time: DateFormatter = {
        let formatter = DateFormatter(); formatter.dateFormat = "HH:mm:ss.SSS"; return formatter
    }()
}

private struct RequestHistoryMenu: View {
    @Bindable var model: WireboltModel
    @Bindable var session: DocumentSession
    @State private var loaded = LoadedHistory()
    @State private var isClearing = false

    /// Entries loaded for a document; the menu stays mounted across tab switches.
    private struct LoadedHistory {
        var documentID: String?
        var entries: [RunHistoryEntry] = []
    }

    private struct LoadKey: Equatable {
        let revision: Int
        let documentID: String
    }

    /// Only this document's entries: another document's never show while its own load.
    private var entries: [RunHistoryEntry] { loaded.documentID == session.id ? loaded.entries : [] }

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
        .task(id: LoadKey(revision: model.historyRevision, documentID: session.id)) {
            let session = session
            let entries = await model.historyEntries(for: session)
            guard !Task.isCancelled else { return }
            loaded = LoadedHistory(documentID: session.id, entries: entries)
        }
        .onChange(of: session.id) { if isClearing { isClearing = false } }
        .alert("Clear History?", isPresented: $isClearing) {
            Button("Cancel", role: .cancel) {}
            Button("Clear", role: .destructive) {
                let session = session
                Task {
                    await model.clearHistory(for: session)
                    if loaded.documentID == session.id { loaded.entries = [] }
                }
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
                Button("Done") { url = editedURL; dismiss() }
                    .keyboardShortcut(.return, modifiers: .command)
                    .help("Done (⌘↩)")
            }.controlSize(.small)
        }.padding(16).frame(width: 566, height: 362)
    }
}

/// Status of the latest run in a fixed-size slot: a spinner while running, then the
/// status symbol and line. Long reason phrases truncate; the full line is in the help tag.
private struct InlineResponseStatus: View {
    @Bindable var session: DocumentSession
    var compact = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Group {
            if session.isRunning || (session.kind == .webSocket && session.socket.status == .connecting) {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityLabel(session.kind == .webSocket ? "Connecting" : "Sending")
            } else if let outcome {
                HStack(spacing: WireboltTheme.Spacing.small) {
                    Image(systemName: symbol(for: outcome))
                        .font(.system(size: compact ? 15 : 14))
                    if !compact {
                        Text(outcome.label)
                            .font(.system(size: 14).monospacedDigit())
                            .lineLimit(1)
                            .truncationMode(.tail)
                            .contentTransition(reduceMotion ? .identity : .numericText())
                    }
                }
                .foregroundStyle(color(for: outcome))
                .help(outcome.detail)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(accessibilityLabel(for: outcome))
            }
        }
        .animation(reduceMotion ? nil : .snappy(duration: 0.2), value: outcome)
    }

    private var outcome: RunOutcomeBadge? {
        if session.kind == .webSocket {
            return session.socket.status == .connected ? .status(101) : nil
        }
        let failure = session.failure
        // The URL is read only after a failure so typing does not re-render the status.
        return RunOutcomeBadge(
            status: session.responseHead?.status,
            failure: failure,
            host: failure == nil ? nil : RunFailureMessage.host(from: session.preparedRun?.url ?? session.draft.url)
        )
    }

    private func symbol(for outcome: RunOutcomeBadge) -> String {
        switch outcome {
        case let .status(status): WireboltTheme.statusSymbol(status)
        case .failed: "exclamationmark.octagon.fill"
        case .cancelled: "xmark.circle.fill"
        }
    }

    /// A user-initiated cancel is not an error, so it stays neutral.
    private func color(for outcome: RunOutcomeBadge) -> Color {
        switch outcome {
        case let .status(status): WireboltTheme.statusColor(status)
        case .failed: WireboltTheme.danger
        case .cancelled: .secondary
        }
    }

    private func accessibilityLabel(for outcome: RunOutcomeBadge) -> String {
        switch outcome {
        case .status: "Status \(outcome.detail)"
        case .failed: "Request failed. \(outcome.detail)"
        case .cancelled: "Request cancelled"
        }
    }
}

/// Ends editing so pending text edits reach the draft, then puts focus back where it was
/// (with the same selection) so sending from the keyboard does not lose the insertion point.
@MainActor
func commitPendingEdits(in window: NSWindow?) {
    guard let window, let responder = window.firstResponder else { return }
    if let editor = responder as? NSTextView, editor.isFieldEditor, let field = editor.delegate as? NSTextField {
        let selection = editor.selectedRange()
        guard window.makeFirstResponder(nil) else { return }
        window.makeFirstResponder(field)
        field.currentEditor()?.selectedRange = selection
    } else if let view = responder as? NSView {
        guard window.makeFirstResponder(nil) else { return }
        window.makeFirstResponder(view)
    }
}

/// The URL text field of one document. Identified by the document at its use, so each tab
/// gets a fresh field (editing, selection and undo state) while the rest of the URL bar
/// stays mounted across tab switches.
private struct RequestURLField: View {
    @Bindable var session: DocumentSession
    let focusTrigger: Int
    let active: Bool
    let submit: () -> Void
    @State private var isEditing = false
    @State private var text: String
    @Environment(\.variableCatalog) private var variables

    init(session: DocumentSession, focusTrigger: Int, active: Bool, submit: @escaping () -> Void) {
        self.session = session
        self.focusTrigger = focusTrigger
        self.active = active
        self.submit = submit
        _text = State(initialValue: session.draft.displayURL)
    }

    var body: some View {
        NativeRequestURLField(text: Binding(
            get: { text },
            set: { text = $0; session.draft.editURL($0) }
        ), isEditing: $isEditing, focusTrigger: focusTrigger, active: active,
            variables: variables, submit: submit)
            .onSubmit(submit)
            .accessibilityLabel("Request URL")
            .onChange(of: session.draft.query) {
                if !isEditing { text = session.draft.displayURL }
            }
            .onChange(of: session.draft.url) {
                if !isEditing { text = session.draft.displayURL }
            }
    }
}

/// The URL text field. `{{name}}` references are colored (accent when defined, red when
/// not) while viewing and editing, the help tag lists their values, and typing `{{`
/// offers the active variables (↑↓ to choose, Return or Tab to insert, Esc to dismiss).
private struct NativeRequestURLField: NSViewRepresentable {
    @Binding var text: String
    @Binding var isEditing: Bool
    let focusTrigger: Int
    let active: Bool
    var variables: VariableCatalog?
    let submit: () -> Void

    private static let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)

    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField()
        field.isBordered = false
        field.drawsBackground = false
        field.font = Self.font
        field.placeholderString = "Enter URL"
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
        let coordinator = context.coordinator
        coordinator.parent = self
        field.cell?.isScrollable = isEditing
        field.cell?.lineBreakMode = isEditing ? .byClipping : .byTruncatingTail
        if field.stringValue != text { field.stringValue = text; coordinator.highlightedState = nil }
        coordinator.highlight(field)
        if context.coordinator.focusTrigger != focusTrigger {
            context.coordinator.focusTrigger = focusTrigger
            if active { field.window?.makeFirstResponder(field); field.selectText(nil) }
        }
    }
    static func dismantleNSView(_: NSTextField, coordinator: Coordinator) { coordinator.closeCompletions() }

    @MainActor
    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: NativeRequestURLField
        var focusTrigger: Int
        /// What the field currently shows, so unchanged updates skip re-highlighting.
        var highlightedState: (text: String, variables: VariableCatalog?, editing: Bool)?
        private var popover: NSPopover?
        private var completions: [VariableCatalog.Entry] = []
        private var selectedIndex = 0
        private var dismissedText: String?
        private var isInserting = false

        init(_ parent: NativeRequestURLField) { self.parent = parent; focusTrigger = parent.focusTrigger }

        func controlTextDidBeginEditing(_ notification: Notification) {
            parent.isEditing = true
            if let field = notification.object as? NSTextField { highlight(field) }
        }
        func controlTextDidEndEditing(_ notification: Notification) {
            closeCompletions()
            dismissedText = nil
            parent.isEditing = false
            // The field editor detaches after this notification; color the cell then.
            guard let field = notification.object as? NSTextField else { return }
            DispatchQueue.main.async { [weak self, weak field] in
                guard let self, let field, field.currentEditor() == nil else { return }
                self.highlightedState = nil
                self.highlight(field)
            }
        }
        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            parent.text = field.stringValue
            highlight(field)
            if !isInserting { updateCompletions(field) }
        }
        @objc func submit() { parent.submit() }

        func control(_ control: NSControl, textView _: NSTextView, doCommandBy selector: Selector) -> Bool {
            guard popover?.isShown == true, !completions.isEmpty, let field = control as? NSTextField else { return false }
            switch selector {
            case #selector(NSResponder.moveDown(_:)): selectedIndex = (selectedIndex + 1) % completions.count
            case #selector(NSResponder.moveUp(_:)): selectedIndex = (selectedIndex + completions.count - 1) % completions.count
            case #selector(NSResponder.insertNewline(_:)), #selector(NSResponder.insertTab(_:)):
                insert(completions[selectedIndex], into: field)
                return true
            case #selector(NSResponder.cancelOperation(_:)):
                dismissedText = field.stringValue
                closeCompletions()
                return true
            default: return false
            }
            showCompletions(for: field)
            return true
        }

        /// Colors references in the field editor while editing, or in the cell otherwise.
        func highlight(_ field: NSTextField) {
            let text = field.stringValue
            let editor = field.currentEditor() as? NSTextView
            let state = (text: text, variables: parent.variables, editing: editor != nil)
            if let current = highlightedState, current.text == state.text, current.variables == state.variables,
               current.editing == state.editing { return }
            highlightedState = state
            let summary = parent.variables.flatMap { text.contains("{{") ? $0.summary(for: text) : nil }
            if field.toolTip != summary { field.toolTip = summary }
            guard let variables = parent.variables, text.contains("{{") else {
                if let storage = editor?.textStorage, storage.length > 0 {
                    storage.addAttribute(.foregroundColor, value: NSColor.textColor, range: NSRange(location: 0, length: storage.length))
                }
                return
            }
            if let storage = editor?.textStorage {
                VariableHighlighting.apply(to: storage, catalog: variables, baseColor: .textColor)
            } else {
                let highlighted = NSMutableAttributedString(attributedString: VariableHighlighting.attributedString(text, font: NativeRequestURLField.font, catalog: variables))
                let paragraph = NSMutableParagraphStyle()
                paragraph.lineBreakMode = .byTruncatingTail
                highlighted.addAttribute(.paragraphStyle, value: paragraph, range: NSRange(location: 0, length: highlighted.length))
                field.attributedStringValue = highlighted
            }
        }

        private func updateCompletions(_ field: NSTextField) {
            let text = field.stringValue
            guard let variables = parent.variables, dismissedText != text, text.contains("{{"),
                  let editor = field.currentEditor() as? NSTextView, editor.selectedRange().length == 0,
                  let reference = VariableTemplate.openReference(in: text, caret: editor.selectedRange().location)
            else { closeCompletions(); return }
            completions = Array(variables.completions(matching: reference.prefix).prefix(VariableCompletionList.limit))
            selectedIndex = 0
            guard !completions.isEmpty else { closeCompletions(); return }
            showCompletions(for: field)
        }

        private func showCompletions(for field: NSTextField) {
            let list = VariableCompletionList(entries: completions, selectedIndex: selectedIndex) { [weak self, weak field] entry in
                guard let self, let field else { return }
                self.insert(entry, into: field)
            }
            // A transient popover closes itself on outside clicks; show a fresh one then.
            if let popover, popover.isShown, let host = popover.contentViewController as? NSHostingController<VariableCompletionList> {
                host.rootView = list
                return
            }
            popover?.close()
            let host = NSHostingController(rootView: list)
            host.sizingOptions = .preferredContentSize
            let popover = NSPopover()
            popover.contentViewController = host
            popover.behavior = .transient
            popover.animates = false
            self.popover = popover
            popover.show(relativeTo: caretRect(in: field), of: field, preferredEdge: .maxY)
        }

        func closeCompletions() {
            popover?.close()
            popover = nil
            completions = []
        }

        private func insert(_ entry: VariableCatalog.Entry, into field: NSTextField) {
            guard let editor = field.currentEditor() as? NSTextView,
                  let reference = VariableTemplate.openReference(in: editor.string, caret: editor.selectedRange().location)
            else { closeCompletions(); return }
            let replacement = reference.isClosed ? entry.name : entry.name + "}}"
            // Through the text view so the insertion is one undoable edit.
            isInserting = true
            defer { isInserting = false }
            if editor.shouldChangeText(in: reference.replacementRange, replacementString: replacement) {
                editor.replaceCharacters(in: reference.replacementRange, with: replacement)
                editor.didChangeText()
            }
            let caret = reference.replacementRange.location + (entry.name as NSString).length + 2
            editor.setSelectedRange(NSRange(location: min(caret, (editor.string as NSString).length), length: 0))
            closeCompletions()
        }

        /// The insertion point in field coordinates, falling back to the field bounds.
        private func caretRect(in field: NSTextField) -> NSRect {
            guard let editor = field.currentEditor() as? NSTextView, let window = field.window else { return field.bounds }
            let screen = editor.firstRect(forCharacterRange: editor.selectedRange(), actualRange: nil)
            guard screen != .zero else { return field.bounds }
            let local = field.convert(window.convertFromScreen(screen), from: nil)
            return NSRect(x: local.minX, y: field.bounds.minY, width: max(1, local.width), height: field.bounds.height)
        }
    }
}

private struct RequestSectionBar: View {
    @Bindable var interface: DocumentPresentationState
    @Bindable var session: DocumentSession
    /// Raw strip geometry, written during layout without re-rendering the bar.
    @State private var geometry = SectionStripGeometry()
    /// What the bar shows for that geometry; it changes only when a tab's visibility does,
    /// so mounting a bar that fits (every tab switch) costs no second pass.
    @State private var overflow = SectionStripOverflow()

    /// One row of constant height: section actions stay pinned at the trailing edge and
    /// the section tabs scroll when the pane is too narrow, so the editor never jumps.
    /// A clipped edge fades out, and a chevron menu lists the sections that are not fully
    /// visible; choosing one (or selecting it any other way) scrolls it into view.
    var body: some View {
        HStack(spacing: WireboltTheme.Spacing.medium) {
            ScrollViewReader { scroll in
                ScrollView(.horizontal) {
                    tabs
                }
                .scrollIndicators(.never)
                .scrollBounceBehavior(.basedOnSize, axes: .horizontal)
                .onScrollGeometryChange(for: SectionScrollMetrics.self) { geometry in
                    SectionScrollMetrics(offset: geometry.contentOffset.x, contentWidth: geometry.contentSize.width,
                        visibleWidth: geometry.containerSize.width)
                } action: { _, metrics in
                    geometry.metrics = metrics
                    publishOverflow()
                }
                .mask { edgeFadeMask }
                .onChange(of: SectionScrollTarget(documentID: session.id, section: interface.requestSection)) { old, new in
                    if old.documentID == new.documentID {
                        withAnimation(.snappy(duration: 0.2)) { scroll.scrollTo(new.section, anchor: .center) }
                    } else if let first = RequestPanelSection.allCases.first {
                        // The bar stays mounted across tab switches: another document starts
                        // at the leading edge, without animation, like a newly shown bar.
                        scroll.scrollTo(first, anchor: .leading)
                    }
                }
            }
            if !hiddenSections.isEmpty {
                Menu("More Sections", systemImage: "chevron.forward.2") {
                    ForEach(hiddenSections) { section in
                        Button(section.rawValue) { interface.requestSection = section }
                    }
                }
                .menuStyle(.borderlessButton).menuIndicator(.hidden)
                .labelStyle(.iconOnly).foregroundStyle(.secondary).fixedSize()
                .help("More Sections")
            }
            // Separates the section tabs from the current section's controls.
            Divider().frame(height: 16)
            tools
        }
        .frame(height: 32)
        .padding(.horizontal, 11)
        .background(WireboltTheme.barBackground)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Request sections")
    }

    private var tabs: some View {
        HStack(spacing: 10) {
            ForEach(RequestPanelSection.allCases) { section in
                PanelTabButton(
                    title: section.rawValue,
                    badge: badge(for: section),
                    indicator: (section == .body && session.draft.body != .empty)
                        || (section == .auth && session.draft.authentication != .none),
                    isSelected: interface.requestSection == section,
                    height: 32,
                    action: { interface.requestSection = section }
                )
                .id(section)
                .onGeometryChange(for: ClosedRange<CGFloat>.self) { proxy in
                    let frame = proxy.frame(in: .named(Self.tabSpace))
                    return frame.minX...frame.maxX
                } action: {
                    geometry.tabExtents[section] = $0
                    publishOverflow()
                }
            }
        }
        .fixedSize(horizontal: true, vertical: false)
        .coordinateSpace(.named(Self.tabSpace))
    }

    private static let tabSpace = "request-section-tabs"
    private static let fadeWidth: CGFloat = 18

    /// Sections whose tab is at least partly scrolled out of view, in tab order.
    private var hiddenSections: [RequestPanelSection] { overflow.hiddenSections }

    private func publishOverflow() {
        let current = geometry.overflow
        if current != overflow { overflow = current }
    }

    private var edgeFadeMask: some View {
        HStack(spacing: 0) {
            LinearGradient(colors: [overflow.clipsLeading ? .clear : .black, .black], startPoint: .leading, endPoint: .trailing)
                .frame(width: Self.fadeWidth)
            Color.black
            LinearGradient(colors: [.black, overflow.clipsTrailing ? .clear : .black], startPoint: .leading, endPoint: .trailing)
                .frame(width: Self.fadeWidth)
        }
    }

    @ViewBuilder private var tools: some View {
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
                .help("Add Key (⇧⌘K)")
                .frame(width: 30, height: 32)
                Menu("Section Actions", systemImage: "ellipsis.circle") {
                    Button("New Entry") { interface.isBulkEditing = false; interface.focusNewKeyTrigger += 1 }
                    Divider()
                    Button("Key-Value Edit") { interface.isBulkEditing = false }
                    Button("Bulk Edit") { interface.isBulkEditing = true }
                    Divider()
                    Button("Clear All", action: clearFields)
                }
                .menuStyle(.borderlessButton).menuIndicator(.hidden)
                .labelStyle(.iconOnly).foregroundStyle(.secondary).fixedSize()
                .frame(width: 30, height: 32)
                .help("Section Actions")
            }
        }.fixedSize().frame(height: 32)
    }

    private func badge(for section: RequestPanelSection) -> Int? {
        switch section {
        case .params: session.draft.query.filter(\.enabled).count
        case .headers: session.draft.headers.filter(\.enabled).count
        case .auth, .body, .note, .settings: nil
        }
    }

    private var fieldsBinding: Binding<[RequestField]>? {
        switch interface.requestSection {
        case .params: $session.draft.query
        case .headers: $session.draft.headers
        case .body, .auth, .note, .settings: nil
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

/// The selected section of a document, so the strip can tell a section change from a tab switch.
private struct SectionScrollTarget: Equatable {
    let documentID: String
    let section: RequestPanelSection
}

private struct SectionScrollMetrics: Equatable {
    var offset: CGFloat = 0
    var contentWidth: CGFloat = 0
    var visibleWidth: CGFloat = 0

    var overflows: Bool { contentWidth > visibleWidth + 0.5 }
    var clipsLeading: Bool { overflows && offset > 0.5 }
    var clipsTrailing: Bool { overflows && offset + visibleWidth < contentWidth - 0.5 }
}

/// The section strip's edge fades and the sections listed in its overflow menu.
private struct SectionStripOverflow: Equatable {
    var clipsLeading = false
    var clipsTrailing = false
    var hiddenSections: [RequestPanelSection] = []
}

/// Layout measurements of the section strip. A plain reference so writing them from
/// geometry callbacks does not invalidate the bar; only the derived overflow does.
@MainActor
private final class SectionStripGeometry {
    var metrics = SectionScrollMetrics()
    var tabExtents: [RequestPanelSection: ClosedRange<CGFloat>] = [:]

    var overflow: SectionStripOverflow {
        guard metrics.overflows else { return SectionStripOverflow() }
        let visible = metrics.offset...(metrics.offset + metrics.visibleWidth)
        return SectionStripOverflow(
            clipsLeading: metrics.clipsLeading,
            clipsTrailing: metrics.clipsTrailing,
            hiddenSections: RequestPanelSection.allCases.filter { section in
                guard let extent = tabExtents[section] else { return false }
                return extent.lowerBound < visible.lowerBound - 0.5 || extent.upperBound > visible.upperBound + 0.5
            }
        )
    }
}

private struct BulkFieldEditor: View {
    @Binding var fields: [RequestField]
    @Environment(\.dismiss) private var dismiss
    @State private var text: String
    @State private var originalFields: [RequestField]

    init(fields: Binding<[RequestField]>) {
        _fields = fields
        _originalFields = State(initialValue: fields.wrappedValue)
        _text = State(initialValue: fields.wrappedValue.map {
            "\($0.enabled ? "" : "# ")\($0.name): \($0.value.editableValue)"
        }.joined(separator: "\n"))
    }

    var body: some View {
        BodyTextEditor(text: $text)
            .onChange(of: text) { fields = parse(text) }
    }

    private func parse(_ source: String) -> [RequestField] {
        // Keep metadata while an incomplete line is temporarily absent during typing.
        let ids = Set(fields.map(\.id))
        return RequestField.parseBulk(source, preserving: fields + originalFields.filter { !ids.contains($0.id) })
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
            HStack(spacing: WireboltTheme.Spacing.xSmall) {
                Text(title)
                // Neutral count capsule: counts are information, not success.
                if let badge, badge > 0 {
                    Text(badge, format: .number)
                        .font(WireboltTheme.Typography.badge)
                        .foregroundStyle(.primary.opacity(0.72))
                        .padding(.horizontal, 5)
                        .frame(minWidth: 16, minHeight: 14)
                        .background(.quaternary, in: Capsule())
                } else if indicator {
                    Circle().fill(.secondary).frame(width: 5, height: 5)
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
        .accessibilityLabel(title)
        .accessibilityValue(badge.map { $0 > 0 ? "\($0) entries" : "" } ?? (indicator ? "configured" : ""))
    }
}

private enum FieldEditorKind {
    case query
    case header

    /// Row noun for VoiceOver labels.
    var noun: String {
        switch self {
        case .query: "parameter"
        case .header: "header"
        }
    }
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
                            kind: kind,
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
    let kind: FieldEditorKind
    let valueWidth: Double
    let onRemove: () -> Void

    @State private var isHovered = false
    private var fieldHeight: Double { FieldEditorMetrics.height(key: field.name, value: field.value.editableValue, valueWidth: valueWidth) }

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            FieldCheckbox(isOn: $field.enabled, label: "Enable \(kind.noun) \(field.name)")
            FieldTextInput("Key", text: $field.name, height: fieldHeight,
                accessibilityLabel: field.name.isEmpty ? "\(kind.noun.capitalized) name" : "\(kind.noun.capitalized) name, \(field.name)")
                .frame(width: 175)
                .padding(.horizontal, 4)

            Color.clear.frame(width: 1)
            FieldTextInput("Value", text: literalBinding($field.value), height: fieldHeight,
                accessibilityLabel: field.name.isEmpty ? "\(kind.noun.capitalized) value" : "Value of \(field.name)")
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
                .help("Remove")
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
                    FieldCheckbox(isOn: $isEnabled, label: "Enable new \(kind.noun)")
                }
                FieldTextInput("New Key", text: $name, height: fieldHeight, accessibilityLabel: "New \(kind.noun) name")
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
                FieldTextInput("New Value", text: $value, height: fieldHeight, accessibilityLabel: "New \(kind.noun) value")
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
                        .help("Discard New Field")
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
            && !name.contains("{{")
    }

    private func updateValueSuggestions() {
        showingValueSuggestions = kind == .header
            && focusedField == .value
            && name.caseInsensitiveCompare("Content-Type") == .orderedSame
            && !value.isEmpty
            && !value.contains("{{")
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
            Text("New Key")
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
    @State private var revealsPassword = false

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
                        TextField("Username", text: credential(username, role: "username"))
                            .labelsHidden()
                    }
                    GridRow {
                        Text("Password")
                        HStack(spacing: WireboltTheme.Spacing.xSmall) {
                            Group {
                                if revealsPassword {
                                    TextField("Password", text: credential(password, role: "password"))
                                } else {
                                    SecureField("Password", text: credential(password, role: "password"))
                                }
                            }
                            .labelsHidden()
                            Button(revealsPassword ? "Hide Password" : "Show Password",
                                   systemImage: revealsPassword ? "eye.slash" : "eye") { revealsPassword.toggle() }
                                .labelStyle(.iconOnly).buttonStyle(.borderless)
                                .help(revealsPassword ? "Hide Password" : "Show Password")
                        }
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
                        .accessibilityLabel("Bearer Token")
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
        Menu {
            Picker("Auth Type", selection: kindBinding) {
                ForEach(AuthenticationKind.allCases.filter { [.none, .basic, .bearer, kindBinding.wrappedValue].contains($0) }) { kind in Text(kind.title).tag(kind) }
            }
            .pickerStyle(.inline).labelsHidden()
        } label: {
            Text(kindBinding.wrappedValue.title)
        }
        .menuStyle(.borderlessButton).controlSize(.small).fixedSize()
        .accessibilityLabel("Auth Type")
        .accessibilityValue(kindBinding.wrappedValue.title)
        .help("Auth Type")
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
            // A borderless menu sized to the chosen type, so it reads as a value rather
            // than another section tab and leaves the section tabs as much room as possible.
            Menu {
                Picker("Content Type", selection: kindBinding) {
                    ForEach(BodyKind.allCases) { kind in
                        Text(kind.title).tag(kind)
                        if [.form, .html, .raw, .file].contains(kind) { Divider() }
                    }
                }
                .pickerStyle(.inline).labelsHidden()
            } label: {
                Text(kindBinding.wrappedValue.title)
            }
            .menuStyle(.borderlessButton).controlSize(.small).fixedSize()
            .accessibilityLabel("Content Type")
            .accessibilityValue(kindBinding.wrappedValue.title)
            .help("Content Type")
            if case let .multipart(parts) = requestBody {
                Button("Add Part", systemImage: "plus") { requestBody = .multipart(parts: parts + [MultipartPart()]) }
                    .labelStyle(.iconOnly).buttonStyle(.borderless)
                    .help("Add Part")
            } else {
                Button("Format Body", systemImage: "wand.and.stars", action: formatBody)
                    .labelStyle(.iconOnly).buttonStyle(.borderless).disabled(!requestBody.canFormat)
                    .help("Format Body")
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
            .help("Body Actions")
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
    var canFormat: Bool {
        switch self {
        case .json, .xml, .html: true
        case .empty, .text, .raw, .formURLEncoded, .multipart, .file: false
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
        let part = part
        let window = NSApp.keyWindow
        let panel = NSSavePanel()
        panel.nameFieldStringValue = part.fileName ?? (part.name.isEmpty ? "part" : part.name)
        panel.canCreateDirectories = true
        Task {
            guard await present(panel, in: window) == .OK, let url = panel.url else { return }
            do {
                let data: Data
                switch part.kind {
                case .file: data = try Data(contentsOf: URL(fileURLWithPath: part.filePath ?? ""), options: .mappedIfSafe)
                case .binary:
                    guard let decoded = Data(base64Encoded: part.value.editableValue) else {
                        throw CocoaError(.fileWriteInapplicableStringEncoding, userInfo: [
                            NSLocalizedDescriptionKey: "The part’s binary value isn’t valid Base64.",
                        ])
                    }
                    data = decoded
                case .text: data = Data(part.value.editableValue.utf8)
                }
                try data.write(to: url, options: .atomic)
            } catch {
                let alert = NSAlert()
                alert.messageText = "The part could not be exported."
                alert.informativeText = error.localizedDescription
                _ = await present(alert, in: window)
            }
        }
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
                    TextField("Part Name", text: $part.name).labelsHidden().frame(height: 26)
                }
                GridRow {
                    Text("Content Type:")
                    HStack(spacing: 0) {
                        TextField("Content Type", text: optional(\.contentType)).labelsHidden()
                        Menu("Content Type") {
                            ForEach(["text/plain", "application/json", "application/xml", "application/octet-stream", "image/png"], id: \.self) { type in
                                Button(type) { part.contentType = type }
                            }
                        }.labelsHidden().frame(width: 26)
                    }.frame(height: 26)
                }
                GridRow {
                    Text("File Name:")
                    TextField("File Name", text: optional(\.fileName)).labelsHidden().frame(height: 26)
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
        let part = $part
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        // This editor is a popover; attach the sheet to the workspace window instead.
        let window = WorkspaceWindowRegistry.primary ?? NSApp.mainWindow
        Task {
            guard await present(panel, in: window) == .OK, let url = panel.url else { return }
            part.wrappedValue.kind = .file
            part.wrappedValue.filePath = url.path
            part.wrappedValue.fileName = url.lastPathComponent
            part.wrappedValue.contentType = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
        }
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
        let update = update
        Task {
            guard await present(panel, in: NSApp.keyWindow) == .OK, let selected = panel.url else { return }
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

/// A system empty state for panels that have nothing to show yet.
struct LightweightPlaceholder: View {
    let title: String
    let systemImage: String
    var description: String?

    var body: some View {
        ContentUnavailableView {
            Label(title, systemImage: systemImage)
        } description: {
            if let description { Text(description) }
        }
    }
}

/// The editor area without an open tab: a first-run welcome while the workspace has no
/// requests, otherwise a pointer to the sidebar. Both offer the next step directly.
private struct NoOpenRequestPlaceholder: View {
    let model: WireboltModel
    let interface: WorkspaceUIState

    var body: some View {
        if model.workspace.collections.allSatisfy({ $0.requests.isEmpty }) {
            ContentUnavailableView {
                Label("Start Your Workspace", systemImage: "paperplane")
            } description: {
                Text("Create a request, or import a cURL command, HAR file, Postman collection or Wirebolt JSON. You can also drop one of those files on this window.")
            } actions: {
                Button("New Request") { interface.makeNewRequest(model: model) }
                    .buttonStyle(.borderedProminent)
                    .help("New Request (⌘N)")
                WorkspaceImportMenu(interface: interface)
                    .fixedSize()
                Button("Open Workspace…") { chooseWorkspace(model: model, interface: interface, create: false) }
                    .disabled(model.isLoadingWorkspace || model.isGitBusy || model.isOAuthBusy)
                    .help("Open Workspace (⌘O)")
            }
        } else {
            ContentUnavailableView {
                Label("No Open Request", systemImage: "doc")
            } description: {
                Text("Select a request in the sidebar, or create one with ⌘N.")
            } actions: {
                Button("New Request") { interface.makeNewRequest(model: model) }
                    .help("New Request (⌘N)")
            }
        }
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
    var isWorkspaceWindow = false
    var requestClose: (NSWindow) -> Void = { _ in }

    func makeNSView(context _: Context) -> WindowConfigurationView {
        let view = WindowConfigurationView()
        view.isWorkspaceWindow = isWorkspaceWindow
        return view
    }

    func updateNSView(_ view: WindowConfigurationView, context _: Context) {
        view.requestClose = requestClose
        view.installCloseGuard()
        // Root updates are frequent; only a width change needs the outline redrawn.
        guard view.sidebarWidth != sidebarWidth else { return }
        view.sidebarWidth = sidebarWidth
        view.updateSidebarOutline()
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
            // Narrow groups use a vertical split so every section remains reachable.
            let vertical = orientation == .bottom || geometry.size.width < 700
            let length = vertical ? geometry.size.height : geometry.size.width
            let minimum: CGFloat = vertical ? 132 : 320
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
    var isWorkspaceWindow = false
    var requestClose: (NSWindow) -> Void = { _ in }
    private let sidebarOutline = SidebarOutlineLayer()

    /// The outline is a layer above the window frame's views (the title bar included), not a
    /// view: AppKit logs adding an unknown view to the window frame, with a symbolicated
    /// call stack that took several milliseconds as the window opened.
    func updateSidebarOutline() {
        guard let window, let frame = window.contentView?.superview, let host = frame.layer else { return }
        if sidebarOutline.superlayer !== host {
            // In front of the view layers, which AppKit keeps ordered after added layers.
            sidebarOutline.zPosition = 1_000
            host.addSublayer(sidebarOutline)
        }
        sidebarOutline.appearance = window.effectiveAppearance
        sidebarOutline.contentsScale = window.backingScaleFactor
        sidebarOutline.frame = host.bounds
        sidebarOutline.sidebarWidth = sidebarWidth
        sidebarOutline.setNeedsDisplay()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateSidebarOutline()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        updateSidebarOutline()
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        // Follows the window size, since this view spans the workspace.
        if let host = sidebarOutline.superlayer, sidebarOutline.frame != host.bounds { updateSidebarOutline() }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        if isWorkspaceWindow {
            // One workspace window per app: a duplicate (for example from a system reopen
            // event) closes and focuses the existing window instead.
            if let primary = WorkspaceWindowRegistry.primary, primary !== window, primary.isVisible {
                DispatchQueue.main.async {
                    window.close()
                    primary.makeKeyAndOrderFront(nil)
                }
                return
            }
            WorkspaceWindowRegistry.primary = window
        }
        // Keep the title for the Window menu, Mission Control and VoiceOver; only hide it visually.
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.toolbarStyle = .unified
        window.tabbingMode = .disallowed
        window.styleMask.insert(.fullSizeContentView)
        window.backgroundColor = .windowBackgroundColor
        window.isOpaque = false
        installCloseGuard()
        updateSidebarOutline()
        observeMainMenuChanges()
        synchronizeEditingMenu()
    }

    /// The close button confirms unsaved edits before the window goes away. Reapplied on
    /// updates because AppKit can rebuild the title bar buttons when the style changes.
    func installCloseGuard() {
        guard isWorkspaceWindow, let window, WorkspaceWindowRegistry.primary === window,
              let close = window.standardWindowButton(.closeButton), close.target !== self
        else { return }
        close.target = self
        close.action = #selector(closeButtonClicked)
    }

    @objc private func closeButtonClicked(_: Any?) {
        guard let window else { return }
        requestClose(window)
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }

    private func observeMainMenuChanges() {
        NotificationCenter.default.removeObserver(self)
        guard let menu = NSApp.mainMenu else { return }
        // SwiftUI rebuilds menus; re-add the AppKit-only item when the Edit menu changes.
        let editMenus = menu.items.compactMap(\.submenu).filter { Self.pasteItem(in: $0) != nil }
        for observed in [menu] + editMenus {
            NotificationCenter.default.addObserver(
                self,
                selector: #selector(mainMenuDidChange),
                name: NSMenu.didAddItemNotification,
                object: observed
            )
        }
    }

    @objc private func mainMenuDidChange(_: Notification) {
        NSObject.cancelPreviousPerformRequests(withTarget: self, selector: #selector(synchronizeEditingMenu), object: nil)
        perform(#selector(synchronizeEditingMenu), with: nil, afterDelay: 0)
    }

    /// Adds Edit ▸ Paste and Match Style, which SwiftUI does not provide. Items are found by
    /// action rather than title so localized menus work; AppKit validates availability.
    @objc private func synchronizeEditingMenu() {
        guard let menu = NSApp.mainMenu else { return }
        for edit in menu.items.compactMap(\.submenu) {
            guard let paste = Self.pasteItem(in: edit) else { continue }
            let matchStyle = #selector(NSTextView.pasteAsPlainText(_:))
            guard !edit.items.contains(where: { $0.action == matchStyle }) else { return }
            let item = NSMenuItem(title: "Paste and Match Style", action: matchStyle, keyEquivalent: "v")
            item.keyEquivalentModifierMask = [.command, .option, .shift]
            edit.insertItem(item, at: edit.index(of: paste) + 1)
            return
        }
    }

    private static func pasteItem(in menu: NSMenu) -> NSMenuItem? {
        menu.items.first { $0.action == #selector(NSText.paste(_:)) }
    }
}

/// Strokes the rounded outline around the sidebar panel with AppKit drawing, so it matches
/// the system separator color in the window's appearance.
private final class SidebarOutlineLayer: CALayer, @unchecked Sendable {
    nonisolated(unsafe) var sidebarWidth = 250.0
    nonisolated(unsafe) var appearance: NSAppearance?

    override init() {
        super.init()
        needsDisplayOnBoundsChange = true
    }

    override init(layer: Any) {
        super.init(layer: layer)
        if let other = layer as? SidebarOutlineLayer {
            sidebarWidth = other.sidebarWidth
            appearance = other.appearance
        }
    }

    required init?(coder: NSCoder) { fatalError("not coded") }

    override func draw(in context: CGContext) {
        guard sidebarWidth > 0 else { return }
        let graphics = NSGraphicsContext(cgContext: context, flipped: false)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = graphics
        let draw = { [self] in
            NSColor.separatorColor.setStroke()
            let outline = NSBezierPath(roundedRect: NSRect(x: 8.5, y: 8.5,
                width: sidebarWidth - 9, height: bounds.height - 17), xRadius: 18, yRadius: 18)
            outline.lineWidth = 1
            outline.stroke()
        }
        if let appearance { appearance.performAsCurrentDrawingAppearance(draw) } else { draw() }
        NSGraphicsContext.restoreGraphicsState()
    }
}
