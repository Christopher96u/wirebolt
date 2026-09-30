import Foundation
import Observation

public protocol RequestRunner: Sendable {
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error>
    func cancel(runID: RunID)
    func proxySettingsChanged()
    func resolveValues(_ values: [ValueSource], variables: [String: ValueSource]) async throws -> [String]
}

public extension RequestRunner {
    func proxySettingsChanged() {}
    func resolveValues(_ values: [ValueSource], variables: [String: ValueSource]) async throws -> [String] {
        try values.map { value in
            guard case let .literal(text) = value, !text.contains("{{") else {
                throw RunFailure(kind: "invalid_request", issues: [RequestIssue(path: "values", kind: "unresolved_value", reference: nil)])
            }
            return text
        }
    }
}

public protocol WorkspacePersistence: Sendable {
    func load() async throws -> WorkspaceDraft
    func save(request: RequestDraft, in collectionID: String) async throws
    func save(environment: EnvironmentDraft) async throws
    func saveSecret(name: String, value: String) async throws
    func readSecret(name: String) async throws -> String?
    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta
    func previewImport(format: ImportFormat, source: String) async throws -> ImportPreview
    func commitImport(format: ImportFormat, source: String) async throws -> WorkspaceDelta
    func commitImportFile(format: ImportFormat, source: String, name: String) async throws -> WorkspaceDelta
    func exportCollection(id: String) async throws -> String
    func exportWorkspace() async throws -> String
    func exportRequest(collectionID: String, id: String) async throws -> String
    /// The workspace folder, used to key local runtime storage such as cookies.
    var location: URL? { get }
}

public extension WorkspacePersistence {
    var location: URL? { nil }
    func readSecret(name _: String) async throws -> String? { nil }
    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta {
        switch command {
        case let .saveRequest(collectionID, location):
            try await save(request: location.request, in: collectionID)
            return WorkspaceDelta(version: 0, kind: .request, affectedIDs: [location.id])
        case let .saveEnvironment(environment):
            try await save(environment: environment)
            return WorkspaceDelta(version: 0, kind: .environment, affectedIDs: [environment.id])
        default:
            throw WorkspaceMutationError.unsupported
        }
    }

    func previewImport(format _: ImportFormat, source _: String) async throws -> ImportPreview {
        throw WorkspaceMutationError.unsupported
    }

    func commitImport(format _: ImportFormat, source _: String) async throws -> WorkspaceDelta {
        throw WorkspaceMutationError.unsupported
    }

    func commitImportFile(format: ImportFormat, source: String, name: String) async throws -> WorkspaceDelta {
        try await commitImport(format: format, source: source)
    }

    func exportCollection(id _: String) async throws -> String { throw WorkspaceMutationError.unsupported }
    func exportWorkspace() async throws -> String { throw WorkspaceMutationError.unsupported }
    func exportRequest(collectionID _: String, id _: String) async throws -> String {
        throw WorkspaceMutationError.unsupported
    }
}

public enum WorkspaceMutationError: Error, Equatable, Sendable {
    case unsupported
}

public protocol GitCollaboration: Sendable {
    func status() async throws -> GitStatusSnapshot
    func pull() async throws -> GitOperationSnapshot
    func commit(message: String) async throws -> GitOperationSnapshot
    func push() async throws -> GitOperationSnapshot
}

@MainActor
@Observable
public final class WireboltModel {
    public static let previewByteLimit = DocumentSession.previewByteLimit

    public var workspace = WorkspaceDraft(name: "Wirebolt") {
        didSet { sidebarSnapshot = nil; sidebarVisibleCache = nil; sidebarUnfilteredCache = nil }
    }
    @ObservationIgnored private var sidebarSnapshot: SidebarSnapshot?
    @ObservationIgnored private var sidebarVisibleCache: (query: String, collapsed: Set<String>, expanded: Set<String>, rows: [SidebarSnapshot.Row])?
    /// The unfiltered outline, kept apart from the last filter's rows so clearing a filter
    /// (or deleting its last character) shows the outline without recomputing it.
    @ObservationIgnored private var sidebarUnfilteredCache: (collapsed: Set<String>, expanded: Set<String>, rows: [SidebarSnapshot.Row])?

    public func sidebarRows(query: String, collapsed: Set<String>, expanded: Set<String>) -> [SidebarSnapshot.Row] {
        let collections = workspace.collections
        let filtering = !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        if filtering, let cache = sidebarVisibleCache, cache.query == query, cache.collapsed == collapsed, cache.expanded == expanded { return cache.rows }
        if !filtering, let cache = sidebarUnfilteredCache, cache.collapsed == collapsed, cache.expanded == expanded { return cache.rows }
        if sidebarSnapshot == nil { sidebarSnapshot = SidebarSnapshot(collections: collections) }
        let rows = sidebarSnapshot!.visible(query: query, collapsedCollections: collapsed, expandedGroups: expanded)
        if filtering { sidebarVisibleCache = (query, collapsed, expanded, rows) } else { sidebarUnfilteredCache = (collapsed, expanded, rows) }
        return rows
    }
    public var selectedEnvironmentID: String?
    public let proxyPreferences: ProxyPreferences
    public let sessions: DocumentSessionStore
    public private(set) var isLoadingWorkspace = false
    /// False until the first workspace load finishes; the window shows a skeleton meanwhile.
    public private(set) var hasLoadedWorkspace = false
    public private(set) var gitStatus: GitStatusSnapshot?
    public private(set) var gitOperation: GitOperationSnapshot?
    public private(set) var gitFailure: GitFailure?
    public private(set) var isGitBusy = false
    public private(set) var isImporting = false
    public var importFailureMessage: String?
    public private(set) var historyEntries: [RunHistoryEntry] = []
    public private(set) var historyRevision = 0
    private var editedSecrets: [String: String] = [:]
    private var dirtySecrets: Set<String> = []
    public private(set) var oauthReceipts: [String: OAuth2TokenReceipt] = [:]
    public private(set) var oauthFailureMessage: String?
    public private(set) var isOAuthBusy = false
    public var isShowingGitCollaboration = false
    /// The open workspace folder, when the persistence has one.
    public private(set) var workspaceLocation: URL?
    /// Increases whenever the active cookie jar changes, so a cookie list can refresh.
    public private(set) var cookieRevision = 0
    public var isShowingWorkspaceSettings = false
    public var settingsTab = "general"

    @ObservationIgnored private let socketConnector: (any WebSocketConnecting)?
    @ObservationIgnored private let runner: any RequestRunner
    @ObservationIgnored private var persistence: (any WorkspacePersistence)?
    @ObservationIgnored private var gitCollaboration: (any GitCollaboration)?
    @ObservationIgnored private let history: HistoryRepository
    @ObservationIgnored private var cookieJar: CookieJar
    /// Used while no workspace folder is known, for example in tests and previews.
    @ObservationIgnored private let defaultCookieJar: CookieJar
    @ObservationIgnored private let cookieDirectory: URL?
    @ObservationIgnored private let oauth2: any OAuth2Authorizing
    @ObservationIgnored private var requestSearchIndex: [String: String] = [:]
    @ObservationIgnored private var rootCreation: Task<Void, any Error>?
    @ObservationIgnored private var transportSave: Task<Void, Never>?
    @ObservationIgnored private var persistedTransport: TransportSettings?
    public private(set) var exportFailureMessage: String?
    public var operationFailure: RunFailure?
    /// The window's undo manager; workspace mutations register their inverse here.
    @ObservationIgnored public weak var undoManager: UndoManager?
    /// Undo and redo run one at a time so each inverse sees the state its predecessor left.
    @ObservationIgnored var undoWork: Task<Void, Never>?

    public nonisolated static var defaultCookieDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appending(path: "Wirebolt/Cookies", directoryHint: .isDirectory)
    }

    public init(
        runner: any RequestRunner,
        socketConnector: (any WebSocketConnecting)? = nil,
        persistence: (any WorkspacePersistence)? = nil,
        gitCollaboration: (any GitCollaboration)? = nil,
        sessions: DocumentSessionStore? = nil,
        history: HistoryRepository = HistoryRepository(),
        cookieJar: CookieJar = CookieJar(),
        cookieDirectory: URL? = WireboltModel.defaultCookieDirectory,
        oauth2: (any OAuth2Authorizing)? = nil,
        proxyPreferences: ProxyPreferences? = nil
    ) {
        self.proxyPreferences = proxyPreferences ?? ProxyPreferences()
        self.socketConnector = socketConnector
        self.runner = runner
        self.persistence = persistence
        self.gitCollaboration = gitCollaboration
        self.sessions = sessions ?? DocumentSessionStore()
        self.history = history
        self.cookieJar = cookieJar
        defaultCookieJar = cookieJar
        self.cookieDirectory = cookieDirectory
        self.oauth2 = oauth2 ?? OAuth2Service()
    }

    public func configurePersistence(
        _ persistence: any WorkspacePersistence,
        gitCollaboration: (any GitCollaboration)? = nil
    ) {
        self.persistence = persistence
        self.gitCollaboration = gitCollaboration
        editedSecrets = [:]
        dirtySecrets = []
        workspaceLocation = persistence.location
        // Each workspace keeps its own cookies, as a browser profile would.
        if let cookieDirectory, let location = persistence.location {
            cookieJar = CookieJar(storageURL: CookieJar.storageURL(forWorkspaceAt: location, in: cookieDirectory))
            // Earlier versions shared one jar between all workspaces and kept session cookies.
            try? FileManager.default.removeItem(at: cookieDirectory.appending(path: "cookies.json"))
        } else {
            cookieJar = defaultCookieJar
        }
        cookieRevision += 1
    }

    public var selectedRequestID: String? {
        guard let session = sessions.activeSession,
              let collectionID = session.collectionID
        else { return nil }
        return "\(collectionID)/\(session.requestID)"
    }

    public var draft: RequestDraft {
        get { sessions.activeSession?.draft ?? RequestDraft() }
        set {
            if let session = sessions.activeSession {
                session.draft = newValue
            } else {
                _ = sessions.open(draft: newValue, forceNewSession: true)
            }
        }
    }

    public var responseHead: ResponseHead? { sessions.activeSession?.responseHead }
    public var responseText: String { sessions.activeSession?.responseText ?? "" }
    public var responseBytes: UInt64 { sessions.activeSession?.responseBytes ?? 0 }
    public var responseWasTruncated: Bool { sessions.activeSession?.responseWasTruncated ?? false }
    public var completion: RunCompletion? { sessions.activeSession?.completion }
    public var failure: RunFailure? { sessions.activeSession?.failure ?? operationFailure }
    public var isRunning: Bool { sessions.activeSession?.isRunning ?? false }

    public var activeVariables: [String: ValueSource] {
        let global = workspace.environments.first { $0.id == WorkspaceDraft.globalEnvironmentID }?.enabledValues ?? [:]
        let selected = workspace.environments.first { $0.id == selectedEnvironmentID }?.enabledValues ?? [:]
        return global.merging(selected, uniquingKeysWith: { _, override in override })
    }

    public var hasUnsavedRequestChanges: Bool {
        sessions.sessions.values.contains(where: \.isDirty)
    }

    /// Dirty documents in tab order, for quit and window-close confirmation.
    public var dirtySessions: [DocumentSession] {
        var seen: Set<String> = []
        return sessions.groups.flatMap(\.tabIDs).compactMap { id in
            guard seen.insert(id).inserted, let session = sessions.session(id: id), session.isDirty else { return nil }
            return session
        }
    }

    /// Saves every dirty document. Stops at the first failure so the caller can keep
    /// the app or window open; documents saved before the failure stay saved.
    @discardableResult
    public func saveAllDirtySessions() async -> Bool {
        for session in dirtySessions {
            guard await save(session, fallbackCollectionID: workspace.collections.first?.id) else { return false }
        }
        await flushWorkspaceTransport()
        return true
    }

    /// Reverts every dirty document to its saved state; never-saved documents close.
    public func discardUnsavedChanges() {
        for session in dirtySessions {
            if let saved = session.savedDraft {
                session.markSaved(saved)
                continue
            }
            for group in sessions.groups where group.tabIDs.contains(session.id) {
                closeDocuments(.one(session.id), in: group.id, allowDirty: true)
            }
        }
    }

    public func normalizedSearchQuery(_ value: String) -> String {
        Self.normalizedSearchText(value)
    }

    public func requestMatches(_ location: RequestLocation, normalizedQuery: String) -> Bool {
        let key = Self.searchKey(for: location)
        if let indexed = requestSearchIndex[key] {
            return indexed.contains(normalizedQuery)
        }
        let indexed = Self.normalizedSearchText([
            location.request.name,
            location.request.method.rawValue,
            location.request.url,
        ].joined(separator: "\u{0}"))
        requestSearchIndex[key] = indexed
        return indexed.contains(normalizedQuery)
    }

    public func connectWebSocket(_ session: DocumentSession) async {
        guard let socketConnector else { return }
        do { try await flushSecrets() } catch {
            operationFailure = RunFailure(kind: "keychain", issues: [])
            return
        }
        var draft = session.draft
        if draft.inheritsWorkspaceTransport { draft.transport = workspace.transport }
        session.socket.connect(input: RunInput(draft: draft, variables: activeVariables,
            workspaceProxy: workspace.proxy, appProxy: proxyPreferences.configuration), connector: socketConnector)
    }

    public func send(_ target: DocumentSession? = nil) async {
        let session = target ?? sessions.activeSession ?? sessions.openTemporary()
        if session.kind == .webSocket {
            if session.socket.status == .connected { await session.socket.send(body: session.draft.body) }
            else { await connectWebSocket(session) }
            return
        }
        guard session.isRunning == false else { return }
        operationFailure = nil
        do { try await flushSecrets() } catch {
            operationFailure = RunFailure(kind: "keychain", issues: [])
            return
        }
        guard !session.isRunning, sessions.session(id: session.id) != nil else { return }
        let runID = RunID()
        var runDraft = session.draft
        if runDraft.inheritsWorkspaceTransport {
            runDraft.transport = workspace.transport
        }
        let variables = activeVariables
        let workspaceProxy = workspace.proxy
        let appProxy = proxyPreferences.configuration
        session.beginRun(runID)
        let effectiveURL: URL?
        do {
            let values = try await runner.resolveValues([.literal(runDraft.url)], variables: variables)
            effectiveURL = values.first.flatMap { URL(string: $0.trimmingCharacters(in: .whitespacesAndNewlines)) }
        } catch {
            await session.finish(runID: runID, failure: (error as? RunFailure) ?? RunFailure(kind: "invalid_request", issues: []))
            return
        }
        guard session.activeRunID == runID, sessions.session(id: session.id) != nil else { return }
        if let url = effectiveURL,
           runDraft.headers.contains(where: {
               $0.enabled && $0.name.caseInsensitiveCompare("cookie") == .orderedSame
           }) == false,
           let cookieHeader = await cookieJar.header(for: url)
        {
            runDraft.headers.append(RequestField(
                name: "Cookie",
                value: .literal(cookieHeader),
                enabled: true,
                sensitive: true
            ))
        }
        guard session.activeRunID == runID, sessions.session(id: session.id) != nil else { return }
        let input = RunInput(
            draft: runDraft,
            variables: variables,
            workspaceProxy: workspaceProxy,
            appProxy: appProxy
        )

        do {
            for try await event in runner.events(for: input, runID: runID) {
                guard session.activeRunID == runID else { return }
                if Task.isCancelled {
                    runner.cancel(runID: runID)
                    session.cancel(runID: runID)
                    return
                }
                if case let .cookies(update) = event, let url = URL(string: update.url) {
                    let received = await cookieJar.store(headers: update.headers, requestURL: url)
                    if !received.isEmpty { cookieRevision += 1 }
                    guard session.activeRunID == runID else { return }
                    session.appendResponseCookies(received)
                }
                await session.consume(event, runID: runID)
            }
            if session.activeRunID == runID {
                await session.finish(runID: runID, failure: nil)
            }
            await recordHistory(session: session, runID: runID)
        } catch is CancellationError {
            session.cancel(runID: runID)
        } catch let error as RunFailure {
            await session.finish(runID: runID, failure: error)
            await recordHistory(session: session, runID: runID)
        } catch {
            await session.finish(runID: runID, failure: RunFailure(kind: "bridge", issues: []))
            await recordHistory(session: session, runID: runID)
        }
    }

    public func cancel(_ target: DocumentSession? = nil) {
        guard let session = target ?? sessions.activeSession,
              let runID = session.activeRunID
        else { return }
        runner.cancel(runID: runID)
        session.cancel(runID: runID)
    }

    @discardableResult
    public func closeDocuments(_ scope: TabCloseScope, in groupID: String? = nil, allowDirty: Bool = false) -> [DocumentSession] {
        let previous = sessions.sessions
        let blocked = sessions.close(scope, in: groupID, allowDirty: allowDirty)
        guard blocked.isEmpty else { return blocked }
        for (id, session) in previous where sessions.session(id: id) == nil {
            cancel(session)
            if allowDirty, let saved = session.savedDraft { session.markSaved(saved) }
        }
        return []
    }

    public func acquireOAuthToken(for session: DocumentSession) async {
        guard case let .oauth2(configuration) = session.draft.authentication,
              let persistence
        else { return }
        isOAuthBusy = true
        oauthFailureMessage = nil
        defer { isOAuthBusy = false }
        do {
            // A client secret typed in the editor must reach Keychain before the token request reads it.
            try await flushSecrets()
            let (token, receipt) = try await oauth2.acquireToken(configuration: configuration)
            try await persistence.saveSecret(name: configuration.accessTokenReference, value: token)
            oauthReceipts[session.id] = receipt
        } catch {
            oauthFailureMessage = error.localizedDescription
        }
    }

    public func historyEntries(for session: DocumentSession) async -> [RunHistoryEntry] {
        await history.list(requestID: session.requestID)
    }

    public func loadHistory(for session: DocumentSession) async {
        historyEntries = await history.list(requestID: session.requestID)
    }

    public func restoreHistory(_ entry: RunHistoryEntry, into session: DocumentSession) async {
        session.restore(entry, viewport: await Self.historyViewport(entry))
    }

    private nonisolated static func historyViewport(_ entry: RunHistoryEntry) async -> Data {
        await Task.detached(priority: .utility) {
            let url = URL(fileURLWithPath: entry.bodyPath)
            guard let reader = try? FileHandle(forReadingFrom: url) else { return Data() }
            defer { try? reader.close() }
            return (try? reader.read(upToCount: ResponseBodyStore.viewportByteCount)) ?? Data()
        }.value
    }

    /// Shows the most recent recorded response in restored tabs that have not run yet.
    private func restoreLatestResponses(for restored: [DocumentSession]) {
        guard !restored.isEmpty else { return }
        Task {
            for session in restored {
                guard let entry = await history.list(requestID: session.requestID).first else { continue }
                let viewport = await Self.historyViewport(entry)
                guard sessions.session(id: session.id) === session, !session.isRunning,
                      session.responseHead == nil, session.failure == nil else { continue }
                session.restore(entry, viewport: viewport)
            }
        }
    }

    public func clearHistory(for session: DocumentSession) async {
        try? await history.clear(requestID: session.requestID)
        historyEntries = []
    }

    private func recordHistory(session: DocumentSession, runID: RunID) async {
        guard let prepared = session.preparedRun, let body = session.bodyStore else { return }
        try? await history.record(
            runID: runID,
            requestID: session.requestID,
            prepared: prepared,
            responseHead: session.responseHead,
            completion: session.completion,
            failure: session.failure,
            body: body
        )
        historyEntries = await history.list(requestID: session.requestID)
        historyRevision += 1
    }

    public func exportWorkspace() async -> String? {
        do { return try await persistence?.exportWorkspace() }
        catch { exportFailureMessage = error.localizedDescription; operationFailure = RunFailure(kind: "export", issues: []); return nil }
    }

    public func exportCollection(id: String) async -> String? {
        do { return try await persistence?.exportCollection(id: id) }
        catch {
            exportFailureMessage = error.localizedDescription
            operationFailure = RunFailure(kind: "export", issues: [])
            return nil
        }
    }

    public func exportRequest(collectionID: String, id: String) async -> String? {
        do { return try await persistence?.exportRequest(collectionID: collectionID, id: id) }
        catch {
            exportFailureMessage = error.localizedDescription
            operationFailure = RunFailure(kind: "export", issues: [])
            return nil
        }
    }

    public func loadWorkspace(restoring layout: SessionLayout? = nil) async {
        guard let persistence else {
            hasLoadedWorkspace = true
            return
        }
        isLoadingWorkspace = true
        defer {
            isLoadingWorkspace = false
            hasLoadedWorkspace = true
        }
        do {
            applyLoadedWorkspace(try await persistence.load(), restoring: layout)
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    @discardableResult
    public func openWorkspace(
        using persistence: any WorkspacePersistence,
        gitCollaboration: (any GitCollaboration)? = nil,
        restoring layout: SessionLayout? = nil
    ) async -> Bool {
        isLoadingWorkspace = true
        defer { isLoadingWorkspace = false }
        do {
            await flushWorkspaceTransport()
            let loaded = try await persistence.load()
            for session in sessions.sessions.values { cancel(session) }
            sessions.removeAll()
            configurePersistence(persistence, gitCollaboration: gitCollaboration)
            transportSave = nil
            persistedTransport = nil
            selectedEnvironmentID = nil
            oauthReceipts = [:]
            historyEntries = []
            gitStatus = nil
            gitOperation = nil
            gitFailure = nil
            applyLoadedWorkspace(loaded, restoring: layout)
            // Inverses recorded for the previous workspace must not touch this one.
            undoManager?.removeAllActions(withTarget: self)
            hasLoadedWorkspace = true
            return true
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
            return false
        }
    }

    public func refreshGitStatus() async {
        await performGitOperation { collaboration in
            let status = try await collaboration.status()
            self.gitStatus = status
        }
    }

    public func pullGit() async {
        guard !hasUnsavedRequestChanges else {
            gitFailure = GitFailure(
                kind: "unsaved_request",
                reason: "Save or discard request edits before pulling."
            )
            return
        }
        await flushWorkspaceTransport()
        await performGitOperation { collaboration in
            let operation = try await collaboration.pull()
            self.apply(operation)
            if operation.outcome == .updated {
                await self.reloadWorkspaceAfterGitUpdate()
            }
        }
    }

    public func commitGit(message: String) async {
        await performGitOperation { collaboration in
            self.apply(try await collaboration.commit(message: message))
        }
    }

    public func pushGit() async {
        await performGitOperation { collaboration in
            self.apply(try await collaboration.push())
        }
    }

    // MARK: Workspace name

    @discardableResult
    public func renameWorkspace(_ proposedName: String) async -> Bool {
        let name = proposedName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let persistence, !name.isEmpty else { return false }
        guard name != workspace.name else { return true }
        do {
            _ = try await persistence.apply(.renameWorkspace(name: name))
            workspace.name = name
            return true
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
            return false
        }
    }

    // MARK: Cookies

    /// The active workspace's cookies, most specific domain first.
    public func cookies() async -> [CookieSnapshot] {
        await cookieJar.all().sorted { ($0.domain, $0.path, $0.name) < ($1.domain, $1.path, $1.name) }
    }

    public func deleteCookie(id: CookieSnapshot.ID) async {
        await cookieJar.delete(id: id)
        cookieRevision += 1
    }

    public func clearCookies() async {
        await cookieJar.removeAll()
        cookieRevision += 1
    }

    /// Opens a saved request. `preview` reuses the group's preview tab instead of adding a tab.
    public func select(_ location: RequestLocation, preview: Bool = false) {
        if preview {
            _ = sessions.openPreview(draft: location.request, collectionID: location.collectionID)
        } else {
            _ = sessions.open(
                draft: location.request,
                collectionID: location.collectionID
            )
        }
    }

    public func saveCurrentRequest(collectionID: String) async {
        guard let session = sessions.activeSession else { return }
        await save(session, fallbackCollectionID: collectionID)
    }

    /// Saves one document at its own location, or in `fallbackCollectionID` when it has none.
    @discardableResult
    public func save(_ session: DocumentSession, fallbackCollectionID: String?) async -> Bool {
        guard let persistence else { return false }
        guard let collectionID = session.collectionID ?? fallbackCollectionID else {
            operationFailure = RunFailure(kind: "workspace", issues: [])
            return false
        }
        do {
            try await flushSecrets()
            let location = RequestLocation(
                collectionID: collectionID,
                groupID: workspace.location(collectionID: collectionID, requestID: session.draft.id)?.groupID,
                order: workspace.location(collectionID: collectionID, requestID: session.draft.id)?.order ?? 0,
                request: session.draft
            )
            _ = try await persistence.apply(.saveRequest(collectionID: collectionID, location: location))
            session.recordSavedDraft(location.request)
            applySavedRequest(location)
            return true
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
            return false
        }
    }

    @discardableResult
    public func saveEnvironment(_ environment: EnvironmentDraft) async -> Bool {
        guard let persistence else { return false }
        do {
            _ = try await persistence.apply(.saveEnvironment(environment))
            if let index = workspace.environments.firstIndex(where: { $0.id == environment.id }) {
                workspace.environments[index] = environment
            } else {
                workspace.environments.append(environment)
            }
            selectedEnvironmentID = environment.id == WorkspaceDraft.globalEnvironmentID ? nil : environment.id
            return true
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
            return false
        }
    }

    public func makeProxyConnectionTest() -> ProxyConnectionTest { ProxyConnectionTest(runner: runner) }

    public func effectiveProxy(for draft: RequestDraft) -> EffectiveProxy {
        .resolve(request: draft.proxy.document, workspace: workspace.proxy, app: proxyPreferences.configuration)
    }

    /// Validate and persist before publishing. Failed saves leave the active policy intact.
    public func applyProxy(_ configuration: ProxyDocument?, scope: ProxyScope,
                           session: DocumentSession? = nil, secrets: [String: String] = [:]) async throws {
        try configuration?.validate()
        if scope == .app && configuration == nil { throw ProxyValidationError("Choose an app default.") }
        if scope == .request && session == nil { throw WorkspaceMutationError.unsupported }
        for (name, value) in secrets {
            guard let persistence else { throw WorkspaceMutationError.unsupported }
            try await persistence.saveSecret(name: name, value: value)
        }
        switch scope {
        case .app: try proxyPreferences.save(configuration ?? .system)
        case .workspace:
            guard let persistence else { throw WorkspaceMutationError.unsupported }
            _ = try await persistence.apply(.saveWorkspaceProxy(configuration))
            workspace.proxy = configuration
        case .request: session?.draft.proxy = ProxySelection(document: configuration)
        }
        runner.proxySettingsChanged()
    }

    /// The model owns pending writes so dismissing a settings view cannot cancel them.
    public func updateWorkspaceTransport(_ value: TransportSettings) {
        guard let persistence else { return }
        if persistedTransport == nil { persistedTransport = workspace.transport }
        workspace.transport = value
        let previous = transportSave
        previous?.cancel()
        transportSave = Task {
            await previous?.value
            do { try await Task.sleep(for: .milliseconds(250)) } catch { return }
            do {
                _ = try await persistence.apply(.saveWorkspaceSettings(value))
                persistedTransport = value
            } catch {
                if workspace.transport == value, let persistedTransport { workspace.transport = persistedTransport }
                operationFailure = RunFailure(kind: "workspace", issues: [])
            }
        }
    }

    public func flushWorkspaceTransport() async { await transportSave?.value }

    public func saveWorkspaceTransport() async {
        guard let persistence else { return }
        do {
            _ = try await persistence.apply(.saveWorkspaceSettings(workspace.transport))
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    @discardableResult
    public func deleteEnvironment(id: String) async -> Bool {
        guard let persistence else { return false }
        do {
            _ = try await persistence.apply(.deleteEnvironment(id: id))
            workspace.environments.removeAll { $0.id == id }
            if selectedEnvironmentID == id {
                selectedEnvironmentID = nil
            }
            return true
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
            return false
        }
    }

    /// Explicit export: resolved credentials are returned only to the caller, never persisted.
    public func curlCommand(for draft: RequestDraft) async -> String? {
        var draft = draft
        if draft.inheritsWorkspaceTransport { draft.transport = workspace.transport }
        let variables = activeVariables
        let proxy: ProxyDocument = switch draft.proxy {
        case .inherit: workspace.proxy ?? proxyPreferences.configuration
        case .direct: .direct
        case .system: .system
        case let .manual(value): value
        }
        do {
            try await flushSecrets()
            let sources = draft.curlValueSources
            let values = try await runner.resolveValues(sources, variables: variables)
            guard sources.count == values.count else { throw RunFailure(kind: "export", issues: []) }
            let resolved = Dictionary(zip(sources, values), uniquingKeysWith: { first, _ in first })
            let url = resolved[.literal(draft.url)]!
            var credentials: [String] = []
            if let references = proxy.curlRoute(for: url)?.credentials {
                credentials = try await runner.resolveValues([.secret(references.username), .secret(references.password)], variables: variables)
                guard credentials.count == 2 else { throw RunFailure(kind: "export", issues: []) }
            }
            return draft.curlCommand { resolved[$0]! } + proxy.curlArguments(for: url, credentials: credentials)
        } catch {
            operationFailure = (error as? RunFailure) ?? RunFailure(kind: "export", issues: [])
            return nil
        }
    }

    public func secretMaterial(for source: ValueSource) -> String {
        switch source {
        case let .literal(value): value
        case let .secret(name): editedSecrets[name] ?? ""
        }
    }

    public func editSecret(name: String, value: String) {
        editedSecrets[name] = value
        dirtySecrets.insert(name)
    }

    public func loadSecret(_ source: ValueSource) async {
        guard case let .secret(name) = source, editedSecrets[name] == nil,
              let persistence else { return }
        do {
            let value = try await persistence.readSecret(name: name) ?? ""
            if editedSecrets[name] == nil { editedSecrets[name] = value }
        } catch { operationFailure = RunFailure(kind: "keychain", issues: []) }
    }

    private func flushSecrets() async throws {
        guard let persistence else {
            if !dirtySecrets.isEmpty { throw WorkspaceMutationError.unsupported }
            return
        }
        for name in dirtySecrets {
            guard let value = editedSecrets[name] else { continue }
            try await persistence.saveSecret(name: name, value: value)
            if editedSecrets[name] == value { dirtySecrets.remove(name) }
        }
        if !dirtySecrets.isEmpty { try await flushSecrets() }
    }

    /// Writes secret values straight to Keychain, for editors with their own Save button.
    /// The in-memory copy is updated so other editors show the new value.
    @discardableResult
    public func saveSecrets(_ values: [String: String]) async -> Bool {
        guard !values.isEmpty else { return true }
        guard let persistence else { return false }
        do {
            for (name, value) in values.sorted(by: { $0.key < $1.key }) {
                try await persistence.saveSecret(name: name, value: value)
                editedSecrets[name] = value
                dirtySecrets.remove(name)
            }
            return true
        } catch {
            operationFailure = RunFailure(kind: "keychain", issues: [])
            return false
        }
    }

    public func saveSecret(name: String, value: String) async {
        guard let persistence else { return }
        do {
            try await persistence.saveSecret(name: name, value: value)
        } catch {
            operationFailure = RunFailure(kind: "keychain", issues: [])
        }
    }

    public func importDocument(url: URL, format: ImportFormat) async {
        guard !isImporting, persistence != nil else { return }
        isImporting = true
        importFailureMessage = nil
        defer { isImporting = false }
        do {
            let source = try await Task.detached(priority: .userInitiated) {
                try String(contentsOf: url, encoding: .utf8)
            }.value
            await commitDocument(source: source, format: format, fileName: url.deletingPathExtension().lastPathComponent)
        } catch { importFailureMessage = "The selected document could not be read." }
    }

    public func importDocument(source: String, format: ImportFormat) async {
        guard !isImporting, persistence != nil else { return }
        isImporting = true
        importFailureMessage = nil
        defer { isImporting = false }
        await commitDocument(source: source, format: format)
    }

    private func commitDocument(source: String, format: ImportFormat, fileName: String? = nil) async {
        guard let persistence else { return }
        await flushWorkspaceTransport()
        let previousIDs = Set(workspace.collections.map(\.id))
        do {
            if let fileName {
                _ = try await persistence.commitImportFile(format: format, source: source, name: fileName)
            } else { _ = try await persistence.commitImport(format: format, source: source) }
            applyLoadedWorkspace(try await persistence.load())
            if let imported = workspace.collections.first(where: { !previousIDs.contains($0.id) }),
               let request = imported.requests.first { select(request) }
        } catch {
            importFailureMessage = "The document could not be imported. Check its format and contents."
        }
    }

    @discardableResult
    public func makeNewRequest(
        kind: DocumentKind = .http,
        collectionID: String? = nil,
        groupID: String? = nil
    ) -> DocumentSession {
        let session = sessions.openTemporary(kind: kind, collectionID: collectionID)
        if let collectionID,
           let collectionIndex = workspace.collections.firstIndex(where: { $0.id == collectionID })
        {
            let location = RequestLocation(
                collectionID: collectionID,
                groupID: groupID,
                order: max(Int.min + 1, min(0, workspace.collections[collectionIndex].requests.map(\.order).min() ?? 0)) - 1,
                request: session.draft
            )
            applySavedRequest(location)
        }
        return session
    }

    public func makeNewEnvironment() -> EnvironmentDraft {
        EnvironmentDraft(
            id: UUID().uuidString.lowercased(),
            name: "New Environment"
        )
    }

    public func createRequest(kind: DocumentKind = .http, collectionID: String? = nil, groupID: String? = nil) async -> DocumentSession? {
        guard persistence != nil else { return nil }
        let collectionID = collectionID ?? WorkspaceDraft.rootCollectionID
        do {
            if collectionID == WorkspaceDraft.rootCollectionID { try await ensureRootCollection() }
        } catch { operationFailure = RunFailure(kind: "workspace", issues: []); return nil }
        guard let collection = workspace.collections.first(where: { $0.id == collectionID }) else { return nil }
        let draft = RequestDraft(id: UUID().uuidString.lowercased(),
            name: kind == .http ? "Untitled Request" : "Untitled WebSocket Request", webSocket: kind == .webSocket)
        let location = RequestLocation(collectionID: collectionID, groupID: groupID,
            order: max(Int.min + 1, min(0, collection.requests.map(\.order).min() ?? 0)) - 1, request: draft)
        guard await perform(.restoreRequest(location), named: "New Request") else { return nil }
        return sessions.open(draft: draft, collectionID: collectionID, kind: kind)
    }

    private func ensureRootCollection() async throws {
        let id = WorkspaceDraft.rootCollectionID
        guard let persistence, !workspace.collections.contains(where: { $0.id == id }) else { return }
        let root = CollectionDraft(id: id, name: "Requests", order: Int.min)
        if rootCreation == nil { rootCreation = Task { _ = try await persistence.apply(.createCollection(root)) } }
        do { try await rootCreation?.value } catch { rootCreation = nil; throw error }
        if !workspace.collections.contains(where: { $0.id == id }) { workspace.collections.insert(root, at: 0) }
        rootCreation = nil
    }

    public func createRootFolder() async -> String? {
        do {
            try await ensureRootCollection()
            return await createGroup(collectionID: WorkspaceDraft.rootCollectionID, name: "Untitled Folder")
        } catch { operationFailure = RunFailure(kind: "workspace", issues: []); return nil }
    }

    public func createCollection(name: String = "New Collection") async {
        let collection = CollectionDraft(
            id: Self.documentID(from: name, fallback: "collection"),
            name: name,
            order: workspace.collections.count
        )
        await perform(.restoreCollection(collection), named: "New Collection")
    }

    public func renameRequest(collectionID: String, requestID: String, name: String) async {
        await perform(.renameRequest(collectionID: collectionID, id: requestID, name: name), named: "Rename")
    }

    public func renameCollection(id: String, name: String) async {
        await perform(.renameCollection(id: id, name: name), named: "Rename")
    }

    public func deleteCollection(id: String) async {
        let name = workspace.collections.first { $0.id == id }?.name ?? ""
        await perform(.removeCollection(id: id), named: "Delete “\(name)”")
    }

    @discardableResult
    public func createGroup(collectionID: String, parentID: String? = nil, name: String = "New Folder") async -> String? {
        guard let collection = workspace.collections.first(where: { $0.id == collectionID }) else { return nil }
        let group = GroupDraft(
            id: Self.documentID(from: name, fallback: "group"),
            name: name,
            parentID: parentID,
            order: collection.groups.filter { $0.parentID == parentID }.count
        )
        let created = await perform(.restoreGroups(collectionID: collectionID, groups: [group], requests: []), named: "New Folder")
        return created ? group.id : nil
    }

    public func renameGroup(collectionID: String, id: String, name: String) async {
        await perform(.renameGroup(collectionID: collectionID, id: id, name: name), named: "Rename")
    }

    public func deleteGroup(collectionID: String, id: String) async {
        let name = workspace.collections.first { $0.id == collectionID }?.groups.first { $0.id == id }?.name ?? ""
        await perform(.removeGroup(collectionID: collectionID, id: id), named: "Delete “\(name)”")
    }

    public func deleteRequest(collectionID: String, requestID: String) async {
        let name = workspace.location(collectionID: collectionID, requestID: requestID)?.request.name ?? ""
        await perform(.removeRequest(collectionID: collectionID, id: requestID), named: "Delete “\(name)”")
    }

    public func duplicateRequest(collectionID: String, requestID: String) async {
        guard let source = workspace.location(collectionID: collectionID, requestID: requestID) else { return }
        let newID = Self.documentID(from: "\(source.request.id)-copy", fallback: "request")
        await perform(.duplicateRequest(collectionID: collectionID, id: requestID, newID: newID,
                                        name: "\(source.request.name) Copy"),
                      named: "Duplicate “\(source.request.name)”")
    }

    private var isReorderingSidebar = false

    public func canReorderSidebar(_ identifier: String, relativeTo target: String,
                                  collectionID: String, parentID: String?) -> Bool {
        let parts = identifier.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3, parts[1] == collectionID,
              let collection = workspace.collections.first(where: { $0.id == collectionID }) else { return false }
        let source = parts[0] + ":" + parts[2]
        let siblings = collection.orderedChildren(parentID: parentID)
        return source != target && siblings.contains(source) && siblings.contains(target)
    }

    public func reorderSidebar(_ identifier: String, relativeTo target: String, after: Bool,
                               collectionID: String, parentID: String?) async {
        guard !isReorderingSidebar,
              canReorderSidebar(identifier, relativeTo: target, collectionID: collectionID, parentID: parentID),
              let collection = workspace.collections.first(where: { $0.id == collectionID }) else { return }
        let parts = identifier.split(separator: "|", omittingEmptySubsequences: false)
        let source = String(parts[0]) + ":" + String(parts[2])
        let previous = collection.orderedChildren(parentID: parentID)
        var items = previous.filter { $0 != source }
        guard let targetIndex = items.firstIndex(of: target) else { return }
        items.insert(source, at: targetIndex + (after ? 1 : 0))
        guard items != previous else { return }
        isReorderingSidebar = true
        defer { isReorderingSidebar = false }
        await perform(.reorder(collectionID: collectionID, parentID: parentID, items: items),
                      named: "Move “\(sidebarItemName(identifier))”")
    }

    /// Whether Move Up (-1) or Move Down (+1) can swap the item with a sibling.
    public func canMoveSidebarItem(_ identifier: String, by delta: Int) -> Bool {
        // Evaluated for menus on every selection change: use the cached, ordered snapshot.
        let parts = identifier.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3 else { return false }
        let rowID = parts[0] == "request" ? "request:\(parts[1])/\(parts[2])" : "group:\(parts[1]):\(parts[2])"
        if sidebarSnapshot == nil { sidebarSnapshot = SidebarSnapshot(collections: workspace.collections) }
        return sidebarSnapshot?.sibling(of: rowID, by: delta) != nil
    }

    /// Moves a request or folder one position among its siblings, like dragging it past its neighbor.
    public func moveSidebarItem(_ identifier: String, by delta: Int) async {
        guard let (collectionID, parentID, neighbor) = sidebarNeighbor(of: identifier, by: delta) else { return }
        await reorderSidebar(identifier, relativeTo: neighbor, after: delta > 0,
                             collectionID: collectionID, parentID: parentID)
    }

    /// Whether the request or folder can move into `parentID` of `collectionID` (nil = top level).
    public func canMoveSidebarItem(_ identifier: String, toCollectionID collectionID: String, parentID: String?) -> Bool {
        let parts = identifier.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3,
              let destination = workspace.collections.first(where: { $0.id == collectionID }),
              parentID.map({ parent in destination.groups.contains { $0.id == parent } }) ?? true
        else { return false }
        switch parts[0] {
        case "request":
            guard let location = workspace.location(collectionID: parts[1], requestID: parts[2]) else { return false }
            return location.collectionID != collectionID || location.groupID != parentID
        case "group":
            guard parts[1] == collectionID,
                  let group = destination.groups.first(where: { $0.id == parts[2] }) else { return false }
            return group.parentID != parentID
                && !(parentID.map { destination.descendantGroupIDs(of: group.id).contains($0) } ?? false)
        default:
            return false
        }
    }

    /// Moves a request or folder to the end of another container, as a drop on that container does.
    public func moveSidebarItem(_ identifier: String, toCollectionID collectionID: String, parentID: String?) async {
        guard canMoveSidebarItem(identifier, toCollectionID: collectionID, parentID: parentID) else { return }
        let parts = identifier.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        let order = nextChildOrder(collectionID: collectionID, parentID: parentID)
        if parts[0] == "request" {
            await moveRequest(fromCollectionID: parts[1], requestID: parts[2], toCollectionID: collectionID,
                              groupID: parentID, order: order)
        } else {
            await moveGroup(collectionID: collectionID, id: parts[2], parentID: parentID, order: order)
        }
    }

    /// The order that places a new child after every existing child of the container.
    public func nextChildOrder(collectionID: String, parentID: String?) -> Int {
        guard let collection = workspace.collections.first(where: { $0.id == collectionID }) else { return 0 }
        return max(collection.groups.filter { $0.parentID == parentID }.map(\.order).max() ?? -1,
                   collection.requests.filter { $0.groupID == parentID }.map(\.order).max() ?? -1) + 1
    }

    private func sidebarNeighbor(of identifier: String, by delta: Int) -> (String, String?, String)? {
        let parts = identifier.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3, delta != 0,
              let collection = workspace.collections.first(where: { $0.id == parts[1] }) else { return nil }
        let parentID: String?
        switch parts[0] {
        case "request":
            guard let location = collection.requests.first(where: { $0.request.id == parts[2] }) else { return nil }
            parentID = location.groupID
        case "group":
            guard let group = collection.groups.first(where: { $0.id == parts[2] }) else { return nil }
            parentID = group.parentID
        default:
            return nil
        }
        let siblings = collection.orderedChildren(parentID: parentID)
        guard let index = siblings.firstIndex(of: parts[0] + ":" + parts[2]),
              siblings.indices.contains(index + delta) else { return nil }
        return (collection.id, parentID, siblings[index + delta])
    }

    private func sidebarItemName(_ identifier: String) -> String {
        let parts = identifier.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        guard parts.count == 3 else { return "" }
        if parts[0] == "request" { return workspace.location(collectionID: parts[1], requestID: parts[2])?.request.name ?? "" }
        return workspace.collections.first { $0.id == parts[1] }?.groups.first { $0.id == parts[2] }?.name ?? ""
    }

    public func moveRequest(
        fromCollectionID: String,
        requestID: String,
        toCollectionID: String,
        groupID: String?,
        order: Int
    ) async {
        let name = workspace.location(collectionID: fromCollectionID, requestID: requestID)?.request.name ?? ""
        await perform(.moveRequest(fromCollectionID: fromCollectionID, id: requestID, toCollectionID: toCollectionID,
                                   groupID: groupID, order: order),
                      named: "Move “\(name)”")
    }

    public func moveGroup(
        collectionID: String,
        id: String,
        parentID: String?,
        order: Int
    ) async {
        let name = workspace.collections.first { $0.id == collectionID }?.groups.first { $0.id == id }?.name ?? ""
        await perform(.moveGroup(collectionID: collectionID, id: id, parentID: parentID, order: order),
                      named: "Move “\(name)”")
    }

    private func applyLoadedWorkspace(_ loaded: WorkspaceDraft, restoring layout: SessionLayout? = nil) {
        workspace = loaded
        persistedTransport = loaded.transport
        rebuildRequestSearchIndex()
        selectedEnvironmentID = loaded.environments.first(where: { $0.id != WorkspaceDraft.globalEnvironmentID })?.id
        guard sessions.activeSession == nil else { return }
        if let layout, sessions.sessions.isEmpty {
            let restored = sessions.restore(layout) { tab in
                loaded.location(collectionID: tab.collectionID, requestID: tab.requestID)
            }
            if !restored.isEmpty {
                restoreLatestResponses(for: restored)
                return
            }
        }
        guard let first = loaded.collections.flatMap(\.requests).first else { return }
        select(first)
    }

    private func applySavedRequest(_ location: RequestLocation) {
        let request = location.request
        let collectionID = location.collectionID
        guard let collectionIndex = workspace.collections.firstIndex(where: { $0.id == collectionID }) else {
            return
        }
        if let requestIndex = workspace.collections[collectionIndex].requests.firstIndex(where: {
            $0.request.id == request.id
        }) {
            workspace.collections[collectionIndex].requests[requestIndex] = location
        } else {
            workspace.collections[collectionIndex].requests.append(location)
        }
        rebuildRequestSearchIndex()
    }

    private func rebuildRequestSearchIndex() {
        var rebuilt: [String: String] = [:]
        rebuilt.reserveCapacity(workspace.collections.reduce(0) { $0 + $1.requests.count })
        for collection in workspace.collections {
            for location in collection.requests {
                rebuilt[Self.searchKey(for: location)] = Self.normalizedSearchText([
                    location.request.name,
                    location.request.method.rawValue,
                    location.request.url,
                ].joined(separator: "\u{0}"))
            }
        }
        requestSearchIndex = rebuilt
    }

    private static func searchKey(for location: RequestLocation) -> String {
        "\(location.collectionID)\u{0}\(location.request.id)"
    }

    private static func normalizedSearchText(_ value: String) -> String {
        value.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current)
    }

    private func closeSessions(collectionID: String, requestID: String? = nil) {
        for group in sessions.groups {
            let ids = group.tabIDs.filter { id in
                guard let session = sessions.session(id: id),
                      session.collectionID == collectionID
                else { return false }
                return requestID == nil || session.requestID == requestID
            }
            for id in ids {
                closeDocuments(.one(id), in: group.id, allowDirty: true)
            }
        }
    }

    private static func documentID(from proposedName: String, fallback: String) -> String {
        let normalized = proposedName.lowercased().unicodeScalars.map { scalar -> Character in
            CharacterSet.alphanumerics.contains(scalar) ? Character(String(scalar)) : "-"
        }
        let base = String(normalized)
            .split(separator: "-", omittingEmptySubsequences: true)
            .joined(separator: "-")
            .prefix(48)
        let stem = base.isEmpty ? fallback : String(base)
        return "\(stem)-\(UUID().uuidString.lowercased().prefix(8))"
    }

    private func performGitOperation(
        _ operation: (any GitCollaboration) async throws -> Void
    ) async {
        guard !isGitBusy else { return }
        guard let gitCollaboration else {
            gitFailure = GitFailure(
                kind: "not_configured",
                reason: "Open a workspace before using Git collaboration."
            )
            return
        }
        gitFailure = nil
        isGitBusy = true
        defer { isGitBusy = false }
        do {
            try await operation(gitCollaboration)
        } catch let failure as GitFailure {
            gitFailure = failure
        } catch {
            gitFailure = GitFailure(kind: "bridge", reason: "Git operation failed.")
        }
    }

    private func apply(_ operation: GitOperationSnapshot) {
        gitOperation = operation
        gitStatus = operation.status
    }

    private func reloadWorkspaceAfterGitUpdate() async {
        guard let persistence else { return }
        do {
            applyLoadedWorkspace(try await persistence.load())
            // Pulled files may no longer match the recorded inverses.
            undoManager?.removeAllActions(withTarget: self)
        } catch {
            gitFailure = GitFailure(
                kind: "workspace_reload",
                reason: "Git updated the workspace, but Wirebolt could not reload it."
            )
        }
    }
}

/// One reversible workspace mutation. Applying a step persists it and returns the step
/// that reverses it, so undo and redo share the code path of the original command.
enum WorkspaceEditStep: Equatable, Sendable {
    case restoreRequest(RequestLocation)
    case removeRequest(collectionID: String, id: String)
    case duplicateRequest(collectionID: String, id: String, newID: String, name: String)
    /// Recreates folders (parents first) and the requests inside them with their IDs and order.
    case restoreGroups(collectionID: String, groups: [GroupDraft], requests: [RequestLocation])
    case removeGroup(collectionID: String, id: String)
    case restoreCollection(CollectionDraft)
    case removeCollection(id: String)
    case renameRequest(collectionID: String, id: String, name: String)
    case renameGroup(collectionID: String, id: String, name: String)
    case renameCollection(id: String, name: String)
    case moveRequest(fromCollectionID: String, id: String, toCollectionID: String, groupID: String?, order: Int)
    case moveGroup(collectionID: String, id: String, parentID: String?, order: Int)
    case reorder(collectionID: String, parentID: String?, items: [String])
}

/// Filled when the step that produces it finishes, so redo can be registered synchronously
/// while undo runs, as UndoManager requires, even though persistence is asynchronous.
@MainActor
private final class PendingEditStep {
    var step: WorkspaceEditStep?
    init(_ step: WorkspaceEditStep? = nil) { self.step = step }
}

extension WireboltModel {
    /// Applies a mutation and, when it succeeds, makes it undoable under `name`.
    @discardableResult
    func perform(_ step: WorkspaceEditStep, named name: String) async -> Bool {
        await undoWork?.value
        guard let inverse = await apply(step) else { return false }
        guard let undoManager else { return true }
        let opensGroup = !undoManager.isUndoing && !undoManager.isRedoing
        if opensGroup { undoManager.beginUndoGrouping() }
        let pending = PendingEditStep(inverse)
        undoManager.registerUndo(withTarget: self) { $0.runUndoStep(pending, named: name) }
        undoManager.setActionName(name)
        if opensGroup { undoManager.endUndoGrouping() }
        return true
    }

    /// Waits for queued undo and redo steps; used by tests and callers that need settled state.
    func finishUndoWork() async {
        await undoWork?.value
    }

    private func runUndoStep(_ pending: PendingEditStep, named name: String) {
        let reverse = PendingEditStep()
        undoManager?.registerUndo(withTarget: self) { $0.runUndoStep(reverse, named: name) }
        undoManager?.setActionName(name)
        let previous = undoWork
        undoWork = Task { [weak self] in
            await previous?.value
            guard let self else { return }
            guard let step = pending.step, let inverse = await self.apply(step) else {
                // The stack no longer describes the workspace; keep what was restored so far.
                self.undoManager?.removeAllActions(withTarget: self)
                return
            }
            reverse.step = inverse
        }
    }

    /// Persists one step and mirrors it in the loaded tree. Returns the inverse, or nil when
    /// nothing changed or persistence failed (failures are reported through `operationFailure`).
    func apply(_ step: WorkspaceEditStep) async -> WorkspaceEditStep? {
        guard let persistence else { return nil }
        do {
            switch step {
            case let .restoreRequest(location):
                guard let collection = workspace.collections.first(where: { $0.id == location.collectionID }),
                      location.groupID.map({ id in collection.groups.contains { $0.id == id } }) ?? true
                else { return nil }
                _ = try await persistence.apply(.saveRequest(collectionID: location.collectionID, location: location))
                applySavedRequest(location)
                return .removeRequest(collectionID: location.collectionID, id: location.request.id)

            case let .removeRequest(collectionID, id):
                guard let location = workspace.location(collectionID: collectionID, requestID: id) else { return nil }
                _ = try await persistence.apply(.deleteRequest(collectionID: collectionID, id: id))
                if let index = workspace.collections.firstIndex(where: { $0.id == collectionID }) {
                    workspace.collections[index].requests.removeAll { $0.request.id == id }
                }
                rebuildRequestSearchIndex()
                closeSessions(collectionID: collectionID, requestID: id)
                return .restoreRequest(location)

            case let .duplicateRequest(collectionID, id, newID, name):
                guard var copy = workspace.location(collectionID: collectionID, requestID: id) else { return nil }
                _ = try await persistence.apply(.duplicateRequest(collectionID: collectionID, id: id, newID: newID, name: name))
                guard let index = workspace.collections.firstIndex(where: { $0.id == collectionID }) else { return nil }
                copy.request.id = newID
                copy.request.name = name
                copy.order = workspace.collections[index].requests.count
                workspace.collections[index].requests.append(copy)
                rebuildRequestSearchIndex()
                return .removeRequest(collectionID: collectionID, id: newID)

            case let .restoreGroups(collectionID, groups, requests):
                guard let root = groups.first,
                      workspace.collections.contains(where: { $0.id == collectionID }) else { return nil }
                for group in groups {
                    _ = try await persistence.apply(.createGroup(collectionID: collectionID, group: group))
                    guard let index = workspace.collections.firstIndex(where: { $0.id == collectionID }) else { return nil }
                    workspace.collections[index].groups.append(group)
                }
                try await restoreRequests(requests, in: collectionID)
                return .removeGroup(collectionID: collectionID, id: root.id)

            case let .removeGroup(collectionID, id):
                guard let collection = workspace.collections.first(where: { $0.id == collectionID }),
                      collection.groups.contains(where: { $0.id == id }) else { return nil }
                let descendantIDs = collection.descendantGroupIDs(of: id)
                let removedRequests = collection.requests.filter { $0.groupID.map(descendantIDs.contains) ?? false }
                _ = try await persistence.apply(.deleteGroup(collectionID: collectionID, id: id))
                for location in removedRequests { closeSessions(collectionID: collectionID, requestID: location.request.id) }
                if let index = workspace.collections.firstIndex(where: { $0.id == collectionID }) {
                    workspace.collections[index].groups.removeAll { descendantIDs.contains($0.id) }
                    workspace.collections[index].requests.removeAll { $0.groupID.map(descendantIDs.contains) ?? false }
                }
                rebuildRequestSearchIndex()
                return .restoreGroups(collectionID: collectionID, groups: collection.subtreeGroups(of: id),
                                      requests: removedRequests)

            case let .restoreCollection(collection):
                guard !workspace.collections.contains(where: { $0.id == collection.id }) else { return nil }
                let empty = CollectionDraft(id: collection.id, name: collection.name, order: collection.order)
                _ = try await persistence.apply(.createCollection(empty))
                workspace.collections.append(empty)
                for group in collection.subtreeGroups(of: nil) {
                    _ = try await persistence.apply(.createGroup(collectionID: collection.id, group: group))
                    guard let index = workspace.collections.firstIndex(where: { $0.id == collection.id }) else { return nil }
                    workspace.collections[index].groups.append(group)
                }
                try await restoreRequests(collection.requests, in: collection.id)
                return .removeCollection(id: collection.id)

            case let .removeCollection(id):
                guard let collection = workspace.collections.first(where: { $0.id == id }) else { return nil }
                _ = try await persistence.apply(.deleteCollection(id: id))
                workspace.collections.removeAll { $0.id == id }
                rebuildRequestSearchIndex()
                closeSessions(collectionID: id)
                return .restoreCollection(collection)

            case let .renameRequest(collectionID, id, name):
                guard var location = workspace.location(collectionID: collectionID, requestID: id),
                      location.request.name != name else { return nil }
                let previous = location.request.name
                location.request.name = name
                _ = try await persistence.apply(.saveRequest(collectionID: collectionID, location: location))
                applySavedRequest(location)
                for session in sessions.sessions.values where session.collectionID == collectionID && session.requestID == id {
                    session.renameSavedRequest(name)
                }
                return .renameRequest(collectionID: collectionID, id: id, name: previous)

            case let .renameGroup(collectionID, id, name):
                guard let collection = workspace.collections.first(where: { $0.id == collectionID }),
                      let previous = collection.groups.first(where: { $0.id == id })?.name, previous != name
                else { return nil }
                _ = try await persistence.apply(.renameGroup(collectionID: collectionID, id: id, name: name))
                if let index = workspace.collections.firstIndex(where: { $0.id == collectionID }),
                   let groupIndex = workspace.collections[index].groups.firstIndex(where: { $0.id == id }) {
                    workspace.collections[index].groups[groupIndex].name = name
                }
                return .renameGroup(collectionID: collectionID, id: id, name: previous)

            case let .renameCollection(id, name):
                guard let previous = workspace.collections.first(where: { $0.id == id })?.name, previous != name
                else { return nil }
                _ = try await persistence.apply(.renameCollection(id: id, name: name))
                if let index = workspace.collections.firstIndex(where: { $0.id == id }) {
                    workspace.collections[index].name = name
                }
                return .renameCollection(id: id, name: previous)

            case let .moveRequest(fromCollectionID, id, toCollectionID, groupID, order):
                guard let source = workspace.location(collectionID: fromCollectionID, requestID: id),
                      workspace.collections.contains(where: { $0.id == toCollectionID }) else { return nil }
                _ = try await persistence.apply(.moveRequest(fromCollectionID: fromCollectionID, requestID: id,
                                                             toCollectionID: toCollectionID, groupID: groupID, order: order))
                if let index = workspace.collections.firstIndex(where: { $0.id == fromCollectionID }) {
                    workspace.collections[index].requests.removeAll { $0.request.id == id }
                }
                if let index = workspace.collections.firstIndex(where: { $0.id == toCollectionID }) {
                    workspace.collections[index].requests.append(RequestLocation(
                        collectionID: toCollectionID, groupID: groupID, order: order, request: source.request))
                }
                for session in sessions.sessions.values where session.collectionID == fromCollectionID && session.requestID == id {
                    session.relocate(to: toCollectionID)
                }
                rebuildRequestSearchIndex()
                return .moveRequest(fromCollectionID: toCollectionID, id: id, toCollectionID: fromCollectionID,
                                    groupID: source.groupID, order: source.order)

            case let .moveGroup(collectionID, id, parentID, order):
                guard let collection = workspace.collections.first(where: { $0.id == collectionID }),
                      let group = collection.groups.first(where: { $0.id == id }) else { return nil }
                _ = try await persistence.apply(.moveGroup(collectionID: collectionID, id: id, parentID: parentID, order: order))
                if let index = workspace.collections.firstIndex(where: { $0.id == collectionID }),
                   let groupIndex = workspace.collections[index].groups.firstIndex(where: { $0.id == id }) {
                    workspace.collections[index].groups[groupIndex].parentID = parentID
                    workspace.collections[index].groups[groupIndex].order = order
                }
                return .moveGroup(collectionID: collectionID, id: id, parentID: group.parentID, order: group.order)

            case let .reorder(collectionID, parentID, items):
                guard let collection = workspace.collections.first(where: { $0.id == collectionID }) else { return nil }
                let previous = collection.orderedChildren(parentID: parentID)
                guard items != previous, Set(items) == Set(previous) else { return nil }
                _ = try await persistence.apply(.reorderChildren(collectionID: collectionID, parentID: parentID, items: items))
                guard let index = workspace.collections.firstIndex(where: { $0.id == collectionID }) else { return nil }
                let orders = Dictionary(uniqueKeysWithValues: items.enumerated().map { ($0.element, $0.offset) })
                for i in workspace.collections[index].groups.indices {
                    if let order = orders["group:" + workspace.collections[index].groups[i].id] {
                        workspace.collections[index].groups[i].order = order
                    }
                }
                for i in workspace.collections[index].requests.indices {
                    if let order = orders["request:" + workspace.collections[index].requests[i].request.id] {
                        workspace.collections[index].requests[i].order = order
                    }
                }
                return .reorder(collectionID: collectionID, parentID: parentID, items: previous)
            }
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
            return nil
        }
    }

    /// Saves each request, then indexes once; a restored subtree can hold many requests.
    private func restoreRequests(_ requests: [RequestLocation], in collectionID: String) async throws {
        guard let persistence, !requests.isEmpty else { return }
        for location in requests {
            _ = try await persistence.apply(.saveRequest(collectionID: collectionID, location: location))
            guard let index = workspace.collections.firstIndex(where: { $0.id == collectionID }) else { return }
            workspace.collections[index].requests.append(location)
        }
        rebuildRequestSearchIndex()
    }
}

/// Menu availability derived from the model. Values are stored and only written when
/// they change, so menus are not rebuilt on every keystroke in the URL or name field.
@MainActor
@Observable
public final class WorkspaceCommandState {
    public private(set) var hasActiveSession = false
    public private(set) var activeKind: DocumentKind?
    public private(set) var socketStatus = WebSocketStatus.disconnected
    public private(set) var hasURL = false
    public private(set) var hasName = false
    public private(set) var isRunning = false
    public private(set) var canGoBack = false
    public private(set) var canGoForward = false
    public private(set) var tabCount = 0
    public private(set) var hasCollections = false

    @ObservationIgnored private weak var model: WireboltModel?

    public init(model: WireboltModel) {
        self.model = model
        track()
    }

    private func track() {
        withObservationTracking {
            update()
        } onChange: { [weak self] in
            Task { @MainActor in self?.track() }
        }
    }

    private func update() {
        guard let model else { return }
        let session = model.sessions.activeSession
        let group = model.sessions.activeGroup
        assign(\.hasActiveSession, session != nil)
        assign(\.activeKind, session?.kind)
        assign(\.socketStatus, session?.socket.status ?? .disconnected)
        assign(\.hasURL, session.map { !$0.draft.url.isEmpty } ?? false)
        assign(\.hasName, session.map { !$0.draft.name.isEmpty } ?? false)
        assign(\.isRunning, session?.isRunning ?? false)
        assign(\.canGoBack, group.map { !$0.backwardTabIDs.isEmpty } ?? false)
        assign(\.canGoForward, group.map { !$0.forwardTabIDs.isEmpty } ?? false)
        assign(\.tabCount, group?.tabIDs.count ?? 0)
        assign(\.hasCollections, !model.workspace.collections.isEmpty)
    }

    private func assign<Value: Equatable>(_ keyPath: ReferenceWritableKeyPath<WorkspaceCommandState, Value>, _ value: Value) {
        if self[keyPath: keyPath] != value { self[keyPath: keyPath] = value }
    }
}

public extension ImportFormat {
    /// Infers the importer for a file opened from Finder or dropped on the window.
    static func detect(fileExtension: String, contents: String) -> ImportFormat? {
        let trimmed = contents.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.hasPrefix("curl ") || trimmed.hasPrefix("curl\t") { return .curl }
        guard let data = trimmed.data(using: .utf8),
              let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        else { return nil }
        if let log = root["log"] as? [String: Any], log["entries"] is [Any] { return .har }
        if let info = root["info"] as? [String: Any], info["schema"] != nil || info["_postman_id"] != nil {
            return .postmanV2
        }
        if root["version"] as? Int == 1, root["nodes"] is [Any] { return .legacyWorkspaceV1 }
        return fileExtension.lowercased() == "har" ? .har : nil
    }
}
