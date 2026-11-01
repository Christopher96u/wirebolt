import Foundation
import Observation

/// Observation and task lifetime adapter for Rust's isolated probe state.
@MainActor @Observable
public final class ProxyConnectionTest {
    public private(set) var isRunning = false
    public private(set) var message: String?
    public private(set) var succeeded = false
    @ObservationIgnored private let state = DomainProxyProbe()
    @ObservationIgnored private let runner: any RequestRunner
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var runID: RunID?
    public init(runner: any RequestRunner) { self.runner = runner }
    public static func validTarget(_ value: String) -> Bool { domainValidProxyTarget(url: value) }
    private func synchronize() {
        let snapshot = state.snapshot()
        if isRunning != snapshot.isRunning { isRunning = snapshot.isRunning }
        if message != snapshot.message { message = snapshot.message }
        if succeeded != snapshot.succeeded { succeeded = snapshot.succeeded }
    }
    public func start(configuration: ProxyDocument, url: String) {
        cancel()
        let draft = state.start(configuration: configuration.domain, url: url)
        synchronize()
        guard let draft, let token = state.snapshot().runId, let uuid = UUID(uuidString: token) else { return }
        let id = RunID(rawValue: uuid)
        runID = id
        let input = RunInput(draft: RequestDraft(domain: draft), variables: [:])
        task = Task { [weak self, runner, state] in
            guard !Task.isCancelled, state.active(id: token) else { return }
            var status: UInt16?
            do {
                for try await event in runner.events(for: input, runID: id) {
                    guard !Task.isCancelled, state.active(id: token) else { return }
                    if case .head(let head) = event { status = head.status }
                }
                state.complete(id: token, status: status, failure: nil)
            } catch {
                let category = RunFailureMessage((error as? RunFailure) ?? RunFailure(kind: "bridge", issues: [])).category
                state.complete(id: token, status: nil, failure: String(describing: category))
            }
            self?.synchronize()
            if self?.runID == id { self?.runID = nil }
        }
    }
    public func cancel() {
        state.cancel()
        if let runID { runner.cancel(runID: runID) }
        runID = nil
        task?.cancel()
        task = nil
        synchronize()
    }
    public func reset() {
        cancel()
        state.reset()
        synchronize()
    }
}
