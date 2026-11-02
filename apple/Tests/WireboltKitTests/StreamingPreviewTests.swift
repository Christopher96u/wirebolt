import Foundation
import Testing
@testable import WireboltKit

@Suite("Progressive response previews") @MainActor
struct StreamingPreviewTests {
    /// Previews publish asynchronously; wait for the expected state rather than a fixed
    /// delay, which slower CI runners can miss.
    private func waitUntil(_ condition: () -> Bool) async throws {
        let deadline = ContinuousClock.now + .seconds(5)
        while !condition(), ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
    }

    @Test func slowChunksAppearBeforeCompletionAndPreserveUTF8() async throws {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        sessionRun.sink.event(event: .head(head: ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1))); session.mirror()
        let bytes = Array("first 🌍 line\n".utf8)
        let split = 8 // Two bytes into the four-byte scalar.
        _ = sessionRun.sink.bytes(data: Data(bytes.prefix(split))); session.mirror()
        try await waitUntil { session.responseBytes == UInt64(split) }
        #expect(session.isRunning)
        #expect(session.responseText == "first ")
        #expect(!session.responseText.contains("�"))
        #expect(session.responseBytes == UInt64(split))
        _ = sessionRun.sink.bytes(data: Data(bytes.dropFirst(split))); session.mirror()
        try await waitUntil { session.responseBytes == UInt64(bytes.count) }
        #expect(session.isRunning)
        #expect(session.responseText == "first 🌍 line\n")
        let store = try #require(session.bodyStore)
        #expect(try await store.viewport() == Data(bytes))
        sessionRun.sink.event(event: .complete(completion: RunCompletion(bytesReceived: UInt64(bytes.count), totalTimeNS: 1))); await sessionRun.complete()
        #expect(!session.isRunning && session.responseText == "first 🌍 line\n")
    }

    @Test func previewStaysBoundedAndFileRetainsEveryByte() async throws {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        let body = Data(repeating: 65, count: DocumentSession.previewByteLimit * 4)
        for chunk in stride(from: 0, to: body.count, by: 1024) {
            _ = sessionRun.sink.bytes(data: body.subdata(in: chunk..<min(chunk + 1024, body.count))); session.mirror()
        }
        sessionRun.sink.event(event: .complete(completion: RunCompletion(bytesReceived: UInt64(body.count), totalTimeNS: 1))); await sessionRun.complete()
        #expect(session.responsePreviewData.count == DocumentSession.previewByteLimit)
        #expect(session.responseText.utf8.count == DocumentSession.previewByteLimit)
        #expect(session.responseBytes == UInt64(body.count))
        #expect(session.responseWasTruncated)
        let store = try #require(session.bodyStore)
        #expect(try await store.viewport(length: body.count) == body)
    }

    @Test func cancellationKeepsPartialFileAndRejectsLateEvents() async throws {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        _ = sessionRun.sink.bytes(data: Data("partial".utf8)); session.mirror()
        sessionRun.cancel()
        try await waitUntil { session.responseText == "partial" }
        #expect(!session.isRunning && session.failure?.kind == .cancelled)
        #expect(session.responseText == "partial")
        _ = sessionRun.sink.bytes(data: Data("late".utf8)); session.mirror()
        #expect(session.responseText == "partial")
        let store = try #require(session.bodyStore)
        #expect(try await store.viewport() == Data("partial".utf8))
        let next = RunID()
        let nextRun = await RuntimeRunFixture.start(session, runID: next)
        _ = nextRun.sink.bytes(data: Data("new".utf8)); session.mirror()
        nextRun.sink.event(event: .complete(completion: RunCompletion(bytesReceived: 3, totalTimeNS: 1))); await nextRun.complete()
        #expect(session.responseText == "new" && session.failure == nil)
    }

    @Test func completionFlushesAQueuedPublication() async {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        _ = sessionRun.sink.bytes(data: Data("done".utf8)); session.mirror()
        sessionRun.sink.event(event: .complete(completion: RunCompletion(bytesReceived: 4, totalTimeNS: 1))); await sessionRun.complete()
        #expect(session.responseText == "done" && session.responseBytes == 4)
    }

    @Test func storageFailureKeepsPartialPreviewAndRejectsLateCompletion() async throws {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        let sessionRun = await RuntimeRunFixture.start(session, runID: run)
        sessionRun.sink.event(event: .head(head: ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1))); session.mirror()
        #expect(sessionRun.sink.bytes(data: Data("partial".utf8)))
        let body = try #require(session.bodyStore)
        try body.rust.finish()
        _ = sessionRun.sink.bytes(data: Data(repeating: 65, count: ResponseBodyStore.searchChunkByteCount)); session.mirror()
        #expect(session.failure?.kind == .storage && !session.isRunning)
        #expect(session.responseText == "partial")
        _ = sessionRun.sink.bytes(data: Data("late".utf8)); session.mirror()
        sessionRun.sink.event(event: .complete(completion: RunCompletion(bytesReceived: 999, totalTimeNS: 1))); await sessionRun.complete()
        #expect(session.completion == nil)
    }

}
