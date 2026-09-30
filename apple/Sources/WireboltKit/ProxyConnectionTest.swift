import Foundation
import Observation

/// An isolated transport probe: no document session, cookies, auth, history or parent mutations.
@MainActor @Observable
public final class ProxyConnectionTest {
    public private(set) var isRunning = false
    public private(set) var message: String?
    public private(set) var succeeded = false
    @ObservationIgnored private let runner: any RequestRunner
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var runID: RunID?
    public init(runner: any RequestRunner) { self.runner = runner }

    public static func validTarget(_ value: String) -> Bool {
        guard let url = URLComponents(string: value), ["http", "https"].contains(url.scheme?.lowercased() ?? ""),
              url.host?.isEmpty == false, url.user == nil, url.password == nil, !value.contains("{{") else { return false }
        return true
    }

    public func start(configuration: ProxyDocument, url: String) {
        cancel()
        guard Self.validTarget(url), (try? configuration.validate()) != nil else {
            message = "Enter a valid HTTP or HTTPS URL and proxy configuration."; return
        }
        let id = RunID()
        runID = id; isRunning = true; succeeded = false; message = nil
        let draft = RequestDraft(method: .head, url: url, proxy: ProxySelection(document: configuration),
                                 transport: TransportSettings(totalTimeoutMS: 5000, readTimeoutMS: 5000))
        let input = RunInput(draft: draft, variables: [:])
        task = Task { [weak self, runner] in
            guard !Task.isCancelled, self?.runID == id else { return }
            var status: UInt16?
            do {
                for try await event in runner.events(for: input, runID: id) {
                    guard !Task.isCancelled, self?.runID == id else { return }
                    if case let .head(head) = event { status = head.status }
                }
                guard let self, self.runID == id else { return }
                self.succeeded = status != nil && status != 407
                self.message = status == 407 ? "Proxy authentication failed (HTTP 407). Check your credentials."
                    : status.map { "Reached destination · HTTP \($0)" } ?? "No HTTP response received."
                self.isRunning = false; self.runID = nil
            } catch {
                guard let self, self.runID == id else { return }
                let category = RunFailureMessage((error as? RunFailure) ?? RunFailure(kind: "bridge", issues: [])).category
                self.message = switch category {
                case .timeout: "Connection timed out after 5 seconds."
                case .tls: "TLS verification failed. Check the certificate and trust settings."
                case .proxy, .secrets: "Proxy configuration or credentials could not be loaded."
                case .cancelled: "Test cancelled."
                default: "Connection failed. Check the proxy address, port and destination."
                }
                self.isRunning = false; self.runID = nil
            }
        }
    }
    public func cancel() {
        if let runID { runner.cancel(runID: runID); message = "Test cancelled."; succeeded = false }
        runID = nil; task?.cancel(); task = nil; isRunning = false
    }
    public func reset() { cancel(); message = nil; succeeded = false }
}
