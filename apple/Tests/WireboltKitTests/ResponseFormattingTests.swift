import Foundation
import Testing
@testable import WireboltKit

@Suite("Response formatting")
struct ResponseFormattingTests {
    @Test("Durations keep sub-millisecond precision and switch to seconds", arguments: [
        (UInt64(0), "<0.1 ms"), (1, "<0.1 ms"), (400_000, "0.4 ms"), (949_000, "0.9 ms"), (960_000, "1 ms"),
        (4_000_000, "4 ms"), (186_400_000, "186 ms"), (999_400_000, "999 ms"), (999_600_000, "1.00 s"),
        (1_234_000_000, "1.23 s"), (4_006_000_000, "4.01 s"), (12_340_000_000, "12.3 s"), (125_000_000_000, "2 min 5 s"),
    ])
    func durations(nanoseconds: UInt64, expected: String) {
        #expect(ResponseFormatting.duration(nanoseconds: nanoseconds) == expected)
    }

    @Test("Elapsed time ticks in tenths of a second")
    func elapsed() {
        #expect(ResponseFormatting.elapsed(seconds: 0) == "0.0 s")
        #expect(ResponseFormatting.elapsed(seconds: 1.26) == "1.3 s")
        #expect(ResponseFormatting.elapsed(seconds: 61) == "1 min 1 s")
    }

    @Test("Byte counts use file-style units and never spell out zero")
    func byteCounts() {
        let locale = Locale(identifier: "en_US")
        #expect(ResponseFormatting.byteCount(0, locale: locale) == "0 bytes")
        #expect(ResponseFormatting.byteCount(19, locale: locale) == "19 bytes")
        // Foundation spells the kilobyte symbol "KB" or "kB" depending on the OS release.
        #expect(ResponseFormatting.byteCount(468_000, locale: locale).lowercased() == "468 kb")
        #expect(ResponseFormatting.byteCount(4_200_000, locale: locale) == "4.2 MB")
    }

    @Test("Standard reason phrases replace the generic fallback")
    func reasonPhrases() {
        #expect(ResponseFormatting.statusLine(200) == "200 OK")
        #expect(ResponseFormatting.statusLine(302) == "302 Found")
        #expect(ResponseFormatting.statusLine(422) == "422 Unprocessable Content")
        #expect(ResponseFormatting.statusLine(599) == "599 Server Error")
        #expect(ResponseFormatting.statusLine(799) == "799 Response")
    }

    @Test("Completion announcements summarise status, time and size")
    func summary() {
        let summary = ResponseFormatting.completionSummary(status: 200, totalTimeNS: 4_000_000, bytes: 468_000, locale: Locale(identifier: "en_US"))
        #expect(summary.lowercased() == "200 ok, 4 ms, 468 kb")
    }
}

@Suite("Run failure messages")
struct RunFailureMessageTests {
    static let kinds = ["cancelled", "total_timeout", "read_timeout", "connect_timeout", "stream_stopped", "response_too_large",
        "connection", "request", "response_body", "proxy", "transport", "tls_configuration", "invalid_request", "invalid_input",
        "internal", "bridge", "keychain", "unresolved_value"]

    @Test("Every bridge failure kind has specific copy", arguments: kinds)
    func everyKindIsMapped(kind: String) {
        let message = RunFailureMessage(RunFailure(kind: kind, issues: []), host: "127.0.0.1:18880")
        #expect(!message.title.isEmpty && !message.message.isEmpty)
        #expect(!message.title.contains("_") && !message.message.contains("_"))
        #expect(!message.title.contains("Tls"))
    }

    @Test("Connection failures name the host and offer network settings")
    func connection() {
        let message = RunFailureMessage(RunFailure(kind: "connection", issues: []), host: RunFailureMessage.host(from: "http://127.0.0.1:18880/echo"))
        #expect(message.title == "Couldn’t Connect to 127.0.0.1:18880")
        #expect(message.suggestsNetworkSettings)
        #expect(RunFailureMessage(RunFailure(kind: "tls_configuration", issues: [])).category == .tls)
        #expect(!RunFailureMessage(RunFailure(kind: "cancelled", issues: [])).suggestsNetworkSettings)
    }

    @Test("Hosts are only derived from concrete absolute URLs")
    func hosts() {
        #expect(RunFailureMessage.host(from: "https://example.com/a") == "example.com")
        #expect(RunFailureMessage.host(from: "{{base}}/a") == nil)
        #expect(RunFailureMessage.host(from: "not a url") == nil)
        #expect(RunFailureMessage.host(from: nil) == nil)
    }

    @Test("Request issues and unknown kinds read as sentences")
    func issues() {
        let issue = RequestIssue(path: "url", kind: "missing_variable", reference: "base")
        let message = RunFailureMessage(RunFailure(kind: "invalid_request", issues: [issue]))
        #expect(message.message == "The variable “base” isn’t defined in the active environment. (url)")
        #expect(RunFailureMessage(RunFailure(kind: "workspace_reload", issues: [])).message == "Workspace reload.")
    }

    @Test("The URL bar shows failures and cancellations instead of an empty status slot")
    func runOutcomeBadges() {
        #expect(RunOutcomeBadge(status: nil, failure: nil) == nil)
        #expect(RunOutcomeBadge(status: 404, failure: nil) == .status(404))
        #expect(RunOutcomeBadge(status: 404, failure: nil)?.label == "404 Not Found")

        let refused = RunOutcomeBadge(status: nil, failure: RunFailure(kind: "connection", issues: []), host: "127.0.0.1:18880")
        #expect(refused == .failed(title: "Couldn’t Connect to 127.0.0.1:18880"))
        #expect(refused?.label == "Failed")
        #expect(refused?.detail == "Couldn’t Connect to 127.0.0.1:18880")

        // Cancelling after the head arrived reports the cancellation, not the partial status.
        let cancelled = RunOutcomeBadge(status: 200, failure: RunFailure(kind: "cancelled", issues: []))
        #expect(cancelled == .cancelled)
        #expect(cancelled?.label == "Cancelled")
    }
}
