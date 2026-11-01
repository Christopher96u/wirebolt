import Foundation

struct RustWorkspacePersistence: WorkspacePersistence, GitCollaboration {
    private let bridge: WorkspaceBridge
    let location: URL?
    var sessionWorkspacePlatform: AppWorkspacePlatform { .workspace(bridge: bridge) }
    var sessionGitPlatform: AppWorkspacePlatform { .workspace(bridge: bridge) }
    var runtimeWorkspaceBridge: WorkspaceBridge? { bridge }
    var sessionSecretPlatform: AppSecretPlatform { AppSecretPlatform.workspace(bridge: bridge) }

    init(
        path: URL = Self.defaultWorkspaceURL,
        mode: WorkspaceOpenMode = .openOrCreate
    ) throws {
        switch workspaceSelectionProblem(path: path.path, mode: mode.rawValue) {
        case .notFound: throw WorkspaceSelectionError.workspaceNotFound
        case .alreadyExists: throw WorkspaceSelectionError.workspaceAlreadyExists
        case nil: break
        }
        // A new workspace starts empty; the first request creates the top-level list, so the
        // sidebar never opens on an empty placeholder collection. Its name follows the folder.
        bridge = try WorkspaceBridge.openOrCreate(path: path.path, name: Self.defaultName(for: path))
        location = path
    }

    /// The folder name, except for the built-in workspace in Application Support.
    static func defaultName(for path: URL) -> String {
        workspaceDefaultName(path: path.standardizedFileURL.path, builtIn: builtInWorkspaceURL.standardizedFileURL.path)
    }

    func load() async throws -> WorkspaceDraft {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            WorkspaceDraft(domain: try bridge.domainSnapshot())
        }.value
    }

    func loadSession(_ application: AppWorkspaceSession) async throws -> WorkspaceDraft {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            WorkspaceDraft(domain: try bridge.loadSession(application: application))
        }.value
    }

    func save(request: RequestDraft, in collectionID: String) async throws {
        _ = try await apply(.saveRequest(location: RequestLocation(collectionID: collectionID, request: request).domain))
    }
    func save(environment: EnvironmentDraft) async throws { _ = try await apply(.saveEnvironment(environment: environment.domain)) }

    func readSecret(name: String) async throws -> String? {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try bridge.readSecret(name: name)
        }.value
    }

    func saveSecret(name: String, value: String) async throws {
        let bridge = bridge
        try await Task.detached(priority: .userInitiated) {
            try bridge.saveSecret(name: name, value: value)
        }.value
    }

    func deleteSecret(name: String) async throws {
        let bridge = bridge
        try await Task.detached(priority: .userInitiated) {
            try bridge.deleteSecret(name: name)
        }.value
    }

    func apply(_ command: SessionWorkspaceCommand) async throws -> WorkspaceDelta {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            let delta = try bridge.applySessionCommand(command: command)
            return WorkspaceDelta(version: delta.version, kind: delta.kind, affectedIDs: delta.affectedIds)
        }.value
    }
    func applySessionCommands(_ commands: [SessionWorkspaceCommand]) async -> SessionCommandBatchResult {
        let bridge = bridge
        return await Task.detached(priority: .userInitiated) { bridge.executeSessionCommands(commands: commands) }.value
    }

    private func execute(_ request: WorkspaceIo) async throws -> WorkspaceReply {
        let reply = await sessionWorkspacePlatform.execute(request: request)
        if case .failed(let failure) = reply {
            throw GitFailure(kind: failure.kind, reason: failure.reason)
        }
        return reply
    }
    func status() async throws -> GitStatusSnapshot {
        guard case .gitStatus(let status) = try await execute(.git(action: .status)) else { throw WorkspaceMutationError.unsupported }
        return GitStatusSnapshot(status)
    }
    func initializeRepository() async throws -> GitStatusSnapshot {
        guard case .gitStatus(let status) = try await execute(.git(action: .initialize)) else { throw WorkspaceMutationError.unsupported }
        return GitStatusSnapshot(status)
    }
    private func git(_ action: WorkspaceGitAction) async throws -> GitOperationSnapshot {
        guard case .gitOperation(let operation) = try await execute(.git(action: action)) else { throw WorkspaceMutationError.unsupported }
        return GitOperationSnapshot(operation)
    }
    func pull() async throws -> GitOperationSnapshot { try await git(.pull) }
    func commit(message: String) async throws -> GitOperationSnapshot { try await git(.commit(message: message)) }
    func push() async throws -> GitOperationSnapshot { try await git(.push) }
    func abortMerge() async throws -> GitOperationSnapshot { try await git(.abort) }

    static var defaultWorkspaceURL: URL {
        let arguments = ProcessInfo.processInfo.arguments
        if let index = arguments.firstIndex(of: "--workspace"), arguments.indices.contains(index + 1) {
            return URL(fileURLWithPath: arguments[index + 1], isDirectory: true)
        }
        if let path = UserDefaults.standard.string(forKey: "workspace.lastOpenedPath") {
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        return builtInWorkspaceURL
    }

    /// The workspace used until another one is created or opened.
    static var builtInWorkspaceURL: URL {
        let applicationSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        )[0]
        return
            applicationSupport
            .appending(path: "Wirebolt", directoryHint: .isDirectory)
            .appending(path: "Workspaces", directoryHint: .isDirectory)
            .appending(path: "Default", directoryHint: .isDirectory)
    }

}

enum WorkspaceOpenMode: String {
    case open
    case create
    case openOrCreate
}

enum WorkspaceSelectionError: LocalizedError {
    case workspaceNotFound
    case workspaceAlreadyExists
    var errorDescription: String? {
        switch self {
        case .workspaceNotFound: "Choose a folder containing wirebolt.toml."
        case .workspaceAlreadyExists: "This folder already contains a workspace. Use Open Workspace."
        }
    }
}
