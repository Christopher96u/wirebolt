import Foundation

struct RustWorkspacePersistence: WorkspacePersistence, GitCollaboration {
    private let bridge: WorkspaceBridge

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
        bridge = try WorkspaceBridge.openOrCreate(path: path.path, name: "Wirebolt")
        if !workspaceExisted {
            try bridge.saveCollection(id: "requests", name: "Requests")
        }
    }

    func load() async throws -> WorkspaceDraft {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            let data = Data(try bridge.snapshotJson().utf8)
            return try JSONDecoder().decode(WorkspaceSnapshotDocument.self, from: data).workspace
        }.value
    }

    func save(request: RequestDraft, in collectionID: String) async throws {
        let bridge = bridge
        let document = SavedRequestDocument(request: request)
        try await Task.detached(priority: .userInitiated) {
            let data = try JSONEncoder.wirebolt.encode(document)
            try bridge.saveRequest(
                collectionId: collectionID,
                requestJson: String(decoding: data, as: UTF8.self)
            )
        }.value
    }

    func save(environment: EnvironmentDraft) async throws {
        let bridge = bridge
        try await Task.detached(priority: .userInitiated) {
            let data = try JSONEncoder.wirebolt.encode(environment)
            try bridge.saveEnvironment(environmentJson: String(decoding: data, as: UTF8.self))
        }.value
    }

    func saveSecret(name: String, value: String) async throws {
        let bridge = bridge
        try await Task.detached(priority: .userInitiated) {
            try bridge.saveSecret(name: name, value: value)
        }.value
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

    static var defaultWorkspaceURL: URL {
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

private enum WorkspaceSelectionError: Error {
    case workspaceNotFound
    case workspaceAlreadyExists
}

private struct WorkspaceSnapshotDocument: Decodable {
    let name: String
    let proxy: ProxyDocument?
    let collections: [CollectionSnapshotDocument]
    let environments: [EnvironmentDraft]

    var workspace: WorkspaceDraft {
        WorkspaceDraft(
            name: name,
            proxy: proxy,
            collections: collections.map(\.collection),
            environments: environments
        )
    }
}

private struct CollectionSnapshotDocument: Decodable {
    let id: String
    let name: String
    let requests: [SavedRequestDocument]

    var collection: CollectionDraft {
        CollectionDraft(
            id: id,
            name: name,
            requests: requests.map {
                RequestLocation(collectionID: id, request: $0.request)
            }
        )
    }
}

private struct SavedRequestDocument: Codable {
    let id: String
    let name: String
    let method: String
    let url: String
    let query: [RequestField]
    let headers: [RequestField]
    let authentication: RequestAuthentication
    let body: RequestBody
    let proxy: ProxyDocument?

    init(request: RequestDraft) {
        id = request.id
        name = request.name
        method = request.method.rawValue
        url = request.url
        query = request.query
        headers = request.headers
        authentication = request.authentication
        body = request.body
        proxy = switch request.proxy {
        case .inherit: nil
        case .system: .system
        case .direct: .direct
        case let .manual(document): document
        }
    }

    var request: RequestDraft {
        let proxySelection: ProxySelection = switch proxy {
        case nil: .inherit
        case .system: .system
        case .direct: .direct
        case let .manual(routes): .manual(.manual(routes: routes))
        }
        return RequestDraft(
            id: id,
            name: name,
            method: HTTPMethod(rawValue: method) ?? .get,
            url: url,
            query: query,
            headers: headers,
            authentication: authentication,
            body: body,
            proxy: proxySelection
        )
    }
}

private extension JSONEncoder {
    static var wirebolt: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }
}
