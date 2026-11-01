import Foundation

/// Compatibility seam for injected runners. Production supplies AppRuntimeTransport.native().
final class RuntimeRunnerAdapter: SessionRuntimeTransport {
    let runner: any RequestRunner
    init(runner: any RequestRunner) { self.runner = runner }
    func resolve(values: [DomainValueSource], variables: [String: DomainValueSource], reply: SessionValuesReply) {
        Task {
            do {
                reply.finish(
                    values: try await runner.resolveValues(
                        values.map(ValueSource.init(domain:)), variables: variables.mapValues(ValueSource.init(domain:))), failure: nil)
            } catch {
                reply.finish(values: [], failure: ((error as? RunFailure) ?? RunFailure(kind: .invalidRequest, issues: [])).sessionRecord)
            }
        }
    }
    func execute(input: DomainRunInput, id: String, sink: SessionRunSink) {
        guard let uuid = UUID(uuidString: id) else {
            sink.finish(failure: RunFailure(kind: .bridge, issues: []).sessionRecord)
            return
        }
        Task {
            do {
                for try await event in runner.events(for: RunInput(domain: input), runID: RunID(rawValue: uuid)) {
                    switch event {
                    case .prepared(let value): sink.event(event: .prepared(snapshot: value.sessionRecord))
                    case .head(let value): sink.event(event: .head(head: value.sessionRecord))
                    case .chunk(let data): sink.bytes(data: data)
                    case .cookies(let value):
                        sink.cookies(url: value.url, headers: value.headers.map { ResponseHeaderValue(name: $0.name, value: $0.value) })
                    case .complete(let value): sink.event(event: .complete(completion: value.sessionRecord))
                    }
                }
                sink.finish(failure: nil)
            } catch is CancellationError { sink.abandon() } catch {
                sink.finish(failure: ((error as? RunFailure) ?? RunFailure(kind: .bridge, issues: [])).sessionRecord)
            }
        }
    }
    func cancel(id: String) { if let uuid = UUID(uuidString: id) { runner.cancel(runID: RunID(rawValue: uuid)) } }
}

final class RuntimePresentationObserver: SessionRuntimeObserver {
    private let update: @MainActor @Sendable () -> Void
    private let input: @MainActor @Sendable () -> RuntimeInput?
    init(update: @escaping @MainActor @Sendable () -> Void, input: @escaping @MainActor @Sendable () -> RuntimeInput?) {
        self.update = update
        self.input = input
    }
    func prepare(reply: SessionInputReply) { Task { @MainActor in reply.finish(input: input()) } }
    func changed() { Task { @MainActor in update() } }
}

/// Native preferences and persistence relay. Rust validates and sequences writes.
final class RuntimeSettingsAdapter: SessionRuntimeSettings {
    let persistence: (any WorkspacePersistence)?
    let preferences: ProxyPreferences
    @MainActor init(persistence: (any WorkspacePersistence)?, preferences: ProxyPreferences) {
        self.persistence = persistence
        self.preferences = preferences
    }
    func proxy(scope: RuntimeProxyScope, configuration: DomainProxy?, reply: SessionOperationReply) {
        Task { @MainActor in
            do {
                switch scope {
                case .app: try preferences.save(configuration.map(ProxyDocument.init(domain:)) ?? .system)
                case .workspace:
                    guard let persistence else { throw WorkspaceMutationError.unsupported }
                    _ = try await persistence.apply(.saveWorkspaceProxy(proxy: configuration))
                case .request: break
                }
                reply.finish(reason: nil)
            } catch { reply.finish(reason: error.localizedDescription) }
        }
    }
    func transport(value: DomainTransport, reply: SessionOperationReply) {
        Task {
            do {
                guard let persistence else { throw WorkspaceMutationError.unsupported }
                _ = try await persistence.apply(.saveWorkspaceSettings(transport: value))
                reply.finish(reason: nil)
            } catch { reply.finish(reason: error.localizedDescription) }
        }
    }
}

@MainActor final class RuntimeOAuthAdapter: SessionRuntimeAuthorizer {
    let authorizer: any OAuth2Authorizing
    let project: @MainActor (OAuth2Configuration) -> Void
    init(authorizer: any OAuth2Authorizing, project: @escaping @MainActor (OAuth2Configuration) -> Void) {
        self.authorizer = authorizer
        self.project = project
    }
    nonisolated func acquire(configuration: DomainOAuthConfiguration, reply: SessionTokenReply) {
        Task { @MainActor in
            do {
                let configuration = OAuth2Configuration(domain: configuration)
                project(configuration)
                let (token, receipt) = try await authorizer.acquireToken(configuration: configuration)
                reply.finish(
                    token: token, receipt: RuntimeTokenReceipt(expiresAt: receipt.expiresAt?.timeIntervalSince1970, scope: receipt.scope),
                    reason: nil)
            } catch {
                reply.finish(token: "", receipt: RuntimeTokenReceipt(expiresAt: nil, scope: nil), reason: error.localizedDescription)
            }
        }
    }
}

private struct RuntimeNativeError: LocalizedError {
    let errorDescription: String?
}
func runtimeError(_ error: any Error) -> any Error {
    switch error {
    case RuntimeOperationError.Unsupported: WorkspaceMutationError.unsupported
    case RuntimeOperationError.Validation(let reason): ProxyValidationError(reason)
    case RuntimeOperationError.Platform(let reason): RuntimeNativeError(errorDescription: reason)
    case RuntimeOperationError.Workspace(let reason): WorkspaceBridgeError.OperationFailed(reason: reason)
    default: error
    }
}

extension WebSocketStatus {
    var rust: SessionSocketStatus {
        switch self {
        case .disconnected: .disconnected
        case .connecting: .connecting
        case .connected: .connected
        }
    }
}

extension ProxyScope {
    var rust: RuntimeProxyScope {
        switch self {
        case .app: .app
        case .workspace: .workspace
        case .request: .request
        }
    }
}
