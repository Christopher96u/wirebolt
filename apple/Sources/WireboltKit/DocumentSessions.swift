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
    private nonisolated static let publicationDelayMS = Int64(sessionPublicationDelayMs())
    public let id: String
    public let socket = WebSocketDocumentState()
    public let kind: DocumentKind
    public private(set) var collectionID: String?
    public let requestID: String
    // The native editor keeps its value projection so a keystroke does not cross FFI.
    // It is synchronized with Rust at command boundaries; equality also drives Observation.
    public var draft: RequestDraft {
        didSet { if !isTouched, draft != savedDraft { isTouched = true } }
    }
    public private(set) var isTouched = false
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
    public private(set) var runStartedAt: Date?
    public private(set) var isAwaitingResponseHead = false
    @ObservationIgnored let rust: AppDocumentSession
    @ObservationIgnored private var progressTask: Task<Void, Never>?
    @ObservationIgnored private var ingress: [Data] = []
    @ObservationIgnored private var ingressBytes = 0

    public init(
        id: String = UUID().uuidString.lowercased(), kind: DocumentKind = .http,
        collectionID: String? = nil, requestID: String? = nil, draft: RequestDraft,
        note: String = "", savedDraft: RequestDraft? = nil
    ) {
        let rust = AppDocumentSession(
            id: id, kind: kind.rust, collectionId: collectionID,
            requestId: requestID, draft: draft.domain, savedDraft: savedDraft?.domain, note: note)
        let document = rust.document()
        self.rust = rust
        self.id = document.id
        self.kind = DocumentKind(document.kind)
        self.collectionID = document.collectionId
        self.requestID = document.requestId
        self.draft = RequestDraft(domain: document.draft)
        self.savedDraft = document.savedDraft.map(RequestDraft.init(domain:))
    }
    public var title: String { draft.name }
    public var isDirty: Bool { savedDraft != draft }
    public var isRunning: Bool { activeRunID != nil }
    var tabProjection: SessionTabDocument {
        SessionTabDocument(
            id: id, kind: kind.rust,
            collectionId: collectionID, requestId: requestID, dirty: isDirty, touched: isTouched)
    }
    func synchronizeDraft() { rust.edit(draft: draft.domain) }
    public func relocate(to collectionID: String) {
        rust.relocate(collectionId: collectionID)
        self.collectionID = collectionID
    }
    public func renameSavedRequest(_ name: String) {
        synchronizeDraft()
        rust.rename(name: name)
        draft.name = name
        savedDraft?.name = name
    }
    public func beginRun(_ runID: RunID) {
        synchronizeDraft()
        progressTask?.cancel()
        progressTask = nil
        ingress = []
        ingressBytes = 0
        isTouched = true
        rust.begin(runId: runID.description, directory: FileManager.default.temporaryDirectory.path)
        mirror()
    }
    public func consume(_ event: RunEvent, runID: RunID) async {
        guard activeRunID == runID else { return }
        switch event {
        case .cookies: return
        case .chunk(let data):
            ingress.append(data)
            ingressBytes += data.count
            if ingressBytes >= ResponseBodyStore.searchChunkByteCount || ingress.count >= 64 { await flush(runID) }
            guard activeRunID == runID else { return }
            if progressTask == nil {
                progressTask = Task { [weak self] in
                    do { try await Task.sleep(for: .milliseconds(Self.publicationDelayMS)) } catch { return }
                    guard let self, self.activeRunID == runID else { return }
                    await self.flush(runID)
                    guard !Task.isCancelled, self.activeRunID == runID else { return }
                    self.rust.publish(runId: runID.description)
                    self.mirror()
                    self.progressTask = nil
                }
            }
            return
        case .prepared(let snapshot): rust.consume(runId: runID.description, event: .prepared(snapshot: snapshot.sessionRecord))
        case .head(let head): rust.consume(runId: runID.description, event: .head(head: head.sessionRecord))
        case .complete(let value):
            await flush(runID)
            progressTask?.cancel()
            progressTask = nil
            let rust = rust
            await Task.detached(priority: .userInitiated) {
                rust.consume(runId: runID.description, event: .complete(completion: value.sessionRecord))
            }.value
        }
        guard activeRunID == runID else { return }
        mirror()
    }
    private func flush(_ runID: RunID) async {
        guard !ingress.isEmpty else { return }
        let chunks = ingress
        let rust = rust
        ingress.removeAll(keepingCapacity: true)
        ingressBytes = 0
        let accepted = await Task.detached(priority: .userInitiated) { rust.appendBatch(runId: runID.description, chunks: chunks) }.value
        if !accepted, activeRunID == runID { mirror() }
    }
    public func finish(runID: RunID, failure: RunFailure?) async {
        guard activeRunID == runID else { return }
        await flush(runID)
        progressTask?.cancel()
        progressTask = nil
        let rust = rust
        await Task.detached(priority: .userInitiated) { rust.finish(runId: runID.description, failure: failure?.sessionRecord) }.value
        guard activeRunID == runID else { return }
        mirror()
    }
    public func cancel(runID: RunID) {
        guard activeRunID == runID else { return }
        if !ingress.isEmpty {
            _ = rust.appendBatch(runId: runID.description, chunks: ingress)
            ingress.removeAll(keepingCapacity: true)
            ingressBytes = 0
        }
        progressTask?.cancel()
        progressTask = nil
        rust.cancel(runId: runID.description)
        mirror()
    }
    public func markSaved(_ draft: RequestDraft) {
        rust.save(draft: draft.domain, replace: true)
        self.draft = draft
        savedDraft = draft
    }
    public func recordSavedDraft(_ draft: RequestDraft) {
        synchronizeDraft()
        rust.save(draft: draft.domain, replace: false)
        savedDraft = draft
    }
    public func resetResponse() {
        rust.reset()
        mirror()
    }
    public func copyCompletedResponse(from source: DocumentSession) {
        rust.copyCompleted(source: source.rust)
        savedDraft = source.savedDraft
        mirror()
        if !source.isRunning { bodyStore = source.bodyStore }
    }
    public func setResponseCookies(_ cookies: [CookieSnapshot]) {
        rust.cookies(cookies: cookies.map(\.sessionRecord), append: false)
        mirror()
    }
    public func appendResponseCookies(_ cookies: [CookieSnapshot]) {
        rust.cookies(cookies: cookies.map(\.sessionRecord), append: true)
        mirror()
    }
    public func restore(_ entry: RunHistoryEntry, viewport: Data) {
        let bytes = entry.completion?.bytesReceived ?? UInt64(viewport.count)
        rust.restore(
            snapshot: SessionRunSnapshot(
                head: entry.responseHead?.sessionRecord, prepared: entry.prepared.sessionRecord,
                text: String(decoding: viewport, as: UTF8.self), data: viewport, bytes: bytes, truncated: bytes > UInt64(viewport.count),
                completion: entry.completion?.sessionRecord, failure: entry.failure?.sessionRecord, activeRunId: nil,
                startedAt: nil, awaitingHead: false, cookies: [], bodyPath: entry.bodyPath), path: entry.bodyPath)
        mirror()
    }
    func mirror() {
        let state = rust.snapshot()
        if !isTouched, rust.wasTouched() { isTouched = true }
        assign(\.responseHead, state.head.map(ResponseHead.init))
        assign(\.preparedRun, state.prepared.map(PreparedRunSnapshot.init))
        assign(\.responseText, state.text)
        assign(\.responsePreviewData, state.data)
        assign(\.responseBytes, state.bytes)
        assign(\.responseWasTruncated, state.truncated)
        assign(\.completion, state.completion.map(RunCompletion.init))
        assign(\.failure, state.failure.map(RunFailure.init))
        assign(\.activeRunID, state.activeRunId.flatMap(UUID.init(uuidString:)).map { RunID(rawValue: $0) })
        assign(\.runStartedAt, state.startedAt.map { Date(timeIntervalSinceReferenceDate: $0) })
        assign(\.isAwaitingResponseHead, state.awaitingHead)
        assign(\.responseCookies, state.cookies.map(CookieSnapshot.init))
        if bodyStore?.url.path != state.bodyPath { bodyStore = rust.body().map { ResponseBodyStore(rust: $0) } }
    }
    private func assign<Value: Equatable>(_ key: ReferenceWritableKeyPath<DocumentSession, Value>, _ value: Value) {
        if self[keyPath: key] != value { self[keyPath: key] = value }
    }
}

extension DocumentKind {
    var rust: SessionKind { self == .http ? .http : .webSocket }
    init(_ record: SessionKind) { self = record == .http ? .http : .webSocket }
}
extension ResponseHead {
    var sessionRecord: SessionHead {
        SessionHead(
            status: status, version: version, headers: headers.map { ResponseHeaderValue(name: $0.name, value: $0.value) },
            timeToHeadersNs: timeToHeadersNS)
    }
    init(_ record: SessionHead) {
        self.init(
            status: record.status, version: record.version, headers: record.headers.map { ResponseHeader(name: $0.name, value: $0.value) },
            timeToHeadersNS: record.timeToHeadersNs)
    }
}
extension RunCompletion {
    var sessionRecord: SessionCompletion { SessionCompletion(bytesReceived: bytesReceived, totalTimeNs: totalTimeNS) }
    init(_ record: SessionCompletion) { self.init(bytesReceived: record.bytesReceived, totalTimeNS: record.totalTimeNs) }
}
extension RunFailure {
    var sessionRecord: SessionFailure {
        SessionFailure(kind: kind, issues: issues.map { SessionIssue(path: $0.path, kind: $0.kind, reference: $0.reference) })
    }
    init(_ record: SessionFailure) {
        self.init(kind: record.kind, issues: record.issues.map { RequestIssue(path: $0.path, kind: $0.kind, reference: $0.reference) })
    }
}
extension PreparedRunSnapshot {
    var sessionRecord: SessionPrepared {
        SessionPrepared(
            proxy: proxy.map { SessionProxy(configuration: $0.configuration.domain, source: $0.source) },
            method: method, url: url, headers: headers.map { SessionPreparedHeader(name: $0.name, value: $0.value, redacted: $0.redacted) },
            body: SessionPreparedBody(
                byteCount: body.byteCount, contentType: body.contentType, textPreview: body.textPreview, redacted: body.redacted),
            transport: transport.domain)
    }
    init(_ record: SessionPrepared) {
        self.init(
            proxy: record.proxy.map {
                EffectiveProxy(
                    configuration: ProxyDocument(domain: $0.configuration),
                    source: $0.source)
            },
            method: record.method, url: record.url,
            headers: record.headers.map { PreparedHeaderSnapshot(name: $0.name, value: $0.value, redacted: $0.redacted) },
            body: PreparedBodySnapshot(
                byteCount: record.body.byteCount, contentType: record.body.contentType, textPreview: record.body.textPreview,
                redacted: record.body.redacted), transport: TransportSettings(domain: record.transport))
    }
}

public struct EditorGroup: Identifiable, Codable, Equatable, Sendable {
    public let id: String
    public var tabIDs: [String]
    public var selectedTabID: String?
    public var backwardTabIDs: [String]
    public var forwardTabIDs: [String]
    /// The tab a single click in the sidebar reuses (shown in italics) until it is kept open.
    public var previewTabID: String?

    public init(
        id: String = UUID().uuidString.lowercased(),
        tabIDs: [String] = [],
        selectedTabID: String? = nil,
        backwardTabIDs: [String] = [],
        forwardTabIDs: [String] = [],
        previewTabID: String? = nil
    ) {
        self.id = id
        self.tabIDs = tabIDs
        self.selectedTabID = selectedTabID
        self.backwardTabIDs = backwardTabIDs
        self.forwardTabIDs = forwardTabIDs
        self.previewTabID = previewTabID
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
        /// Index of the preview tab, if the group has one. Absent in older layouts.
        public let previewIndex: Int?

        public init(tabs: [Tab], selectedIndex: Int?, previewIndex: Int? = nil) {
            self.tabs = tabs
            self.selectedIndex = selectedIndex
            self.previewIndex = previewIndex
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
        return sessionDecodeLayout(data: data).map(SessionLayout.init)
    }

    /// Entries for workspace folders that no longer exist are dropped on each save.
    public func save(_ layout: SessionLayout, forWorkspace path: String) {
        let prior = defaults.dictionary(forKey: Self.defaultsKey) as? [String: Data] ?? [:]
        let retained = Set(sessionExistingWorkspacePaths(paths: Array(prior.keys)))
        var stored = prior.filter { retained.contains($0.key) }
        stored[path] = sessionEncodeLayout(layout: layout.sessionRecord)
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
    public private(set) var sessions: [String: DocumentSession] {
        didSet { refreshActiveRequest() }
    }
    public private(set) var groups: [EditorGroup] {
        didSet {
            if groupCount != groups.count { groupCount = groups.count }
            refreshActiveRequest()
        }
    }
    /// Changes only when a split opens or closes, so layout that depends on the
    /// number of editor groups does not re-render on every tab selection.
    public private(set) var groupCount = 1
    public var activeGroupID: String {
        didSet {
            rust.activateGroup(id: activeGroupID)
            refreshActiveRequest()
        }
    }
    @ObservationIgnored private let rust = AppSessionTabs()

    /// The saved request of the active document.
    private typealias RequestKey = RuntimeRequestKey

    /// One observable flag per saved request that has been asked about, so each sidebar row
    /// depends only on its own flag: a selection change updates two rows, not every row.
    @Observable
    final class ActiveRequestFlag {
        var isActive: Bool
        init(isActive: Bool) { self.isActive = isActive }
    }

    @ObservationIgnored private var activeRequestKey: RequestKey?
    @ObservationIgnored private var activeRequestFlags: [RequestKey: ActiveRequestFlag] = [:]

    /// Whether the saved request is the active document's; observes only that request.
    public func isActiveRequest(collectionID: String, requestID: String) -> Bool {
        let key = RequestKey(collectionId: collectionID, requestId: requestID)
        let flag =
            activeRequestFlags[key]
            ?? {
                let flag = ActiveRequestFlag(isActive: key == activeRequestKey)
                activeRequestFlags[key] = flag
                return flag
            }()
        return flag.isActive
    }

    /// Brings the flags up to date with the active document. Runs after every change to
    /// the groups, the active group or the open documents; call it after moving a
    /// document's request to another collection.
    public func refreshActiveRequest() {
        let session = activeSession
        let key = sessionActiveRequestKey(collectionId: session?.collectionID, requestId: session?.requestID)
        guard key != activeRequestKey else { return }
        if let previous = activeRequestKey { activeRequestFlags[previous]?.isActive = false }
        activeRequestKey = key
        if let key { activeRequestFlags[key]?.isActive = true }
    }

    public init() {
        let snapshot = rust.snapshot()
        sessions = [:]
        groups = snapshot.groups.map(EditorGroup.init)
        activeGroupID = snapshot.activeGroupId

    }

    /// Saved-request tabs per editor group. Unsaved temporary tabs are not restorable.
    public var layout: SessionLayout {
        synchronizeTabs()
        return SessionLayout(rust.layout())
    }

    /// Reopens a persisted layout into an empty store. Tabs whose request no longer
    /// exists are skipped; groups left without tabs are dropped.
    @discardableResult
    public func restore(
        _ layout: SessionLayout,
        resolve: (SessionLayout.Tab) -> RequestLocation?
    ) -> [DocumentSession] {
        guard sessions.isEmpty else { return [] }
        let result = rust.restore(layout: layout.sessionRecord, resolved: layout.groups.flatMap { $0.tabs }.map { resolve($0)?.domain })
        let restored = result.documents.map { document in
            DocumentSession(
                id: document.id, kind: DocumentKind(document.kind), collectionID: document.collectionId,
                requestID: document.requestId, draft: RequestDraft(domain: document.draft),
                savedDraft: document.savedDraft.map(RequestDraft.init(domain:)))
        }
        sessions = Dictionary(uniqueKeysWithValues: restored.map { ($0.id, $0) })
        mirrorTabs(result.snapshot)
        return restored
    }

    /// Applies the Rust-selected layout or first-document fallback in one bridge call.
    func restoreWorkspace(_ application: AppWorkspaceOperations, layout: SessionLayout?) -> [DocumentSession] {
        synchronizeTabs()
        let restored = application.restoreTabs(tabs: rust, layout: layout?.sessionRecord)
        let projected = restored.result.documents.map { document in
            DocumentSession(
                id: document.id, kind: DocumentKind(document.kind), collectionID: document.collectionId,
                requestID: document.requestId, draft: RequestDraft(domain: document.draft),
                savedDraft: document.savedDraft.map(RequestDraft.init(domain:)))
        }
        for session in projected { sessions[session.id] = session }
        mirrorTabs(restored.result.snapshot)
        return restored.restoreHistory ? projected : []
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
        rust.reopen(document: session.tabProjection)
        mirrorTabs()
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
        synchronizeTabs()
        let candidate = DocumentSession(
            kind: kind, collectionID: collectionID, requestID: draft.id,
            draft: draft, savedDraft: isSaved ? draft : nil)
        let id = rust.open(document: candidate.tabProjection, group: groupID, force: forceNewSession, preview: false)
        if id == candidate.id { sessions[id] = candidate }
        mirrorTabs()
        return sessions[id]!
    }

    /// Opens a saved request in the group's preview tab: an untouched preview tab is
    /// replaced in place, otherwise a new preview tab is added. A request that is already
    /// open in the group is selected and keeps its pinned or preview state.
    @discardableResult
    public func openPreview(
        draft: RequestDraft,
        collectionID: String?,
        in groupID: String? = nil
    ) -> DocumentSession {
        synchronizeTabs()
        let candidate = DocumentSession(collectionID: collectionID, requestID: draft.id, draft: draft, savedDraft: draft)
        let id = rust.open(document: candidate.tabProjection, group: groupID, force: false, preview: true)
        if id == candidate.id { sessions[id] = candidate }
        mirrorTabs()
        return sessions[id]!
    }

    /// True while the tab is its group's preview tab and has not been edited or sent.
    public func isPreview(tabID: String) -> Bool {
        guard let session = sessions[tabID], !session.isTouched else { return false }
        return groups.contains { $0.previewTabID == tabID }
    }

    /// Keeps a preview tab open, so the next single click opens another tab.
    public func pin(tabID: String) {
        rust.pin(id: tabID)
        mirrorTabs()
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
        rust.select(id: tabID, group: groupID)
        mirrorTabs()
    }

    public func goBack() {
        rust.navigate(forward: false)
        mirrorTabs()
    }

    public func goForward() {
        rust.navigate(forward: true)
        mirrorTabs()
    }

    @discardableResult
    public func split(tabID: String) -> String? {
        guard let source = sessions[tabID] else { return nil }
        let id = rust.split()
        mirrorTabs()
        let copy = open(draft: source.draft, collectionID: source.collectionID, kind: source.kind, in: id, forceNewSession: true)
        copy.copyCompletedResponse(from: source)
        return id
    }

    @discardableResult
    public func close(
        _ scope: TabCloseScope,
        in groupID: String? = nil,
        allowDirty: Bool = false
    ) -> [DocumentSession] {
        synchronizeTabs()
        let blocked = rust.close(scope: scope.sessionRecord, group: groupID, allowDirty: allowDirty)
        guard blocked.isEmpty else { return blocked.compactMap { sessions[$0] } }
        mirrorTabs()
        return []
    }

    func closeWorkspace(_ scope: TabCloseScope, in groupID: String?, allowDirty: Bool) -> (
        blocked: [DocumentSession], closed: [DocumentSession]
    ) {
        let documents = sessions.values.map { session in
            session.synchronizeDraft()
            return session.rust.document()
        }
        let result = rust.closeWorkspace(documents: documents, scope: scope.sessionRecord, group: groupID, allowDirty: allowDirty)
        let closed = result.removed.compactMap { effect -> DocumentSession? in
            guard let session = sessions[effect.id] else { return nil }
            if let saved = effect.saved { session.markSaved(RequestDraft(domain: saved)) }
            return session
        }
        let blocked = result.blocked.compactMap { sessions[$0] }
        mirrorTabs(result.snapshot)
        return (blocked, closed)
    }

    public func removeAll() {
        for session in sessions.values { session.socket.disconnect() }
        rust.removeAll()
        sessions.removeAll(keepingCapacity: true)
        mirrorTabs()
    }

    private func synchronizeTabs() { rust.synchronize(documents: sessions.values.map(\.tabProjection)) }
    private func mirrorTabs(_ snapshot: SessionTabSnapshot? = nil) {
        let snapshot = snapshot ?? rust.snapshot()
        let retained = Set(snapshot.sessionIds)
        for id in sessions.keys where !retained.contains(id) {
            sessions[id]?.socket.disconnect()
            sessions[id] = nil
        }
        let next = snapshot.groups.map(EditorGroup.init)
        if groups != next { groups = next }
        if activeGroupID != snapshot.activeGroupId { activeGroupID = snapshot.activeGroupId }
    }
}

extension EditorGroup {
    init(_ record: SessionEditorGroup) {
        self.init(
            id: record.id, tabIDs: record.tabIds, selectedTabID: record.selectedTabId,
            backwardTabIDs: record.backwardTabIds, forwardTabIDs: record.forwardTabIds, previewTabID: record.previewTabId)
    }
}
extension SessionLayout {
    var sessionRecord: AppSessionLayout {
        AppSessionLayout(
            groups: groups.map {
                AppSessionLayoutGroup(
                    tabs: $0.tabs.map {
                        SessionLayoutTab(collectionId: $0.collectionID, requestId: $0.requestID)
                    }, selectedIndex: $0.selectedIndex.map(Int64.init), previewIndex: $0.previewIndex.map(Int64.init))
            }, activeGroupIndex: Int64(activeGroupIndex))
    }
    init(_ record: AppSessionLayout) {
        self.init(
            groups: record.groups.map {
                Group(
                    tabs: $0.tabs.map { Tab(collectionID: $0.collectionId, requestID: $0.requestId) },
                    selectedIndex: $0.selectedIndex.map(Int.init), previewIndex: $0.previewIndex.map(Int.init))
            }, activeGroupIndex: Int(record.activeGroupIndex))
    }
}
extension TabCloseScope {
    var sessionRecord: SessionCloseScope {
        switch self {
        case .one(let id): .one(id: id)
        case .others(let id): .others(id: id)
        case .rightOf(let id): .rightOf(id: id)
        case .all: .all
        }
    }
}

extension EditorGroup {
    var workspaceRecord: SessionEditorGroup {
        SessionEditorGroup(
            id: id, tabIds: tabIDs, selectedTabId: selectedTabID,
            backwardTabIds: backwardTabIDs, forwardTabIds: forwardTabIDs, previewTabId: previewTabID)
    }
}

extension TabCloseScope {
    init(_ record: SessionCloseScope) {
        switch record {
        case .one(let id): self = .one(id)
        case .others(let id): self = .others(id)
        case .rightOf(let id): self = .rightOf(id)
        case .all: self = .all
        }
    }
}
