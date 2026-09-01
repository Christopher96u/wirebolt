import Foundation
import Testing
@testable import WireboltKit

@Suite("Wirebolt model")
struct WireboltModelTests {
    @Test("streams a response into bounded visible state")
    @MainActor
    func streamsResponse() async {
        let runner = StubRunner(events: [
            .head(ResponseHead(status: 200, version: "HTTP/2", headers: [], timeToHeadersNS: 10)),
            .chunk(Data("hello".utf8)),
            .complete(RunCompletion(bytesReceived: 5, totalTimeNS: 20)),
        ])
        let model = WireboltModel(runner: runner)
        model.draft.url = "https://example.com"

        await model.send()

        #expect(model.responseHead?.status == 200)
        #expect(model.responseText == "hello")
        #expect(model.responseBytes == 5)
        #expect(model.completion?.totalTimeNS == 20)
        #expect(model.isRunning == false)
    }

    @Test("surfaces structured validation failures")
    @MainActor
    func reportsFailure() async {
        let issue = RequestIssue(path: "url", kind: "invalid_url", reference: nil)
        let model = WireboltModel(runner: StubRunner(failure: RunFailure(kind: "invalid_request", issues: [issue])))

        await model.send()

        #expect(model.failure?.issues == [issue])
    }

    @Test("loads hierarchy and selects first request")
    @MainActor
    func loadsWorkspace() async {
        let request = RequestDraft(id: "health", name: "Health", url: "https://example.com/health")
        let location = RequestLocation(collectionID: "api", request: request)
        let workspace = WorkspaceDraft(
            name: "Demo",
            collections: [CollectionDraft(id: "api", name: "API", requests: [location])],
            environments: [EnvironmentDraft(id: "local", name: "Local")]
        )
        let model = WireboltModel(runner: StubRunner(events: []), persistence: StubPersistence(workspace: workspace))

        await model.loadWorkspace()

        #expect(model.workspace == workspace)
        #expect(model.draft.id == "health")
        #expect(model.selectedEnvironmentID == "local")
    }

    @Test("indexes workspace search once and matches case and diacritics")
    @MainActor
    func indexesWorkspaceSearch() async {
        let request = RequestDraft(
            id: "status",
            name: "État Service",
            method: .get,
            url: "https://example.com/health"
        )
        let location = RequestLocation(collectionID: "api", request: request)
        let workspace = WorkspaceDraft(
            name: "Search",
            collections: [CollectionDraft(id: "api", name: "API", requests: [location])]
        )
        let model = WireboltModel(
            runner: StubRunner(),
            persistence: StubPersistence(workspace: workspace)
        )

        await model.loadWorkspace()

        #expect(model.requestMatches(
            location,
            normalizedQuery: model.normalizedSearchQuery("ETAT")
        ))
        #expect(model.requestMatches(
            location,
            normalizedQuery: model.normalizedSearchQuery("example.com/health")
        ))
    }

    @Test("caps the visible response without changing total bytes")
    @MainActor
    func capsPreview() async {
        let body = Data(repeating: 65, count: WireboltModel.previewByteLimit + 128)
        let runner = StubRunner(events: [
            .head(ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1)),
            .chunk(body),
            .complete(RunCompletion(bytesReceived: UInt64(body.count), totalTimeNS: 2)),
        ])
        let model = WireboltModel(runner: runner)

        await model.send()

        #expect(model.responseText.utf8.count == WireboltModel.previewByteLimit)
        #expect(model.responseBytes == UInt64(body.count))
        #expect(model.responseWasTruncated)
    }

    @Test("persists a request through the workspace seam")
    @MainActor
    func persistsRequest() async {
        let persistence = PersistenceRecorder()
        let model = WireboltModel(runner: StubRunner(), persistence: persistence)
        model.draft = RequestDraft(id: "health", name: "Health")

        await model.saveCurrentRequest(collectionID: "api")

        let saved = await persistence.savedRequest
        #expect(saved?.0.id == "health")
        #expect(saved?.1 == "api")
    }

    @Test("encodes secret references without secret material")
    func encodesRequestInput() throws {
        let draft = RequestDraft(
            method: .post,
            url: "https://example.com",
            authentication: .bearer(token: .secret("api.token")),
            body: .json(value: "{\"ok\":true}"),
            proxy: .direct
        )

        let workspaceProxy = ProxyDocument.manual(routes: [
            ProxyRouteDocument(
                destination: "all",
                endpoint: "http://proxy.internal:8080/",
                credentials: ProxyCredentialsDocument(
                    username: "proxy.username",
                    password: "proxy.password"
                )
            ),
        ])
        let data = try JSONEncoder().encode(RunInput(
            draft: draft,
            variables: [:],
            workspaceProxy: workspaceProxy
        ))
        let json = String(decoding: data, as: UTF8.self)

        #expect(json.contains("api.token"))
        #expect(json.contains("request_proxy"))
        #expect(json.contains("proxy.internal"))
        #expect(json.contains("proxy.username"))
        #expect(json.contains("secret material") == false)
    }

    @Test("round-trips a manual workspace proxy without secret material")
    func roundTripsManualProxy() throws {
        let proxy = ProxyDocument.manual(routes: [
            ProxyRouteDocument(
                destination: "https",
                endpoint: "socks5h://proxy.internal:1080/",
                credentials: ProxyCredentialsDocument(
                    username: "proxy.username",
                    password: "proxy.password"
                )
            ),
        ])

        let data = try JSONEncoder().encode(proxy)
        let decoded = try JSONDecoder().decode(ProxyDocument.self, from: data)

        #expect(decoded == proxy)
        #expect(String(decoding: data, as: UTF8.self).contains("secret material") == false)
    }

    @Test("decodes the stable Git bridge document")
    func decodesGitBridgeDocument() throws {
        let data = Data(#"{"branch":"main","upstream":"origin/main","ahead":1,"behind":2,"changes":[{"path":"wirebolt.toml","previous_path":null,"staged":"type_changed","unstaged":"none","conflicted":false}]}"#.utf8)

        let status = try JSONDecoder().decode(GitStatusSnapshot.self, from: data)

        #expect(status.branch == "main")
        #expect(status.ahead == 1)
        #expect(status.behind == 2)
        #expect(status.changes.first?.staged == .typeChanged)
    }

    @Test("runs Git actions only when explicitly requested")
    @MainActor
    func runsGitActionsExplicitly() async {
        let collaboration = GitRecorder()
        let model = WireboltModel(runner: StubRunner(), gitCollaboration: collaboration)

        #expect(await collaboration.calls == [])

        await model.refreshGitStatus()
        await model.commitGit(message: "save workspace")
        await model.pushGit()

        #expect(await collaboration.calls == ["status", "commit:save workspace", "push"])
        #expect(model.gitStatus?.branch == "main")
        #expect(model.gitOperation?.outcome == .pushed)
        #expect(model.isGitBusy == false)
    }

    @Test("refuses to pull over unsaved request edits")
    @MainActor
    func protectsUnsavedRequestEdits() async {
        let request = RequestDraft(id: "health", name: "Health", url: "https://example.com")
        let workspace = WorkspaceDraft(
            name: "Demo",
            collections: [CollectionDraft(
                id: "api",
                name: "API",
                requests: [RequestLocation(collectionID: "api", request: request)]
            )]
        )
        let collaboration = GitRecorder()
        let model = WireboltModel(
            runner: StubRunner(),
            persistence: StubPersistence(workspace: workspace),
            gitCollaboration: collaboration
        )
        await model.loadWorkspace()
        model.draft.url = "https://edited.example.com"

        await model.pullGit()

        #expect(await collaboration.calls == [])
        #expect(model.gitFailure?.kind == "unsaved_request")
    }

    @Test("reloads workspace after a successful pull")
    @MainActor
    func reloadsAfterPull() async {
        let initial = WorkspaceDraft(name: "Before")
        let updated = WorkspaceDraft(name: "After")
        let persistence = SequencedPersistence(workspaces: [initial, updated])
        let collaboration = GitRecorder(pullOutcome: .updated)
        let model = WireboltModel(
            runner: StubRunner(),
            persistence: persistence,
            gitCollaboration: collaboration
        )
        await model.loadWorkspace()

        await model.pullGit()

        #expect(model.workspace.name == "After")
        #expect(model.gitOperation?.outcome == .updated)
    }

    @Test("keeps drafts and responses isolated per tab")
    @MainActor
    func isolatesDocumentSessions() async throws {
        let runner = RoutedRunner()
        let model = WireboltModel(runner: runner)
        let first = model.makeNewRequest()
        first.draft.name = "First"
        first.draft.url = "https://example.com/first"
        let firstSend = Task { await model.send() }
        await runner.release(runFor: "https://example.com/first", body: "one")
        await firstSend.value

        let second = model.makeNewRequest()
        second.draft.name = "Second"
        second.draft.url = "https://example.com/second"
        let secondSend = Task { await model.send() }
        await runner.release(runFor: "https://example.com/second", body: "two")
        await secondSend.value

        #expect(first.draft.name == "First")
        #expect(first.responseText == "one")
        #expect(second.draft.name == "Second")
        #expect(second.responseText == "two")
    }

    @Test("supports split groups and tab close scopes")
    @MainActor
    func managesEditorGroups() throws {
        let store = DocumentSessionStore()
        let first = store.openTemporary()
        first.draft.name = "First"
        let second = store.openTemporary()
        second.draft.name = "Second"
        let third = store.openTemporary()
        third.markSaved(third.draft)

        let splitID = try #require(store.split(tabID: third.id))
        #expect(store.groups.count == 2)
        #expect(store.activeGroupID == splitID)

        store.select(tabID: second.id, in: store.groups[0].id)
        let blocked = store.close(.others(second.id), in: store.groups[0].id)
        #expect(blocked.map(\.title) == ["First"])
        _ = store.close(.others(second.id), in: store.groups[0].id, allowDirty: true)
        #expect(store.groups[0].tabIDs == [second.id])
    }

    @Test("applies hierarchical workspace mutations without reloading")
    @MainActor
    func appliesWorkspaceDeltas() async {
        let persistence = MutationRecorder()
        let model = WireboltModel(runner: StubRunner(), persistence: persistence)

        await model.createCollection(name: "Payments")
        let collectionID = model.workspace.collections[0].id
        await model.createGroup(collectionID: collectionID, name: "OAuth")
        let groupID = model.workspace.collections[0].groups[0].id
        await model.renameGroup(collectionID: collectionID, id: groupID, name: "Authentication")

        #expect(model.workspace.collections[0].name == "Payments")
        #expect(model.workspace.collections[0].groups[0].name == "Authentication")
        #expect(await persistence.loadCount == 0)
        #expect(await persistence.commands.count == 3)
    }

    @Test("finds nested group descendants iteratively")
    func groupDescendants() {
        let collection = CollectionDraft(
            id: "api",
            name: "API",
            groups: [
                GroupDraft(id: "one", name: "One"),
                GroupDraft(id: "two", name: "Two", parentID: "one"),
                GroupDraft(id: "three", name: "Three", parentID: "two"),
            ]
        )

        #expect(collection.descendantGroupIDs(of: "one") == Set(["one", "two", "three"]))
    }

    @Test("environment rows preserve identity, order and disabled state")
    func stableEnvironmentRows() throws {
        let environment = EnvironmentDraft(
            id: "local",
            name: "Local",
            variables: [
                EnvironmentVariableDraft(id: "second", key: "disabled", value: .literal("no"), enabled: false, order: 2),
                EnvironmentVariableDraft(id: "first", key: "host", value: .literal("example.com"), enabled: true, order: 1),
            ]
        )
        let data = try JSONEncoder().encode(environment)
        let decoded = try JSONDecoder().decode(EnvironmentDraft.self, from: data)

        #expect(decoded == environment)
        #expect(decoded.variables.map(\.id) == ["second", "first"])
        #expect(decoded.enabledValues == ["host": .literal("example.com")])
    }

    @Test("custom methods and stable field IDs survive request encoding")
    func customMethodAndFields() throws {
        let method = try #require(HTTPMethod(rawValue: "PURGE"))
        let draft = RequestDraft(
            method: method,
            url: "https://example.com",
            headers: [RequestField(id: "stable-header", name: "X-Test", value: .literal("yes"))]
        )
        let input = RunInput(draft: draft, variables: [:])
        let data = try JSONEncoder().encode(input)
        let json = String(decoding: data, as: UTF8.self)

        #expect(json.contains("PURGE"))
        #expect(json.contains("stable-header"))
    }

    @Test("request transport visibly inherits workspace defaults until overridden")
    @MainActor
    func transportInheritance() async {
        let runner = InputCapturingRunner()
        let model = WireboltModel(runner: runner)
        model.workspace.transport = TransportSettings(
            validateTLS: false,
            followRedirects: true,
            maximumRedirects: 4,
            totalTimeoutMS: 123,
            readTimeoutMS: 45
        )
        let session = model.makeNewRequest()
        session.draft.url = "https://example.com"
        session.draft.transport = TransportSettings(validateTLS: true, totalTimeoutMS: 999)

        await model.send()
        #expect(await runner.lastInput?.validateTLS == false)
        #expect(await runner.lastInput?.totalTimeoutMS == 123)

        session.draft.inheritsWorkspaceTransport = false
        await model.send()
        #expect(await runner.lastInput?.validateTLS == true)
        #expect(await runner.lastInput?.totalTimeoutMS == 999)
    }

    @Test("workspace transport saves through one versioned command")
    @MainActor
    func savesWorkspaceTransport() async {
        let persistence = MutationRecorder()
        let model = WireboltModel(runner: StubRunner(), persistence: persistence)
        model.workspace.transport.validateTLS = false

        await model.saveWorkspaceTransport()

        let commands = await persistence.commands
        guard case let .saveWorkspaceSettings(settings) = commands.first else {
            Issue.record("Expected workspace settings command")
            return
        }
        #expect(settings.validateTLS == false)
    }

    @Test("keeps the exact prepared request immutable while the draft changes")
    @MainActor
    func keepsPreparedSnapshotImmutable() async {
        let runner = RoutedRunner()
        let model = WireboltModel(runner: runner)
        let session = model.makeNewRequest()
        session.draft.url = "https://example.com/original"
        let send = Task { await model.send() }

        await runner.emitPrepared(runFor: "https://example.com/original")
        session.draft.url = "https://example.com/edited"
        await runner.release(runFor: "https://example.com/original", body: "ok")
        await send.value

        #expect(session.preparedRun?.url == "https://example.com/original")
        #expect(session.draft.url == "https://example.com/edited")
    }

    @Test("stores acquired OAuth tokens in Keychain persistence without putting material in the draft")
    @MainActor
    func acquiresOAuthToken() async throws {
        let persistence = SecretPersistenceRecorder()
        let oauth = OAuthAuthorizerStub(token: "access-material")
        let model = WireboltModel(
            runner: StubRunner(),
            persistence: persistence,
            oauth2: oauth
        )
        let session = model.makeNewRequest()
        let configuration = OAuth2Configuration(
            grant: .clientCredentials,
            tokenURL: "https://identity.example.com/token",
            clientID: "wirebolt",
            accessTokenReference: "oauth.test-token"
        )
        session.draft.authentication = .oauth2(configuration: configuration)

        await model.acquireOAuthToken(for: session)

        #expect(await persistence.savedSecret?.0 == "oauth.test-token")
        #expect(await persistence.savedSecret?.1 == "access-material")
        let encoded = try JSONEncoder().encode(RunInput(draft: session.draft, variables: [:]))
        #expect(String(decoding: encoded, as: UTF8.self).contains("access-material") == false)
        #expect(model.oauthReceipts[session.id] != nil)
    }
}

private struct StubRunner: RequestRunner {
    let events: [RunEvent]
    let failure: RunFailure?

    init(events: [RunEvent] = [], failure: RunFailure? = nil) {
        self.events = events
        self.failure = failure
    }

    func events(for _: RunInput, runID _: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { continuation in
            for event in events { continuation.yield(event) }
            if let failure { continuation.finish(throwing: failure) } else { continuation.finish() }
        }
    }

    func cancel(runID _: RunID) {}
}

private actor InputCapturingRunner: RequestRunner {
    private(set) var lastInput: RunInput?

    nonisolated func events(
        for input: RunInput,
        runID _: RunID
    ) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { continuation in
            Task {
                await self.capture(input)
                continuation.yield(.prepared(PreparedRunSnapshot(
                    method: input.method,
                    url: input.url,
                    headers: [],
                    body: PreparedBodySnapshot(byteCount: 0, contentType: nil, textPreview: nil, redacted: false),
                    transport: TransportSettings(
                        validateTLS: input.validateTLS,
                        followRedirects: input.followRedirects,
                        maximumRedirects: Int(input.maximumRedirects),
                        totalTimeoutMS: input.totalTimeoutMS,
                        readTimeoutMS: input.readTimeoutMS,
                        clientCertificateReference: input.clientCertificateReference,
                        customCAPath: input.customCAPath
                    )
                )))
                continuation.yield(.complete(RunCompletion(bytesReceived: 0, totalTimeNS: 1)))
                continuation.finish()
            }
        }
    }

    nonisolated func cancel(runID _: RunID) {}

    private func capture(_ input: RunInput) { lastInput = input }
}

private actor RoutedRunner: RequestRunner {
    private struct Pending {
        let input: RunInput
        let continuation: AsyncThrowingStream<RunEvent, any Error>.Continuation
    }

    private var pendingByRunID: [RunID: Pending] = [:]

    nonisolated func events(
        for input: RunInput,
        runID: RunID
    ) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { continuation in
            Task { await self.install(input: input, runID: runID, continuation: continuation) }
        }
    }

    nonisolated func cancel(runID: RunID) {
        Task { await self.finishCancellation(runID: runID) }
    }

    func release(runFor url: String, body: String) async {
        while true {
            if let match = pendingByRunID.first(where: { $0.value.input.url == url }) {
                pendingByRunID[match.key] = nil
                match.value.continuation.yield(.head(ResponseHead(
                    status: 200,
                    version: "HTTP/2",
                    headers: [],
                    timeToHeadersNS: 1
                )))
                match.value.continuation.yield(.chunk(Data(body.utf8)))
                match.value.continuation.yield(.complete(RunCompletion(
                    bytesReceived: UInt64(body.utf8.count),
                    totalTimeNS: 2
                )))
                match.value.continuation.finish()
                return
            }
            await Task.yield()
        }
    }

    func emitPrepared(runFor url: String) async {
        while true {
            if let match = pendingByRunID.first(where: { $0.value.input.url == url }) {
                match.value.continuation.yield(.prepared(PreparedRunSnapshot(
                    method: match.value.input.method,
                    url: match.value.input.url,
                    headers: [],
                    body: PreparedBodySnapshot(
                        byteCount: 0,
                        contentType: nil,
                        textPreview: nil,
                        redacted: false
                    ),
                    transport: TransportSettings()
                )))
                return
            }
            await Task.yield()
        }
    }

    private func install(
        input: RunInput,
        runID: RunID,
        continuation: AsyncThrowingStream<RunEvent, any Error>.Continuation
    ) {
        pendingByRunID[runID] = Pending(input: input, continuation: continuation)
    }

    private func finishCancellation(runID: RunID) {
        pendingByRunID.removeValue(forKey: runID)?.continuation.finish(throwing: CancellationError())
    }
}

private struct StubPersistence: WorkspacePersistence {
    let workspace: WorkspaceDraft

    func load() async throws -> WorkspaceDraft { workspace }
    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}
}

private actor PersistenceRecorder: WorkspacePersistence {
    var savedRequest: (RequestDraft, String)?

    func load() async throws -> WorkspaceDraft {
        WorkspaceDraft(name: "Demo")
    }

    func save(request: RequestDraft, in collectionID: String) async throws {
        savedRequest = (request, collectionID)
    }

    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}
}

private actor SecretPersistenceRecorder: WorkspacePersistence {
    private(set) var savedSecret: (String, String)?

    func load() async throws -> WorkspaceDraft { WorkspaceDraft(name: "OAuth") }
    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name: String, value: String) async throws { savedSecret = (name, value) }
}

@MainActor
private final class OAuthAuthorizerStub: OAuth2Authorizing {
    let token: String

    init(token: String) { self.token = token }

    func acquireToken(
        configuration _: OAuth2Configuration
    ) async throws -> (String, OAuth2TokenReceipt) {
        (token, OAuth2TokenReceipt(expiresAt: Date().addingTimeInterval(60), scope: "read"))
    }
}

private actor SequencedPersistence: WorkspacePersistence {
    private var workspaces: [WorkspaceDraft]

    init(workspaces: [WorkspaceDraft]) {
        self.workspaces = workspaces
    }

    func load() async throws -> WorkspaceDraft {
        if workspaces.count > 1 {
            return workspaces.removeFirst()
        }
        return workspaces[0]
    }

    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}
}

private actor MutationRecorder: WorkspacePersistence {
    private(set) var commands: [WorkspaceCommand] = []
    private(set) var loadCount = 0

    func load() async throws -> WorkspaceDraft {
        loadCount += 1
        return WorkspaceDraft(name: "Mutations")
    }

    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}

    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta {
        commands.append(command)
        return WorkspaceDelta(
            version: UInt64(commands.count),
            kind: .collection,
            affectedIDs: []
        )
    }
}

private actor GitRecorder: GitCollaboration {
    private(set) var calls: [String] = []
    private let pullOutcome: GitOperationOutcome

    init(pullOutcome: GitOperationOutcome = .upToDate) {
        self.pullOutcome = pullOutcome
    }

    func status() async throws -> GitStatusSnapshot {
        calls.append("status")
        return .cleanMain
    }

    func pull() async throws -> GitOperationSnapshot {
        calls.append("pull")
        return GitOperationSnapshot(outcome: pullOutcome, revision: "abc", status: .cleanMain)
    }

    func commit(message: String) async throws -> GitOperationSnapshot {
        calls.append("commit:\(message)")
        return GitOperationSnapshot(outcome: .committed, revision: "abc", status: .cleanMain)
    }

    func push() async throws -> GitOperationSnapshot {
        calls.append("push")
        return GitOperationSnapshot(outcome: .pushed, revision: "abc", status: .cleanMain)
    }
}

private extension GitStatusSnapshot {
    static let cleanMain = GitStatusSnapshot(
        branch: "main",
        upstream: "origin/main",
        ahead: 0,
        behind: 0,
        changes: []
    )
}
