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
    case raw = "Raw"
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
    case raw = "Raw"
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
}

@MainActor
@Observable
final class DocumentPresentationState {
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
        let current = group.tabIDs.firstIndex(of: group.selectedTabID ?? "") ?? 0
        let target = index ?? ((current + offset + group.tabIDs.count) % group.tabIDs.count)
        guard group.tabIDs.indices.contains(target) else { return }
        model.sessions.select(tabID: group.tabIDs[target]); synchronizeSelection(model: model)
    }
    var activeTabID: String?
    /// The folder of the open workspace; editor layouts are remembered per folder.
    @ObservationIgnored var workspaceURL: URL?
    var sidebarFilter = ""
    var isShowingImporter = false
    var isShowingCurlImporter = false
    var importFormat: ImportFormat = .postmanV2
    var importStatus: String?
    var focusURLTrigger = 0
    var focusSearchTrigger = 0
    /// Moves keyboard focus into the active editor group's response content.
    var focusResponseTrigger = 0
    var dirtyCloseRequest: DirtyCloseRequest?
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
            .flatMap(ResponsePanelSection.init(rawValue:)) ?? .body
        responseRenderer = defaults.string(forKey: Self.responseRendererKey)
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

    func makeNewRequest(model: WireboltModel, kind: DocumentKind = .http, rename: Bool = true) {
        persistActivePresentation()
        Task {
            guard let session = await model.createRequest(kind: kind) else { return }
            activeTabID = session.id
            presentationByTabID[session.id] = DocumentPresentationState()
            requestSection = kind == .http ? .params : .body
            responseSection = .body
            renamingRequestID = rename ? model.selectedRequestID : nil
            focusSidebarTrigger += 1
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
            renamingRequestID = model.selectedRequestID
            focusSidebarTrigger += 1
        }
    }

    func resetForWorkspace(model: WireboltModel) {
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
    @ObservationIgnored var typeSelectBuffer = ""
    @ObservationIgnored var typeSelectTime = Date.distantPast

    func makeNewFolder(model: WireboltModel, collectionID: String? = nil, parentID: String? = nil) {
        Task {
            if let collectionID { renamingGroupID = await model.createGroup(collectionID: collectionID, parentID: parentID, name: "Untitled Folder") }
            else { renamingGroupID = await model.createRootFolder() }
        }
    }

    func promptForNewGroup(collectionID: String) {
        workspaceNamePrompt = WorkspaceNamePrompt(
            target: .group(collectionID: collectionID, id: nil),
            title: "New Folder",
            initialName: "New Folder"
        )
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
        let collectionID: String
        let requests: [RequestLocation]
        let folderCount: Int
        switch target {
        case let .request(id, requestID):
            collectionID = id
            requests = model.workspace.location(collectionID: id, requestID: requestID).map { [$0] } ?? []
            folderCount = 0
        case let .group(id, groupID):
            guard let collection = model.workspace.collections.first(where: { $0.id == id }) else { return "" }
            let folders = collection.descendantGroupIDs(of: groupID)
            collectionID = id
            requests = collection.requests.filter { $0.groupID.map(folders.contains) ?? false }
            folderCount = folders.count - 1
        case let .collection(id):
            guard let collection = model.workspace.collections.first(where: { $0.id == id }) else { return "" }
            collectionID = id
            requests = collection.requests
            folderCount = collection.groups.count
        }
        let ids = Set(requests.map(\.request.id))
        let hasUnsavedEdits = model.sessions.sessions.values.contains {
            $0.isDirty && $0.collectionID == collectionID && ids.contains($0.requestID)
        }
        var parts: [String] = []
        let isContainer = if case .request = target { false } else { true }
        if isContainer {
            let contents = [
                requests.isEmpty ? nil : requests.count == 1 ? "1 request" : "\(requests.count) requests",
                folderCount == 0 ? nil : folderCount == 1 ? "1 folder" : "\(folderCount) folders",
            ].compactMap(\.self)
            if !contents.isEmpty { parts.append("Its \(contents.joined(separator: " and ")) will also be deleted.") }
        }
        if hasUnsavedEdits { parts.append("Unsaved changes in open tabs will be discarded and can’t be restored.") }
        guard !parts.isEmpty else { return "" }
        parts.append("You can undo the deletion with Edit ▸ Undo (⌘Z).")
        return parts.joined(separator: " ")
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
        let groupID = targetGroupID ?? model.sessions.activeGroupID
        let previous = model.sessions.activeSession
        let previousPresentation = previous.flatMap { presentationByTabID[$0.id] }
        let blocked = model.closeDocuments(scope, in: groupID)
        guard blocked.isEmpty else {
            dirtyCloseRequest = DirtyCloseRequest(
                scope: scope,
                groupID: groupID,
                documentTitles: blocked.map(\.title)
            )
            return
        }
        discardPresentation(for: scope, model: model)
        synchronizeSelection(model: model)
        if model.sessions.activeSession == nil {
            lastClosedSession = previous
            lastClosedPresentation = previousPresentation
            NSApp.keyWindow?.performClose(nil)
        }
    }

    func confirmDirtyClose(model: WireboltModel) {
        guard let request = dirtyCloseRequest else { return }
        let previous = model.sessions.activeSession
        let previousPresentation = previous.flatMap { presentationByTabID[$0.id] }
        model.closeDocuments(request.scope, in: request.groupID, allowDirty: true)
        dirtyCloseRequest = nil
        discardPresentation(for: request.scope, model: model)
        synchronizeSelection(model: model)
        if model.sessions.activeSession == nil {
            lastClosedSession = previous
            lastClosedPresentation = previousPresentation
            NSApp.keyWindow?.performClose(nil)
        }
    }

    func selectImportedFile(_ url: URL) {
        let hasAccess = url.startAccessingSecurityScopedResource()
        defer {
            if hasAccess { url.stopAccessingSecurityScopedResource() }
        }
        importStatus = "Ready to import \(url.lastPathComponent)"
    }

    func reportImportFailure() {
        importStatus = "The collection could not be imported"
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
        guard model.sessions.activeSession == nil, let session = lastClosedSession else { return }
        model.sessions.reopen(session)
        if let state = lastClosedPresentation { presentationByTabID[session.id] = state }
        lastClosedSession = nil
        lastClosedPresentation = nil
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

    var isFilteringSidebar: Bool {
        !sidebarFilter.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    func visibleSidebarRows(model: WireboltModel) -> [SidebarSnapshot.Row] {
        model.sidebarRows(query: sidebarFilter, collapsed: collapsedSidebarCollections, expanded: expandedSidebarGroups)
    }

    /// The highlighted row: a selected folder or collection, otherwise the active request.
    func sidebarSelectionRowID(model: WireboltModel) -> String? {
        sidebarCursor ?? model.selectedRequestID.map { "request:" + $0 }
    }

    func isSidebarRowExpanded(_ row: SidebarSnapshot.Row) -> Bool {
        row.isExpanded(collapsed: collapsedSidebarCollections, expanded: expandedSidebarGroups, filtering: isFilteringSidebar)
    }

    func setSidebarRow(_ row: SidebarSnapshot.Row, expanded: Bool) {
        switch row.content {
        case .collection(let collection):
            if expanded { collapsedSidebarCollections.remove(collection.id) } else { collapsedSidebarCollections.insert(collection.id) }
        case .group(let collection, let group):
            let key = collection.id + ":" + group.id
            if expanded { expandedSidebarGroups.insert(key) } else { expandedSidebarGroups.remove(key) }
        case .request:
            break
        }
    }

    /// Selecting a request row opens it; folders and collections are only highlighted.
    func selectSidebarRow(_ id: String, model: WireboltModel, scroll: Bool = false) {
        guard let row = visibleSidebarRows(model: model).first(where: { $0.id == id }) else { return }
        if case .request(let location) = row.content {
            sidebarCursor = nil
            if model.selectedRequestID != location.id { activateSavedRequest(location, model: model) }
        } else {
            sidebarCursor = id
        }
        if scroll { sidebarScrollTarget = id }
    }

    /// ↑/↓ over every visible row, including folders and collections.
    func moveSidebarSelection(_ delta: Int, model: WireboltModel) {
        let rows = visibleSidebarRows(model: model)
        guard let id = SidebarNavigation.step(from: sidebarSelectionRowID(model: model), by: delta, in: rows) else { return }
        selectSidebarRow(id, model: model, scroll: true)
    }

    /// ← collapses or selects the parent; → expands or selects the first child.
    func moveSidebarSelectionHorizontally(right: Bool, model: WireboltModel) {
        let rows = visibleSidebarRows(model: model)
        let current = sidebarSelectionRowID(model: model)
        let outcome = right
            ? SidebarNavigation.right(from: current, in: rows, isExpanded: isSidebarRowExpanded)
            : SidebarNavigation.left(from: current, in: rows, isExpanded: isSidebarRowExpanded)
        switch outcome {
        case let .select(id): selectSidebarRow(id, model: model, scroll: true)
        case let .expand(id), let .collapse(id):
            guard let row = rows.first(where: { $0.id == id }) else { return }
            // Collapsing hides the active request; keep the container selected instead.
            if case .collapse = outcome { sidebarCursor = id }
            setSidebarRow(row, expanded: !isSidebarRowExpanded(row))
        case .none: break
        }
    }

    /// Type-select: letters typed within a second extend the search prefix.
    func typeSelectInSidebar(_ characters: String, model: WireboltModel) -> Bool {
        let now = Date()
        if now.timeIntervalSince(typeSelectTime) > 1 { typeSelectBuffer = "" }
        guard !(typeSelectBuffer.isEmpty && characters == " ") else { return false }
        typeSelectBuffer += characters
        typeSelectTime = now
        let rows = visibleSidebarRows(model: model)
        guard let id = SidebarNavigation.typeSelect(typeSelectBuffer, from: sidebarSelectionRowID(model: model), in: rows)
        else { return true }
        selectSidebarRow(id, model: model, scroll: true)
        return true
    }

    /// Return renames the selected row, as in Finder and the Xcode navigator.
    func renameSidebarSelection(model: WireboltModel) {
        guard let id = sidebarSelectionRowID(model: model),
              let row = visibleSidebarRows(model: model).first(where: { $0.id == id }) else { return }
        beginRename(row)
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
        guard let id = sidebarSelectionRowID(model: model),
              let row = visibleSidebarRows(model: model).first(where: { $0.id == id }) else { return }
        if row.isContainer { setSidebarRow(row, expanded: !isSidebarRowExpanded(row)) } else { focusURLTrigger += 1 }
    }

    /// The selected request or folder as a move identifier ("kind|collection|id").
    func sidebarMoveIdentifier(model: WireboltModel) -> String? {
        if let cursor = sidebarCursor {
            return visibleSidebarRows(model: model).first { $0.id == cursor }?.moveIdentifier
        }
        guard let session = model.sessions.activeSession, let collectionID = session.collectionID,
              model.workspace.location(collectionID: collectionID, requestID: session.requestID) != nil
        else { return nil }
        return "request|\(collectionID)|\(session.requestID)"
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
