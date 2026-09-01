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
    case timeline = "Timeline"

    var id: Self { self }
}

enum ResponseRenderer: String, CaseIterable, Identifiable {
    case json = "JSON"
    case tree = "JSON Tree View"
    case image = "Image"
    case xml = "XML"
    case html = "HTML"
    case raw = "Raw"

    var id: Self { self }
}

struct WireboltDocumentTab: Identifiable, Equatable {
    let id: String
    var draft: RequestDraft
    var status: UInt16?
    var isDirty: Bool

    var title: String { draft.name }
}

struct DemoSidebarRequest: Identifiable, Equatable {
    let id: String
    let draft: RequestDraft
    let status: UInt16
}

struct DemoSidebarCollection: Identifiable, Equatable {
    let id: String
    let name: String
    let requests: [DemoSidebarRequest]
}

@MainActor
@Observable
final class WorkspaceUIState {
    @ObservationIgnored private let defaults: UserDefaults

    var columnVisibility = NavigationSplitViewVisibility.all
    var requestSection: RequestPanelSection {
        didSet { defaults.set(requestSection.rawValue, forKey: Self.requestSectionKey) }
    }
    var responseSection: ResponsePanelSection {
        didSet { defaults.set(responseSection.rawValue, forKey: Self.responseSectionKey) }
    }
    var responseRenderer: ResponseRenderer {
        didSet { defaults.set(responseRenderer.rawValue, forKey: Self.responseRendererKey) }
    }
    var openTabs = WorkspaceUIState.initialTabs
    var activeTabID: String {
        didSet { defaults.set(activeTabID, forKey: Self.activeTabKey) }
    }
    var sidebarFilter = ""
    var isShowingImporter = false
    var importStatus: String?
    var didImportDemoCollection: Bool {
        didSet { defaults.set(didImportDemoCollection, forKey: Self.importedCollectionKey) }
    }
    var focusURLTrigger = 0
    var focusSearchTrigger = 0

    let demoCollections = WorkspaceUIState.collections

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        requestSection = defaults.string(forKey: Self.requestSectionKey)
            .flatMap(RequestPanelSection.init(rawValue:)) ?? .headers
        responseSection = defaults.string(forKey: Self.responseSectionKey)
            .flatMap(ResponsePanelSection.init(rawValue:)) ?? .body
        responseRenderer = defaults.string(forKey: Self.responseRendererKey)
            .flatMap(ResponseRenderer.init(rawValue:)) ?? .json
        let storedTabID = defaults.string(forKey: Self.activeTabKey)
        activeTabID = Self.initialTabs.contains(where: { $0.id == storedTabID })
            ? storedTabID ?? "patch-request"
            : "patch-request"
        didImportDemoCollection = defaults.bool(forKey: Self.importedCollectionKey)
    }

    var activeTab: WireboltDocumentTab? {
        openTabs.first(where: { $0.id == activeTabID })
    }

    var visibleDemoCollections: [DemoSidebarCollection] {
        let query = sidebarFilter.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return demoCollections }
        return demoCollections.compactMap { collection in
            let requests = collection.requests.filter {
                $0.draft.name.localizedCaseInsensitiveContains(query)
                    || $0.draft.method.rawValue.localizedCaseInsensitiveContains(query)
                    || $0.draft.url.localizedCaseInsensitiveContains(query)
            }
            guard collection.name.localizedCaseInsensitiveContains(query) || !requests.isEmpty else {
                return nil
            }
            return DemoSidebarCollection(
                id: collection.id,
                name: collection.name,
                requests: requests.isEmpty ? collection.requests : requests
            )
        }
    }

    func activateDemoRequest(_ request: DemoSidebarRequest, model: WireboltModel) {
        activate(
            WireboltDocumentTab(
                id: request.id,
                draft: request.draft,
                status: request.status,
                isDirty: false
            ),
            model: model
        )
    }

    func activateSavedRequest(_ location: RequestLocation, model: WireboltModel) {
        model.select(location)
        activate(
            WireboltDocumentTab(
                id: location.id,
                draft: location.request,
                status: nil,
                isDirty: false
            ),
            model: model,
            updateDraft: false
        )
    }

    func activateTab(id: String, model: WireboltModel) {
        guard let tab = openTabs.first(where: { $0.id == id }) else { return }
        activeTabID = id
        model.draft = tab.draft
        requestSection = tab.draft.body == .empty ? .params : .body
        responseSection = .body
    }

    func makeNewRequest(model: WireboltModel) {
        model.makeNewRequest()
        let tab = WireboltDocumentTab(
            id: model.draft.id,
            draft: model.draft,
            status: nil,
            isDirty: true
        )
        openTabs.append(tab)
        activeTabID = tab.id
        requestSection = .params
        responseSection = .body
        focusURLTrigger += 1
    }

    func closeTab(id: String, model: WireboltModel) {
        guard let index = openTabs.firstIndex(where: { $0.id == id }) else { return }
        let wasActive = activeTabID == id
        openTabs.remove(at: index)

        guard wasActive else { return }
        guard !openTabs.isEmpty else {
            makeNewRequest(model: model)
            return
        }
        let nextIndex = min(index, openTabs.count - 1)
        activateTab(id: openTabs[nextIndex].id, model: model)
    }

    func closeActiveTab(model: WireboltModel) {
        closeTab(id: activeTabID, model: model)
    }

    func captureActiveDraft(_ draft: RequestDraft) {
        guard let index = openTabs.firstIndex(where: { $0.id == activeTabID }),
              openTabs[index].draft.id == draft.id,
              openTabs[index].draft != draft
        else { return }

        openTabs[index].draft = draft
        openTabs[index].isDirty = true
    }

    func selectImportedFile(_ url: URL) {
        let hasAccess = url.startAccessingSecurityScopedResource()
        defer {
            if hasAccess { url.stopAccessingSecurityScopedResource() }
        }
        didImportDemoCollection = true
        importStatus = nil
    }

    func reportImportFailure() {
        importStatus = "The collection could not be imported"
    }

    func demoResponseText(for requestID: String?) -> String {
        switch requestID {
        case "get-request":
            return Self.getRequestResponse
        case "list-users":
            return Self.usersResponse
        case "health-check":
            return #"{"status":"ok","region":"syd1","version":"1.0.0"}"#
        case "update-user":
            return #"{"error":"validation_failed","field":"role","message":"Unknown role"}"#
        case "delete-user":
            return #"{"deleted":true,"id":"usr_8f31"}"#
        default:
            return Self.postmanEchoResponse
        }
    }

    private func activate(
        _ tab: WireboltDocumentTab,
        model: WireboltModel,
        updateDraft: Bool = true
    ) {
        if let index = openTabs.firstIndex(where: { $0.id == tab.id }) {
            openTabs[index] = tab
        } else {
            openTabs.append(tab)
        }
        activeTabID = tab.id
        if updateDraft { model.draft = tab.draft }
        requestSection = tab.draft.body == .empty ? .params : .body
        responseSection = .body
    }

    private static let listUsersDraft = RequestDraft(
        id: "list-users",
        name: "List users",
        method: .get,
        url: "https://api.example.com/v1/users",
        query: [
            RequestField(name: "page", value: .literal("1")),
            RequestField(name: "limit", value: .literal("20")),
            RequestField(name: "sort", value: .literal("created_at:desc")),
        ],
        headers: [
            RequestField(name: "Accept", value: .literal("application/json")),
            RequestField(name: "X-Request-ID", value: .literal("{{request_id}}")),
        ]
    )

    private static let createUserDraft = RequestDraft(
        id: "create-user",
        name: "Create user",
        method: .post,
        url: "https://api.example.com/v1/users",
        headers: [
            RequestField(name: "Content-Type", value: .literal("application/json")),
            RequestField(name: "Accept", value: .literal("application/json")),
            RequestField(name: "X-Request-ID", value: .literal("{{request_id}}")),
        ],
        body: .json(value: """
        {
          "name": "Ada Lovelace",
          "role": "admin",
          "active": true
        }
        """)
    )

    private static let updateUserDraft = RequestDraft(
        id: "update-user",
        name: "Update user",
        method: .patch,
        url: "https://api.example.com/v1/users/usr_8f31",
        headers: [RequestField(name: "Content-Type", value: .literal("application/json"))],
        body: .json(value: """
        {
          "role": "owner"
        }
        """)
    )

    private static let healthDraft = RequestDraft(
        id: "health-check",
        name: "Health check",
        method: .get,
        url: "https://api.example.com/health"
    )

    private static let patchRequestDraft = RequestDraft(
        id: "patch-request",
        name: "PATCH Request",
        method: .patch,
        url: "https://postman-echo.com/patch",
        headers: [
            RequestField(name: "Content-Type", value: .literal("application/json")),
            RequestField(name: "X-Data", value: .literal("GET API Token")),
            RequestField(name: "Early-Data", value: .literal("123")),
        ]
    )

    private static let initialTabs = [
        WireboltDocumentTab(id: "patch-request", draft: patchRequestDraft, status: 200, isDirty: false),
    ]

    private static let requestSectionKey = "workspace.requestSection"
    private static let responseSectionKey = "workspace.responseSection"
    private static let responseRendererKey = "workspace.responseRenderer"
    private static let activeTabKey = "workspace.activeTab"
    private static let importedCollectionKey = "workspace.didImportDemoCollection"

    private static let collections = [
        DemoSidebarCollection(
            id: "accounts",
            name: "Accounts",
            requests: [
                DemoSidebarRequest(id: "list-users", draft: listUsersDraft, status: 200),
                DemoSidebarRequest(id: "create-user", draft: createUserDraft, status: 201),
                DemoSidebarRequest(
                    id: "delete-user",
                    draft: RequestDraft(
                        id: "delete-user",
                        name: "Delete user",
                        method: .delete,
                        url: "https://api.example.com/v1/users/usr_8f31"
                    ),
                    status: 204
                ),
            ]
        ),
        DemoSidebarCollection(
            id: "authentication",
            name: "Authentication",
            requests: [
                DemoSidebarRequest(
                    id: "bearer-token",
                    draft: RequestDraft(
                        id: "bearer-token",
                        name: "Bearer token",
                        method: .get,
                        url: "https://api.example.com/v1/me",
                        authentication: .bearer(token: .secret("auth.token"))
                    ),
                    status: 200
                ),
            ]
        ),
        DemoSidebarCollection(
            id: "health",
            name: "Health",
            requests: [DemoSidebarRequest(id: "health-check", draft: healthDraft, status: 204)]
        ),
    ]

    private static let createResponse = """
    {
      "id": "usr_8f31",
      "name": "Ada Lovelace",
      "role": "admin",
      "active": true,
      "created_at": "2026-08-31T10:42:18Z"
    }
    """

    private static let postmanEchoResponse = """
    {
      "args": {},
      "data": "This is expected to be sent back as part of response body.",
      "files": {},
      "form": {},
      "headers": {
        "x-forwarded-proto": "https",
        "x-forwarded-port": "443",
        "host": "postman-echo.com",
        "x-amzn-trace-id": "Root=1-664ea701-4ada005007faa55d2e270404",
        "content-length": "58",
        "content-type": "application/json"
      },
      "json": null,
      "url": "https://postman-echo.com/patch"
    }
    """

    private static let getRequestResponse = """
    {
      "args": {
        "foo1": "bar1",
        "foo2": "bar2"
      },
      "headers": {
        "x-amzn-trace-id": "Root=1-664ea94f-644f1b9f5d5715a64f0fd513",
        "x-forwarded-port": "443",
        "x-forwarded-proto": "https",
        "host": "postman-echo.com"
      },
      "url": "https://postman-echo.com/get?foo1=bar1&foo2=bar2"
    }
    """

    private static let usersResponse = """
    {
      "data": [
        {
          "id": "usr_8f31",
          "name": "Ada Lovelace",
          "role": "admin",
          "active": true
        },
        {
          "id": "usr_9c2b",
          "name": "Alan Turing",
          "role": "editor",
          "active": true
        }
      ],
      "meta": {
        "total": 42,
        "next_cursor": "cur_29af"
      }
    }
    """
}

enum WireboltTheme {
    static let actionBlue = Color.blue
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
