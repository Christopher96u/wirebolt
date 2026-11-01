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
        session.beginRun(run)
        await session.consume(.head(ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1)), runID: run)
        let bytes = Array("first 🌍 line\n".utf8)
        let split = 8 // Two bytes into the four-byte scalar.
        await session.consume(.chunk(Data(bytes.prefix(split))), runID: run)
        try await waitUntil { session.responseBytes == UInt64(split) }
        #expect(session.isRunning)
        #expect(session.responseText == "first ")
        #expect(!session.responseText.contains("�"))
        #expect(session.responseBytes == UInt64(split))
        await session.consume(.chunk(Data(bytes.dropFirst(split))), runID: run)
        try await waitUntil { session.responseBytes == UInt64(bytes.count) }
        #expect(session.isRunning)
        #expect(session.responseText == "first 🌍 line\n")
        let store = try #require(session.bodyStore)
        #expect(try await store.viewport() == Data(bytes))
        await session.consume(.complete(RunCompletion(bytesReceived: UInt64(bytes.count), totalTimeNS: 1)), runID: run)
        #expect(!session.isRunning && session.responseText == "first 🌍 line\n")
    }

    @Test func previewStaysBoundedAndFileRetainsEveryByte() async throws {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        session.beginRun(run)
        let body = Data(repeating: 65, count: DocumentSession.previewByteLimit * 4)
        for chunk in stride(from: 0, to: body.count, by: 1024) {
            await session.consume(.chunk(body.subdata(in: chunk..<min(chunk + 1024, body.count))), runID: run)
        }
        await session.consume(.complete(RunCompletion(bytesReceived: UInt64(body.count), totalTimeNS: 1)), runID: run)
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
        session.beginRun(run)
        await session.consume(.chunk(Data("partial".utf8)), runID: run)
        session.cancel(runID: run)
        try await waitUntil { session.responseText == "partial" }
        #expect(!session.isRunning && session.failure?.kind == .cancelled)
        #expect(session.responseText == "partial")
        await session.consume(.chunk(Data("late".utf8)), runID: run)
        #expect(session.responseText == "partial")
        let store = try #require(session.bodyStore)
        #expect(try await store.viewport() == Data("partial".utf8))
        let next = RunID()
        session.beginRun(next)
        await session.consume(.chunk(Data("new".utf8)), runID: next)
        await session.consume(.complete(RunCompletion(bytesReceived: 3, totalTimeNS: 1)), runID: next)
        #expect(session.responseText == "new" && session.failure == nil)
    }

    @Test func completionFlushesAQueuedPublication() async {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        session.beginRun(run)
        await session.consume(.chunk(Data("done".utf8)), runID: run)
        await session.consume(.complete(RunCompletion(bytesReceived: 4, totalTimeNS: 1)), runID: run)
        #expect(session.responseText == "done" && session.responseBytes == 4)
    }

    @Test func storageFailureKeepsPartialPreviewAndRejectsLateCompletion() async throws {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        session.beginRun(run)
        await session.consume(.head(ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1)), runID: run)
        #expect(session.rust.appendBatch(runId: run.description, chunks: [Data("partial".utf8)]))
        let body = try #require(session.bodyStore)
        try body.rust.finish()
        await session.consume(.chunk(Data(repeating: 65, count: ResponseBodyStore.searchChunkByteCount)), runID: run)
        #expect(session.failure?.kind == .storage && !session.isRunning)
        #expect(session.responseText == "partial")
        await session.consume(.chunk(Data("late".utf8)), runID: run)
        await session.consume(.complete(RunCompletion(bytesReceived: 999, totalTimeNS: 1)), runID: run)
        #expect(session.completion == nil)
    }

    @Test func completedRunKeepsCompletionForARejectedLateBatch() async {
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        session.beginRun(run)
        await session.consume(.chunk(Data("late batch".utf8)), runID: run)
        // Model a completion arriving in Rust before the native publication task resumes.
        session.rust.consume(runId: run.description, event: .complete(completion: RunCompletion(bytesReceived: 0, totalTimeNS: 1)))
        await session.consume(.complete(RunCompletion(bytesReceived: 0, totalTimeNS: 1)), runID: run)
        #expect(session.completion != nil && session.failure == nil)
    }
}
