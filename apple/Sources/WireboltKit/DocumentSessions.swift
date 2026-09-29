import Foundation
import Observation

public struct RunID: Codable, CustomStringConvertible, Hashable, Sendable {
    public let rawValue: UUID

    public init(rawValue: UUID = UUID()) {
        self.rawValue = rawValue
    }

    public var description: String { rawValue.uuidString.lowercased() }
}

public enum DocumentKind: String, Codable, Equatable, Sendable {
    case http
    case webSocket = "web_socket"
}

@MainActor
@Observable
public final class DocumentSession: Identifiable {
    public static let previewByteLimit = ResponseBodyStore.viewportByteCount

    public let id: String
    public let socket = WebSocketDocumentState()
    public let kind: DocumentKind
    public private(set) var collectionID: String?
    public let requestID: String
    public var draft: RequestDraft
    public var note: String {
        get { draft.note }
        set { draft.note = newValue }
    }
    public private(set) var savedDraft: RequestDraft?
    public private(set) var responseHead: ResponseHead?
    public private(set) var preparedRun: PreparedRunSnapshot?
    public private(set) var responseText = ""
    public private(set) var responsePreviewData = Data()
    public private(set) var responseBytes: UInt64 = 0
    public private(set) var responseWasTruncated = false
    public private(set) var completion: RunCompletion?
    public private(set) var failure: RunFailure?
    public private(set) var activeRunID: RunID?
    public private(set) var bodyStore: ResponseBodyStore?
    public private(set) var responseCookies: [CookieSnapshot] = []
    /// When the active run started; drives the in-pane elapsed timer.
    public private(set) var runStartedAt: Date?
    /// True while a run is active and its response head has not arrived yet.
    /// The previous response (if any) stays presented until then.
    public private(set) var isAwaitingResponseHead = false

    @ObservationIgnored private var presentedBodyBytes = 0
    /// Output of the active run staged until its head (or terminal event) replaces the previous response.
    @ObservationIgnored private var pendingBodyStore: ResponseBodyStore?
    @ObservationIgnored private var pendingPreparedRun: PreparedRunSnapshot?
    @ObservationIgnored private var pendingCookies: [CookieSnapshot] = []

    public init(
        id: String = UUID().uuidString.lowercased(),
        kind: DocumentKind = .http,
        collectionID: String? = nil,
        requestID: String? = nil,
        draft: RequestDraft,
        note: String = "",
        savedDraft: RequestDraft? = nil
    ) {
        self.id = id
        self.kind = draft.webSocket || draft.url.hasPrefix("ws://") || draft.url.hasPrefix("wss://") ? .webSocket : kind
        self.collectionID = collectionID
        self.requestID = requestID ?? draft.id
        self.draft = draft.separatingURLQuery
        self.draft.webSocket = self.kind == .webSocket
        if !note.isEmpty { self.draft.note = note }
        self.savedDraft = savedDraft == draft ? self.draft : savedDraft?.separatingURLQuery
    }

    public var title: String { draft.name }
    public var isDirty: Bool { savedDraft != draft }
    public var isRunning: Bool { activeRunID != nil }

    public func relocate(to collectionID: String) { self.collectionID = collectionID }

    public func renameSavedRequest(_ name: String) {
        draft.name = name
        savedDraft?.name = name
    }

    public func beginRun(_ runID: RunID) {
        failure = nil
        pendingBodyStore = try? ResponseBodyStore(runID: runID)
        pendingPreparedRun = nil
        pendingCookies = []
        isAwaitingResponseHead = true
        runStartedAt = Date()
        activeRunID = runID
    }

    /// Replaces the previous response with the active run's staged output.
    private func presentPendingRun() {
        guard isAwaitingResponseHead else { return }
        let store = pendingBodyStore, prepared = pendingPreparedRun, cookies = pendingCookies
        resetResponse()
        bodyStore = store
        preparedRun = prepared
        responseCookies = cookies
        pendingBodyStore = nil
        pendingPreparedRun = nil
        pendingCookies = []
        isAwaitingResponseHead = false
    }

    private func endRun() {
        activeRunID = nil
        runStartedAt = nil
    }

    public func consume(_ event: RunEvent, runID: RunID) async {
        guard activeRunID == runID else { return }
        switch event {
        case .cookies: break
        case let .prepared(snapshot):
            if isAwaitingResponseHead { pendingPreparedRun = snapshot } else { preparedRun = snapshot }
        case let .head(head):
            presentPendingRun()
            responseHead = head
        case let .chunk(data):
            presentPendingRun()
            try? await bodyStore?.append(data)
            responseBytes += UInt64(data.count)
            let remaining = max(Self.previewByteLimit - presentedBodyBytes, 0)
            guard remaining > 0 else {
                responseWasTruncated = true
                return
            }
            let prefix = data.prefix(remaining)
            responsePreviewData.append(prefix)
            responseText.append(String(decoding: prefix, as: UTF8.self))
            presentedBodyBytes += prefix.count
            responseWasTruncated = prefix.count < data.count
        case let .complete(value):
            presentPendingRun()
            try? await bodyStore?.finish()
            completion = value
            responseBytes = value.bytesReceived
            endRun()
        }
    }

    public func finish(runID: RunID, failure: RunFailure?) async {
        guard activeRunID == runID else { return }
        presentPendingRun()
        try? await bodyStore?.finish()
        self.failure = failure
        endRun()
    }

    public func cancel(runID: RunID) {
        guard activeRunID == runID else { return }
        presentPendingRun()
        failure = RunFailure(kind: "cancelled", issues: [])
        endRun()
    }

    public func markSaved(_ draft: RequestDraft) {
        self.draft = draft
        savedDraft = draft
    }

    public func recordSavedDraft(_ draft: RequestDraft) { savedDraft = draft }

    public func resetResponse() {
        responseHead = nil
        preparedRun = nil
        responseText = ""
        responsePreviewData = Data()
        responseBytes = 0
        responseWasTruncated = false
        completion = nil
        failure = nil
        bodyStore = nil
        responseCookies = []
        presentedBodyBytes = 0
    }

    public func copyCompletedResponse(from source: DocumentSession) {
        savedDraft = source.savedDraft
        guard !source.isRunning else { return }
        responseHead = source.responseHead
        preparedRun = source.preparedRun
        responseText = source.responseText
        responsePreviewData = source.responsePreviewData
        responseBytes = source.responseBytes
        responseWasTruncated = source.responseWasTruncated
        completion = source.completion
        failure = source.failure
        bodyStore = source.bodyStore
        responseCookies = source.responseCookies
        presentedBodyBytes = source.presentedBodyBytes
    }

    public func setResponseCookies(_ cookies: [CookieSnapshot]) {
        responseCookies = cookies
    }

    /// Cookies can arrive (for example across redirects) before the head that presents the run.
    public func appendResponseCookies(_ cookies: [CookieSnapshot]) {
        if isAwaitingResponseHead { pendingCookies += cookies } else { responseCookies += cookies }
    }

    public func restore(_ entry: RunHistoryEntry, viewport: Data) {
        preparedRun = entry.prepared
        responseHead = entry.responseHead
        completion = entry.completion
        responseText = String(decoding: viewport, as: UTF8.self)
        responsePreviewData = viewport
        responseBytes = entry.completion?.bytesReceived ?? UInt64(viewport.count)
        responseWasTruncated = responseBytes > UInt64(viewport.count)
        pendingBodyStore = nil
        pendingPreparedRun = nil
        pendingCookies = []
        isAwaitingResponseHead = false
        endRun()
        failure = entry.failure
        bodyStore = try? ResponseBodyStore(existingURL: URL(fileURLWithPath: entry.bodyPath))
    }
}

public struct EditorGroup: Identifiable, Codable, Equatable, Sendable {
    public let id: String
    public var tabIDs: [String]
    public var selectedTabID: String?
    public var backwardTabIDs: [String]
    public var forwardTabIDs: [String]

    public init(
        id: String = UUID().uuidString.lowercased(),
        tabIDs: [String] = [],
        selectedTabID: String? = nil,
        backwardTabIDs: [String] = [],
        forwardTabIDs: [String] = []
    ) {
        self.id = id
        self.tabIDs = tabIDs
        self.selectedTabID = selectedTabID
        self.backwardTabIDs = backwardTabIDs
        self.forwardTabIDs = forwardTabIDs
    }
}

/// The restorable shape of the editor: saved requests open per group and the selection.
public struct SessionLayout: Codable, Equatable, Sendable {
    public struct Tab: Codable, Equatable, Sendable {
        public let collectionID: String
        public let requestID: String

        public init(collectionID: String, requestID: String) {
            self.collectionID = collectionID
            self.requestID = requestID
        }
    }

    public struct Group: Codable, Equatable, Sendable {
        public let tabs: [Tab]
        public let selectedIndex: Int?

        public init(tabs: [Tab], selectedIndex: Int?) {
            self.tabs = tabs
            self.selectedIndex = selectedIndex
        }
    }

    public let groups: [Group]
    public let activeGroupIndex: Int

    public init(groups: [Group], activeGroupIndex: Int = 0) {
        self.groups = groups
        self.activeGroupIndex = activeGroupIndex
    }

    public var isEmpty: Bool { groups.isEmpty }
}

/// Remembers the editor layout per workspace folder so relaunch reopens the same tabs.
public struct SessionLayoutStore {
    public static let defaultsKey = "workspace.sessionLayouts"
    private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    public func layout(forWorkspace path: String) -> SessionLayout? {
        guard let data = (defaults.dictionary(forKey: Self.defaultsKey) as? [String: Data])?[path] else { return nil }
        return try? JSONDecoder().decode(SessionLayout.self, from: data)
    }

    /// Entries for workspace folders that no longer exist are dropped on each save.
    public func save(_ layout: SessionLayout, forWorkspace path: String) {
        var stored = (defaults.dictionary(forKey: Self.defaultsKey) as? [String: Data] ?? [:])
            .filter { FileManager.default.fileExists(atPath: $0.key) }
        stored[path] = try? JSONEncoder().encode(layout)
        defaults.set(stored, forKey: Self.defaultsKey)
    }
}

public enum TabCloseScope: Equatable, Sendable {
    case one(String)
    case others(String)
    case rightOf(String)
    case all
}

@MainActor
@Observable
public final class DocumentSessionStore {
    public private(set) var sessions: [String: DocumentSession]
    public private(set) var groups: [EditorGroup] {
        didSet { if groupCount != groups.count { groupCount = groups.count } }
    }
    /// Changes only when a split opens or closes, so layout that depends on the
    /// number of editor groups does not re-render on every tab selection.
    public private(set) var groupCount = 1
    public var activeGroupID: String

    public init() {
        let group = EditorGroup()
        sessions = [:]
        groups = [group]
        activeGroupID = group.id
    }

    /// Saved-request tabs per editor group. Unsaved temporary tabs are not restorable.
    public var layout: SessionLayout {
        var restorable: [(id: String, group: SessionLayout.Group)] = []
        for group in groups {
            let tabs = group.tabIDs.compactMap { id -> (String, SessionLayout.Tab)? in
                guard let session = sessions[id], let collectionID = session.collectionID else { return nil }
                return (id, SessionLayout.Tab(collectionID: collectionID, requestID: session.requestID))
            }
            guard !tabs.isEmpty else { continue }
            restorable.append((group.id, SessionLayout.Group(
                tabs: tabs.map(\.1),
                selectedIndex: tabs.firstIndex { $0.0 == group.selectedTabID }
            )))
        }
        return SessionLayout(
            groups: restorable.map(\.group),
            activeGroupIndex: restorable.firstIndex { $0.id == activeGroupID } ?? 0
        )
    }

    /// Reopens a persisted layout into an empty store. Tabs whose request no longer
    /// exists are skipped; groups left without tabs are dropped.
    @discardableResult
    public func restore(
        _ layout: SessionLayout,
        resolve: (SessionLayout.Tab) -> RequestLocation?
    ) -> [DocumentSession] {
        guard sessions.isEmpty else { return [] }
        var restoredGroups: [EditorGroup] = []
        var restored: [DocumentSession] = []
        var activeID: String?
        for (groupIndex, saved) in layout.groups.enumerated() {
            var group = EditorGroup()
            var selectedID: String?
            for (tabIndex, tab) in saved.tabs.enumerated() {
                guard let location = resolve(tab) else { continue }
                let session = DocumentSession(
                    collectionID: location.collectionID,
                    requestID: location.request.id,
                    draft: location.request,
                    savedDraft: location.request
                )
                sessions[session.id] = session
                group.tabIDs.append(session.id)
                restored.append(session)
                if tabIndex == saved.selectedIndex { selectedID = session.id }
            }
            guard !group.tabIDs.isEmpty else { continue }
            group.selectedTabID = selectedID ?? group.tabIDs.last
            restoredGroups.append(group)
            if groupIndex == layout.activeGroupIndex { activeID = group.id }
        }
        guard let first = restoredGroups.first else { return [] }
        groups = restoredGroups
        activeGroupID = activeID ?? first.id
        return restored
    }

    public var activeGroup: EditorGroup? {
        groups.first(where: { $0.id == activeGroupID })
    }

    public var activeSession: DocumentSession? {
        guard let selected = activeGroup?.selectedTabID else { return nil }
        return sessions[selected]
    }

    public func session(id: String) -> DocumentSession? {
        sessions[id]
    }

    public func reopen(_ session: DocumentSession) {
        sessions[session.id] = session
        mutateGroup(id: activeGroupID) { group in
            if !group.tabIDs.contains(session.id) { group.tabIDs.append(session.id) }
            select(session.id, in: &group)
        }
    }

    @discardableResult
    public func open(
        draft: RequestDraft,
        collectionID: String? = nil,
        kind: DocumentKind = .http,
        in groupID: String? = nil,
        forceNewSession: Bool = false,
        isSaved: Bool = true
    ) -> DocumentSession {
        let targetGroupID = groupID ?? activeGroupID
        let kind: DocumentKind = draft.webSocket || draft.url.hasPrefix("ws://") || draft.url.hasPrefix("wss://") ? .webSocket : kind
        if !forceNewSession,
           let existing = sessions.values.first(where: {
               $0.requestID == draft.id && $0.collectionID == collectionID && $0.kind == kind
                   && groups.first(where: { $0.id == targetGroupID })?.tabIDs.contains($0.id) == true
           })
        {
            select(tabID: existing.id, in: targetGroupID)
            return existing
        }

        let session = DocumentSession(
            kind: kind,
            collectionID: collectionID,
            requestID: draft.id,
            draft: draft,
            savedDraft: isSaved ? draft : nil
        )
        sessions[session.id] = session
        mutateGroup(id: targetGroupID) { group in
            group.tabIDs.append(session.id)
            select(session.id, in: &group)
        }
        activeGroupID = targetGroupID
        return session
    }

    @discardableResult
    public func openTemporary(
        kind: DocumentKind = .http,
        collectionID: String? = nil
    ) -> DocumentSession {
        let method: HTTPMethod = .get
        let title = kind == .http ? "Untitled Request" : "Untitled WebSocket Request"
        return open(
            draft: RequestDraft(
                id: UUID().uuidString.lowercased(),
                name: title,
                method: method
            ),
            collectionID: collectionID,
            kind: kind,
            forceNewSession: true,
            isSaved: false
        )
    }

    public func select(tabID: String, in groupID: String? = nil) {
        let targetGroupID = groupID ?? activeGroupID
        guard sessions[tabID] != nil,
              groups.first(where: { $0.id == targetGroupID })?.tabIDs.contains(tabID) == true else { return }
        mutateGroup(id: targetGroupID) { group in
            guard group.tabIDs.contains(tabID) else { return }
            select(tabID, in: &group)
        }
        activeGroupID = targetGroupID
    }

    public func goBack() {
        mutateGroup(id: activeGroupID) { group in
            guard let destination = group.backwardTabIDs.popLast() else { return }
            if let selected = group.selectedTabID { group.forwardTabIDs.append(selected) }
            group.selectedTabID = destination
        }
    }

    public func goForward() {
        mutateGroup(id: activeGroupID) { group in
            guard let destination = group.forwardTabIDs.popLast() else { return }
            if let selected = group.selectedTabID { group.backwardTabIDs.append(selected) }
            group.selectedTabID = destination
        }
    }

    @discardableResult
    public func split(tabID: String) -> String? {
        guard let source = sessions[tabID] else { return nil }
        let group = EditorGroup()
        groups.append(group)
        activeGroupID = group.id
        let copy = open(
            draft: source.draft,
            collectionID: source.collectionID,
            kind: source.kind,
            in: group.id,
            forceNewSession: true
        )
        copy.note = source.note
        copy.copyCompletedResponse(from: source)
        return group.id
    }

    @discardableResult
    public func close(
        _ scope: TabCloseScope,
        in groupID: String? = nil,
        allowDirty: Bool = false
    ) -> [DocumentSession] {
        let targetGroupID = groupID ?? activeGroupID
        guard let group = groups.first(where: { $0.id == targetGroupID }) else { return [] }
        let targetIDs: [String] = switch scope {
        case let .one(id): [id]
        case let .others(id): group.tabIDs.filter { $0 != id }
        case let .rightOf(id):
            if let index = group.tabIDs.firstIndex(of: id) {
                Array(group.tabIDs.dropFirst(index + 1))
            } else {
                []
            }
        case .all: group.tabIDs
        }
        let blocked = targetIDs.compactMap { sessions[$0] }.filter(\.isDirty)
        guard allowDirty || blocked.isEmpty else { return blocked }

        mutateGroup(id: targetGroupID) { mutable in
            let priorSelection = mutable.selectedTabID
            mutable.tabIDs.removeAll { targetIDs.contains($0) }
            mutable.backwardTabIDs.removeAll { targetIDs.contains($0) }
            mutable.forwardTabIDs.removeAll { targetIDs.contains($0) }
            if let priorSelection, targetIDs.contains(priorSelection) {
                mutable.selectedTabID = mutable.tabIDs.last
            }
        }
        removeUnreferencedSessions(targetIDs)
        removeEmptySecondaryGroups()
        return []
    }

    public func removeAll() {
        for session in sessions.values { session.socket.disconnect() }
        sessions.removeAll(keepingCapacity: true)
        let group = EditorGroup()
        groups = [group]
        activeGroupID = group.id
    }

    private func select(_ tabID: String, in group: inout EditorGroup) {
        guard group.selectedTabID != tabID else { return }
        if let current = group.selectedTabID { group.backwardTabIDs.append(current) }
        group.selectedTabID = tabID
        group.forwardTabIDs.removeAll(keepingCapacity: true)
    }

    private func mutateGroup(id: String, _ mutation: (inout EditorGroup) -> Void) {
        guard let index = groups.firstIndex(where: { $0.id == id }) else { return }
        mutation(&groups[index])
    }

    private func removeUnreferencedSessions(_ candidateIDs: [String]) {
        let referenced = Set(groups.flatMap(\.tabIDs))
        for id in candidateIDs where !referenced.contains(id) {
            sessions[id]?.socket.disconnect()
            sessions[id] = nil
        }
    }

    private func removeEmptySecondaryGroups() {
        groups.removeAll { $0.id != activeGroupID && $0.tabIDs.isEmpty }
        if groups.first(where: { $0.id == activeGroupID })?.tabIDs.isEmpty == true,
           groups.count > 1
        {
            groups.removeAll { $0.id == activeGroupID }
            activeGroupID = groups[0].id
        }
    }
}
