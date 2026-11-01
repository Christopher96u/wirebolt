import Foundation

// Compatibility stores implement this typed seam. Native stores provide a direct
// Rust platform, so a batch never round-trips through Swift for each command.
final class WorkspacePlatformAdapter: WorkspaceIoPlatform {
    let persistence: (any WorkspacePersistence)?
    let git: (any GitCollaboration)?
    init(persistence: (any WorkspacePersistence)? = nil, git: (any GitCollaboration)? = nil) {
        self.persistence = persistence
        self.git = git
    }
    func execute(request: WorkspaceIo, reply: WorkspaceIoReply) {
        Task {
            do { reply.complete(reply: try await execute(request)) } catch let error as GitFailure {
                reply.complete(reply: .failed(failure: WorkspaceFailure(kind: error.kind, reason: error.reason, localized: true)))
            } catch {
                let reason = (error as? LocalizedError)?.errorDescription
                reply.complete(
                    reply: .failed(
                        failure: WorkspaceFailure(kind: .bridge, reason: reason ?? error.localizedDescription, localized: reason != nil)))
            }
        }
    }
    private func requiredPersistence() throws -> any WorkspacePersistence {
        guard let persistence else { throw WorkspaceMutationError.unsupported }
        return persistence
    }

    private func execute(_ request: WorkspaceIo) async throws -> WorkspaceReply {
        switch request {
        case .load:
            let persistence = try requiredPersistence()
            return .loaded(workspace: try await persistence.load().domain)
        case .commands(let commands):
            let persistence = try requiredPersistence()
            return .commands(result: await persistence.applySessionCommands(commands))
        case .import(let format, let source, let name):
            let persistence = try requiredPersistence()
            let result =
                if let name { try await persistence.commitImportFile(format: format, source: source, name: name) } else {
                    try await persistence.commitImport(format: format, source: source)
                }
            return .imported(summary: result.summary.workspaceRecord)
        case .export(let target):
            let persistence = try requiredPersistence()
            let content: String
            switch target {
            case .workspace: content = try await persistence.exportWorkspace()
            case .collection(let id): content = try await persistence.exportCollection(id: id)
            case .request(let collectionID, let id): content = try await persistence.exportRequest(collectionID: collectionID, id: id)
            }
            return .exported(content: content)
        case .git(let action):
            guard let git else { throw WorkspaceMutationError.unsupported }
            switch action {
            case .status: return .gitStatus(status: try await git.status().workspaceRecord)
            case .initialize: return .gitStatus(status: try await git.initializeRepository().workspaceRecord)
            case .pull: return .gitOperation(operation: try await git.pull().workspaceRecord)
            case .commit(let message): return .gitOperation(operation: try await git.commit(message: message).workspaceRecord)
            case .push: return .gitOperation(operation: try await git.push().workspaceRecord)
            case .abort: return .gitOperation(operation: try await git.abortMerge().workspaceRecord)
            }
        }
    }
}

extension ImportSummary {
    var workspaceRecord: WorkspaceImportSummary {
        WorkspaceImportSummary(
            collectionNames: collectionNames, requestCount: UInt64(requestCount), groupCount: UInt64(groupCount),
            environments: environments.map { WorkspaceImportedEnvironment(id: $0.id, name: $0.name) }, warnings: warnings)
    }
    init(_ record: WorkspaceImportSummary) {
        self.init(
            collectionNames: record.collectionNames, requestCount: Int(record.requestCount), groupCount: Int(record.groupCount),
            environments: record.environments.map { .init(id: $0.id, name: $0.name) }, warnings: record.warnings)
    }
}
extension GitStatusSnapshot {
    var workspaceRecord: WorkspaceGitStatus {
        WorkspaceGitStatus(
            branch: branch, upstream: upstream, ahead: ahead, behind: behind,
            changes: changes.map {
                WorkspaceGitChange(
                    path: $0.path, previousPath: $0.previousPath, staged: $0.staged, unstaged: $0.unstaged, conflicted: $0.conflicted)
            }, merging: merging)
    }
    init(_ record: WorkspaceGitStatus) {
        self.init(
            branch: record.branch, upstream: record.upstream, ahead: record.ahead, behind: record.behind,
            changes: record.changes.map {
                GitChangeSnapshot(
                    path: $0.path, previousPath: $0.previousPath,
                    staged: $0.staged, unstaged: $0.unstaged, conflicted: $0.conflicted)
            }, merging: record.merging)
    }
}
extension GitOperationSnapshot {
    var workspaceRecord: WorkspaceGitOperation {
        WorkspaceGitOperation(outcome: outcome, revision: revision, status: status.workspaceRecord)
    }
    init(_ record: WorkspaceGitOperation) {
        self.init(outcome: record.outcome, revision: record.revision, status: GitStatusSnapshot(record.status))
    }
}

/// Captures native editor projections at Rust-selected save boundaries.
final class WorkspaceDocumentAdapter: WorkspaceDocumentPlatform {
    private let readDraft: @MainActor @Sendable (String) -> SessionDocument?
    private let didSave: @MainActor @Sendable (WorkspaceSave) -> Void
    init(read: @escaping @MainActor @Sendable (String) -> SessionDocument?, saved: @escaping @MainActor @Sendable (WorkspaceSave) -> Void) {
        readDraft = read
        didSave = saved
    }
    func read(id: String, reply: WorkspaceDocumentReply) {
        Task { @MainActor in reply.finish(document: readDraft(id)) }
    }
    func saved(result: WorkspaceSave, reply: WorkspaceDocumentReply) {
        Task { @MainActor in
            didSave(result)
            reply.finish(document: nil)
        }
    }
}
