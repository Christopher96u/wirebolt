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
        // A new workspace starts empty; the first request creates the top-level list, so the
        // sidebar never opens on an empty placeholder collection. Its name follows the folder.
        bridge = try WorkspaceBridge.openOrCreate(path: path.path, name: Self.defaultName(for: path))
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

    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta {
        let bridge = bridge
        let document = WorkspaceCommandDocument(command)
        return try await Task.detached(priority: .userInitiated) {
            let encoded = try JSONEncoder.wirebolt.encode(document)
            let response = try bridge.applyWorkspaceCommand(
                commandJson: String(decoding: encoded, as: UTF8.self)
            )
            return try JSONDecoder().decode(WorkspaceDelta.self, from: Data(response.utf8))
        }.value
    }

    func previewImport(format: ImportFormat, source: String) async throws -> ImportPreview {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            let json = try bridge.previewImport(format: format.rawValue, source: source)
            return try JSONDecoder().decode(ImportPreview.self, from: Data(json.utf8))
        }.value
    }

    func commitImport(format: ImportFormat, source: String) async throws -> WorkspaceDelta {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            let json = try bridge.commitImport(format: format.rawValue, source: source)
            return try JSONDecoder().decode(WorkspaceDelta.self, from: Data(json.utf8))
        }.value
    }

    func commitImportFile(format: ImportFormat, source: String, name: String) async throws -> WorkspaceDelta {
        let bridge = bridge
        return try await Task.detached(priority: .userInitiated) {
            let json = try bridge.commitImportFile(format: format.rawValue, source: source, name: name)
            return try JSONDecoder().decode(WorkspaceDelta.self, from: Data(json.utf8))
        }.value
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

private struct WorkspaceSnapshotDocument: Decodable {
    let name: String
    let proxy: ProxyDocument?
    let transport: TransportSettings
    let collections: [CollectionSnapshotDocument]
    let environments: [EnvironmentDraft]

    private enum CodingKeys: String, CodingKey {
        case name, proxy, transport, collections, environments
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        name = try container.decode(String.self, forKey: .name)
        proxy = try container.decodeIfPresent(ProxyDocument.self, forKey: .proxy)
        transport = try container.decodeIfPresent(TransportSettings.self, forKey: .transport)
            ?? TransportSettings()
        collections = try container.decode([CollectionSnapshotDocument].self, forKey: .collections)
        environments = try container.decode([EnvironmentDraft].self, forKey: .environments)
    }

    var workspace: WorkspaceDraft {
        WorkspaceDraft(
            name: name,
            proxy: proxy,
            transport: transport,
            collections: collections.map(\.collection),
            environments: environments
        )
    }
}

private struct CollectionSnapshotDocument: Decodable {
    let id: String
    let name: String
    let order: Int
    let groups: [GroupDraft]
    let requests: [SavedRequestDocument]

    private enum CodingKeys: String, CodingKey { case id, name, order, groups, requests }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        name = try container.decode(String.self, forKey: .name)
        order = try container.decodeIfPresent(Int.self, forKey: .order) ?? 0
        groups = try container.decodeIfPresent([GroupDraft].self, forKey: .groups) ?? []
        requests = try container.decode([SavedRequestDocument].self, forKey: .requests)
    }

    var collection: CollectionDraft {
        CollectionDraft(
            id: id,
            name: name,
            order: order,
            groups: groups,
            requests: requests.map {
                RequestLocation(
                    collectionID: id,
                    groupID: $0.groupID,
                    order: $0.order,
                    request: $0.request
                )
            }
        )
    }
}

private struct SavedRequestDocument: Codable {
    let id: String
    let name: String
    var groupID: String?
    var order: Int
    let method: String
    let url: String
    let webSocket: Bool?
    let note: String?
    let query: [RequestField]
    let headers: [RequestField]
    let authentication: RequestAuthentication
    let body: RequestBody
    let proxy: ProxyDocument?
    let transport: TransportSettings
    let inheritsWorkspaceTransport: Bool

    private enum CodingKeys: String, CodingKey {
        case id, name, order, method, url, query, headers, authentication, body, proxy, transport
        case webSocket = "web_socket"
        case note
        case inheritsWorkspaceTransport = "inherits_workspace_transport"
        case groupID = "group_id"
    }

    init(request: RequestDraft) {
        id = request.id
        name = request.name
        groupID = nil
        order = 0
        method = request.method.rawValue
        url = request.url
        webSocket = request.webSocket
        note = request.note
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
        transport = request.transport
        inheritsWorkspaceTransport = request.inheritsWorkspaceTransport
    }

    init(location: RequestLocation) {
        self.init(request: location.request)
        groupID = location.groupID
        order = location.order
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
            webSocket: webSocket ?? false,
            note: note ?? "",
            query: query,
            headers: headers,
            authentication: authentication,
            body: body,
            proxy: proxySelection,
            transport: transport,
            inheritsWorkspaceTransport: inheritsWorkspaceTransport
        )
    }
}

private struct WorkspaceCommandDocument: Encodable {
    private let command: WorkspaceCommand

    init(_ command: WorkspaceCommand) {
        self.command = command
    }

    private enum CodingKeys: String, CodingKey {
        case kind, id, name, order, groups, requests, group, location, request, transport
        case environment, proxy, items
        case collectionID = "collection_id"
        case parentID = "parent_id"
        case newID = "new_id"
        case fromCollectionID = "from_collection_id"
        case requestID = "request_id"
        case toCollectionID = "to_collection_id"
        case groupID = "group_id"
    }

    func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch command {
        case let .reorderChildren(collectionID, parentID, items):
            try container.encode("reorder_children", forKey: .kind)
            try container.encode(collectionID, forKey: .collectionID)
            try container.encodeIfPresent(parentID, forKey: .parentID)
            try container.encode(items, forKey: .items)
        case let .saveWorkspaceProxy(proxy):
            try container.encode("save_workspace_proxy", forKey: .kind)
            try container.encode(proxy, forKey: .proxy)
        case let .saveWorkspaceSettings(transport):
            try container.encode("save_workspace_settings", forKey: .kind)
            try container.encode(transport, forKey: .transport)
        case let .renameWorkspace(name):
            try container.encode("rename_workspace", forKey: .kind)
            try container.encode(name, forKey: .name)
        case let .createCollection(collection):
            try container.encode("create_collection", forKey: .kind)
            try container.encode(collection.id, forKey: .id)
            try container.encode(collection.name, forKey: .name)
            try container.encode(collection.order, forKey: .order)
        case let .renameCollection(id, name):
            try container.encode("rename_collection", forKey: .kind)
            try container.encode(id, forKey: .id)
            try container.encode(name, forKey: .name)
        case let .deleteCollection(id):
            try container.encode("delete_collection", forKey: .kind)
            try container.encode(id, forKey: .id)
        case let .createGroup(collectionID, group):
            try container.encode("create_group", forKey: .kind)
            try container.encode(collectionID, forKey: .collectionID)
            try container.encode(group, forKey: .group)
        case let .renameGroup(collectionID, id, name):
            try container.encode("rename_group", forKey: .kind)
            try container.encode(collectionID, forKey: .collectionID)
            try container.encode(id, forKey: .id)
            try container.encode(name, forKey: .name)
        case let .deleteGroup(collectionID, id):
            try container.encode("delete_group", forKey: .kind)
            try container.encode(collectionID, forKey: .collectionID)
            try container.encode(id, forKey: .id)
        case let .moveGroup(collectionID, id, parentID, order):
            try container.encode("move_group", forKey: .kind)
            try container.encode(collectionID, forKey: .collectionID)
            try container.encode(id, forKey: .id)
            try container.encodeIfPresent(parentID, forKey: .parentID)
            try container.encode(order, forKey: .order)
        case let .saveRequest(collectionID, location):
            try container.encode("save_request", forKey: .kind)
            try container.encode(collectionID, forKey: .collectionID)
            try container.encode(SavedRequestDocument(location: location), forKey: .request)
        case let .deleteRequest(collectionID, id):
            try container.encode("delete_request", forKey: .kind)
            try container.encode(collectionID, forKey: .collectionID)
            try container.encode(id, forKey: .id)
        case let .duplicateRequest(collectionID, id, newID, name):
            try container.encode("duplicate_request", forKey: .kind)
            try container.encode(collectionID, forKey: .collectionID)
            try container.encode(id, forKey: .id)
            try container.encode(newID, forKey: .newID)
            try container.encode(name, forKey: .name)
        case let .moveRequest(fromCollectionID, requestID, toCollectionID, groupID, order):
            try container.encode("move_request", forKey: .kind)
            try container.encode(fromCollectionID, forKey: .fromCollectionID)
            try container.encode(requestID, forKey: .requestID)
            try container.encode(toCollectionID, forKey: .toCollectionID)
            try container.encodeIfPresent(groupID, forKey: .groupID)
            try container.encode(order, forKey: .order)
        case let .saveEnvironment(environment):
            try container.encode("save_environment", forKey: .kind)
            try container.encode(environment, forKey: .environment)
        case let .deleteEnvironment(id):
            try container.encode("delete_environment", forKey: .kind)
            try container.encode(id, forKey: .id)
        }
    }
}

private extension JSONEncoder {
    static var wirebolt: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }
}
