import Foundation
import Synchronization
#if canImport(WireboltKit)
@testable import WireboltKit
#endif

/// Exercises the production runtime and sink without network, preferences or Keychain access.
@MainActor
final class RuntimeRunFixture {
    let runtime: AppSessionRuntime
    let sink: SessionRunSink
    let session: DocumentSession
    private let task: Task<Void, Never>

    private init(runtime: AppSessionRuntime, sink: SessionRunSink, session: DocumentSession, task: Task<Void, Never>) {
        self.runtime = runtime
        self.sink = sink
        self.session = session
        self.task = task
    }

    static func start(_ session: DocumentSession, runID: RunID = RunID(), directory: URL = FileManager.default.temporaryDirectory) async -> RuntimeRunFixture {
        let transport = CapturedRuntimeTransport()
        let historyRoot = FileManager.default.temporaryDirectory.appending(path: "wirebolt-fixture-history-\(UUID().uuidString)")
        let runtime = AppSessionRuntime(history: AppRunHistory(root: historyRoot.path), cookies: AppCookieJar(storage: nil))
        let context = RuntimeContext(transport: TransportSettings().domain, workspaceProxy: nil, appProxy: .direct, variables: [:])
        let observer = FixtureRuntimeObserver(session: session, context: context)
        let task = Task { @MainActor in
            _ = await runtime.send(document: session.rust,
                options: RuntimeRunOptions(context: context, id: runID.description, directory: directory.path),
                transport: AppRuntimeTransport.foreign(platform: transport), observer: observer,
                secrets: AppSessionSecrets(), platform: nil)
            session.mirror()
            try? FileManager.default.removeItem(at: historyRoot)
        }
        var incoming = transport.incoming.makeAsyncIterator()
        let sink = await incoming.next()!
        session.mirror()
        return RuntimeRunFixture(runtime: runtime, sink: sink, session: session, task: task)
    }

    func complete() async {
        sink.finish(failure: nil)
        await task.value
        session.mirror()
    }

    func cancel() {
        runtime.cancel(document: session.rust)
        session.mirror()
    }
}

private final class CapturedRuntimeTransport: SessionRuntimeTransport {
    let sink = Mutex<SessionRunSink?>(nil)
    let incoming: AsyncStream<SessionRunSink>
    private let continuation: AsyncStream<SessionRunSink>.Continuation
    init() { (incoming, continuation) = AsyncStream.makeStream() }
    func resolve(values: [DomainValueSource], variables: [String: DomainValueSource], reply: SessionValuesReply) {
        reply.finish(values: values.map { if case .literal(let value) = $0 { value } else { "" } }, failure: nil)
    }
    func execute(input: DomainRunInput, id: String, sink: SessionRunSink) {
        self.sink.withLock { $0 = sink }
        continuation.yield(sink)
        continuation.finish()
    }
    func cancel(id: String) { sink.withLock { $0 }?.abandon() }
}

private final class FixtureRuntimeObserver: SessionRuntimeObserver {
    private let input: RuntimeInput
    private let changedProjection: @MainActor @Sendable () -> Void
    @MainActor init(session: DocumentSession, context: RuntimeContext) {
        input = RuntimeInput(draft: session.draft.domain, context: context)
        changedProjection = { session.mirror() }
    }
    func prepare(reply: SessionInputReply) { reply.finish(input: input) }
    func changed() { Task { @MainActor in changedProjection() } }
}

/// Keeps model sends pending so close/switch tests exercise the model's owning runtime.
final class PendingRequestRunner: RequestRunner {
    private let pending = Mutex<[RunID: AsyncThrowingStream<RunEvent, any Error>.Continuation]>([:])
    private let incoming: AsyncStream<RunID>
    private let continuation: AsyncStream<RunID>.Continuation
    private let cancellations = Mutex(0)
    var cancellationCount: Int { cancellations.withLock { $0 } }
    init() { (incoming, continuation) = AsyncStream.makeStream() }
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { stream in
            pending.withLock { $0[runID] = stream }
            continuation.yield(runID)
        }
    }
    func cancel(runID: RunID) {
        cancellations.withLock { $0 += 1 }
        pending.withLock { $0.removeValue(forKey: runID) }?.finish(throwing: CancellationError())
    }
    @MainActor func start(_ model: WireboltModel, session: DocumentSession) async -> Task<Void, Never> {
        let task = Task { await model.send(session) }
        var incoming = incoming.makeAsyncIterator()
        _ = await incoming.next()
        session.mirror()
        return task
    }
}
