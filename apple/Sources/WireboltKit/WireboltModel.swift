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
}

public extension WorkspacePersistence {
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
        didSet { sidebarSnapshot = nil; sidebarVisibleCache = nil }
    }
    @ObservationIgnored private var sidebarSnapshot: SidebarSnapshot?
    @ObservationIgnored private var sidebarVisibleCache: (query: String, collapsed: Set<String>, expanded: Set<String>, rows: [SidebarSnapshot.Row])?

    public func sidebarRows(query: String, collapsed: Set<String>, expanded: Set<String>) -> [SidebarSnapshot.Row] {
        let collections = workspace.collections
        if let cache = sidebarVisibleCache, cache.query == query, cache.collapsed == collapsed, cache.expanded == expanded { return cache.rows }
        if sidebarSnapshot == nil { sidebarSnapshot = SidebarSnapshot(collections: collections) }
        let rows = sidebarSnapshot!.visible(query: query, collapsedCollections: collapsed, expandedGroups: expanded)
        sidebarVisibleCache = (query, collapsed, expanded, rows)
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
    public var isShowingWorkspaceSettings = false
    public var settingsTab = "general"

    @ObservationIgnored private let socketConnector: (any WebSocketConnecting)?
    @ObservationIgnored private let runner: any RequestRunner
    @ObservationIgnored private var persistence: (any WorkspacePersistence)?
    @ObservationIgnored private var gitCollaboration: (any GitCollaboration)?
    @ObservationIgnored private let history: HistoryRepository
    @ObservationIgnored private let cookieJar: CookieJar
    @ObservationIgnored private let oauth2: any OAuth2Authorizing
    @ObservationIgnored private var requestSearchIndex: [String: String] = [:]
    @ObservationIgnored private var rootCreation: Task<Void, any Error>?
    @ObservationIgnored private var transportSave: Task<Void, Never>?
    @ObservationIgnored private var persistedTransport: TransportSettings?
    public private(set) var exportFailureMessage: String?
    public var operationFailure: RunFailure?

    public init(
        runner: any RequestRunner,
        socketConnector: (any WebSocketConnecting)? = nil,
        persistence: (any WorkspacePersistence)? = nil,
        gitCollaboration: (any GitCollaboration)? = nil,
        sessions: DocumentSessionStore? = nil,
        history: HistoryRepository = HistoryRepository(),
        cookieJar: CookieJar = CookieJar(
            storageURL: FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appending(path: "Wirebolt/Cookies/cookies.json")
        ),
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
                    guard session.activeRunID == runID else { return }
                    session.setResponseCookies(session.responseCookies + received)
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

    public func select(_ location: RequestLocation) {
        _ = sessions.open(
            draft: location.request,
            collectionID: location.collectionID
        )
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
        guard let persistence else { return nil }
        let collectionID = collectionID ?? WorkspaceDraft.rootCollectionID
        do {
            if collectionID == WorkspaceDraft.rootCollectionID { try await ensureRootCollection() }
            guard let collection = workspace.collections.first(where: { $0.id == collectionID }) else { return nil }
            let draft = RequestDraft(id: UUID().uuidString.lowercased(),
                name: kind == .http ? "Untitled Request" : "Untitled WebSocket Request", webSocket: kind == .webSocket)
            let location = RequestLocation(collectionID: collectionID, groupID: groupID,
                order: max(Int.min + 1, min(0, collection.requests.map(\.order).min() ?? 0)) - 1, request: draft)
            _ = try await persistence.apply(.saveRequest(collectionID: collectionID, location: location))
            applySavedRequest(location)
            return sessions.open(draft: draft, collectionID: collectionID, kind: kind)
        } catch { operationFailure = RunFailure(kind: "workspace", issues: []); return nil }
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
        guard let persistence else { return }
        let collection = CollectionDraft(
            id: Self.documentID(from: name, fallback: "collection"),
            name: name,
            order: workspace.collections.count
        )
        do {
            _ = try await persistence.apply(.createCollection(collection))
            workspace.collections.append(collection)
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func renameRequest(collectionID: String, requestID: String, name: String) async {
        guard let persistence,
              var location = workspace.location(collectionID: collectionID, requestID: requestID)
        else { return }
        location.request.name = name
        do {
            _ = try await persistence.apply(.saveRequest(collectionID: collectionID, location: location))
            applySavedRequest(location)
            for session in sessions.sessions.values where session.collectionID == collectionID && session.requestID == requestID {
                session.renameSavedRequest(name)
            }
        } catch { operationFailure = RunFailure(kind: "workspace", issues: []) }
    }

    public func renameCollection(id: String, name: String) async {
        guard let persistence,
              let index = workspace.collections.firstIndex(where: { $0.id == id })
        else { return }
        do {
            _ = try await persistence.apply(.renameCollection(id: id, name: name))
            workspace.collections[index].name = name
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func deleteCollection(id: String) async {
        guard let persistence else { return }
        do {
            _ = try await persistence.apply(.deleteCollection(id: id))
            workspace.collections.removeAll { $0.id == id }
            closeSessions(collectionID: id)
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    @discardableResult
    public func createGroup(collectionID: String, parentID: String? = nil, name: String = "New Folder") async -> String? {
        guard let persistence,
              let collectionIndex = workspace.collections.firstIndex(where: { $0.id == collectionID })
        else { return nil }
        let siblings = workspace.collections[collectionIndex].groups.filter { $0.parentID == parentID }
        let group = GroupDraft(
            id: Self.documentID(from: name, fallback: "group"),
            name: name,
            parentID: parentID,
            order: siblings.count
        )
        do {
            _ = try await persistence.apply(.createGroup(collectionID: collectionID, group: group))
            workspace.collections[collectionIndex].groups.append(group)
            return group.id
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
            return nil
        }
    }

    public func renameGroup(collectionID: String, id: String, name: String) async {
        guard let persistence,
              let collectionIndex = workspace.collections.firstIndex(where: { $0.id == collectionID }),
              let groupIndex = workspace.collections[collectionIndex].groups.firstIndex(where: { $0.id == id })
        else { return }
        do {
            _ = try await persistence.apply(.renameGroup(collectionID: collectionID, id: id, name: name))
            workspace.collections[collectionIndex].groups[groupIndex].name = name
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func deleteGroup(collectionID: String, id: String) async {
        guard let persistence,
              let collectionIndex = workspace.collections.firstIndex(where: { $0.id == collectionID })
        else { return }
        do {
            _ = try await persistence.apply(.deleteGroup(collectionID: collectionID, id: id))
            let descendantIDs = workspace.collections[collectionIndex].descendantGroupIDs(of: id)
            let removedRequestIDs = workspace.collections[collectionIndex].requests.filter {
                $0.groupID.map(descendantIDs.contains) ?? false
            }.map { $0.request.id }
            for requestID in removedRequestIDs { closeSessions(collectionID: collectionID, requestID: requestID) }
            workspace.collections[collectionIndex].groups.removeAll { descendantIDs.contains($0.id) }
            workspace.collections[collectionIndex].requests.removeAll { location in
                location.groupID.map(descendantIDs.contains) ?? false
            }
            rebuildRequestSearchIndex()
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func deleteRequest(collectionID: String, requestID: String) async {
        guard let persistence,
              let collectionIndex = workspace.collections.firstIndex(where: { $0.id == collectionID })
        else { return }
        do {
            _ = try await persistence.apply(.deleteRequest(collectionID: collectionID, id: requestID))
            workspace.collections[collectionIndex].requests.removeAll { $0.request.id == requestID }
            rebuildRequestSearchIndex()
            closeSessions(collectionID: collectionID, requestID: requestID)
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func duplicateRequest(collectionID: String, requestID: String) async {
        guard let persistence,
              let collectionIndex = workspace.collections.firstIndex(where: { $0.id == collectionID }),
              let source = workspace.collections[collectionIndex].requests.first(where: {
                  $0.request.id == requestID
              })
        else { return }
        let newID = Self.documentID(from: "\(source.request.id)-copy", fallback: "request")
        let name = "\(source.request.name) Copy"
        do {
            _ = try await persistence.apply(.duplicateRequest(
                collectionID: collectionID,
                id: requestID,
                newID: newID,
                name: name
            ))
            var copy = source
            copy.request.id = newID
            copy.request.name = name
            copy.order = workspace.collections[collectionIndex].requests.count
            workspace.collections[collectionIndex].requests.append(copy)
            rebuildRequestSearchIndex()
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
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
        guard !isReorderingSidebar, let persistence,
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
        do {
            _ = try await persistence.apply(.reorderChildren(collectionID: collectionID, parentID: parentID, items: items))
            guard let index = workspace.collections.firstIndex(where: { $0.id == collectionID }) else { return }
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
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func moveRequest(
        fromCollectionID: String,
        requestID: String,
        toCollectionID: String,
        groupID: String?,
        order: Int
    ) async {
        guard let persistence,
              let sourceCollectionIndex = workspace.collections.firstIndex(where: {
                  $0.id == fromCollectionID
              }),
              let sourceRequestIndex = workspace.collections[sourceCollectionIndex].requests.firstIndex(where: {
                  $0.request.id == requestID
              }),
              let destinationIndex = workspace.collections.firstIndex(where: { $0.id == toCollectionID })
        else { return }
        do {
            _ = try await persistence.apply(.moveRequest(
                fromCollectionID: fromCollectionID,
                requestID: requestID,
                toCollectionID: toCollectionID,
                groupID: groupID,
                order: order
            ))
            var location = workspace.collections[sourceCollectionIndex].requests.remove(
                at: sourceRequestIndex
            )
            location = RequestLocation(
                collectionID: toCollectionID,
                groupID: groupID,
                order: order,
                request: location.request
            )
            let resolvedDestinationIndex = workspace.collections.firstIndex(where: {
                $0.id == toCollectionID
            }) ?? destinationIndex
            workspace.collections[resolvedDestinationIndex].requests.append(location)
            for session in sessions.sessions.values where session.collectionID == fromCollectionID && session.requestID == requestID {
                session.relocate(to: toCollectionID)
            }
            rebuildRequestSearchIndex()
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func moveGroup(
        collectionID: String,
        id: String,
        parentID: String?,
        order: Int
    ) async {
        guard let persistence,
              let collectionIndex = workspace.collections.firstIndex(where: { $0.id == collectionID }),
              let groupIndex = workspace.collections[collectionIndex].groups.firstIndex(where: { $0.id == id })
        else { return }
        do {
            _ = try await persistence.apply(.moveGroup(
                collectionID: collectionID,
                id: id,
                parentID: parentID,
                order: order
            ))
            workspace.collections[collectionIndex].groups[groupIndex].parentID = parentID
            workspace.collections[collectionIndex].groups[groupIndex].order = order
        } catch {
            operationFailure = RunFailure(kind: "workspace", issues: [])
        }
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
        } catch {
            gitFailure = GitFailure(
                kind: "workspace_reload",
                reason: "Git updated the workspace, but Wirebolt could not reload it."
            )
        }
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
