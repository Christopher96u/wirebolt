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
}

@MainActor
@Observable
final class WorkspaceUIState {
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
    func addKey() { activePresentation.isBulkEditing = false; activePresentation.focusNewKeyTrigger += 1 }
    func selectTab(index: Int? = nil, offset: Int = 0, model: WireboltModel) {
        guard let group = model.sessions.activeGroup, !group.tabIDs.isEmpty else { return }
        let current = group.tabIDs.firstIndex(of: group.selectedTabID ?? "") ?? 0
        let target = index ?? ((current + offset + group.tabIDs.count) % group.tabIDs.count)
        guard group.tabIDs.indices.contains(target) else { return }
        model.sessions.select(tabID: group.tabIDs[target]); synchronizeSelection(model: model)
    }
    var activeTabID: String?
    var sidebarFilter = ""
    var isShowingImporter = false
    var isShowingCurlImporter = false
    var importFormat: ImportFormat = .postmanV2
    var importStatus: String?
    var focusURLTrigger = 0
    var focusSearchTrigger = 0
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

    func activateSavedRequest(_ location: RequestLocation, model: WireboltModel) {
        persistActivePresentation()
        model.select(location)
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
    var collapsedSidebarCollections: Set<String> = []
    var expandedSidebarGroups: Set<String> = []
    var renamingRequestID: String?

    func moveSidebarSelection(_ delta: Int, model: WireboltModel) {
        let visible = model.sidebarRows(query: sidebarFilter, collapsed: collapsedSidebarCollections, expanded: expandedSidebarGroups).compactMap { row -> RequestLocation? in
            if case .request(let location) = row.content { return location }
            return nil
        }
        guard !visible.isEmpty else { return }
        let current = visible.firstIndex(where: { $0.id == model.selectedRequestID }) ?? (delta > 0 ? -1 : visible.count)
        activateSavedRequest(visible[min(max(0, current + delta), visible.count - 1)], model: model)
    }

    var renamingGroupID: String?

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

    func requestDelete(_ target: WorkspaceDeleteTarget, title: String) {
        workspaceDeleteRequest = WorkspaceDeleteRequest(target: target, title: title)
    }

    func confirmWorkspaceDelete(model: WireboltModel) {
        guard let request = workspaceDeleteRequest else { return }
        workspaceDeleteRequest = nil
        Task {
            switch request.target {
            case let .collection(id): await model.deleteCollection(id: id)
            case let .group(collectionID, id):
                await model.deleteGroup(collectionID: collectionID, id: id)
            case let .request(collectionID, id):
                await model.deleteRequest(collectionID: collectionID, requestID: id)
            }
        }
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
        guard let id = model.sessions.activeSession?.id else { return }
        close(.one(id), model: model)
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
        defaults.set(tabID, forKey: Self.activeTabKey)
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
    private static let activeTabKey = "workspace.activeTab"
}

extension FocusedValues {
    @Entry var workspaceCommands: WorkspaceUIState?
}
