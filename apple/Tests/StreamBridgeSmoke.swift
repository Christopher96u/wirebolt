import Foundation

extension BridgeSmoke {
    /// Exercise the real C callback buffer with chunks below its 32 KiB threshold.
    /// The server deliberately keeps the connection open after sending its body.
    static func verifySlowStreaming(_ runner: RustRequestRunner) async throws {
        let server = Process()
        server.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
        server.arguments = ["-u", "-c", #"""
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *args): pass
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'text/plain; charset=utf-8')
        self.send_header('Transfer-Encoding', 'chunked')
        self.end_headers()
        try:
            for part in [b'first \xf0\x9f', b'\x8c\x8d line\n']:
                self.wfile.write(('%x\r\n' % len(part)).encode() + part + b'\r\n')
                self.wfile.flush()
                time.sleep(0.1)
            time.sleep(1)
            self.wfile.write(b'0\r\n\r\n')
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError): pass
server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
print(server.server_port, flush=True)
server.serve_forever()
"""#]
        let output = Pipe()
        server.standardOutput = output
        server.standardError = FileHandle.nullDevice
        try server.run()
        defer { if server.isRunning { server.terminate() } }
        let portText = String(decoding: output.fileHandleForReading.availableData, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        guard let port = UInt16(portText) else { preconditionFailure("Streaming fixture did not publish its port") }
        let draft = RequestDraft(url: "http://127.0.0.1:\(port)/stream", proxy: .direct,
            transport: TransportSettings(totalTimeoutMS: 4_000, readTimeoutMS: 2_000))
        let started = ContinuousClock.now
        var bytes = Data(), sawEarlyChunk = false, completed = false
        for try await event in runner.events(for: RunInput(draft: draft, variables: [:]), runID: RunID()) {
            switch event {
            case let .chunk(chunk):
                if bytes.isEmpty {
                    precondition(started.duration(to: .now) < .milliseconds(800), "A slow small chunk must appear before stream completion")
                    sawEarlyChunk = true
                }
                bytes.append(chunk)
            case .complete: completed = true
            default: break
            }
        }
        precondition(sawEarlyChunk && completed && String(decoding: bytes, as: UTF8.self) == "first 🌍 line\n")
        let cancelledRun = RunID()
        var cancelled = false, received = false
        do {
            for try await event in runner.events(for: RunInput(draft: draft, variables: [:]), runID: cancelledRun) {
                if case .chunk = event, !received { received = true; runner.cancel(runID: cancelledRun) }
                if case .complete = event { preconditionFailure("A cancelled stream must never complete successfully") }
            }
        } catch let failure as RunFailure { cancelled = failure.kind == "cancelled" }
        precondition(received && cancelled, "Cancelling a slow chunked stream must end only its run")
        try await verifyRuntimeSend(runner, draft: draft)
        print("slow_chunked_stream_and_cancellation=passed bytes=\(bytes.count)")
    }
    @MainActor private static func verifyRuntimeSend(_ runner: RustRequestRunner, draft: RequestDraft) async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: "wirebolt-runtime-smoke-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let model = WireboltModel(runner: runner, history: HistoryRepository(root: directory), cookieDirectory: nil)
        let session = model.sessions.open(draft: draft)
        let started = ContinuousClock.now
        let send = Task { await model.send(session) }
        while session.responseText.isEmpty && started.duration(to: .now) < .seconds(3) {
            try await Task.sleep(for: .milliseconds(5))
        }
        precondition(!session.responseText.isEmpty && session.isRunning, "Rust runtime must publish small chunks before completion")
        precondition(started.duration(to: .now) < .milliseconds(800))
        session.draft.url = "https://example.com/edited"
        await send.value
        precondition(session.responseText == "first 🌍 line\n" && session.failure == nil)
        precondition(session.preparedRun?.url == draft.url && session.draft.url != draft.url)
        let count = await model.historyCount(for: session)
        precondition(count == 1)
        let second = model.sessions.open(draft: draft, forceNewSession: true)
        let pending = Task { await model.send(second) }
        let cancelStarted = ContinuousClock.now
        while second.responseText.isEmpty && cancelStarted.duration(to: .now) < .seconds(3) {
            try await Task.sleep(for: .milliseconds(5))
        }
        model.cancel(second)
        await pending.value
        precondition(second.failure?.kind == "cancelled" && session.failure == nil)
        let cancelledHistoryCount = await model.historyCount(for: second)
        precondition(cancelledHistoryCount == 2, "Native cancellation retains the prepared partial run in history")
        print("rust_runtime_send_stream_history_and_scoped_cancellation=passed")
    }

}
