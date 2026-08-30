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
        #expect(!model.isRunning)
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
        #expect(!json.contains("secret material"))
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
        #expect(!String(decoding: data, as: UTF8.self).contains("secret material"))
    }
}

private struct StubRunner: RequestRunner {
    let events: [RunEvent]
    let failure: RunFailure?

    init(events: [RunEvent] = [], failure: RunFailure? = nil) {
        self.events = events
        self.failure = failure
    }

    func events(for _: RunInput) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { continuation in
            for event in events { continuation.yield(event) }
            if let failure { continuation.finish(throwing: failure) } else { continuation.finish() }
        }
    }

    func cancel() {}
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
