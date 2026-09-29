import Foundation

/// Human-readable response metrics shared by the response pane, status line and announcements.
public enum ResponseFormatting {
    /// "<0.1 ms", "0.4 ms", "12 ms", "1.23 s", "12.3 s", "2 min 5 s".
    public static func duration(nanoseconds: UInt64) -> String {
        duration(seconds: Double(nanoseconds) / 1_000_000_000)
    }

    public static func duration(seconds: Double) -> String {
        let milliseconds = max(0, seconds) * 1000
        if milliseconds < 0.05 { return "<0.1 ms" }
        if milliseconds < 0.95 { return String(format: "%.1f ms", milliseconds) }
        if milliseconds < 999.5 { return "\(Int(milliseconds.rounded())) ms" }
        let seconds = milliseconds / 1000
        if seconds < 9.995 { return String(format: "%.2f s", seconds) }
        if seconds < 59.95 { return String(format: "%.1f s", seconds) }
        let whole = Int(seconds.rounded())
        return "\(whole / 60) min \(whole % 60) s"
    }

    /// A coarse, steadily ticking elapsed time for an in-flight run: "0.4 s", "12.3 s", "2 min 5 s".
    public static func elapsed(seconds: Double) -> String {
        let seconds = max(0, seconds)
        if seconds < 59.95 { return String(format: "%.1f s", seconds) }
        let whole = Int(seconds)
        return "\(whole / 60) min \(whole % 60) s"
    }

    /// File-style byte counts ("468 bytes", "18.6 KB", "4.2 MB") that never spell out zero.
    public static func byteCount(_ bytes: UInt64, locale: Locale = .autoupdatingCurrent) -> String {
        Int64(clamping: bytes).formatted(.byteCount(style: .file, spellsOutZero: false).locale(locale))
    }

    /// "302 Found"; unknown codes fall back to their class ("599 Server Error").
    public static func statusLine(_ status: UInt16) -> String {
        "\(status) \(reasonPhrase(for: status))"
    }

    /// Standard reason phrases (IANA HTTP Status Code Registry). HTTP/2+ has no server phrase.
    public static func reasonPhrase(for status: UInt16) -> String {
        if let phrase = reasonPhrases[status] { return phrase }
        return switch status {
        case 100..<200: "Informational"
        case 200..<300: "Success"
        case 300..<400: "Redirection"
        case 400..<500: "Client Error"
        case 500..<600: "Server Error"
        default: "Response"
        }
    }

    /// A single VoiceOver-friendly summary such as "200 OK, 4 ms, 468 KB".
    public static func completionSummary(status: UInt16?, totalTimeNS: UInt64?, bytes: UInt64, locale: Locale = .autoupdatingCurrent) -> String {
        [status.map(statusLine), totalTimeNS.map { duration(nanoseconds: $0) }, byteCount(bytes, locale: locale)]
            .compactMap { $0 }.joined(separator: ", ")
    }

    private static let reasonPhrases: [UInt16: String] = [
        100: "Continue", 101: "Switching Protocols", 102: "Processing", 103: "Early Hints",
        200: "OK", 201: "Created", 202: "Accepted", 203: "Non-Authoritative Information", 204: "No Content",
        205: "Reset Content", 206: "Partial Content", 207: "Multi-Status", 208: "Already Reported", 226: "IM Used",
        300: "Multiple Choices", 301: "Moved Permanently", 302: "Found", 303: "See Other", 304: "Not Modified",
        305: "Use Proxy", 307: "Temporary Redirect", 308: "Permanent Redirect",
        400: "Bad Request", 401: "Unauthorized", 402: "Payment Required", 403: "Forbidden", 404: "Not Found",
        405: "Method Not Allowed", 406: "Not Acceptable", 407: "Proxy Authentication Required", 408: "Request Timeout",
        409: "Conflict", 410: "Gone", 411: "Length Required", 412: "Precondition Failed", 413: "Content Too Large",
        414: "URI Too Long", 415: "Unsupported Media Type", 416: "Range Not Satisfiable", 417: "Expectation Failed",
        418: "I'm a Teapot", 421: "Misdirected Request", 422: "Unprocessable Content", 423: "Locked",
        424: "Failed Dependency", 425: "Too Early", 426: "Upgrade Required", 428: "Precondition Required",
        429: "Too Many Requests", 431: "Request Header Fields Too Large", 451: "Unavailable For Legal Reasons",
        500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway", 503: "Service Unavailable",
        504: "Gateway Timeout", 505: "HTTP Version Not Supported", 506: "Variant Also Negotiates",
        507: "Insufficient Storage", 508: "Loop Detected", 510: "Not Extended", 511: "Network Authentication Required",
    ]
}

/// What the URL bar's status slot shows for a document's latest run.
public enum RunOutcomeBadge: Equatable, Sendable {
    /// The response status, such as "200 OK".
    case status(UInt16)
    /// The run failed; `title` is the failure's headline, used as the help tag.
    case failed(title: String)
    case cancelled

    /// A failure wins over a status: it is the latest outcome, and the response pane shows
    /// the failure too. Nil when nothing has run yet.
    public init?(status: UInt16?, failure: RunFailure?, host: String? = nil) {
        if let failure {
            self = failure.kind == "cancelled" ? .cancelled : .failed(title: RunFailureMessage(failure, host: host).title)
        } else if let status {
            self = .status(status)
        } else {
            return nil
        }
    }

    /// Short text for the wide layout: "200 OK", "Failed" or "Cancelled".
    public var label: String {
        switch self {
        case let .status(status): ResponseFormatting.statusLine(status)
        case .failed: "Failed"
        case .cancelled: "Cancelled"
        }
    }

    /// The full description for the help tag and VoiceOver.
    public var detail: String {
        switch self {
        case let .status(status): ResponseFormatting.statusLine(status)
        case let .failed(title): title
        case .cancelled: "Request Cancelled"
        }
    }
}

/// User-facing copy for every run failure kind the Rust bridge and Swift runners produce.
public struct RunFailureMessage: Equatable, Sendable {
    public enum Category: Equatable, Sendable {
        case cancelled, connection, timeout, tls, proxy, secrets, invalidRequest, response, internalError
    }

    public let category: Category
    public let title: String
    public let message: String

    public var systemImage: String {
        switch category {
        case .cancelled: "xmark.circle"
        case .connection, .proxy: "network.slash"
        case .timeout: "clock.badge.exclamationmark"
        case .tls: "lock.slash"
        case .secrets: "key.slash"
        case .invalidRequest: "exclamationmark.triangle"
        case .response, .internalError: "exclamationmark.circle"
        }
    }

    /// Whether the request's network settings (proxy, TLS, timeouts) are a plausible fix.
    public var suggestsNetworkSettings: Bool {
        [.connection, .timeout, .tls, .proxy].contains(category)
    }

    public init(_ failure: RunFailure, host: String? = nil) {
        let target = host.map { " \($0)" } ?? ""
        let to = host.map { " to \($0)" } ?? ""
        switch failure.kind {
        case "cancelled":
            self.init(.cancelled, "Request Cancelled", "The request was cancelled before the response finished.")
        case "connection":
            self.init(.connection, "Couldn’t Connect\(to)",
                "The connection failed before the server responded. Check that the server is running and that the address, port, proxy and TLS settings are correct.")
        case "connect_timeout", "timeout":
            self.init(.timeout, "Connection\(to) Timed Out",
                "The server didn’t accept the connection in time. Check the address and your network or proxy settings.")
        case "total_timeout":
            self.init(.timeout, "Request\(to) Timed Out",
                "The response didn’t finish within the total timeout. Increase the timeout in Settings or try again.")
        case "read_timeout":
            self.init(.timeout, "Server\(target.isEmpty ? "" : " at\(target)") Stopped Responding",
                "No data arrived within the read timeout. Increase the timeout in Settings or try again.")
        case "tls":
            self.init(.tls, "TLS Verification Failed", "The server’s certificate couldn’t be verified. Check the certificate and trust settings.")
        case "tls_configuration":
            self.init(.tls, "TLS Settings Couldn’t Be Loaded",
                "The client certificate or custom CA for this request couldn’t be loaded. Check the TLS settings.")
        case "proxy", "proxy_configuration":
            self.init(.proxy, "Proxy Unavailable",
                "The proxy for this request couldn’t be set up. Check the proxy address, port and credentials.")
        case "keychain":
            self.init(.secrets, "Secrets Unavailable",
                "Wirebolt couldn’t read secrets from the Keychain. Unlock the Keychain and try again.")
        case "request":
            self.init(.invalidRequest, "Couldn’t Send the Request",
                "The request failed while it was being sent. Check the body and any attached file.")
        case "response_body":
            self.init(.response, "Couldn’t Read the Response", "The response body couldn’t be received or decoded.")
        case "response_too_large":
            self.init(.response, "Response Too Large", "The response exceeded the maximum size Wirebolt stores.")
        case "stream_stopped":
            self.init(.response, "Response Stopped", "Receiving the response was stopped before it finished.")
        case "transport":
            self.init(.connection, "Request\(to) Failed", "The HTTP connection failed while the request was in progress. Try again.")
        case "invalid_request", "invalid_input", "unresolved_value":
            self.init(.invalidRequest, "Invalid Request",
                failure.issues.first.map(Self.describe) ?? "Check the URL, headers and body, then try again.")
        case "internal", "bridge":
            self.init(.internalError, "Request Failed", "Wirebolt couldn’t run the request. Try again.")
        default:
            self.init(.internalError, "Request Failed", Self.sentence(failure.kind))
        }
    }

    private init(_ category: Category, _ title: String, _ message: String) {
        self.category = category
        self.title = title
        self.message = message
    }

    /// "host:port" of an absolute URL; nil for templates or unparsable text.
    public static func host(from url: String?) -> String? {
        guard let url, !url.contains("{{"), let components = URLComponents(string: url.trimmingCharacters(in: .whitespaces)),
              let host = components.host, !host.isEmpty else { return nil }
        return components.port.map { "\(host):\($0)" } ?? host
    }

    static func describe(_ issue: RequestIssue) -> String {
        let reference = issue.reference.map { "“\($0)”" }
        let detail = switch issue.kind {
        case "invalid_method": "The HTTP method isn’t valid."
        case "invalid_url": "The URL isn’t valid."
        case "invalid_header_name": "A header name isn’t valid."
        case "invalid_header_value": "A header value isn’t valid."
        case "conflicting_header": "Two headers conflict with each other."
        case "invalid_json": "The JSON body isn’t valid."
        case "invalid_body": "The body isn’t valid."
        case "file_unavailable": "The body file can’t be read."
        case "invalid_template": "A {{variable}} template isn’t valid."
        case "template_too_deep": "Variables are nested too deeply."
        case "resolved_value_too_large": "A resolved value is too large."
        case "missing_variable", "unresolved_value": "The variable \(reference ?? "") isn’t defined in the active environment."
        case "cyclic_variable": "The variable \(reference ?? "") refers to itself."
        case "missing_secret": "The secret \(reference ?? "") isn’t in the Keychain."
        default: sentence(issue.kind)
        }
        let location = issue.path.isEmpty || issue.path == "values" ? "" : " (\(issue.path))"
        return detail.replacingOccurrences(of: "  ", with: " ") + location
    }

    /// "some_kind" → "Some kind." without title-casing acronyms into "Tls".
    static func sentence(_ kind: String) -> String {
        let words = kind.replacingOccurrences(of: "_", with: " ")
        guard let first = words.first else { return "The request failed." }
        return first.uppercased() + words.dropFirst() + "."
    }
}
