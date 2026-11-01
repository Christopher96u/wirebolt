import Foundation

struct RustWorkspacePersistence: WorkspacePersistence, GitCollaboration {
    private let bridge: WorkspaceBridge
    let location: URL?
    var runtimeWorkspaceBridge: WorkspaceBridge? { bridge }
    var sessionSecretPlatform: AppSecretPlatform { AppSecretPlatform.workspace(bridge: bridge) }

    init(
        path: URL = Self.defaultWorkspaceURL,
        mode: WorkspaceOpenMode = .openOrCreate
    ) throws {
        let manifest = path.appending(path: "wirebolt.toml")
        let workspaceExisted = FileManager.default.fileExists(atPath: manifest.path)
        switch mode {
        case .open where !workspaceExisted:
            throw WorkspaceSelectionError.workspaceNotFound
        case .create where workspaceExisted:
            throw WorkspaceSelectionError.workspaceAlreadyExists
        case .open, .create, .openOrCreate:
            break
        }
        // A new workspace starts empty; the first request creates the top-level list, so the
        // sidebar never opens on an empty placeholder collection. Its name follows the folder.
        bridge = try WorkspaceBridge.openOrCreate(path: path.path, name: Self.defaultName(for: path))
        location = path
    }

    /// The folder name, except for the built-in workspace in Application Support.
    static func defaultName(for path: URL) -> String {
        let name = path.standardizedFileURL.lastPathComponent.trimmingCharacters(in: .whitespacesAndNewlines)
        guard path.standardizedFileURL != builtInWorkspaceURL.standardizedFileURL, !name.isEmpty, name != "/" else {
            return "Wirebolt"
        }
        return name
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
        _ = try await apply(.saveRequest(collectionID: collectionID, location: RequestLocation(collectionID: collectionID, request: request)))
    }
    func save(environment: EnvironmentDraft) async throws { _ = try await apply(.saveEnvironment(environment)) }

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

    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            let delta = try bridge.applySessionCommand(command: command.sessionRecord)
            return WorkspaceDelta(version: delta.version, kind: WorkspaceNodeKind(rawValue: delta.kind)!, affectedIDs: delta.affectedIds)
        }.value
    }
    func applySessionCommands(_ commands: [SessionWorkspaceCommand]) async -> SessionCommandBatchResult {
        let bridge = bridge
        return await Task.detached(priority: .userInitiated) { bridge.executeSessionCommands(commands: commands) }.value
    }

        func previewImport(format: ImportFormat, source: String) async throws -> ImportPreview {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            let json = try Self.importDocument { try bridge.previewImport(format: format.rawValue, source: source) }
            return try JSONDecoder().decode(ImportPreview.self, from: Data(json.utf8))
        }.value
    }

    func commitImport(format: ImportFormat, source: String) async throws -> ImportResult {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            let json = try Self.importDocument { try bridge.commitImport(format: format.rawValue, source: source) }
            return try JSONDecoder().decode(ImportResult.self, from: Data(json.utf8))
        }.value
    }

    func commitImportFile(format: ImportFormat, source: String, name: String) async throws -> ImportResult {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            let json = try Self.importDocument {
                try bridge.commitImportFile(format: format.rawValue, source: source, name: name)
            }
            return try JSONDecoder().decode(ImportResult.self, from: Data(json.utf8))
        }.value
    }

    /// Keeps the importer's user-facing reason, which never quotes the source.
    private static func importDocument(_ operation: () throws -> String) throws -> String {
        do { return try operation() }
        catch let WorkspaceBridgeError.OperationFailed(reason) {
            throw ImportFailure(reason: reason)
        }
    }

    private struct ImportFailure: LocalizedError {
        let reason: String
        var errorDescription: String? { reason }
    }

    func exportWorkspace() async throws -> String {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try Self.exportDocument { try bridge.exportWorkspaceJson() }
        }.value
    }

    func exportCollection(id: String) async throws -> String {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try Self.exportDocument { try bridge.exportCollectionJson(id: id) }
        }.value
    }

    func exportRequest(collectionID: String, id: String) async throws -> String {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try Self.exportDocument { try bridge.exportRequestJson(collectionId: collectionID, id: id) }
        }.value
    }

    private static func exportDocument(_ operation: () throws -> String) throws -> String {
        do { return try operation() }
        catch let WorkspaceBridgeError.OperationFailed(reason) {
            throw ExportError(reason: reason)
        }
    }

    private struct ExportError: LocalizedError {
        let reason: String
        var errorDescription: String? { reason }
    }

    func status() async throws -> GitStatusSnapshot {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try Self.decodeGitDocument(GitStatusSnapshot.self) {
                try bridge.gitStatusJson()
            }
        }.value
    }

    func pull() async throws -> GitOperationSnapshot {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try Self.decodeGitDocument(GitOperationSnapshot.self) {
                try bridge.gitPullJson()
            }
        }.value
    }

    func commit(message: String) async throws -> GitOperationSnapshot {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try Self.decodeGitDocument(GitOperationSnapshot.self) {
                try bridge.gitCommitJson(message: message)
            }
        }.value
    }

    func push() async throws -> GitOperationSnapshot {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try Self.decodeGitDocument(GitOperationSnapshot.self) {
                try bridge.gitPushJson()
            }
        }.value
    }

    func abortMerge() async throws -> GitOperationSnapshot {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try Self.decodeGitDocument(GitOperationSnapshot.self) {
                try bridge.gitAbortMergeJson()
            }
        }.value
    }

    func initializeRepository() async throws -> GitStatusSnapshot {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            try Self.decodeGitDocument(GitStatusSnapshot.self) {
                try bridge.gitInitializeJson()
            }
        }.value
    }

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
        return applicationSupport
            .appending(path: "Wirebolt", directoryHint: .isDirectory)
            .appending(path: "Workspaces", directoryHint: .isDirectory)
            .appending(path: "Default", directoryHint: .isDirectory)
    }

    private static func decodeGitDocument<Value: Decodable>(
        _ type: Value.Type,
        operation: () throws -> String
    ) throws -> Value {
        do {
            let json = try operation()
            return try JSONDecoder().decode(type, from: Data(json.utf8))
        } catch let GitBridgeError.OperationFailed(kind, reason) {
            throw GitFailure(kind: kind, reason: reason)
        } catch let error as GitFailure {
            throw error
        } catch {
            throw GitFailure(kind: "invalid_bridge_response", reason: "Git returned an invalid response.")
        }
    }
}

enum WorkspaceOpenMode {
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
