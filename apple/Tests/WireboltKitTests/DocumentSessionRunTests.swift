import Foundation
import Testing
@testable import WireboltKit

@Suite("Document session runs")
@MainActor
struct DocumentSessionRunTests {
    private func completedSession(body: String) async -> DocumentSession {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        sessionRun.sink.event(event: .head(head: ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1))); session.mirror()
        _ = sessionRun.sink.bytes(data: Data(body.utf8)); session.mirror()
        sessionRun.sink.event(event: .complete(completion: RunCompletion(bytesReceived: UInt64(body.utf8.count), totalTimeNS: 1))); await sessionRun.complete()
        return session
    }

    @Test("A resend keeps the previous response until the new head arrives")
    func previousResponseStaysVisible() async throws {
        let session = await completedSession(body: "previous")
        let previousStore = try #require(session.bodyStore)
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        #expect(session.isRunning && session.isAwaitingResponseHead)
        #expect(session.runStartedAt != nil)
        #expect(session.responseText == "previous" && session.responseHead?.status == 200)
        #expect(session.bodyStore === previousStore)

        sessionRun.sink.event(event: .head(head: ResponseHead(status: 404, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1))); session.mirror()
        #expect(!session.isAwaitingResponseHead)
        #expect(session.responseHead?.status == 404)
        #expect(session.responseText.isEmpty && session.completion == nil)
        #expect(session.bodyStore !== previousStore)

        _ = sessionRun.sink.bytes(data: Data("next".utf8)); session.mirror()
        sessionRun.sink.event(event: .complete(completion: RunCompletion(bytesReceived: 4, totalTimeNS: 1))); await sessionRun.complete()
        #expect(session.responseText == "next" && !session.isRunning && session.runStartedAt == nil)
    }

    @Test("A failure before the head replaces the previous response")
    func failureReplacesPrevious() async {
        let session = await completedSession(body: "previous")
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        sessionRun.sink.finish(failure: RunFailure(kind: .connection, issues: [])); await sessionRun.complete()
        #expect(session.failure?.kind == .connection)
        #expect(session.responseHead == nil && session.responseText.isEmpty)
        #expect(session.bodyStore != nil, "history keeps a body store for the failed run")
        #expect(!session.isAwaitingResponseHead && session.runStartedAt == nil)
    }

    @Test("Cancelling before the head presents a cancelled run")
    func cancelBeforeHead() async {
        let session = await completedSession(body: "previous")
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        sessionRun.cancel()
        sessionRun.sink.event(event: .head(head: ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1))); session.mirror()
        #expect(session.failure?.kind == .cancelled)
        #expect(session.responseHead == nil && session.responseText.isEmpty)
    }

    @Test("Cookies received before the head belong to the new response")
    func cookiesBeforeHead() async {
        let session = await completedSession(body: "previous")
        let cookie = CookieSnapshot(name: "id", value: "1", domain: "example.com", hostOnly: true, path: "/",
            secure: false, httpOnly: false, sameSite: "", expiresAt: nil)
        session.appendResponseCookies([cookie])
        #expect(session.responseCookies == [cookie])
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        session.appendResponseCookies([cookie, cookie])
        #expect(session.responseCookies == [cookie])
        sessionRun.sink.event(event: .head(head: ResponseHead(status: 302, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1))); session.mirror()
        #expect(session.responseCookies == [cookie, cookie])
    }
}
