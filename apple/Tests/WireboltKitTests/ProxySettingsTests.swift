import Foundation
import Testing
import Synchronization
@testable import WireboltKit

@Suite("Proxy settings")
struct ProxySettingsTests {
    static let manual = ProxyDocument.manual(routes: [ProxyRouteDocument(destination: "all", endpoint: "http://localhost:8080")])

    @Test("Request overrides workspace, workspace overrides app, and each selection replaces the whole policy")
    func precedence() {
        for app in [Self.manual, .system, .direct] {
            let inherited = EffectiveProxy.resolve(request: nil, workspace: nil, app: app)
            #expect(inherited.configuration == app)
            #expect(inherited.source == .appDefault)
            for workspace in [Self.manual, .system, .direct] {
                #expect(EffectiveProxy.resolve(request: nil, workspace: workspace, app: app).configuration == workspace)
                #expect(EffectiveProxy.resolve(request: nil, workspace: workspace, app: app).source == .workspace)
                for request in [Self.manual, .system, .direct] {
                    let effective = EffectiveProxy.resolve(request: request, workspace: workspace, app: app)
                    #expect(effective.configuration == request)
                    #expect(effective.source == .request)
                }
            }
        }
    }

    @Test("App settings survive reopening and never modify another preferences store")
    @MainActor func preferences() throws {
        let name = "wirebolt-proxy-test-\(UUID())"
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }
        let preferences = ProxyPreferences(defaults: defaults)
        #expect(preferences.configuration == .system)
        try preferences.save(Self.manual)
        #expect(ProxyPreferences(defaults: defaults).configuration == Self.manual)
        #expect(ProxyPreferences().configuration == .system)
        #expect(throws: ProxyValidationError.self) { try preferences.save(.manual(routes: [])) }
        #expect(preferences.configuration == Self.manual)
    }

    @Test("Manual editor rejects invalid ports, overlap, credentials in endpoints and SOCKS4 authentication")
    func validation() throws {
        var form = ProxyFormDraft(configuration: Self.manual)
        form.routes[0].port = "70000"
        #expect(throws: ProxyValidationError.self) { try form.document() }
        form.routes[0].port = "abc"
        #expect(throws: ProxyValidationError.self) { try form.document() }
        form.routes[0].port = "8080"
        form.routes[0].host = "user:password@localhost"
        #expect(throws: ProxyValidationError.self) { try form.document() }
        form.routes[0].host = "localhost"
        form.routes.append(form.routes[0])
        #expect(throws: ProxyValidationError.self) { try form.document() }
        form.routes = [form.routes[0]]
        form.routes[0].scheme = "socks4"
        form.routes[0].authenticated = true
        form.routes[0].username = "user"
        form.routes[0].password = "sensitive-password"
        #expect(throws: ProxyValidationError.self) { try form.document() }
        form.routes[0].scheme = "socks5h"
        #expect(try form.document() != nil)
    }

    @Test("Authentication encodes references only and unchanged credentials are preserved")
    func credentials() throws {
        var form = ProxyFormDraft(configuration: Self.manual)
        form.routes[0].authenticated = true
        form.routes[0].username = "sensitive-user"
        form.routes[0].password = "sensitive-password"
        let document = try #require(try form.document())
        let encoded = String(decoding: try JSONEncoder().encode(document), as: UTF8.self)
        #expect(!encoded.contains("sensitive-user"))
        #expect(!encoded.contains("sensitive-password"))
        #expect(form.secrets.count == 2)
        let reopened = ProxyFormDraft(configuration: document)
        #expect(try reopened.document() == document)
        #expect(reopened.secrets.isEmpty)
    }

    @Test("Route preview maps WebSocket schemes and reports bypass when no route matches")
    func summaries() {
        let config = ProxyDocument.manual(routes: [ProxyRouteDocument(destination: "https", endpoint: "socks5h://localhost:1080")])
        let effective = EffectiveProxy.resolve(request: nil, workspace: config, app: .direct)
        #expect(effective.summary(for: "wss://example.com") == "SOCKS5H · localhost:1080")
        #expect(effective.summary(for: "http://example.com") == "Direct · No matching proxy route")
    }

    @Test("Model applies only the selected scope, persists workspace reset and sends app policy through the bridge")
    @MainActor func applyAndSend() async throws {
        let persistence = ProxyPersistence()
        let runner = ProxyRunner()
        let model = WireboltModel(runner: runner, persistence: persistence)
        let session = model.sessions.open(draft: RequestDraft(url: "http://example.invalid"))
        try await model.applyProxy(Self.manual, scope: .app)
        #expect(model.workspace.proxy == nil)
        #expect(session.draft.proxy == .inherit)
        try await model.applyProxy(.direct, scope: .workspace)
        #expect(model.proxyPreferences.configuration == Self.manual)
        try await model.applyProxy(.system, scope: .request, session: session)
        #expect(model.workspace.proxy == .direct)
        #expect(session.draft.proxy == .system)
        #expect(session.isDirty)
        await model.send(session)
        #expect(await runner.input?.appProxy == Self.manual)
        #expect(await runner.input?.workspaceProxy == .direct)
        #expect(await runner.input?.requestProxy == .system)
        try await model.applyProxy(nil, scope: .workspace)
        #expect(model.workspace.proxy == nil)
        #expect(await persistence.commands == [.saveWorkspaceProxy(.direct), .saveWorkspaceProxy(nil)])
        try await model.applyProxy(nil, scope: .request, session: session)
        #expect(model.effectiveProxy(for: session.draft).source == .appDefault)
        let curl = await model.curlCommand(for: session.draft)
        #expect(curl?.contains("localhost:8080") == true)
    }

    @Test("Proxy credentials are saved before the policy, and a failed Keychain write leaves it inactive")
    @MainActor func credentialSave() async throws {
        var form = ProxyFormDraft(configuration: Self.manual)
        form.routes[0].authenticated = true
        form.routes[0].username = "fixture-user"
        form.routes[0].password = "fixture-password"
        let document = try form.document()
        let persistence = ProxyPersistence()
        let model = WireboltModel(runner: ProxyRunner(), persistence: persistence)
        try await model.applyProxy(document, scope: .workspace, secrets: form.secrets)
        #expect(await persistence.savedSecrets == form.secrets)
        #expect(await persistence.commands == [.saveWorkspaceProxy(document)])
        let failing = ProxyPersistence(rejectSecrets: true)
        let other = WireboltModel(runner: ProxyRunner(), persistence: failing)
        do { try await other.applyProxy(document, scope: .workspace, secrets: form.secrets); Issue.record("Keychain failure should stop the save") }
        catch {
            #expect(other.workspace.proxy == nil)
            #expect(await failing.commands.isEmpty)
        }
    }

    @Test("WebSocket captures the app policy and only applies changes on reconnect")
    @MainActor func webSocketPolicy() async throws {
        let connector = ProxySocketConnector()
        let model = WireboltModel(runner: ProxyRunner(), socketConnector: connector)
        let session = model.sessions.open(draft: RequestDraft(url: "ws://example.invalid/socket"))
        try await model.applyProxy(Self.manual, scope: .app)
        await model.connectWebSocket(session)
        #expect(connector.input.withLock { $0?.appProxy } == Self.manual)
        try await model.applyProxy(.direct, scope: .app)
        #expect(connector.input.withLock { $0?.appProxy } == Self.manual)
        session.socket.disconnect()
        await model.connectWebSocket(session)
        #expect(connector.input.withLock { $0?.appProxy } == .direct)
        session.socket.disconnect()
    }

    @Test("Connection probe sends only HEAD with the selected policy, has no document side effects, and handles HTTP 407")
    @MainActor func connectionProbe() async {
        let runner = ProbeRunner(status: 200)
        let model = WireboltModel(runner: runner)
        let probe = model.makeProxyConnectionTest()
        probe.start(configuration: Self.manual, url: "http://example.invalid/health")
        for _ in 0..<100 where probe.isRunning { await Task.yield() }
        let input = runner.input.withLock { $0 }
        #expect(input?.method == "HEAD")
        #expect(input?.requestProxy == Self.manual)
        #expect(input?.headers.isEmpty == true)
        #expect(input?.body == .empty)
        #expect(input?.authentication == RequestAuthentication.none)
        #expect(input?.variables.isEmpty == true)
        #expect(input?.totalTimeoutMS == 5000)
        #expect(input?.followRedirects == false)
        #expect(probe.succeeded)
        #expect(model.sessions.sessions.isEmpty)
        #expect(model.workspace.proxy == nil)
        let authProbe = ProxyConnectionTest(runner: ProbeRunner(status: 407))
        authProbe.start(configuration: Self.manual, url: "http://example.invalid/health")
        for _ in 0..<100 where authProbe.isRunning { await Task.yield() }
        #expect(!authProbe.succeeded)
        #expect(authProbe.message?.contains("407") == true)
    }

    @Test("Cancelling a queued probe stops it before transport starts, and invalid URLs never run")
    @MainActor func cancelProbe() async {
        let runner = ProbeRunner(status: 200)
        let probe = ProxyConnectionTest(runner: runner)
        probe.start(configuration: .direct, url: "https://example.invalid")
        probe.cancel()
        await Task.yield()
        #expect(runner.input.withLock { $0 } == nil)
        #expect(!probe.isRunning)
        #expect(probe.message == "Test cancelled.")
        for url in ["file:///etc/passwd", "{{host}}", "https://user:password@example.invalid", "invalid"] {
            probe.start(configuration: .direct, url: url)
            #expect(!probe.isRunning)
            #expect(runner.input.withLock { $0 } == nil)
        }
    }

    @Test("Failed persistence never activates an unsaved workspace policy")
    @MainActor func failedSave() async {
        let model = WireboltModel(runner: ProxyRunner())
        do { try await model.applyProxy(Self.manual, scope: .workspace); Issue.record("Save should fail") }
        catch { #expect(model.workspace.proxy == nil) }
    }
}

private actor ProxyPersistence: WorkspacePersistence {
    var commands: [WorkspaceCommand] = []
    var savedSecrets: [String: String] = [:]
    let rejectSecrets: Bool
    init(rejectSecrets: Bool = false) { self.rejectSecrets = rejectSecrets }
    func load() async throws -> WorkspaceDraft { WorkspaceDraft(name: "Test") }
    func save(request: RequestDraft, in collectionID: String) async throws {}
    func save(environment: EnvironmentDraft) async throws {}
    func saveSecret(name: String, value: String) async throws {
        if rejectSecrets { throw WorkspaceMutationError.unsupported }
        savedSecrets[name] = value
    }
    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta {
        commands.append(command)
        return WorkspaceDelta(version: 1, kind: .workspace, affectedIDs: [])
    }
}
private actor ProxyRunner: RequestRunner {
    var input: RunInput?
    nonisolated func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { continuation in
            Task { await capture(input); continuation.finish() }
        }
    }
    func capture(_ value: RunInput) { input = value }
    nonisolated func cancel(runID: RunID) {}
}

private final class ProxySocketConnector: WebSocketConnecting {
    let input = Mutex<RunInput?>(nil)
    func connection(for input: RunInput) -> any WebSocketTransport {
        self.input.withLock { $0 = input }
        return ProxySocketTransport()
    }
}
private struct ProxySocketTransport: WebSocketTransport {
    func events() -> AsyncThrowingStream<WebSocketEvent, any Error> { AsyncThrowingStream { _ in } }
    func send(_ data: Data, binary: Bool) -> Bool { true }
    func disconnect() {}
}

private final class ProbeRunner: RequestRunner {
    let input = Mutex<RunInput?>(nil)
    let status: UInt16
    init(status: UInt16) { self.status = status }
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        self.input.withLock { $0 = input }
        return AsyncThrowingStream { continuation in
            continuation.yield(.head(ResponseHead(status: status, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1)))
            continuation.finish()
        }
    }
    func cancel(runID: RunID) {}
}
