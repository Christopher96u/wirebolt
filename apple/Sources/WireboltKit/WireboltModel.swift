import Foundation
import Observation

public protocol RequestRunner: Sendable {
    func events(for input: RunInput) -> AsyncThrowingStream<RunEvent, any Error>
    func cancel()
}

public protocol WorkspacePersistence: Sendable {
    func load() async throws -> WorkspaceDraft
    func save(request: RequestDraft, in collectionID: String) async throws
    func save(environment: EnvironmentDraft) async throws
    func saveSecret(name: String, value: String) async throws
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
    public static let previewByteLimit = 5 * 1024 * 1024

    public var workspace = WorkspaceDraft(name: "Wirebolt")
    public var selectedRequestID: String?
    public var selectedEnvironmentID: String?
    public var draft = RequestDraft()
    public private(set) var responseHead: ResponseHead?
    public private(set) var responseText = ""
    public private(set) var responseBytes: UInt64 = 0
    public private(set) var responseWasTruncated = false
    public private(set) var completion: RunCompletion?
    public private(set) var failure: RunFailure?
    public private(set) var isRunning = false
    public private(set) var isLoadingWorkspace = false
    public private(set) var gitStatus: GitStatusSnapshot?
    public private(set) var gitOperation: GitOperationSnapshot?
    public private(set) var gitFailure: GitFailure?
    public private(set) var isGitBusy = false
    public var isShowingGitCollaboration = false

    @ObservationIgnored private let runner: any RequestRunner
    @ObservationIgnored private var persistence: (any WorkspacePersistence)?
    @ObservationIgnored private var gitCollaboration: (any GitCollaboration)?
    @ObservationIgnored private var presentedBodyBytes = 0

    public init(
        runner: any RequestRunner,
        persistence: (any WorkspacePersistence)? = nil,
        gitCollaboration: (any GitCollaboration)? = nil
    ) {
        self.runner = runner
        self.persistence = persistence
        self.gitCollaboration = gitCollaboration
    }

    public var activeVariables: [String: ValueSource] {
        workspace.environments
            .first(where: { $0.id == selectedEnvironmentID })?
            .variables ?? [:]
    }

    public var hasUnsavedRequestChanges: Bool {
        if let selectedRequestID,
           let saved = workspace.collections
               .flatMap(\.requests)
               .first(where: { $0.id == selectedRequestID })
        {
            return saved.request != draft
        }
        return draft != RequestDraft()
    }

    public func send() async {
        guard !isRunning else { return }
        resetResponse()
        isRunning = true
        defer { isRunning = false }

        do {
            for try await event in runner.events(for: RunInput(
                draft: draft,
                variables: activeVariables,
                workspaceProxy: workspace.proxy
            )) {
                if Task.isCancelled {
                    runner.cancel()
                    return
                }
                consume(event)
            }
        } catch let error as RunFailure {
            failure = error
        } catch {
            failure = RunFailure(kind: "bridge", issues: [])
        }
    }

    public func cancel() {
        runner.cancel()
    }

    public func loadWorkspace() async {
        guard let persistence else { return }
        isLoadingWorkspace = true
        defer { isLoadingWorkspace = false }
        do {
            workspace = try await persistence.load()
            selectedEnvironmentID = workspace.environments.first?.id
            if let first = workspace.collections.flatMap(\.requests).first {
                select(first)
            }
        } catch {
            failure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func openWorkspace(
        using persistence: any WorkspacePersistence,
        gitCollaboration: (any GitCollaboration)? = nil
    ) async {
        self.persistence = persistence
        self.gitCollaboration = gitCollaboration
        gitStatus = nil
        gitOperation = nil
        gitFailure = nil
        await loadWorkspace()
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
        selectedRequestID = location.id
        draft = location.request
        resetResponse()
    }

    public func saveCurrentRequest(collectionID: String) async {
        guard let persistence else { return }
        do {
            try await persistence.save(request: draft, in: collectionID)
            workspace = try await persistence.load()
            if let saved = workspace.collections
                .flatMap(\.requests)
                .first(where: { $0.request.id == draft.id })
            {
                select(saved)
            }
        } catch {
            failure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func saveEnvironment(_ environment: EnvironmentDraft) async {
        guard let persistence else { return }
        do {
            try await persistence.save(environment: environment)
            workspace = try await persistence.load()
            selectedEnvironmentID = environment.id
        } catch {
            failure = RunFailure(kind: "workspace", issues: [])
        }
    }

    public func saveSecret(name: String, value: String) async {
        guard let persistence else { return }
        do {
            try await persistence.saveSecret(name: name, value: value)
        } catch {
            failure = RunFailure(kind: "keychain", issues: [])
        }
    }

    public func makeNewRequest() {
        selectedRequestID = nil
        draft = RequestDraft(id: UUID().uuidString.lowercased(), name: "New Request")
        resetResponse()
    }

    public func makeNewEnvironment() -> EnvironmentDraft {
        EnvironmentDraft(
            id: UUID().uuidString.lowercased(),
            name: "New Environment"
        )
    }

    private func consume(_ event: RunEvent) {
        switch event {
        case let .head(head):
            responseHead = head
        case let .chunk(data):
            responseBytes += UInt64(data.count)
            let remaining = max(Self.previewByteLimit - presentedBodyBytes, 0)
            guard remaining > 0 else {
                responseWasTruncated = true
                return
            }
            let prefix = data.prefix(remaining)
            responseText.append(String(decoding: prefix, as: UTF8.self))
            presentedBodyBytes += prefix.count
            responseWasTruncated = prefix.count < data.count
        case let .complete(value):
            completion = value
            responseBytes = value.bytesReceived
        }
    }

    private func resetResponse() {
        responseHead = nil
        responseText = ""
        responseBytes = 0
        responseWasTruncated = false
        completion = nil
        failure = nil
        presentedBodyBytes = 0
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
            workspace = try await persistence.load()
            selectedEnvironmentID = workspace.environments.first?.id
            if let selectedRequestID,
               let selected = workspace.collections
                   .flatMap(\.requests)
                   .first(where: { $0.id == selectedRequestID })
            {
                select(selected)
            } else if let first = workspace.collections.flatMap(\.requests).first {
                select(first)
            } else {
                selectedRequestID = nil
                draft = RequestDraft()
            }
        } catch {
            gitFailure = GitFailure(
                kind: "workspace_reload",
                reason: "Git updated the workspace, but Wirebolt could not reload it."
            )
        }
    }
}
