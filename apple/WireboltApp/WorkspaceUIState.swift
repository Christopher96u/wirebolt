import AppKit
import Foundation
import Observation
import SwiftUI

enum RequestPanelSection: String, CaseIterable, Identifiable {
    case params = "Params"
    case headers = "Headers"
    case body = "Body"
    case auth = "Auth"
    case note = "Note"
    case settings = "Settings"

    var id: Self { self }
}

enum ResponsePanelSection: String, CaseIterable, Identifiable {
    case headers = "Headers"
    case body = "Body"
    case cookies = "Cookies"
    case raw = "Raw HTTP"
    case request = "Request"

    var id: Self { self }
}

enum ResponseRenderer: String, CaseIterable, Identifiable {
    case json = "JSON"
    case tree = "JSON Tree View"
    case image = "Image"
    case xml = "XML"
    case html = "HTML"
    case webView = "Webview"
    case raw = "Text"
    case hex = "Hex"

    var id: Self { self }
}

enum ResponseOrientation: String, CaseIterable {
    case bottom
    case right
}

@MainActor
@Observable
final class ResponseLayoutState {
    var requestHeight: CGFloat?
    var requestWidth: CGFloat?
    /// Whether the group is wide enough to place the response on the right.
    var fitsResponseOnRight = true
}

@MainActor
@Observable
final class DocumentPresentationState {
    var responseFindTrigger = 0
    let editorStorage = EditorPresentationStorage()
    var requestSection: RequestPanelSection
    var responseSection: ResponsePanelSection = .body
    var responseRenderer: ResponseRenderer = .json
    var usesAutomaticRenderer = true
    var isBulkEditing = false
    var previewsNotes = false
    var focusNewKeyTrigger = 0

    init(requestSection: RequestPanelSection = .params) {
        self.requestSection = requestSection
    }
}

struct DirtyCloseRequest: Identifiable, Equatable {
    let id = UUID()
    let scope: TabCloseScope
    let groupID: String
    let documentTitles: [String]
    let documentIDs: [String]
}

enum WorkspaceNameTarget: Equatable {
    case collection(id: String?)
    case group(collectionID: String, id: String?)
}

struct WorkspaceNamePrompt: Identifiable, Equatable {
    let id = UUID()
    let target: WorkspaceNameTarget
    let title: String
    let initialName: String
}

enum WorkspaceDeleteTarget: Equatable {
    case collection(id: String)
    case group(collectionID: String, id: String)
    case request(collectionID: String, id: String)
}

struct WorkspaceDeleteRequest: Identifiable, Equatable {
    let id = UUID()
    let target: WorkspaceDeleteTarget
    let title: String
    /// What else the deletion removes, shown in the confirmation.
    var detail = ""
}

/// Move Up/Down availability for the focused sidebar's selected request or folder.
struct SidebarMoveCommands: Equatable {
    let identifier: String
    let canMoveUp: Bool
    let canMoveDown: Bool
}

@MainActor
@Observable
final class WorkspaceUIState {
    var sidebarDragIdentifier: String?
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let rustInterface = AppWorkspaceInterface()

    var columnVisibility = NavigationSplitViewVisibility.all
    private var fallbackPresentation = DocumentPresentationState()
    private var activePresentation: DocumentPresentationState {
        guard let activeTabID else { return fallbackPresentation }
        return presentationByTabID[activeTabID] ?? fallbackPresentation
    }
    var requestSection: RequestPanelSection {
        get { activePresentation.requestSection }
        set { activePresentation.requestSection = newValue; persistActivePresentation() }
    }
    var responseSection: ResponsePanelSection {
        get { activePresentation.responseSection }
        set { activePresentation.responseSection = newValue; persistActivePresentation() }
    }
    var responseRenderer: ResponseRenderer {
        get { activePresentation.responseRenderer }
        set {
            activePresentation.responseRenderer = newValue
            persistActivePresentation()
            PerformanceProbe.rendererReady(newValue.rawValue)
        }
    }
    var responseOrientation: ResponseOrientation = .bottom {
        didSet { defaults.set(responseOrientation.rawValue, forKey: Self.responseOrientationKey) }
    }
    private var responseLayoutsByGroupID: [String: ResponseLayoutState] = [:]

    /// True when at least one of the groups can show the response on the right.
    func fitsResponseOnRight(groupIDs: [String]) -> Bool {
        groupIDs.isEmpty || groupIDs.contains { responseLayoutsByGroupID[$0]?.fitsResponseOnRight ?? true }
    }

    func responseLayout(for groupID: String) -> ResponseLayoutState {
        if let layout = responseLayoutsByGroupID[groupID] { return layout }
        let layout = ResponseLayoutState()
        responseLayoutsByGroupID[groupID] = layout
        return layout
    }
    var isBulkEditing: Bool {
        get { activePresentation.isBulkEditing }
        set { activePresentation.isBulkEditing = newValue }
    }
    var canEditFields: Bool { activeTabID != nil && (requestSection == .params || requestSection == .headers) }
    /// Focuses the new-key row of the visible key-value table, switching to Params when the
    /// visible section has none.
    func addKey() {
        if !canEditFields { requestSection = .params }
        activePresentation.isBulkEditing = false
        activePresentation.focusNewKeyTrigger += 1
    }
    func selectTab(index: Int? = nil, offset: Int = 0, model: WireboltModel) {
        guard let group = model.sessions.activeGroup, !group.tabIDs.isEmpty else { return }
        guard let target = sessionTabDestination(tabs: group.tabIDs, selected: group.selectedTabID, index: index.map(Int64.init), offset: Int64(offset)) else { return }
        model.sessions.select(tabID: target); synchronizeSelection(model: model)
    }
    var activeTabID: String?
    /// The folder of the open workspace; editor layouts are remembered per folder.
    @ObservationIgnored var workspaceURL: URL?
    var sidebarFilter = "" {
        didSet {
            let filtering = workspaceSidebarFiltering(query: sidebarFilter)
            if sidebarIsFiltering != filtering { sidebarIsFiltering = filtering }
        }
    }
    private var sidebarIsFiltering = false
    var isShowingImporter = false
    var isShowingCurlImporter = false
        var importFormat: ImportFormat = .postmanV2
    var focusURLTrigger = 0
    var pendingURLFocusID: String?
    var focusSearchTrigger = 0
    /// Moves keyboard focus into the active editor group's response content.
    var focusResponseTrigger = 0
    var collectionsExpanded = true
    var isShowingEnvironmentInspector = false
    var hasEnvironmentEdits = false
    @ObservationIgnored var saveEnvironmentEdits: (@MainActor () async -> Bool)?
    var environmentCloseTrigger = 0

    func toggleEnvironmentInspector() {
        if isShowingEnvironmentInspector { environmentCloseTrigger += 1 }
        else { isShowingEnvironmentInspector = true }
    }

    var dirtyCloseRequest: DirtyCloseRequest? { didSet { if dirtyCloseRequest == nil { rustInterface.cancelClose() } } }
    var workspaceNamePrompt: WorkspaceNamePrompt?
    var workspaceDeleteRequest: WorkspaceDeleteRequest?

    var isShowingDirtyClose: Bool {
        get { dirtyCloseRequest != nil }
        set {
            if newValue == false { dirtyCloseRequest = nil }
        }
    }

    private var presentationByTabID: [String: DocumentPresentationState] = [:]
    private var lastClosedSession: DocumentSession?
    private var lastClosedPresentation: DocumentPresentationState?

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        requestSection = defaults.string(forKey: Self.requestSectionKey)
            .flatMap(RequestPanelSection.init(rawValue:)) ?? .params
        responseSection = defaults.string(forKey: Self.responseSectionKey)
            .map { $0 == "Raw" ? "Raw HTTP" : $0 }
            .flatMap(ResponsePanelSection.init(rawValue:)) ?? .body
        responseRenderer = defaults.string(forKey: Self.responseRendererKey)
            .map { $0 == "Raw" ? "Text" : $0 }
            .flatMap(ResponseRenderer.init(rawValue:)) ?? .json
        responseOrientation = defaults.string(forKey: Self.responseOrientationKey)
            .flatMap(ResponseOrientation.init(rawValue:)) ?? .bottom
    }

    func synchronizeSelection(model: WireboltModel) {
        guard let session = model.sessions.activeSession else {
            activeTabID = nil
            return
        }
        activatePresentation(for: session.id, defaultBody: session.draft.body)
    }

    /// Opens a saved request from the sidebar. A plain click or arrow key reuses the preview
    /// tab; ⌘-click (or `preview: false`) opens a regular tab that stays open.
    func activateSavedRequest(_ location: RequestLocation, model: WireboltModel, preview: Bool? = nil) {
        persistActivePresentation()
        let commandClick = NSApp.currentEvent.map { $0.type == .leftMouseUp || $0.type == .leftMouseDown ? $0.modifierFlags.contains(.command) : false } ?? false
        model.select(location, preview: preview ?? !commandClick)
        synchronizeSelection(model: model)
        PerformanceProbe.tabSwitched()
    }

    func activateTab(id: String, groupID: String? = nil, model: WireboltModel) {
        persistActivePresentation()
        model.sessions.select(tabID: id, in: groupID)
        synchronizeSelection(model: model)
        PerformanceProbe.tabSwitched()
    }

    func makeNewRequest(model: WireboltModel, kind: DocumentKind = .http, rename: Bool = false) {
        persistActivePresentation()
        Task {
            guard let session = await model.createRequest(kind: kind) else { return }
            activeTabID = session.id
            presentationByTabID[session.id] = DocumentPresentationState()
            requestSection = kind == .http ? .params : .body
            responseSection = .body
            renamingRequestID = rename ? model.selectedRequestID : nil
            if rename { focusSidebarTrigger += 1 } else { pendingURLFocusID = session.id; focusURLTrigger += 1 }
        }
    }

    func makeNewRequest(
        model: WireboltModel,
        kind: DocumentKind = .http,
        collectionID: String,
        groupID: String? = nil
    ) {
        persistActivePresentation()
        Task {
            guard let session = await model.createRequest(
                kind: kind,
                collectionID: collectionID,
                groupID: groupID
            ) else { return }
            activeTabID = session.id
            presentationByTabID[session.id] = DocumentPresentationState()
            requestSection = kind == .http ? .params : .body
            responseSection = .body
            renamingRequestID = nil
            pendingURLFocusID = session.id
            focusURLTrigger += 1
        }
    }

    func resetForWorkspace(model: WireboltModel) {
        rustInterface.reset()
        isShowingEnvironmentInspector = false
        hasEnvironmentEdits = false
        saveEnvironmentEdits = nil
        pendingURLFocusID = nil
        presentationByTabID = [:]
        responseLayoutsByGroupID = [:]
        lastClosedSession = nil
        lastClosedPresentation = nil
        fallbackPresentation = DocumentPresentationState()
        activeTabID = nil
        sidebarFilter = ""
        collapsedSidebarCollections = []
        expandedSidebarGroups = []
        renamingRequestID = nil
        renamingGroupID = nil
        renamingCollectionID = nil
        sidebarCursor = nil
        synchronizeSelection(model: model)
    }

    func promptForNewCollection() {
        workspaceNamePrompt = WorkspaceNamePrompt(
            target: .collection(id: nil),
            title: "New Collection",
            initialName: "New Collection"
        )
    }

    func promptForCollectionRename(_ collection: CollectionDraft) {
        workspaceNamePrompt = WorkspaceNamePrompt(
            target: .collection(id: collection.id),
            title: "Rename Collection",
            initialName: collection.name
        )
    }

    var focusSidebarTrigger = 0
    /// Set when Focus Sidebar has to show the sidebar first; the sidebar takes focus on appear.
    @ObservationIgnored var focusesSidebarOnAppear = false

    /// Shows the sidebar if it is hidden and moves keyboard focus to it.
    func focusSidebar() {
        if columnVisibility == .detailOnly {
            focusesSidebarOnAppear = true
            columnVisibility = .all
        } else {
            focusSidebarTrigger += 1
        }
    }
    var collapsedSidebarCollections: Set<String> = []
    var expandedSidebarGroups: Set<String> = []
    var renamingRequestID: String?
    var renamingGroupID: String?
    var renamingCollectionID: String?
    /// A folder or collection row selected in the sidebar; nil means the active request's row.
    var sidebarCursor: String?
    /// The row that keyboard navigation last selected, so the sidebar can scroll it into view.
    var sidebarScrollTarget: String?

    func makeNewFolder(model: WireboltModel, collectionID: String? = nil, parentID: String? = nil) {
        Task {
            if let collectionID { renamingGroupID = await model.createGroup(collectionID: collectionID, parentID: parentID, name: "Untitled Folder") }
            else { renamingGroupID = await model.createRootFolder() }
        }
    }

    func promptForGroupRename(collectionID: String, group: GroupDraft) {
        workspaceNamePrompt = WorkspaceNamePrompt(
            target: .group(collectionID: collectionID, id: group.id),
            title: "Rename Folder",
            initialName: group.name
        )
    }

    /// Deletes at once when Undo can bring everything back; asks first when the deletion
    /// also removes other items or discards unsaved edits (HIG: no alerts for common,
    /// undoable actions).
    func requestDelete(_ target: WorkspaceDeleteTarget, title: String, model: WireboltModel) {
        let detail = deleteConsequences(of: target, model: model)
        if detail.isEmpty {
            performDelete(target, model: model)
        } else {
            workspaceDeleteRequest = WorkspaceDeleteRequest(target: target, title: title, detail: detail)
        }
    }

    /// Delete in the focused sidebar deletes the selected row.
    func requestDeleteOfSelection(model: WireboltModel) {
        if let cursor = sidebarCursor,
           let row = model.sidebarRows(query: sidebarFilter, collapsed: collapsedSidebarCollections, expanded: expandedSidebarGroups)
            .first(where: { $0.id == cursor }) {
            switch row.content {
            case .collection(let collection):
                requestDelete(.collection(id: collection.id), title: collection.name, model: model)
            case .group(let collection, let group):
                requestDelete(.group(collectionID: collection.id, id: group.id), title: group.name, model: model)
            case .request(let location):
                requestDelete(.request(collectionID: location.collectionID, id: location.request.id), title: location.request.name, model: model)
            }
            return
        }
        guard let session = model.sessions.activeSession, let collectionID = session.collectionID,
              let location = model.workspace.location(collectionID: collectionID, requestID: session.requestID)
        else { return }
        requestDelete(.request(collectionID: collectionID, id: location.request.id), title: location.request.name, model: model)
    }

    func confirmWorkspaceDelete(model: WireboltModel) {
        guard let request = workspaceDeleteRequest else { return }
        workspaceDeleteRequest = nil
        performDelete(request.target, model: model)
    }

    private func performDelete(_ target: WorkspaceDeleteTarget, model: WireboltModel) {
        // Deleted documents must not be restored by the last-closed-tab fallback.
        lastClosedSession = nil
        lastClosedPresentation = nil
        rustInterface.forgetClosed()
        sidebarCursor = nil
        Task {
            switch target {
            case let .collection(id): await model.deleteCollection(id: id)
            case let .group(collectionID, id):
                await model.deleteGroup(collectionID: collectionID, id: id)
            case let .request(collectionID, id):
                await model.deleteRequest(collectionID: collectionID, requestID: id)
            }
        }
    }

    /// Keeps a preview tab open (double-click on the tab or Keep Open).
    func pinTab(id: String, model: WireboltModel) {
        model.sessions.pin(tabID: id)
    }

    /// Describes what a deletion removes besides the item itself; empty when nothing else is lost.
    func deleteConsequences(of target: WorkspaceDeleteTarget, model: WireboltModel) -> String {
        let target: SessionDeleteTarget = switch target {
        case let .collection(id): .collection(id: id)
        case let .group(collectionID, id): .group(collectionId: collectionID, id: id)
        case let .request(collectionID, id): .request(collectionId: collectionID, id: id)
        }
        return model.deleteConsequences(target)
    }

    func openInNewSplit(tabID: String, model: WireboltModel) {
        persistActivePresentation()
        guard model.sessions.split(tabID: tabID) != nil else { return }
        synchronizeSelection(model: model)
    }

    func closeTab(id: String, model: WireboltModel) {
        close(.one(id), model: model)
    }

    func closeActiveTab(model: WireboltModel) {
        guard let id = model.sessions.activeSession?.id else {
            // With no tab left, ⌘W closes the window like other Mac apps.
            NSApp.keyWindow?.performClose(nil)
            return
        }
        close(.one(id), model: model)
    }

    func savedSessionLayout(for url: URL? = nil) -> SessionLayout? {
        guard let path = (url ?? workspaceURL)?.standardizedFileURL.path else { return nil }
        return SessionLayoutStore(defaults: defaults).layout(forWorkspace: path)
    }

    /// Remembers open tabs so relaunch or reopening the workspace restores them.
    func persistSessionLayout(model: WireboltModel) {
        guard model.hasLoadedWorkspace, let path = workspaceURL?.standardizedFileURL.path else { return }
        SessionLayoutStore(defaults: defaults).save(model.sessions.layout, forWorkspace: path)
    }

    func close(_ scope: TabCloseScope, model: WireboltModel, in targetGroupID: String? = nil) {
        close(scope, model: model, groupID: targetGroupID ?? model.sessions.activeGroupID, allowDirty: false)
    }

    private func close(_ scope: TabCloseScope, model: WireboltModel, groupID: String, allowDirty: Bool) {
        let previous = model.sessions.activeSession
        let previousPresentation = previous.flatMap { presentationByTabID[$0.id] }
        let blocked = model.closeDocuments(scope, in: groupID, allowDirty: allowDirty)
        guard blocked.isEmpty else {
            rustInterface.blockClose(request: WorkspaceDirtyClose(scope: scope.sessionRecord, groupId: groupID,
                ids: blocked.map(\.id), titles: blocked.map(\.title)))
            dirtyCloseRequest = DirtyCloseRequest(scope: scope, groupID: groupID, documentTitles: blocked.map(\.title), documentIDs: blocked.map(\.id))
            return
        }
        discardPresentation(for: scope, model: model)
        synchronizeSelection(model: model)
        if rustInterface.closed(previous: previous?.id, active: model.sessions.activeSession?.id) {
            lastClosedSession = previous
            lastClosedPresentation = previousPresentation
            NSApp.keyWindow?.performClose(nil)
        }
    }

    func saveDirtyClose(model: WireboltModel) {
        guard let request = rustInterface.takeClose() else { return }
        dirtyCloseRequest = nil
        Task {
            guard await model.saveDocuments(request.ids) else { return }
            close(TabCloseScope(request.scope), model: model, in: request.groupId)
        }
    }

    func confirmDirtyClose(model: WireboltModel) {
        guard let request = rustInterface.takeClose() else { return }
        dirtyCloseRequest = nil
        close(TabCloseScope(request.scope), model: model, groupID: request.groupId, allowDirty: true)
    }

    func presentation(for session: DocumentSession) -> DocumentPresentationState {
        if let existing = presentationByTabID[session.id] { return existing }
        let state = DocumentPresentationState(requestSection: session.draft.body == .empty ? .params : .body)
        presentationByTabID[session.id] = state
        return state
    }

    private func activatePresentation(for tabID: String, defaultBody: RequestBody) {
        if presentationByTabID[tabID] == nil {
            let state = DocumentPresentationState(requestSection: defaultBody == .empty ? .params : .body)
            presentationByTabID[tabID] = state
        }
        activeTabID = tabID
    }

    private func persistActivePresentation() {
        guard activeTabID != nil else { return }
        defaults.set(requestSection.rawValue, forKey: Self.requestSectionKey)
        defaults.set(responseSection.rawValue, forKey: Self.responseSectionKey)
        defaults.set(responseRenderer.rawValue, forKey: Self.responseRendererKey)
        defaults.set(responseOrientation.rawValue, forKey: Self.responseOrientationKey)
    }

    private func discardPresentation(for scope: TabCloseScope, model: WireboltModel) {
        let remaining = Set(model.sessions.sessions.keys)
        presentationByTabID = presentationByTabID.filter { remaining.contains($0.key) }
    }

    func reopenLastDocument(model: WireboltModel) {
        guard let id = rustInterface.reopen(hasActive: model.sessions.activeSession != nil),
              let session = lastClosedSession, session.id == id else { return }
        model.sessions.reopen(session)
        if let state = lastClosedPresentation { presentationByTabID[session.id] = state }
        lastClosedSession = nil; lastClosedPresentation = nil
        synchronizeSelection(model: model)
    }

    private static let requestSectionKey = "workspace.requestSection"
    private static let responseSectionKey = "workspace.responseSection"
    private static let responseRendererKey = "workspace.responseRenderer"
    private static let responseOrientationKey = "workspace.responseOrientation"
}

extension FocusedValues {
    @Entry var workspaceCommands: WorkspaceUIState?
    @Entry var sidebarMove: SidebarMoveCommands?
}

// MARK: - Sidebar outline navigation

extension WorkspaceUIState {
    var isRenamingInSidebar: Bool {
        renamingRequestID != nil || renamingGroupID != nil || renamingCollectionID != nil
    }

    var isFilteringSidebar: Bool { sidebarIsFiltering }

    func visibleSidebarRows(model: WireboltModel) -> [SidebarSnapshot.Row] {
        guard collectionsExpanded || isFilteringSidebar else { return [] }
        return model.sidebarRows(query: sidebarFilter, collapsed: collapsedSidebarCollections, expanded: expandedSidebarGroups)
    }

    /// The highlighted row: a selected folder or collection, otherwise the active request.
    func sidebarSelectionRowID(model: WireboltModel) -> String? {
        workspaceSidebarSelection(cursor: sidebarCursor, selected: model.selectedRequestID)
    }

    func isSidebarRowExpanded(_ row: SidebarSnapshot.Row) -> Bool {
        row.isExpanded(collapsed: collapsedSidebarCollections, expanded: expandedSidebarGroups, filtering: isFilteringSidebar)
    }

    func setSidebarRow(_ row: SidebarSnapshot.Row, expanded: Bool) {
        applySidebar(rustInterface.sidebar(input: sidebarState, rows: [row.workspaceMetadata], selectedRequest: nil,
            action: .setExpanded(id: row.id, expanded: expanded)))
    }

    /// Selecting a request row opens it; folders and collections are only highlighted.
    func selectSidebarRow(_ id: String, model: WireboltModel, scroll: Bool = false) {
        _ = navigateSidebar(.select(id: id, scroll: scroll), model: model)
    }

    /// ↑/↓ over every visible row, including folders and collections.
    func moveSidebarSelection(_ delta: Int, model: WireboltModel) {
        _ = navigateSidebar(.step(delta: Int64(delta)), model: model)
    }

    /// ← collapses or selects the parent; → expands or selects the first child.
    func moveSidebarSelectionHorizontally(right: Bool, model: WireboltModel) {
        _ = navigateSidebar(.arrow(right: right), model: model)
    }

    /// Type-select: letters typed within a second extend the search prefix.
    func typeSelectInSidebar(_ characters: String, model: WireboltModel) -> Bool {
        navigateSidebar(.type(characters: characters, time: Date().timeIntervalSince1970), model: model)
    }

    /// Return renames the selected row, as in Finder and the Xcode navigator.
    func renameSidebarSelection(model: WireboltModel) {
        _ = navigateSidebar(.rename, model: model)
    }

    func beginRename(_ row: SidebarSnapshot.Row) {
        switch row.content {
        case .collection(let collection): renamingCollectionID = collection.id
        case .group(_, let group): renamingGroupID = group.id
        case .request(let location): renamingRequestID = location.id
        }
    }

    /// ⌘↓ opens the selection: a request moves focus to its URL, a container toggles.
    func openSidebarSelection(model: WireboltModel) {
        _ = navigateSidebar(.open, model: model)
    }

    /// The selected request or folder as a move identifier ("kind|collection|id").
    func sidebarMoveIdentifier(model: WireboltModel) -> String? {
        let saved = model.sessions.activeSession.map { session in session.collectionID.map { model.workspace.location(collectionID: $0, requestID: session.requestID) != nil } ?? false } ?? false
        return workspaceSidebarMoveIdentifier(cursor: sidebarCursor, selected: model.selectedRequestID,
            rows: sidebarCursor == nil ? [] : visibleSidebarRows(model: model).map(\.workspaceMetadata), saved: saved)
    }

    private var sidebarState: WorkspaceSidebarState {
        WorkspaceSidebarState(query: sidebarFilter, cursor: sidebarCursor, scrollTarget: sidebarScrollTarget,
            collapsed: Array(collapsedSidebarCollections), expanded: Array(expandedSidebarGroups), collectionsExpanded: collectionsExpanded)
    }

    @discardableResult
    private func navigateSidebar(_ action: WorkspaceSidebarAction, model: WireboltModel) -> Bool {
        let rows = visibleSidebarRows(model: model)
        let result = rustInterface.sidebar(input: sidebarState, rows: rows.map(\.workspaceMetadata),
            selectedRequest: model.selectedRequestID, action: action)
        applySidebar(result)
        if let id = result.openRequest, let row = rows.first(where: { $0.id == id }), case let .request(location) = row.content {
            activateSavedRequest(location, model: model)
        }
        if let id = result.renameRow, let row = rows.first(where: { $0.id == id }) { beginRename(row) }
        if result.focusUrl { focusURLTrigger += 1 }
        return result.handled
    }

    private func applySidebar(_ result: WorkspaceSidebarResult) {
        let state = result.state
        if sidebarCursor != state.cursor { sidebarCursor = state.cursor }
        if sidebarScrollTarget != state.scrollTarget { sidebarScrollTarget = state.scrollTarget }
        let collapsed = Set(state.collapsed), expanded = Set(state.expanded)
        if collapsedSidebarCollections != collapsed { collapsedSidebarCollections = collapsed }
        if expandedSidebarGroups != expanded { expandedSidebarGroups = expanded }
    }

    func sidebarMoveCommands(model: WireboltModel) -> SidebarMoveCommands? {
        guard let identifier = sidebarMoveIdentifier(model: model) else { return nil }
        return SidebarMoveCommands(identifier: identifier,
                                   canMoveUp: model.canMoveSidebarItem(identifier, by: -1),
                                   canMoveDown: model.canMoveSidebarItem(identifier, by: 1))
    }

    func moveSidebarItem(_ identifier: String, by delta: Int, model: WireboltModel) {
        Task { await model.moveSidebarItem(identifier, by: delta) }
    }
}
