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

    @ObservationIgnored private let runner: any RequestRunner
    @ObservationIgnored private var persistence: (any WorkspacePersistence)?
    @ObservationIgnored private var presentedBodyBytes = 0

    public init(runner: any RequestRunner, persistence: (any WorkspacePersistence)? = nil) {
        self.runner = runner
        self.persistence = persistence
    }

    public var activeVariables: [String: ValueSource] {
        workspace.environments
            .first(where: { $0.id == selectedEnvironmentID })?
            .variables ?? [:]
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

    public func openWorkspace(using persistence: any WorkspacePersistence) async {
        self.persistence = persistence
        await loadWorkspace()
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
}
