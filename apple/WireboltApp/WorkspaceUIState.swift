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

struct DocumentPresentationState: Equatable {
    var requestSection: RequestPanelSection = .params
    var responseSection: ResponsePanelSection = .body
    var responseRenderer: ResponseRenderer = .json
    var responseOrientation: ResponseOrientation = .bottom
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
    var requestSection: RequestPanelSection {
        didSet { persistActivePresentation() }
    }
    var responseSection: ResponsePanelSection {
        didSet { persistActivePresentation() }
    }
    var responseRenderer: ResponseRenderer {
        didSet {
            persistActivePresentation()
            PerformanceProbe.rendererReady(responseRenderer.rawValue)
        }
    }
    var responseOrientation: ResponseOrientation {
        didSet { persistActivePresentation() }
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

    func makeNewRequest(model: WireboltModel, kind: DocumentKind = .http) {
        persistActivePresentation()
        let session = model.makeNewRequest(kind: kind)
        activeTabID = session.id
        presentationByTabID[session.id] = DocumentPresentationState()
        requestSection = kind == .http ? .params : .body
        responseSection = .body
        focusURLTrigger += 1
    }

    func makeNewRequest(
        model: WireboltModel,
        kind: DocumentKind = .http,
        collectionID: String,
        groupID: String? = nil
    ) {
        persistActivePresentation()
        let session = model.makeNewRequest(
            kind: kind,
            collectionID: collectionID,
            groupID: groupID
        )
        activeTabID = session.id
        presentationByTabID[session.id] = DocumentPresentationState()
        requestSection = kind == .http ? .params : .body
        responseSection = .body
        focusURLTrigger += 1
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

    func close(_ scope: TabCloseScope, model: WireboltModel) {
        let groupID = model.sessions.activeGroupID
        let blocked = model.sessions.close(scope, in: groupID)
        guard blocked.isEmpty else {
            dirtyCloseRequest = DirtyCloseRequest(
                scope: scope,
                groupID: groupID,
                documentTitles: blocked.map(\.title)
            )
            return
        }
        discardPresentation(for: scope, model: model)
        ensureOneTab(model: model)
        synchronizeSelection(model: model)
    }

    func confirmDirtyClose(model: WireboltModel) {
        guard let request = dirtyCloseRequest else { return }
        _ = model.sessions.close(request.scope, in: request.groupID, allowDirty: true)
        dirtyCloseRequest = nil
        discardPresentation(for: request.scope, model: model)
        ensureOneTab(model: model)
        synchronizeSelection(model: model)
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

    private func activatePresentation(for tabID: String, defaultBody: RequestBody) {
        activeTabID = tabID
        let fallback = DocumentPresentationState(
            requestSection: defaultBody == .empty ? .params : .body
        )
        let state = presentationByTabID[tabID] ?? fallback
        requestSection = state.requestSection
        responseSection = state.responseSection
        responseRenderer = state.responseRenderer
        responseOrientation = state.responseOrientation
        defaults.set(tabID, forKey: Self.activeTabKey)
    }

    private func persistActivePresentation() {
        guard let activeTabID else { return }
        presentationByTabID[activeTabID] = DocumentPresentationState(
            requestSection: requestSection,
            responseSection: responseSection,
            responseRenderer: responseRenderer,
            responseOrientation: responseOrientation
        )
        defaults.set(requestSection.rawValue, forKey: Self.requestSectionKey)
        defaults.set(responseSection.rawValue, forKey: Self.responseSectionKey)
        defaults.set(responseRenderer.rawValue, forKey: Self.responseRendererKey)
        defaults.set(responseOrientation.rawValue, forKey: Self.responseOrientationKey)
    }

    private func discardPresentation(for scope: TabCloseScope, model: WireboltModel) {
        let remaining = Set(model.sessions.sessions.keys)
        presentationByTabID = presentationByTabID.filter { remaining.contains($0.key) }
    }

    private func ensureOneTab(model: WireboltModel) {
        if model.sessions.activeSession == nil {
            makeNewRequest(model: model)
        }
    }

    private static let requestSectionKey = "workspace.requestSection"
    private static let responseSectionKey = "workspace.responseSection"
    private static let responseRendererKey = "workspace.responseRenderer"
    private static let responseOrientationKey = "workspace.responseOrientation"
    private static let activeTabKey = "workspace.activeTab"
}

enum WireboltTheme {
    /// Brand indigo sampled from the approved UI reference: #6159E5.
    static let primaryAccent = Color(
        red: 97.0 / 255.0,
        green: 89.0 / 255.0,
        blue: 229.0 / 255.0
    )
    static let paneBackground = Color(nsColor: .textBackgroundColor)
    static let barBackground = AnyShapeStyle(.bar)
    static let separator = Color(nsColor: .separatorColor)
    static let success = Color(red: 0.31, green: 0.80, blue: 0.34)

    static let nsJSONKey = adaptiveColor(
        light: NSColor(srgbRed: 0.56, green: 0.24, blue: 0.22, alpha: 1),
        dark: NSColor(srgbRed: 0.50, green: 0.72, blue: 0.84, alpha: 1)
    )
    static let nsJSONString = adaptiveColor(
        light: NSColor(srgbRed: 0.22, green: 0.46, blue: 0.59, alpha: 1),
        dark: NSColor(srgbRed: 0.82, green: 0.58, blue: 0.49, alpha: 1)
    )
    static let nsJSONNumber = adaptiveColor(
        light: NSColor(srgbRed: 0.24, green: 0.44, blue: 0.56, alpha: 1),
        dark: NSColor(srgbRed: 0.75, green: 0.69, blue: 0.46, alpha: 1)
    )
    static let nsJSONBoolean = adaptiveColor(
        light: NSColor(srgbRed: 0.31, green: 0.49, blue: 0.29, alpha: 1),
        dark: NSColor(srgbRed: 0.62, green: 0.72, blue: 0.43, alpha: 1)
    )
    static let nsJSONNull = adaptiveColor(
        light: NSColor(srgbRed: 0.52, green: 0.30, blue: 0.56, alpha: 1),
        dark: NSColor(srgbRed: 0.72, green: 0.54, blue: 0.77, alpha: 1)
    )

    static let jsonKey = Color(nsColor: nsJSONKey)
    static let jsonString = Color(nsColor: nsJSONString)
    static let jsonNumber = Color(nsColor: nsJSONNumber)
    static let jsonBoolean = Color(nsColor: nsJSONBoolean)
    static let jsonNull = Color(nsColor: nsJSONNull)
    static let treeKey = Color(red: 0.25, green: 0.50, blue: 0.68)
    static let treeValue = Color(red: 0.64, green: 0.29, blue: 0.27)

    static func methodColor(_ method: HTTPMethod) -> Color {
        switch method {
        case .get, .head, .options: Color(red: 0.31, green: 0.78, blue: 0.35)
        case .post: Color(red: 0.20, green: 0.59, blue: 0.86)
        case .put: Color(red: 0.82, green: 0.61, blue: 0.18)
        case .patch: Color(red: 0.28, green: 0.63, blue: 0.82)
        case .delete: Color(red: 0.86, green: 0.28, blue: 0.29)
        default: primaryAccent
        }
    }

    static func requestBarMethodColor(_ method: HTTPMethod) -> Color {
        method == .patch ? success : methodColor(method)
    }

    static func statusColor(_ status: UInt16) -> Color {
        switch status {
        case 200 ..< 300: success
        case 300 ..< 400: .orange
        default: .red
        }
    }

    private static func adaptiveColor(light: NSColor, dark: NSColor) -> NSColor {
        NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
        }
    }
}
