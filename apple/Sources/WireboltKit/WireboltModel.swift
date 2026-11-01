import Foundation
import Observation

public protocol RequestRunner: Sendable {
    var sessionTransport: AppRuntimeTransport { get }
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error>
    func cancel(runID: RunID)
    func proxySettingsChanged()
    func resolveValues(_ values: [ValueSource], variables: [String: ValueSource]) async throws -> [String]
}

extension RequestRunner {
    public var sessionTransport: AppRuntimeTransport { AppRuntimeTransport.foreign(platform: RuntimeRunnerAdapter(runner: self)) }
    public func proxySettingsChanged() {}
    public func resolveValues(_ values: [ValueSource], variables: [String: ValueSource]) async throws -> [String] {
        try values.map { value in
            guard case .literal(let text) = value, !text.contains("{{") else {
                throw RunFailure(kind: .invalidRequest, issues: [RequestIssue(path: "values", kind: "unresolved_value", reference: nil)])
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
    func apply(_ command: SessionWorkspaceCommand) async throws -> WorkspaceDelta
    func applySessionCommands(_ commands: [SessionWorkspaceCommand]) async -> SessionCommandBatchResult
    func commitImport(format: ImportFormat, source: String) async throws -> ImportResult
    func commitImportFile(format: ImportFormat, source: String, name: String) async throws -> ImportResult
    func exportCollection(id: String) async throws -> String
    func exportWorkspace() async throws -> String
    func exportRequest(collectionID: String, id: String) async throws -> String
    /// The workspace folder, used to key local runtime storage such as cookies.
    var location: URL? { get }
    var sessionSecretPlatform: AppSecretPlatform { get }
    var sessionWorkspacePlatform: AppWorkspacePlatform { get }
    var runtimeWorkspaceBridge: WorkspaceBridge? { get }
}

extension WorkspacePersistence {
    public var sessionWorkspacePlatform: AppWorkspacePlatform { .foreign(platform: WorkspacePlatformAdapter(persistence: self)) }
    public func loadSession(_ application: AppWorkspaceSession) async throws -> WorkspaceDraft {
        let workspace = try await load()
        application.replace(workspace: workspace.domain)
        return workspace
    }
    public var location: URL? { nil }
    public var runtimeWorkspaceBridge: WorkspaceBridge? { nil }
    public var sessionSecretPlatform: AppSecretPlatform { AppSecretPlatform.foreign(platform: SessionSecretAdapter(persistence: self)) }
    public func readSecret(name _: String) async throws -> String? { nil }
    public func deleteSecret(name _: String) async throws {}
    public func apply(_ command: SessionWorkspaceCommand) async throws -> WorkspaceDelta {
        switch command {
        case .saveRequest(let location):
            try await save(request: RequestDraft(domain: location.request), in: location.collectionId)
            return WorkspaceDelta(version: 0, kind: .request, affectedIDs: [RequestLocation(domain: location).id])
        case .saveEnvironment(let environment):
            try await save(environment: EnvironmentDraft(domain: environment))
            return WorkspaceDelta(version: 0, kind: .environment, affectedIDs: [environment.id])
        default:
            throw WorkspaceMutationError.unsupported
        }
    }

    public func applySessionCommands(_ commands: [SessionWorkspaceCommand]) async -> SessionCommandBatchResult {
        var completed: UInt64 = 0
        do {
            for command in commands {
                _ = try await apply(command)
                completed += 1
            }
            return SessionCommandBatchResult(completed: completed, failureReason: nil)
        } catch { return SessionCommandBatchResult(completed: completed, failureReason: error.localizedDescription) }
    }

    public func commitImport(format _: ImportFormat, source _: String) async throws -> ImportResult {
        throw WorkspaceMutationError.unsupported
    }

    public func commitImportFile(format: ImportFormat, source: String, name _: String) async throws -> ImportResult {
        try await commitImport(format: format, source: source)
    }

    public func exportCollection(id _: String) async throws -> String { throw WorkspaceMutationError.unsupported }
    public func exportWorkspace() async throws -> String { throw WorkspaceMutationError.unsupported }
    public func exportRequest(collectionID _: String, id _: String) async throws -> String {
        throw WorkspaceMutationError.unsupported
    }
}

public enum WorkspaceMutationError: Error, Equatable, Sendable {
    case unsupported
}

public protocol GitCollaboration: Sendable {
    var sessionGitPlatform: AppWorkspacePlatform { get }
    func status() async throws -> GitStatusSnapshot
    func pull() async throws -> GitOperationSnapshot
    func commit(message: String) async throws -> GitOperationSnapshot
    func push() async throws -> GitOperationSnapshot
    func abortMerge() async throws -> GitOperationSnapshot
    func initializeRepository() async throws -> GitStatusSnapshot
}

extension GitCollaboration {
    public var sessionGitPlatform: AppWorkspacePlatform { .foreign(platform: WorkspacePlatformAdapter(git: self)) }
    public func abortMerge() async throws -> GitOperationSnapshot {
        throw GitFailure(kind: .unsupported, reason: "This workspace cannot abort a merge.")
    }

    public func initializeRepository() async throws -> GitStatusSnapshot {
        throw GitFailure(kind: .unsupported, reason: "This workspace cannot create a Git repository.")
    }
}

@MainActor
@Observable
public final class WireboltModel {
    public static let previewByteLimit = DocumentSession.previewByteLimit

    public var workspace = WorkspaceDraft(name: "Wirebolt") {
        didSet {
            sidebarSnapshot = nil
            sidebarVisibleCache = nil
            sidebarUnfilteredCache = nil
            if !mirroringWorkspace { workspaceNeedsSynchronization = true }
        }
    }
    @ObservationIgnored private let rustWorkspace: AppWorkspaceSession
    @ObservationIgnored private let rustOperations: AppWorkspaceOperations
    @ObservationIgnored private var mirroringWorkspace = false
    @ObservationIgnored private var workspaceNeedsSynchronization = true
    @ObservationIgnored private var sidebarSnapshot: SidebarSnapshot?
    @ObservationIgnored private var sidebarVisibleCache:
        (query: String, collapsed: Set<String>, expanded: Set<String>, rows: [SidebarSnapshot.Row])?
    /// The unfiltered outline, kept apart from the last filter's rows so clearing a filter
    /// (or deleting its last character) shows the outline without recomputing it.
    @ObservationIgnored private var sidebarUnfilteredCache: (collapsed: Set<String>, expanded: Set<String>, rows: [SidebarSnapshot.Row])?

    public func sidebarRows(query: String, collapsed: Set<String>, expanded: Set<String>) -> [SidebarSnapshot.Row] {
        let collections = workspace.collections
        let filtering = !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        if filtering, let cache = sidebarVisibleCache, cache.query == query, cache.collapsed == collapsed, cache.expanded == expanded {
            return cache.rows
        }
        if !filtering, let cache = sidebarUnfilteredCache, cache.collapsed == collapsed, cache.expanded == expanded { return cache.rows }
        if sidebarSnapshot == nil { sidebarSnapshot = SidebarSnapshot(collections: collections) }
        let rows = sidebarSnapshot!.visible(query: query, collapsedCollections: collapsed, expandedGroups: expanded)
        if filtering {
            sidebarVisibleCache = (query, collapsed, expanded, rows)
        } else {
            sidebarUnfilteredCache = (collapsed, expanded, rows)
        }
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
        let rustWorkspace = AppWorkspaceSession()
        self.rustWorkspace = rustWorkspace
        rustOperations = AppWorkspaceOperations(workspace: rustWorkspace)
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
        workspaceDirtyIds(groups: sessions.groups.map(\.workspaceRecord), documents: sessions.sessions.values.map(\.tabProjection))
            .compactMap { sessions.session(id: $0) }
    }

    /// Saves every dirty document. Stops at the first failure so the caller can keep
    /// the app or window open; documents saved before the failure stay saved.
    @discardableResult
    public func saveAllDirtySessions() async -> Bool {
        await saveDocuments(dirtySessions.map(\.id), flushTransport: true, retainingDocuments: true)
    }

    @discardableResult
    public func saveDocuments(_ ids: [String], flushTransport: Bool = false, retainingDocuments: Bool = false) async -> Bool {
        guard let persistence else { return ids.isEmpty }
        let targets = Dictionary(uniqueKeysWithValues: ids.compactMap { id in sessions.session(id: id).map { (id, $0) } })
        let documents = AppWorkspaceDocuments.foreign(
            platform: WorkspaceDocumentAdapter(
                read: { id in
                    let session = retainingDocuments ? targets[id] : self.sessions.session(id: id)
                    guard let session else { return nil }
                    self.synchronizeWorkspace()
                    session.synchronizeDraft()
                    return session.rust.document()
                },
                saved: { result in
                    self.applySave(result, session: retainingDocuments ? targets[result.documentId] : nil)
                    self.mirrorSecretEdits()
                    self.mirrorWorkspacePresentation()
                }))
        let succeeded = await rustOperations.saveAllLive(
            ids: ids, platform: persistence.sessionWorkspacePlatform,
            documents: documents, secrets: rustSecrets, secretPlatform: persistence.sessionSecretPlatform)
        if succeeded, flushTransport { await flushWorkspaceTransport() }
        return succeeded
    }

    private func applySave(_ result: WorkspaceSave, session target: DocumentSession? = nil) {
        guard let session = target ?? sessions.session(id: result.documentId) else { return }
        let authentication = RequestAuthentication(
            domain: workspaceSavedAuthentication(
                current: session.draft.authentication.domain,
                original: result.originalAuthentication, saved: result.draft.authentication))
        if authentication != session.draft.authentication { session.draft.authentication = authentication }
        if let location = result.location {
            session.recordSavedDraft(RequestDraft(domain: location.request))
            applySavedRequest(RequestLocation(domain: location))
        }
    }

    /// Reverts every dirty document to its saved state; never-saved documents close.
    public func discardUnsavedChanges() {
        let documents = dirtySessions.map { session in
            session.synchronizeDraft()
            return session.rust.document()
        }
        for effect in workspaceDiscard(documents: documents) {
            guard let session = sessions.session(id: effect.id) else { continue }
            if let saved = effect.saved {
                session.markSaved(RequestDraft(domain: saved))
            } else {
                for group in sessions.groups where group.tabIDs.contains(session.id) {
                    closeDocuments(.one(session.id), in: group.id, allowDirty: true)
                }
            }
        }
    }

    public func normalizedSearchQuery(_ value: String) -> String { domainNormalizeSearch(text: value) }

    public func requestMatches(_ location: RequestLocation, normalizedQuery: String) -> Bool {
        let request = location.request
        return rustOperations.requestMatches(
            request: DomainSidebarRequest(
                id: request.id, collectionId: location.collectionID,
                groupId: location.groupID, order: Int64(location.order), name: request.name, method: request.method.rawValue,
                url: request.url), normalized: normalizedQuery)
    }

    public func connectWebSocket(_ session: DocumentSession) async {
        guard let socketConnector else { return }
        do {
            let input = try await rustSecrets.socketInput(
                draft: session.draft.domain, context: runtimeContext,
                platform: persistence?.sessionSecretPlatform, observer: runtimeObserver(for: session))
            mirrorSecretEdits()
            session.socket.connect(input: RunInput(domain: input), connector: socketConnector)
        } catch {
            mirrorSecretEdits()
            operationFailure = RunFailure(kind: .keychain, issues: [])
        }
    }

    public func send(_ target: DocumentSession? = nil) async {
        let session = target ?? sessions.activeSession ?? sessions.openTemporary()
        switch sessionSendAction(kind: session.kind.rust, socket: session.socket.status.rust, running: session.isRunning) {
        case .socketSend:
            await session.socket.send(body: session.draft.body)
            return
        case .socketConnect:
            await connectWebSocket(session)
            return
        case .ignore: return
        case .http: break
        }
        operationFailure = nil
        session.synchronizeDraft()
        let runtime = runtime
        let failure = await withTaskCancellationHandler {
            await runtime.send(
                document: session.rust,
                options: RuntimeRunOptions(
                    context: runtimeContext, id: RunID().description,
                    directory: FileManager.default.temporaryDirectory.path), transport: runner.sessionTransport,
                observer: runtimeObserver(for: session, requireOpen: true),
                secrets: rustSecrets, platform: persistence?.sessionSecretPlatform)
        } onCancel: {
            runtime.cancel(document: session.rust)
        }
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
        let result = sessions.closeWorkspace(scope, in: groupID, allowDirty: allowDirty)
        for session in result.closed { cancel(session) }
        return result.blocked
    }

    public func acquireOAuthToken(for session: DocumentSession) async {
        guard case .oauth2 = session.draft.authentication, let persistence else { return }
        session.synchronizeDraft()
        await runtime.acquireToken(
            document: session.rust, secrets: rustSecrets, platform: persistence.sessionSecretPlatform,
            authorizer: RuntimeOAuthAdapter(authorizer: oauth2) { [weak session] in
                session?.draft.authentication = .oauth2(configuration: $0)
            },
            observer: runtimeObserver(for: session, oauth: true))
        mirrorOAuthRuntime()
        mirrorSecretEdits()
    }

    public func historyEntries(for session: DocumentSession) async -> [RunHistoryEntry] {
        do { return try await runtime.historyEntries(id: session.requestID).map(RunHistoryEntry.init) }
        catch { operationFailure = RunFailure(kind: .storage, issues: []); return [] }
    }
    public func historyCount(for session: DocumentSession) async -> Int {
        do { return Int(clamping: try await runtime.historyCount(id: session.requestID)) }
        catch { operationFailure = RunFailure(kind: .storage, issues: []); return 0 }
    }
    public func loadHistory(for session: DocumentSession) async {
        do { try await runtime.loadHistory(id: session.requestID) }
        catch { operationFailure = RunFailure(kind: .storage, issues: []) }
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

    public func exportWorkspace() async -> String? { await export(.workspace) }

    public func exportCollection(id: String) async -> String? { await export(.collection(id: id)) }

    public func exportRequest(collectionID: String, id: String) async -> String? {
        await export(.request(collectionId: collectionID, id: id))
    }

    private func export(_ target: WorkspaceExport) async -> String? {
        guard let persistence else { return nil }
        let result = await rustOperations.export(target: target, platform: persistence.sessionWorkspacePlatform)
        mirrorWorkspacePresentation()
        return result
    }

    public func loadWorkspace(restoring layout: SessionLayout? = nil) async {
        let begin = rustOperations.beginLoad(configured: persistence != nil)
        mirrorWorkspacePresentation()
        guard begin, let persistence else { return }
        let loaded = await rustOperations.load(platform: persistence.sessionWorkspacePlatform, switching: false)
        mirrorWorkspacePresentation()
        if let loaded { applyLoadedWorkspace(loaded, restoring: layout) }
    }

    @discardableResult
    public func openWorkspace(
        using persistence: any WorkspacePersistence,
        gitCollaboration: (any GitCollaboration)? = nil,
        restoring layout: SessionLayout? = nil
    ) async -> Bool {
        _ = rustOperations.beginLoad(configured: true)
        mirrorWorkspacePresentation()
        await flushWorkspaceTransport()
        guard let loaded = await rustOperations.load(platform: persistence.sessionWorkspacePlatform, switching: true) else {
            mirrorWorkspacePresentation()
            return false
        }
        for session in sessions.sessions.values { cancel(session) }
        sessions.removeAll()
        await purgeReleasedSecrets()
        rustSecrets.clearObserved()
        configurePersistence(persistence, gitCollaboration: gitCollaboration)
        transportSave = nil
        runtime.resetWorkspace()
        oauthReceipts = [:]
        historyEntries = []
        requestActivity = [:]
        applyLoadedWorkspace(loaded, restoring: layout)
        undoManager?.removeAllActions(withTarget: self)
        mirrorWorkspacePresentation()
        return true
    }

    public func refreshGitStatus() async { await performGitOperation(.status) }

    public func pullGit() async { await performGitOperation(.pull) }

    public func commitGit(message: String) async { await performGitOperation(.commit(message: message)) }

    public func pushGit() async { await performGitOperation(.push) }

    /// Abandons the merge a conflicted pull started and reloads the restored documents.
    public func abortGitMerge() async { await performGitOperation(.abort) }

    /// Runs `git init` in the workspace folder so it becomes the repository root.
    public func initializeGitRepository() async { await performGitOperation(.initialize) }

    // MARK: Workspace name

    @discardableResult
    public func renameWorkspace(_ proposedName: String) async -> Bool {
        guard let persistence else { return false }
        synchronizeWorkspace()
        let name = await rustOperations.rename(proposed: proposedName, platform: persistence.sessionWorkspacePlatform)
        mirrorWorkspacePresentation()
        if let name {
            mirroringWorkspace = true
            workspace.name = name
            mirroringWorkspace = false
        }
        return name != nil
    }

    // MARK: Cookies

    /// The active workspace's cookies, most specific domain first.
    public func cookies() async -> [CookieSnapshot] { runtime.cookies().map(CookieSnapshot.init) }
    public func deleteCookie(id: CookieSnapshot.ID) async {
        runtime.deleteCookie(id: id)
        mirrorRuntime()
    }
    public func clearCookies() async {
        runtime.clearCookies()
        mirrorRuntime()
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
        synchronizeWorkspace()
        session.synchronizeDraft()
        let result = await rustOperations.save(
            document: session.rust.document(), fallback: fallbackCollectionID,
            platform: persistence.sessionWorkspacePlatform, secrets: rustSecrets, secretPlatform: persistence.sessionSecretPlatform)
        mirrorSecretEdits()
        mirrorWorkspacePresentation()
        applySave(result, session: session)
        return result.succeeded
    }

    @discardableResult
    public func saveEnvironment(_ environment: EnvironmentDraft) async -> Bool {
        // An unchanged environment is a successful save without an undo entry.
        synchronizeWorkspace()
        if rustOperations.environmentUnchanged(environment: environment.domain) { return true }
        return await perform(.saveEnvironment(environment: environment.domain)).succeeded
    }

    public func makeProxyConnectionTest() -> ProxyConnectionTest { ProxyConnectionTest(runner: runner) }

    public func effectiveProxy(for draft: RequestDraft) -> EffectiveProxy {
        .resolve(request: draft.proxy.document, workspace: workspace.proxy, app: proxyPreferences.configuration)
    }

    /// Validate and persist before publishing. Failed saves leave the active policy intact.
    public func applyProxy(
        _ configuration: ProxyDocument?, scope: ProxyScope,
        session: DocumentSession? = nil, secrets: [String: String] = [:]
    ) async throws {
        do {
            try await runtime.applyProxy(
                configuration: configuration?.domain, scope: scope.rust,
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
        let platform = runtimeSettings()
        let runtime = runtime
        transportSave = Task {
            if let result = await runtime.savePendingTransport(generation: generation, platform: platform) {
                if workspace.transport != TransportSettings(domain: result.value) {
                    workspace.transport = TransportSettings(domain: result.value)
                }
                if result.failed { operationFailure = RunFailure(kind: .workspace, issues: []) }
            }
        }
    }
    public func flushWorkspaceTransport() async { await transportSave?.value }
    public func saveWorkspaceTransport() async {
        guard persistence != nil else { return }
        do { try await runtime.saveTransport(value: workspace.transport.domain, platform: runtimeSettings()) } catch {
            operationFailure = RunFailure(kind: .workspace, issues: [])
        }
    }

    @discardableResult
    public func deleteEnvironment(id: String) async -> Bool {
        await perform(.deleteEnvironment(id: id)).succeeded
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
        } catch {
            operationFailure = (error as? RunFailure) ?? RunFailure(kind: .export, issues: [])
            return nil
        }
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
        guard case .secret(let name) = source, let persistence else { return }
        do {
            _ = try await rustSecrets.load(name: name, platform: persistence.sessionSecretPlatform)
            mirrorSecretEdits()
        } catch { operationFailure = RunFailure(kind: .keychain, issues: []) }
    }
    private func flushSecrets() async throws {
        guard let persistence else {
            if !rustSecrets.dirty().isEmpty { throw WorkspaceMutationError.unsupported }
            return
        }
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
        let documents = sessions.sessions.values.map { session in
            session.synchronizeDraft()
            return session.rust.document()
        }
        await rustSecrets.purgeWorkspace(
            workspace: workspace.domain, documents: documents,
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
        do { _ = try await rustSecrets.copy(renames: renames, platform: persistence.sessionSecretPlatform) } catch {
            mirrorSecretEdits()
            throw error
        }
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
            mirrorSecretEdits()
            return true
        } catch {
            mirrorSecretEdits()
            operationFailure = RunFailure(kind: .keychain, issues: [])
            return false
        }
    }

    public func saveSecret(name: String, value: String) async {
        guard let persistence else { return }
        do {
            try await rustSecrets.saveMaterial(name: name, value: value, platform: persistence.sessionSecretPlatform)
        } catch {
            operationFailure = RunFailure(kind: .keychain, issues: [])
        }
    }

    public func importExternalFile(_ url: URL, preferred: ImportFormat? = nil) async {
        let formats = ImportFormat.localizedFormats
        let input = await Task.detached(priority: .userInitiated) {
            workspaceReadImport(path: url.path, preferred: preferred, formats: formats)
        }.value
        switch input {
        case .bruno: await importBrunoCollection(at: url)
        case .document(let source, let format):
            await importDocument(source: source, format: format, url: url)
        case .failure(let message): importFailureMessage = message
        }
    }

    public func importDocument(url: URL, format: ImportFormat) async {
        guard beginImport() else { return }
        let source = await Task.detached(priority: .userInitiated) { workspaceReadDocument(path: url.path) }.value
        guard let source else {
            finishImportFailure(ImportFormat.unreadableMessage(fileName: url.lastPathComponent))
            return
        }
        _ = await commitDocument(
            source: source, format: format, fileName: url.deletingPathExtension().lastPathComponent, displayName: url.lastPathComponent)
    }

    public func importDocument(source: String, format: ImportFormat, url: URL) async {
        guard beginImport() else { return }
        _ = await commitDocument(
            source: source, format: format, fileName: url.deletingPathExtension().lastPathComponent, displayName: url.lastPathComponent)
    }

    public func importDocument(source: String, format: ImportFormat) async {
        guard beginImport() else { return }
        _ = await commitDocument(source: source, format: format)
    }

    public func importBrunoCollection(at url: URL) async {
        guard beginImport() else { return }
        do {
            let source = try await Task.detached(priority: .userInitiated) { try BrunoCollectionSource.bundle(at: url) }.value
            _ = await commitDocument(
                source: source, format: .brunoFolder, fileName: url.deletingPathExtension().lastPathComponent,
                displayName: url.lastPathComponent)
        } catch BrunoCollectionSource.ReadError.tooLarge { finishImportFailure("“\(url.lastPathComponent)” is too large to import.") } catch
        { finishImportFailure("“\(url.lastPathComponent)” isn’t a Bruno collection or couldn’t be read.") }
    }

    public func importPastedDocument(source: String, format: ImportFormat) async -> String? {
        guard beginImport(pasted: true) else { return nil }
        return await commitDocument(source: source, format: format, pasted: true)
    }

    private func beginImport(pasted: Bool = false) -> Bool {
        let begin = rustOperations.beginImport(configured: persistence != nil, pasted: pasted)
        if begin, !pasted { importFailureMessage = nil }
        mirrorWorkspacePresentation()
        return begin
    }
    private func finishImportFailure(_ message: String, pasted: Bool = false) {
        _ = rustOperations.finishImportFailure(message: message, pasted: pasted)
        if !pasted { importFailureMessage = message }
        mirrorWorkspacePresentation()
    }
    private func commitDocument(
        source: String, format: ImportFormat, fileName: String? = nil, displayName: String? = nil, pasted: Bool = false
    ) async -> String? {
        guard let persistence else { return nil }
        await flushWorkspaceTransport()
        synchronizeWorkspace()
        let loaded = await rustOperations.import(
            request: WorkspaceImport(
                source: source, format: format, name: fileName,
                displayName: displayName, selected: selectedEnvironmentID, pasted: pasted), platform: persistence.sessionWorkspacePlatform)
        let value = rustOperations.presentation()
        if let loaded {
            applyLoadedWorkspace(loaded)
            if let location = loaded.select { select(RequestLocation(domain: location)) }
            importSummary = value.importSummary.map(ImportSummary.init)
        }
        if !pasted { importFailureMessage = value.importFailure }
        mirrorWorkspacePresentation()
        return value.importFailure
    }

    @discardableResult
    public func makeNewRequest(kind: DocumentKind = .http, collectionID: String? = nil, groupID: String? = nil) -> DocumentSession {
        synchronizeWorkspace()
        let location = rustOperations.newRequest(webSocket: kind == .webSocket, collectionId: collectionID ?? "", groupId: groupID)
        let draft = RequestDraft(domain: location.request)
        let session = sessions.open(draft: draft, collectionID: collectionID, kind: kind, forceNewSession: true, isSaved: false)
        if collectionID != nil { applySavedRequest(RequestLocation(domain: location)) }
        return session
    }

    public func makeNewEnvironment() -> EnvironmentDraft { EnvironmentDraft(domain: workspaceNewEnvironment()) }

    public func createRequest(kind: DocumentKind = .http, collectionID: String? = nil, groupID: String? = nil) async -> DocumentSession? {
        let result = await perform(.createRequest(webSocket: kind == .webSocket, collectionId: collectionID, groupId: groupID))
        guard result.succeeded, let id = result.createdId,
            let location = result.plan?.collections.flatMap(\.requests).first(where: { $0.request.id == id })
        else { return nil }
        return sessions.open(draft: RequestDraft(domain: location.request), collectionID: location.collectionId, kind: kind)
    }

    public func createRootFolder() async -> String? {
        await createGroup(collectionID: WorkspaceDraft.rootCollectionID, name: "Untitled Folder")
    }
    public func createCollection(name: String = "New Collection") async { await perform(.createCollection(name: name)) }
    public func renameRequest(collectionID: String, requestID: String, name: String) async {
        await perform(.renameRequest(collectionId: collectionID, id: requestID, name: name))
    }
    public func renameCollection(id: String, name: String) async { await perform(.renameCollection(id: id, name: name)) }
    public func deleteCollection(id: String) async { await perform(.deleteCollection(id: id)) }
    @discardableResult
    public func createGroup(collectionID: String, parentID: String? = nil, name: String = "New Folder") async -> String? {
        let result = await perform(.createGroup(collectionId: collectionID, parentId: parentID, name: name))
        return result.succeeded ? result.createdId : nil
    }
    public func renameGroup(collectionID: String, id: String, name: String) async {
        await perform(.renameGroup(collectionId: collectionID, id: id, name: name))
    }
    public func deleteGroup(collectionID: String, id: String) async { await perform(.deleteGroup(collectionId: collectionID, id: id)) }
    public func deleteRequest(collectionID: String, requestID: String) async {
        await perform(.deleteRequest(collectionId: collectionID, id: requestID))
    }
    public func duplicateRequest(collectionID: String, requestID: String) async {
        await perform(.duplicateRequest(collectionId: collectionID, id: requestID))
    }

    public func canReorderSidebar(_ identifier: String, relativeTo target: String, collectionID: String, parentID: String?) -> Bool {
        synchronizeWorkspace()
        return rustOperations.canReorder(identifier: identifier, target: target, collectionId: collectionID, parent: parentID)
    }
    public func reorderSidebar(_ identifier: String, relativeTo target: String, after: Bool, collectionID: String, parentID: String?) async
    {
        await perform(.reorder(identifier: identifier, target: target, after: after, collectionId: collectionID, parentId: parentID))
    }
    public func canMoveSidebarItem(_ identifier: String, by delta: Int) -> Bool {
        if sidebarSnapshot == nil { sidebarSnapshot = SidebarSnapshot(collections: workspace.collections) }
        return sidebarSnapshot?.canMove(identifier, by: delta) ?? false
    }
    public func moveSidebarItem(_ identifier: String, by delta: Int) async {
        await perform(.moveBy(identifier: identifier, delta: Int64(delta)))
    }
    public func canMoveSidebarItem(_ identifier: String, toCollectionID collectionID: String, parentID: String?) -> Bool {
        synchronizeWorkspace()
        return rustOperations.canMoveTo(identifier: identifier, collectionId: collectionID, parent: parentID)
    }
    public func moveSidebarItem(_ identifier: String, toCollectionID collectionID: String, parentID: String?) async {
        await perform(.moveTo(identifier: identifier, collectionId: collectionID, parentId: parentID))
    }
    public func nextChildOrder(collectionID: String, parentID: String?) -> Int {
        synchronizeWorkspace()
        return Int(rustOperations.nextOrder(collectionId: collectionID, parent: parentID))
    }
    public func moveRequest(fromCollectionID: String, requestID: String, toCollectionID: String, groupID: String?, order: Int) async {
        await perform(
            .moveRequest(
                fromCollectionId: fromCollectionID, id: requestID, toCollectionId: toCollectionID, groupId: groupID, order: Int64(order)))
    }
    public func moveGroup(collectionID: String, id: String, parentID: String?, order: Int) async {
        await perform(.moveGroup(collectionId: collectionID, id: id, parentId: parentID, order: Int64(order)))
    }

    private func applyLoadedWorkspace(_ loaded: WorkspaceLoad, restoring layout: SessionLayout? = nil) {
        mirroringWorkspace = true
        workspace = WorkspaceDraft(domain: loaded.workspace)
        mirroringWorkspace = false
        workspaceNeedsSynchronization = false
        observeSecrets(workspace.secretReferences)
        runtime.resetTransport()
        selectedEnvironmentID = loaded.selectedEnvironmentId
        let restored = sessions.restoreWorkspace(rustOperations, layout: layout)
        restoreLatestResponses(for: restored)
    }

    private func applySavedRequest(_ location: RequestLocation) {
        mirroringWorkspace = true
        defer { mirroringWorkspace = false }
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
    }

    public func deleteConsequences(_ target: SessionDeleteTarget) -> String {
        if workspaceNeedsSynchronization {
            rustWorkspace.replace(workspace: workspace.domain)
            workspaceNeedsSynchronization = false
        }
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

    private func performGitOperation(_ action: WorkspaceGitAction) async {
        let begin = rustOperations.beginGit(configured: gitCollaboration != nil, action: action, dirty: hasUnsavedRequestChanges)
        mirrorWorkspacePresentation()
        guard begin, let gitCollaboration else { return }
        if case .pull = action { await flushWorkspaceTransport() }
        let loaded = await rustOperations.git(
            action: action, platform: gitCollaboration.sessionGitPlatform,
            persistence: persistence?.sessionWorkspacePlatform)
        if let loaded {
            applyLoadedWorkspace(loaded)
            undoManager?.removeAllActions(withTarget: self)
            await purgeReleasedSecrets()
        }
        mirrorWorkspacePresentation()
    }

    private func synchronizeWorkspace() {
        if workspaceNeedsSynchronization {
            rustWorkspace.replace(workspace: workspace.domain)
            workspaceNeedsSynchronization = false
        }
    }

    private func mirrorWorkspacePresentation() {
        let value = rustOperations.presentation()
        if isLoadingWorkspace != value.loading { isLoadingWorkspace = value.loading }
        if hasLoadedWorkspace != value.loaded { hasLoadedWorkspace = value.loaded }
        if isGitBusy != value.gitBusy { isGitBusy = value.gitBusy }
        if isImporting != value.importing { isImporting = value.importing }
        let status = value.gitStatus.map(GitStatusSnapshot.init)
        if gitStatus != status { gitStatus = status }
        let operation = value.gitOperation.map(GitOperationSnapshot.init)
        if gitOperation != operation { gitOperation = operation }
        let failure = value.gitFailure.map { GitFailure(kind: $0.kind, reason: $0.reason) }
        if gitFailure != failure { gitFailure = failure }
        if let kind = rustOperations.takeFailure() { operationFailure = RunFailure(kind: kind, issues: []) }
        if let reason = value.exportFailure { exportFailureMessage = reason }
    }

}

extension WireboltModel {
    @discardableResult
    private func perform(_ command: WorkspaceAction) async -> WorkspaceMutation {
        await undoWork?.value
        guard let persistence else { return WorkspaceMutation(succeeded: false, plan: nil, undo: nil, createdId: nil, root: nil) }
        synchronizeWorkspace()
        let result = await rustOperations.mutate(
            command: command, selected: selectedEnvironmentID,
            platform: persistence.sessionWorkspacePlatform, secrets: rustSecrets, secretPlatform: persistence.sessionSecretPlatform)
        mirrorSecretEdits()
        applyMutation(result)
        mirrorWorkspacePresentation()
        if let token = result.undo, let undoManager {
            let opensGroup = !undoManager.isUndoing && !undoManager.isRedoing
            if opensGroup { undoManager.beginUndoGrouping() }
            undoManager.registerUndo(withTarget: self) { $0.runUndoStep(token) }
            undoManager.setActionName(token.name)
            if opensGroup { undoManager.endUndoGrouping() }
        }
        if undoManager == nil { rustOperations.clearUndo() }
        return result
    }

    func finishUndoWork() async { await undoWork?.value }

    private func runUndoStep(_ token: WorkspaceUndoToken) {
        guard let reverse = rustOperations.reverseToken(token: token.token) else {
            undoManager?.removeAllActions(withTarget: self)
            return
        }
        undoManager?.registerUndo(withTarget: self) { $0.runUndoStep(reverse) }
        undoManager?.setActionName(token.name)
        let previous = undoWork
        undoWork = Task { [weak self] in
            await previous?.value
            guard let self, let persistence = self.persistence else { return }
            self.synchronizeWorkspace()
            let result = await self.rustOperations.undo(
                token: token.token, reverse: reverse.token, selected: self.selectedEnvironmentID,
                platform: persistence.sessionWorkspacePlatform, secrets: self.rustSecrets, secretPlatform: persistence.sessionSecretPlatform
            )
            self.mirrorSecretEdits()
            self.applyMutation(result)
            self.mirrorWorkspacePresentation()
            if !result.succeeded { self.undoManager?.removeAllActions(withTarget: self) }
        }
    }

    private func applyMutation(_ result: WorkspaceMutation) {
        mirroringWorkspace = true
        defer { mirroringWorkspace = false }
        if let root = result.root, !workspace.collections.contains(where: { $0.id == root.id }) {
            workspace.collections.insert(CollectionDraft(domain: root), at: 0)
        }
        guard let plan = result.plan else { return }
        workspace.collections.removeAll { plan.removedCollectionIds.contains($0.id) }
        for collection in plan.collections {
            let projected = CollectionDraft(domain: collection)
            if let index = workspace.collections.firstIndex(where: { $0.id == collection.id }) {
                if workspace.collections[index] != projected { workspace.collections[index] = projected }
            } else {
                workspace.collections.append(projected)
            }
        }
        if let environments = plan.environments { workspace.environments = environments.map(EnvironmentDraft.init(domain:)) }
        if plan.changesEnvironmentSelection { selectedEnvironmentID = plan.selectedEnvironmentId }
        for effect in plan.effects {
            switch effect {
            case .close(let collectionID, let requestID): closeSessions(collectionID: collectionID, requestID: requestID)
            case .rename(let collectionID, let requestID, let name):
                for session in sessions.sessions.values where session.collectionID == collectionID && session.requestID == requestID {
                    session.renameSavedRequest(name)
                }
            case .relocate(let collectionID, let requestID, let destination):
                for session in sessions.sessions.values where session.collectionID == collectionID && session.requestID == requestID {
                    session.relocate(to: destination)
                }
                sessions.refreshActiveRequest()
            }
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

extension ImportFormat {
    /// The name used in menus and messages.
    public var displayName: String { workspaceImportDisplayName(format: self) }

    /// Explains a failed import with the importer's reason when it has one.
    public static func failureMessage(for error: any Error, fileName: String?) -> String {
        workspaceImportFailure(reason: (error as? LocalizedError)?.errorDescription, name: fileName)
    }

    public static func unreadableMessage(fileName: String) -> String { workspaceImportUnreadable(name: fileName) }

    public static func unrecognizedMessage(fileName: String, contents: String) -> String {
        workspaceImportUnrecognized(name: fileName, contents: contents, formats: localizedFormats)
    }
    public static let allCases: [ImportFormat] = [.curl, .har, .legacyWorkspaceV1, .postmanV2, .insomnia, .bruno, .brunoFolder]

    public static var localizedFormats: String {
        ListFormatter.localizedString(byJoining: allCases.filter { $0 != .brunoFolder }.map(\.displayName))
    }

    /// Infers the importer for a file opened from Finder or dropped on the window.
    public static func detect(fileExtension: String, contents: String) -> ImportFormat? {
        domainDetectImport(extension: fileExtension, contents: contents)
    }
}

/// Finds Keychain reference names in workspace folders on disk without opening them as
/// workspaces: only request and environment documents are read, as plain text.
public enum WorkspaceSecretScan {
    public nonisolated static func referencedNames(_ candidates: Set<String>, in workspaces: [URL]) -> Set<String> {
        Set(sessionScanSecrets(candidates: Array(candidates), workspaces: workspaces.map(\.path)))
    }
}

private final class SessionSecretAdapter: SessionSecretPlatform {
    let persistence: any WorkspacePersistence
    init(persistence: any WorkspacePersistence) { self.persistence = persistence }
    func execute(operation: SessionSecretOperation, name: String, value: String?, reply: SessionSecretReply) {
        Task {
            do {
                let result: String?
                switch operation {
                case .read: result = try await persistence.readSecret(name: name)
                case .save:
                    try await persistence.saveSecret(name: name, value: value ?? "")
                    result = nil
                case .remove:
                    try await persistence.deleteSecret(name: name)
                    result = nil
                }
                reply.finish(value: result, failed: false)
            } catch { reply.finish(value: nil, failed: true) }
        }
    }
}
extension WireboltModel {
    fileprivate var runtimeContext: RuntimeContext {
        RuntimeContext(
            transport: workspace.transport.domain, workspaceProxy: workspace.proxy?.domain,
            appProxy: proxyPreferences.configuration.domain, variables: activeVariables.mapValues(\.domain))
    }
    fileprivate func runtimeObserver(for session: DocumentSession, requireOpen: Bool = false, oauth: Bool = false)
        -> RuntimePresentationObserver
    {
        RuntimePresentationObserver(
            update: { [weak self, weak session] in
                session?.mirror()
                self?.mirrorCookieRevision()
                if oauth { self?.mirrorOAuthRuntime() }
            },
            input: { [weak self, weak session] in
                guard let self, let session, !requireOpen || self.sessions.session(id: session.id) === session else { return nil }
                return RuntimeInput(draft: session.draft.domain, context: self.runtimeContext)
            })
    }
    fileprivate func mirrorSecretEdits() {
        let values = rustSecrets.cached()
        if editedSecrets != values { editedSecrets = values }
    }
    fileprivate func mirrorOAuthRuntime() {
        let state = runtime.oauthSnapshot()
        if isOAuthBusy != state.busy { isOAuthBusy = state.busy }
        let failure = state.failure.map { runtimeError($0).localizedDescription }
        if oauthFailureMessage != failure { oauthFailureMessage = failure }
        let receipts = state.receipts.mapValues {
            OAuth2TokenReceipt(expiresAt: $0.expiresAt.map(Date.init(timeIntervalSince1970:)), scope: $0.scope)
        }
        if oauthReceipts != receipts { oauthReceipts = receipts }
    }
    fileprivate func mirrorCookieRevision() {
        let value = Int(clamping: runtime.cookieRevision())
        if cookieRevision != value { cookieRevision = value }
    }
    fileprivate func mirrorRuntime() {
        let state = runtime.snapshot()
        let entries = state.historyEntries.map(RunHistoryEntry.init)
        if historyEntries != entries { historyEntries = entries }
        if historyRevision != Int(clamping: state.historyRevision) { historyRevision = Int(clamping: state.historyRevision) }
        if cookieRevision != Int(clamping: state.cookieRevision) { cookieRevision = Int(clamping: state.cookieRevision) }
        let activity = state.requestActivity.mapValues {
            RequestActivity(date: Date(timeIntervalSinceReferenceDate: $0.date), status: $0.status)
        }
        if requestActivity != activity { requestActivity = activity }
    }
    fileprivate func runtimeSettings() -> AppRuntimeSettings {
        AppRuntimeSettings(
            bridge: persistence?.runtimeWorkspaceBridge,
            platform: RuntimeSettingsAdapter(persistence: persistence, preferences: proxyPreferences))
    }
}
