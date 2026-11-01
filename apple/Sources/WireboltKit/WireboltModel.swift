import Foundation
import Observation

public protocol RequestRunner: Sendable {
    var sessionTransport: AppRuntimeTransport { get }
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error>
    func cancel(runID: RunID)
    func proxySettingsChanged()
    func resolveValues(_ values: [ValueSource], variables: [String: ValueSource]) async throws -> [String]
}

public extension RequestRunner {
    var sessionTransport: AppRuntimeTransport { AppRuntimeTransport.foreign(platform: RuntimeRunnerAdapter(runner: self)) }
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
    func loadSession(_ application: AppWorkspaceSession) async throws -> WorkspaceDraft
    func save(request: RequestDraft, in collectionID: String) async throws
    func save(environment: EnvironmentDraft) async throws
    func saveSecret(name: String, value: String) async throws
    func readSecret(name: String) async throws -> String?
    /// Removes a Keychain item. Deleting a missing item succeeds.
    func deleteSecret(name: String) async throws
    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta
    func applySessionCommands(_ commands: [SessionWorkspaceCommand]) async -> SessionCommandBatchResult
    func previewImport(format: ImportFormat, source: String) async throws -> ImportPreview
        func commitImport(format: ImportFormat, source: String) async throws -> ImportResult
    func commitImportFile(format: ImportFormat, source: String, name: String) async throws -> ImportResult
    func exportCollection(id: String) async throws -> String
    func exportWorkspace() async throws -> String
    func exportRequest(collectionID: String, id: String) async throws -> String
    /// The workspace folder, used to key local runtime storage such as cookies.
    var location: URL? { get }
    var sessionSecretPlatform: AppSecretPlatform { get }
    var runtimeWorkspaceBridge: WorkspaceBridge? { get }
}

public extension WorkspacePersistence {
    func loadSession(_ application: AppWorkspaceSession) async throws -> WorkspaceDraft {
        let workspace = try await load(); application.replace(workspace: workspace.domain); return workspace
    }
    var location: URL? { nil }
    var runtimeWorkspaceBridge: WorkspaceBridge? { nil }
    var sessionSecretPlatform: AppSecretPlatform { AppSecretPlatform.foreign(platform: SessionSecretAdapter(persistence: self)) }
    func readSecret(name _: String) async throws -> String? { nil }
    func deleteSecret(name _: String) async throws {}
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

    func applySessionCommands(_ commands: [SessionWorkspaceCommand]) async -> SessionCommandBatchResult {
        var completed: UInt64 = 0
        do {
            for command in commands { _ = try await apply(WorkspaceCommand(command)); completed += 1 }
            return SessionCommandBatchResult(completed: completed, failureReason: nil)
        } catch { return SessionCommandBatchResult(completed: completed, failureReason: error.localizedDescription) }
    }

    func previewImport(format _: ImportFormat, source _: String) async throws -> ImportPreview {
        throw WorkspaceMutationError.unsupported
    }

        func commitImport(format _: ImportFormat, source _: String) async throws -> ImportResult {
        throw WorkspaceMutationError.unsupported
    }

    func commitImportFile(format: ImportFormat, source: String, name _: String) async throws -> ImportResult {
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
    func abortMerge() async throws -> GitOperationSnapshot
    func initializeRepository() async throws -> GitStatusSnapshot
}

public extension GitCollaboration {
    func abortMerge() async throws -> GitOperationSnapshot {
        throw GitFailure(kind: "unsupported", reason: "This workspace cannot abort a merge.")
    }

    func initializeRepository() async throws -> GitStatusSnapshot {
        throw GitFailure(kind: "unsupported", reason: "This workspace cannot create a Git repository.")
    }
}

@MainActor
@Observable
public final class WireboltModel {
    public static let previewByteLimit = DocumentSession.previewByteLimit

    public var workspace = WorkspaceDraft(name: "Wirebolt") {
        didSet { sidebarSnapshot = nil; sidebarVisibleCache = nil; sidebarUnfilteredCache = nil; if !mirroringWorkspace { workspaceNeedsSynchronization = true } }
    }
    @ObservationIgnored private let rustWorkspace = AppWorkspaceSession()
    @ObservationIgnored private var mirroringWorkspace = false
    @ObservationIgnored private var workspaceNeedsSynchronization = true
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
    public var selectedEnvironmentID: String? { didSet { runtime.selectEnvironment(id: selectedEnvironmentID) } }
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
    /// The last successful import, until the user dismisses its summary.
    public var importSummary: ImportSummary?
    public private(set) var historyEntries: [RunHistoryEntry] = []
    public private(set) var historyRevision = 0
    public struct RequestActivity: Equatable, Sendable {
        public let date: Date
        public let status: UInt16?
    }
    public private(set) var requestActivity: [String: RequestActivity] = [:]
    private var editedSecrets: [String: String] = [:]
    @ObservationIgnored private let runtime: AppSessionRuntime
    /// Keychain names Wirebolt generated that this workspace used during this session. Those
    /// no longer referenced are deleted only when the undo stack is discarded (switching
    /// workspace, reloading after a pull, quitting), so an undone deletion still finds its value.
    @ObservationIgnored private let rustSecrets = AppSessionSecrets()
    /// Other workspace folders this Mac knows about (Open Recent, the built-in workspace).
    /// A Keychain item one of them still references is never deleted.
    @ObservationIgnored public var knownWorkspaceLocations: @MainActor () -> [URL] = { [] }
    public private(set) var oauthReceipts: [String: OAuth2TokenReceipt] = [:]
    public private(set) var oauthFailureMessage: String?
    public private(set) var isOAuthBusy = false
    public var isShowingGitCollaboration = false
    /// The open workspace folder, when the persistence has one.
    public private(set) var workspaceLocation: URL?
    /// Increases whenever the active cookie jar changes, so a cookie list can refresh.
    public private(set) var cookieRevision = 0
    public var isShowingWorkspaceSettings = false
    public var isShowingAppSettings = false
    public var settingsTab = "appearance"

    @ObservationIgnored private let socketConnector: (any WebSocketConnecting)?
    @ObservationIgnored private let runner: any RequestRunner
    @ObservationIgnored private var persistence: (any WorkspacePersistence)?
    @ObservationIgnored private var gitCollaboration: (any GitCollaboration)?
    /// Used while no workspace folder is known, for example in tests and previews.
    @ObservationIgnored private let cookieDirectory: URL?
    @ObservationIgnored private let oauth2: any OAuth2Authorizing
    @ObservationIgnored private var requestSearchIndex: [String: String] = [:]
    @ObservationIgnored private var rootCreation: Task<Void, any Error>?
    @ObservationIgnored private var transportSave: Task<Void, Never>?
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
        runtime = AppSessionRuntime(history: history.rust, cookies: cookieJar.rust)
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
        rustSecrets.resetEditor()
        workspaceLocation = persistence.location
        runtime.configureCookies(workspace: persistence.location?.standardizedFileURL.path, directory: cookieDirectory?.path)
        mirrorRuntime()

    }

    public var selectedRequestID: String? {
        guard let session = sessions.activeSession,
              let collectionID = session.collectionID
        else { return nil }
        return "\(collectionID)/\(session.requestID)"
    }

    /// Whether `location` is the active document's saved request. Observes only that
    /// request, so a selection change re-renders two sidebar rows rather than every row.
    public func isSelectedRequest(_ location: RequestLocation) -> Bool {
        sessions.isActiveRequest(collectionID: location.collectionID, requestID: location.request.id)
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
        runtime.activeVariables(environments: workspace.environments.map(\.domain)).mapValues(ValueSource.init(domain:))
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
        do {
            let input = try await rustSecrets.socketInput(draft: session.draft.domain, context: runtimeContext,
                platform: persistence?.sessionSecretPlatform, observer: runtimeObserver(for: session))
            mirrorSecretEdits()
            session.socket.connect(input: RunInput(domain: input), connector: socketConnector)
        } catch { mirrorSecretEdits(); operationFailure = RunFailure(kind: "keychain", issues: []) }
    }

    public func send(_ target: DocumentSession? = nil) async {
        let session = target ?? sessions.activeSession ?? sessions.openTemporary()
        switch sessionSendAction(kind: session.kind.rust, socket: session.socket.status.rust, running: session.isRunning) {
        case .socketSend: await session.socket.send(body: session.draft.body); return
        case .socketConnect: await connectWebSocket(session); return
        case .ignore: return
        case .http: break
        }
        operationFailure = nil
        session.synchronizeDraft()
        let runtime = runtime
        let failure = await withTaskCancellationHandler {
            await runtime.send(document: session.rust, options: RuntimeRunOptions(context: runtimeContext, id: RunID().description,
                directory: FileManager.default.temporaryDirectory.path), transport: runner.sessionTransport,
                observer: runtimeObserver(for: session, requireOpen: true),
                secrets: rustSecrets, platform: persistence?.sessionSecretPlatform)
        } onCancel: { runtime.cancel(document: session.rust) }
        if let failure { operationFailure = RunFailure(failure) }
        mirrorSecretEdits()
        session.mirror()
        mirrorRuntime()
    }

    public func cancel(_ target: DocumentSession? = nil) {
        guard let session = target ?? sessions.activeSession else { return }
        runtime.cancel(document: session.rust)
        session.mirror()
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
        guard case .oauth2 = session.draft.authentication, let persistence else { return }
        session.synchronizeDraft()
        await runtime.acquireToken(document: session.rust, secrets: rustSecrets, platform: persistence.sessionSecretPlatform,
            authorizer: RuntimeOAuthAdapter(authorizer: oauth2) { [weak session] in session?.draft.authentication = .oauth2(configuration: $0) },
            observer: runtimeObserver(for: session, oauth: true))
        mirrorOAuthRuntime()
        mirrorSecretEdits()
    }

    public func historyEntries(for session: DocumentSession) async -> [RunHistoryEntry] {
        await runtime.historyEntries(id: session.requestID).map(RunHistoryEntry.init)
    }
    public func historyCount(for session: DocumentSession) async -> Int {
        Int(clamping: await runtime.historyCount(id: session.requestID))
    }
    public func loadHistory(for session: DocumentSession) async {
        await runtime.loadHistory(id: session.requestID)
        mirrorRuntime()
    }
    public func restoreHistory(_ entry: RunHistoryEntry, into session: DocumentSession) async {
        await runtime.restoreHistory(entry: entry.sessionRecord, document: session.rust)
        session.mirror()
    }
    private func restoreLatestResponses(for restored: [DocumentSession]) {
        Task {
            for session in restored {
                guard sessions.session(id: session.id) === session else { continue }
                await runtime.restoreLatest(document: session.rust, observer: runtimeObserver(for: session, requireOpen: true))
                session.mirror()
            }
            mirrorRuntime()
        }
    }
    public func clearHistory(for session: DocumentSession) async {
        await runtime.clearHistory(id: session.requestID)
        mirrorRuntime()
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
            applyLoadedWorkspace(try await persistence.loadSession(rustWorkspace), restoring: layout)
            workspaceNeedsSynchronization = false
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
            let loaded = try await persistence.loadSession(rustWorkspace)
            for session in sessions.sessions.values { cancel(session) }
            sessions.removeAll()
            // The previous workspace's undo stack is discarded below, so its released
            // Keychain items can go.
            await purgeReleasedSecrets()
            rustSecrets.clearObserved()
            configurePersistence(persistence, gitCollaboration: gitCollaboration)
            transportSave = nil
            runtime.resetWorkspace()
            selectedEnvironmentID = nil
            oauthReceipts = [:]
            historyEntries = []
            requestActivity = [:]
            gitStatus = nil
            gitOperation = nil
            gitFailure = nil
            applyLoadedWorkspace(loaded, restoring: layout)
            workspaceNeedsSynchronization = false
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

    /// Abandons the merge a conflicted pull started and reloads the restored documents.
    public func abortGitMerge() async {
        await performGitOperation { collaboration in
            self.apply(try await collaboration.abortMerge())
            await self.reloadWorkspaceAfterGitUpdate()
        }
    }

    /// Runs `git init` in the workspace folder so it becomes the repository root.
    public func initializeGitRepository() async {
        await performGitOperation { collaboration in
            let status = try await collaboration.initializeRepository()
            self.gitOperation = nil
            self.gitStatus = status
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
    public func cookies() async -> [CookieSnapshot] { runtime.cookies().map(CookieSnapshot.init) }
    public func deleteCookie(id: CookieSnapshot.ID) async { runtime.deleteCookie(id: id); mirrorRuntime() }
    public func clearCookies() async { runtime.clearCookies(); mirrorRuntime() }

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
            let migrated = try await migratingLegacySecrets(session.draft)
            if migrated != session.draft { session.draft = migrated }
            observeSecrets(session.draft.secretReferences)
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
        if workspace.environments.first(where: { $0.id == environment.id }) == environment { return true }
        return await perform(.restoreEnvironment(environment, index: workspace.environments.firstIndex { $0.id == environment.id } ?? workspace.environments.count), named: "Edit Environment")
    }

    public func makeProxyConnectionTest() -> ProxyConnectionTest { ProxyConnectionTest(runner: runner) }

    public func effectiveProxy(for draft: RequestDraft) -> EffectiveProxy {
        .resolve(request: draft.proxy.document, workspace: workspace.proxy, app: proxyPreferences.configuration)
    }

    /// Validate and persist before publishing. Failed saves leave the active policy intact.
    public func applyProxy(_ configuration: ProxyDocument?, scope: ProxyScope,
                           session: DocumentSession? = nil, secrets: [String: String] = [:]) async throws {
        do {
            try await runtime.applyProxy(configuration: configuration?.domain, scope: scope.rust,
                hasSession: session != nil, secrets: secrets, secretPlatform: persistence?.sessionSecretPlatform,
                platform: runtimeSettings())
            switch scope {
            case .app: break
            case .workspace: workspace.proxy = configuration
            case .request: session?.draft.proxy = ProxySelection(document: configuration)
            }
            runner.proxySettingsChanged()
        } catch { throw runtimeError(error) }
    }

    public func updateWorkspaceTransport(_ value: TransportSettings) {
        guard persistence != nil else { return }
        let generation = runtime.updateTransport(previous: workspace.transport.domain, value: value.domain)
        workspace.transport = value
        let platform = runtimeSettings(), runtime = runtime
        transportSave = Task {
            if let result = await runtime.savePendingTransport(generation: generation, platform: platform) {
                if workspace.transport != TransportSettings(domain: result.value) { workspace.transport = TransportSettings(domain: result.value) }
                if result.failed { operationFailure = RunFailure(kind: "workspace", issues: []) }
            }
        }
    }
    public func flushWorkspaceTransport() async { await transportSave?.value }
    public func saveWorkspaceTransport() async {
        guard persistence != nil else { return }
        do { try await runtime.saveTransport(value: workspace.transport.domain, platform: runtimeSettings()) }
        catch { operationFailure = RunFailure(kind: "workspace", issues: []) }
    }

    @discardableResult
    public func deleteEnvironment(id: String) async -> Bool {
        await perform(.removeEnvironment(id: id), named: "Delete Environment")
    }

    /// Explicit export keeps Keychain references in the generated command.
    public func curlCommand(for draft: RequestDraft) async -> String? {
        let context = runtimeContext
        do {
            try await flushSecrets()
            return try await runtime.curlCommand(draft: draft.domain, context: context, transport: runner.sessionTransport)
        } catch let RuntimeRequestError.Failure(failure) {
            operationFailure = RunFailure(failure)
            return nil
        } catch { operationFailure = (error as? RunFailure) ?? RunFailure(kind: "export", issues: []); return nil }
    }
    public func secretMaterial(for source: ValueSource) -> String {
        _ = editedSecrets
        return rustSecrets.material(source: source.domain)
    }
    public func editSecret(name: String, value: String) {
        rustSecrets.edit(name: name, value: value)
        mirrorSecretEdits()
    }
    public func loadSecret(_ source: ValueSource) async {
        guard case let .secret(name) = source, let persistence else { return }
        do { _ = try await rustSecrets.load(name: name, platform: persistence.sessionSecretPlatform); mirrorSecretEdits() }
        catch { operationFailure = RunFailure(kind: "keychain", issues: []) }
    }
    private func flushSecrets() async throws {
        guard let persistence else { if !rustSecrets.dirty().isEmpty { throw WorkspaceMutationError.unsupported }; return }
        defer { mirrorSecretEdits() }
        try await rustSecrets.flush(platform: persistence.sessionSecretPlatform)
    }

    private func observeSecrets(_ names: some Sequence<String>) {
        rustSecrets.observe(names: Array(names))
    }

    /// Whether quitting may have Keychain items to clean up.
    public var hasReleasableSecrets: Bool { rustSecrets.hasObserved() }

    /// Deletes Keychain items Wirebolt generated for requests and variables that no saved
    /// document or open tab references any more. Call only when the undo stack is being
    /// discarded; items of deletions that can still be undone must stay.
    public func purgeReleasedSecrets() async {
        guard let persistence, rustSecrets.hasObserved() else { return }
        let documents = sessions.sessions.values.map { session in session.synchronizeDraft(); return session.rust.document() }
        await rustSecrets.purgeWorkspace(workspace: workspace.domain, documents: documents,
            current: workspaceLocation?.standardizedFileURL.path,
            others: knownWorkspaceLocations().map(\.standardizedFileURL.path), platform: persistence.sessionSecretPlatform)
        mirrorSecretEdits()
    }
    private func migratingLegacySecrets(_ draft: RequestDraft) async throws -> RequestDraft {
        guard let persistence else { throw WorkspaceMutationError.unsupported }
        defer { mirrorSecretEdits() }
        return try await RequestDraft(domain: rustSecrets.migrate(draft: draft.domain, platform: persistence.sessionSecretPlatform))
    }

    /// Copies Keychain values from old to new names. Missing values stay missing.
    private func copySecrets(_ renames: [String: String]) async throws {
        guard let persistence else { throw WorkspaceMutationError.unsupported }
        do { _ = try await rustSecrets.copy(renames: renames, platform: persistence.sessionSecretPlatform) }
        catch { mirrorSecretEdits(); throw error }
        mirrorSecretEdits()
    }

    /// Writes secret values straight to Keychain, for editors with their own Save button.
    /// The in-memory copy is updated so other editors show the new value.
    @discardableResult
    public func saveSecrets(_ values: [String: String]) async -> Bool {
        guard !values.isEmpty else { return true }
        guard let persistence else { return false }
        do {
            try await rustSecrets.save(values: values, platform: persistence.sessionSecretPlatform)
            mirrorSecretEdits(); return true
        } catch { mirrorSecretEdits(); operationFailure = RunFailure(kind: "keychain", issues: []); return false }
    }

    public func saveSecret(name: String, value: String) async {
        guard let persistence else { return }
        do {
            try await rustSecrets.saveMaterial(name: name, value: value, platform: persistence.sessionSecretPlatform)
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
            await importFile(source: source, format: format, url: url)
        } catch {
            importFailureMessage = ImportFormat.unreadableMessage(fileName: url.lastPathComponent)
        }
    }

    /// Imports contents already read from `url`, naming the collection after the file.
    public func importDocument(source: String, format: ImportFormat, url: URL) async {
        guard !isImporting, persistence != nil else { return }
        isImporting = true
        importFailureMessage = nil
        defer { isImporting = false }
        await importFile(source: source, format: format, url: url)
    }

    public func importDocument(source: String, format: ImportFormat) async {
        guard !isImporting, persistence != nil else { return }
        isImporting = true
        importFailureMessage = nil
        defer { isImporting = false }
        importFailureMessage = await commitDocument(source: source, format: format)
    }

    /// Imports a Bruno collection folder, or a single `.bru` request file.
    public func importBrunoCollection(at url: URL) async {
        guard !isImporting, persistence != nil else { return }
        isImporting = true
        importFailureMessage = nil
        defer { isImporting = false }
        do {
            let source = try await Task.detached(priority: .userInitiated) {
                try BrunoCollectionSource.bundle(at: url)
            }.value
            importFailureMessage = await commitDocument(
                source: source,
                format: .brunoFolder,
                fileName: url.deletingPathExtension().lastPathComponent,
                displayName: url.lastPathComponent
            )
        } catch BrunoCollectionSource.ReadError.tooLarge {
            importFailureMessage = "“\(url.lastPathComponent)” is too large to import."
        } catch {
            importFailureMessage = "“\(url.lastPathComponent)” isn’t a Bruno collection or couldn’t be read."
        }
    }

    /// Imports pasted text and returns why it failed instead of raising the window alert,
    /// so a sheet can show the reason in place.
    public func importPastedDocument(source: String, format: ImportFormat) async -> String? {
        guard !isImporting, persistence != nil else { return nil }
        isImporting = true
        defer { isImporting = false }
        return await commitDocument(source: source, format: format)
    }

    private func importFile(source: String, format: ImportFormat, url: URL) async {
        importFailureMessage = await commitDocument(
            source: source,
            format: format,
            fileName: url.deletingPathExtension().lastPathComponent,
            displayName: url.lastPathComponent
        )
    }

    /// Commits an import and returns a failure message, or `nil` after success.
    private func commitDocument(
        source: String,
        format: ImportFormat,
        fileName: String? = nil,
        displayName: String? = nil
    ) async -> String? {
        guard let persistence else { return nil }
        await flushWorkspaceTransport()
        let previousIDs = Set(workspace.collections.map(\.id))
        let previousEnvironmentID = selectedEnvironmentID
        do {
            let result = if let fileName {
                try await persistence.commitImportFile(format: format, source: source, name: fileName)
            } else {
                try await persistence.commitImport(format: format, source: source)
            }
            applyLoadedWorkspace(try await persistence.load())
            // Keep the user's environment; otherwise select the one the import created
            // so its variables resolve right away.
            let environmentIDs = Set(workspace.environments.map(\.id))
            if let previousEnvironmentID, environmentIDs.contains(previousEnvironmentID) {
                selectedEnvironmentID = previousEnvironmentID
            } else if let created = result.summary.environments.first(where: { environmentIDs.contains($0.id) }) {
                selectedEnvironmentID = created.id
            }
            importSummary = result.summary
            if let imported = workspace.collections.first(where: { !previousIDs.contains($0.id) }),
               let request = imported.requests.first { select(request) }
            return nil
        } catch {
            return ImportFormat.failureMessage(for: error, fileName: displayName)
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
        runtime.resetTransport()
        observeSecrets(loaded.secretReferences)
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

    public func deleteConsequences(_ target: SessionDeleteTarget) -> String {
        if workspaceNeedsSynchronization { rustWorkspace.replace(workspace: workspace.domain); workspaceNeedsSynchronization = false }
        return rustWorkspace.deleteConsequences(target: target, documents: sessions.sessions.values.map(\.tabProjection))
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
            await purgeReleasedSecrets()
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
    case restoreEnvironment(EnvironmentDraft, index: Int)
    case removeEnvironment(id: String)
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
            // Credential copies are platform effects; their naming policy is already Rust.
            var ownAuthentication: RequestAuthentication?
            var renames: [String: String] = [:]
            if case let .duplicateRequest(collectionID, id, _, _) = step,
               let source = workspace.location(collectionID: collectionID, requestID: id) {
                try await flushSecrets()
                let own = source.request.authentication.withOwnKeychainNames(onlyLegacy: false)
                ownAuthentication = own.0; renames = own.1; try await copySecrets(renames)
            }
            if workspaceNeedsSynchronization { rustWorkspace.replace(workspace: workspace.domain); workspaceNeedsSynchronization = false }
            guard var plan = rustWorkspace.plan(selected: selectedEnvironmentID, step: step.sessionRecord) else { return nil }
            observeSecrets(plan.observedSecrets)
            let outcome = await persistence.applySessionCommands(plan.commands)
            let failed = outcome.failureReason != nil
            if failed {
                guard let partial = rustWorkspace.partial(plan: plan, completed: outcome.completed) else {
                    operationFailure = RunFailure(kind: "workspace", issues: []); return nil
                }
                plan = partial
            }
            if case let .duplicateRequest(collectionID, id, newID, _) = step, !renames.isEmpty,
               let authentication = ownAuthentication, let index = plan.collections.firstIndex(where: { $0.id == collectionID }),
               let requestIndex = plan.collections[index].requests.firstIndex(where: { $0.request.id == newID }) {
                plan.collections[index].requests[requestIndex].request.authentication = authentication.domain
                var persisted = RequestLocation(domain: plan.collections[index].requests[requestIndex])
                persisted.order = (workspace.location(collectionID: collectionID, requestID: id)?.order ?? 0) + 1
                _ = try await persistence.apply(.saveRequest(collectionID: collectionID, location: persisted))
            }
            rustWorkspace.commit(plan: plan)
            mirroringWorkspace = true
            defer { mirroringWorkspace = false }
            workspace.collections.removeAll { plan.removedCollectionIds.contains($0.id) }
            for collection in plan.collections {
                let projected = CollectionDraft(domain: collection)
                if let index = workspace.collections.firstIndex(where: { $0.id == collection.id }) {
                    if workspace.collections[index] != projected { workspace.collections[index] = projected }
                } else { workspace.collections.append(projected) }
            }
            if let environments = plan.environments { workspace.environments = environments.map(EnvironmentDraft.init(domain:)) }
            if plan.changesEnvironmentSelection { selectedEnvironmentID = plan.selectedEnvironmentId }
            observeSecrets(plan.observedSecrets)
            for effect in plan.effects {
                switch effect {
                case let .close(collectionID, requestID): closeSessions(collectionID: collectionID, requestID: requestID)
                case let .rename(collectionID, requestID, name):
                    for session in sessions.sessions.values where session.collectionID == collectionID && session.requestID == requestID { session.renameSavedRequest(name) }
                case let .relocate(collectionID, requestID, destination):
                    for session in sessions.sessions.values where session.collectionID == collectionID && session.requestID == requestID { session.relocate(to: destination) }
                    sessions.refreshActiveRequest()
                }
            }
            rebuildRequestSearchIndex()
            if failed { operationFailure = RunFailure(kind: "workspace", issues: []); return nil }
            return WorkspaceEditStep(plan.inverse)
        } catch { operationFailure = RunFailure(kind: "workspace", issues: []); return nil }
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
    /// The name used in menus and messages.
    var displayName: String {
        switch self {
        case .curl: "cURL"
        case .har: "HAR"
        case .legacyWorkspaceV1: "Wirebolt JSON"
        case .postmanV2: "Postman Collection v2"
        case .insomnia: "Insomnia"
        case .bruno: "Bruno collection"
        case .brunoFolder: "Bruno collection folder"
        }
    }

    /// Explains a failed import with the importer's reason when it has one.
    static func failureMessage(for error: any Error, fileName: String?) -> String {
        let subject = fileName.map { "“\($0)” couldn’t be imported." } ?? "The import failed."
        guard let reason = (error as? LocalizedError)?.errorDescription, !reason.isEmpty else {
            return "\(subject) Check its format and contents."
        }
        return "\(subject) \(sentence(reason))"
    }

    static func unreadableMessage(fileName: String) -> String {
        "“\(fileName)” couldn’t be read. Check that it’s a UTF-8 text file you have permission to open."
    }

    /// Explains why dropped or opened contents matched no importer.
    static func unrecognizedMessage(fileName: String, contents: String) -> String {
        if isOpenAPI(contents) {
            return "“\(fileName)” is an OpenAPI or Swagger document. OpenAPI import isn’t supported yet."
        }
        let formats = ListFormatter.localizedString(byJoining: allCases.filter { $0 != .brunoFolder }.map(\.displayName))
        return "“\(fileName)” isn’t a format Wirebolt can import. Supported formats: \(formats)."
    }

    private static func isOpenAPI(_ contents: String) -> Bool {
        let head = contents.prefix(4_096)
        if head.contains("\"openapi\"") || head.contains("\"swagger\"") { return true }
        return head.split(whereSeparator: \.isNewline).prefix(40).contains { line in
            line.hasPrefix("openapi:") || line.hasPrefix("swagger:")
        }
    }

    /// Bridge reasons may be lowercase phrases; show them as sentences.
    private static func sentence(_ reason: String) -> String {
        var text = reason.trimmingCharacters(in: .whitespacesAndNewlines)
        if let first = text.first, first.isLowercase { text = first.uppercased() + text.dropFirst() }
        if let last = text.last, !".!?".contains(last) { text += "." }
        return text
    }

    /// Infers the importer for a file opened from Finder or dropped on the window.
    static func detect(fileExtension: String, contents: String) -> ImportFormat? {
        domainDetectImport(extension: fileExtension, contents: contents).flatMap(ImportFormat.init(rawValue:))
    }
}

/// Finds Keychain reference names in workspace folders on disk without opening them as
/// workspaces: only request and environment documents are read, as plain text.
public enum WorkspaceSecretScan {
    public nonisolated static func referencedNames(_ candidates: Set<String>, in workspaces: [URL]) -> Set<String> {
        Set(sessionScanSecrets(candidates: Array(candidates), workspaces: workspaces.map(\.path)))
    }
}

extension WorkspaceEditStep {
    var sessionRecord: SessionWorkspaceEdit {
        switch self {
        case let .restoreEnvironment(value, index): .restoreEnvironment(environment: value.domain, index: UInt64(max(0, index)))
        case let .removeEnvironment(id): .removeEnvironment(id: id)
        case let .restoreRequest(value): .restoreRequest(location: value.domain)
        case let .removeRequest(collection, id): .removeRequest(collectionId: collection, id: id)
        case let .duplicateRequest(collection, id, newID, name): .duplicateRequest(collectionId: collection, id: id, newId: newID, name: name)
        case let .restoreGroups(collection, groups, requests): .restoreGroups(collectionId: collection, groups: groups.map(\.domain), requests: requests.map(\.domain))
        case let .removeGroup(collection, id): .removeGroup(collectionId: collection, id: id)
        case let .restoreCollection(value): .restoreCollection(collection: value.domain)
        case let .removeCollection(id): .removeCollection(id: id)
        case let .renameRequest(collection, id, name): .renameRequest(collectionId: collection, id: id, name: name)
        case let .renameGroup(collection, id, name): .renameGroup(collectionId: collection, id: id, name: name)
        case let .renameCollection(id, name): .renameCollection(id: id, name: name)
        case let .moveRequest(from, id, to, group, order): .moveRequest(fromCollectionId: from, id: id, toCollectionId: to, groupId: group, order: Int64(order))
        case let .moveGroup(collection, id, parent, order): .moveGroup(collectionId: collection, id: id, parentId: parent, order: Int64(order))
        case let .reorder(collection, parent, items): .reorder(collectionId: collection, parentId: parent, items: items)
        }
    }
    init(_ record: SessionWorkspaceEdit) {
        switch record {
        case let .restoreEnvironment(value, index): self = .restoreEnvironment(EnvironmentDraft(domain: value), index: Int(index))
        case let .removeEnvironment(id): self = .removeEnvironment(id: id)
        case let .restoreRequest(value): self = .restoreRequest(RequestLocation(domain: value))
        case let .removeRequest(collection, id): self = .removeRequest(collectionID: collection, id: id)
        case let .duplicateRequest(collection, id, newID, name): self = .duplicateRequest(collectionID: collection, id: id, newID: newID, name: name)
        case let .restoreGroups(collection, groups, requests): self = .restoreGroups(collectionID: collection, groups: groups.map(GroupDraft.init(domain:)), requests: requests.map(RequestLocation.init(domain:)))
        case let .removeGroup(collection, id): self = .removeGroup(collectionID: collection, id: id)
        case let .restoreCollection(value): self = .restoreCollection(CollectionDraft(domain: value))
        case let .removeCollection(id): self = .removeCollection(id: id)
        case let .renameRequest(collection, id, name): self = .renameRequest(collectionID: collection, id: id, name: name)
        case let .renameGroup(collection, id, name): self = .renameGroup(collectionID: collection, id: id, name: name)
        case let .renameCollection(id, name): self = .renameCollection(id: id, name: name)
        case let .moveRequest(from, id, to, group, order): self = .moveRequest(fromCollectionID: from, id: id, toCollectionID: to, groupID: group, order: Int(order))
        case let .moveGroup(collection, id, parent, order): self = .moveGroup(collectionID: collection, id: id, parentID: parent, order: Int(order))
        case let .reorder(collection, parent, items): self = .reorder(collectionID: collection, parentID: parent, items: items)
        }
    }
}
extension WorkspaceCommand {
    init(_ record: SessionWorkspaceCommand) {
        switch record {
        case let .saveWorkspaceProxy(value): self = .saveWorkspaceProxy(value.map(ProxyDocument.init(domain:)))
        case let .saveWorkspaceSettings(value): self = .saveWorkspaceSettings(TransportSettings(domain: value))
        case let .renameWorkspace(name): self = .renameWorkspace(name: name)
        case let .saveEnvironment(value): self = .saveEnvironment(EnvironmentDraft(domain: value))
        case let .deleteEnvironment(id): self = .deleteEnvironment(id: id)
        case let .saveRequest(value): self = .saveRequest(collectionID: value.collectionId, location: RequestLocation(domain: value))
        case let .deleteRequest(collection, id): self = .deleteRequest(collectionID: collection, id: id)
        case let .duplicateRequest(collection, id, newID, name): self = .duplicateRequest(collectionID: collection, id: id, newID: newID, name: name)
        case let .createGroup(collection, group): self = .createGroup(collectionID: collection, group: GroupDraft(domain: group))
        case let .deleteGroup(collection, id): self = .deleteGroup(collectionID: collection, id: id)
        case let .createCollection(value): self = .createCollection(CollectionDraft(domain: value))
        case let .deleteCollection(id): self = .deleteCollection(id: id)
        case let .renameGroup(collection, id, name): self = .renameGroup(collectionID: collection, id: id, name: name)
        case let .renameCollection(id, name): self = .renameCollection(id: id, name: name)
        case let .moveRequest(from, id, to, group, order): self = .moveRequest(fromCollectionID: from, requestID: id, toCollectionID: to, groupID: group, order: Int(order))
        case let .moveGroup(collection, id, parent, order): self = .moveGroup(collectionID: collection, id: id, parentID: parent, order: Int(order))
        case let .reorderChildren(collection, parent, items): self = .reorderChildren(collectionID: collection, parentID: parent, items: items)
        }
    }
}

extension WorkspaceCommand {
    var sessionRecord: SessionWorkspaceCommand {
        switch self {
        case let .saveWorkspaceProxy(value): .saveWorkspaceProxy(proxy: value?.domain)
        case let .saveWorkspaceSettings(value): .saveWorkspaceSettings(transport: value.domain)
        case let .renameWorkspace(name): .renameWorkspace(name: name)
        case let .saveEnvironment(value): .saveEnvironment(environment: value.domain)
        case let .deleteEnvironment(id): .deleteEnvironment(id: id)
        case let .saveRequest(_, value): .saveRequest(location: value.domain)
        case let .deleteRequest(collection, id): .deleteRequest(collectionId: collection, id: id)
        case let .duplicateRequest(collection, id, newID, name): .duplicateRequest(collectionId: collection, id: id, newId: newID, name: name)
        case let .createGroup(collection, group): .createGroup(collectionId: collection, group: group.domain)
        case let .deleteGroup(collection, id): .deleteGroup(collectionId: collection, id: id)
        case let .createCollection(value): .createCollection(collection: value.domain)
        case let .deleteCollection(id): .deleteCollection(id: id)
        case let .renameGroup(collection, id, name): .renameGroup(collectionId: collection, id: id, name: name)
        case let .renameCollection(id, name): .renameCollection(id: id, name: name)
        case let .moveRequest(from, id, to, group, order): .moveRequest(fromCollectionId: from, requestId: id, toCollectionId: to, groupId: group, order: Int64(order))
        case let .moveGroup(collection, id, parent, order): .moveGroup(collectionId: collection, id: id, parentId: parent, order: Int64(order))
        case let .reorderChildren(collection, parent, items): .reorderChildren(collectionId: collection, parentId: parent, items: items)
        }
    }
}

private final class SessionSecretAdapter: SessionSecretPlatform {
    let persistence: any WorkspacePersistence
    init(persistence: any WorkspacePersistence) { self.persistence = persistence }
    func execute(operation: String, name: String, value: String?, reply: SessionSecretReply) {
        Task {
            do {
                let result: String?
                switch operation {
                case "read": result = try await persistence.readSecret(name: name)
                case "save": try await persistence.saveSecret(name: name, value: value ?? ""); result = nil
                default: try await persistence.deleteSecret(name: name); result = nil
                }
                reply.finish(value: result, failed: false)
            } catch { reply.finish(value: nil, failed: true) }
        }
    }
}
private extension WireboltModel {
    var runtimeContext: RuntimeContext {
        RuntimeContext(transport: workspace.transport.domain, workspaceProxy: workspace.proxy?.domain,
            appProxy: proxyPreferences.configuration.domain, variables: activeVariables.mapValues(\.domain))
    }
    func runtimeObserver(for session: DocumentSession, requireOpen: Bool = false, oauth: Bool = false) -> RuntimePresentationObserver {
        RuntimePresentationObserver(update: { [weak self, weak session] in session?.mirror(); self?.mirrorCookieRevision(); if oauth { self?.mirrorOAuthRuntime() } }, input: { [weak self, weak session] in
            guard let self, let session, !requireOpen || self.sessions.session(id: session.id) === session else { return nil }
            return RuntimeInput(draft: session.draft.domain, context: self.runtimeContext)
        })
    }
    func mirrorSecretEdits() { let values = rustSecrets.cached(); if editedSecrets != values { editedSecrets = values } }
    func mirrorOAuthRuntime() {
        let state = runtime.oauthSnapshot()
        if isOAuthBusy != state.busy { isOAuthBusy = state.busy }
        let failure = state.failure.map { runtimeError($0).localizedDescription }
        if oauthFailureMessage != failure { oauthFailureMessage = failure }
        let receipts = state.receipts.mapValues { OAuth2TokenReceipt(expiresAt: $0.expiresAt.map(Date.init(timeIntervalSince1970:)), scope: $0.scope) }
        if oauthReceipts != receipts { oauthReceipts = receipts }
    }
    func mirrorCookieRevision() {
        let value = Int(clamping: runtime.cookieRevision())
        if cookieRevision != value { cookieRevision = value }
    }
    func mirrorRuntime() {
        let state = runtime.snapshot()
        let entries = state.historyEntries.map(RunHistoryEntry.init)
        if historyEntries != entries { historyEntries = entries }
        if historyRevision != Int(clamping: state.historyRevision) { historyRevision = Int(clamping: state.historyRevision) }
        if cookieRevision != Int(clamping: state.cookieRevision) { cookieRevision = Int(clamping: state.cookieRevision) }
        let activity = state.requestActivity.mapValues { RequestActivity(date: Date(timeIntervalSinceReferenceDate: $0.date), status: $0.status) }
        if requestActivity != activity { requestActivity = activity }
    }
    func runtimeSettings() -> AppRuntimeSettings {
        AppRuntimeSettings(bridge: persistence?.runtimeWorkspaceBridge, platform: RuntimeSettingsAdapter(persistence: persistence, preferences: proxyPreferences))
    }
}
