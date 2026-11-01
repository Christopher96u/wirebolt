import Foundation
import Synchronization

public enum ResponseFormatting {
    public static func duration(nanoseconds: UInt64) -> String { duration(seconds: Double(nanoseconds) / 1_000_000_000) }
    public static func duration(seconds: Double) -> String { appFormatDuration(seconds: seconds) }
    public static func elapsed(seconds: Double) -> String { appFormatElapsed(seconds: seconds) }
    // Foundation owns locale-specific units, punctuation and digit grouping.
    public static func byteCount(_ bytes: UInt64, locale: Locale = .autoupdatingCurrent) -> String {
        // The response bar formats the same size on every update; formatting is not cheap.
        let key = ByteCountKey(bytes: bytes, locale: locale.identifier)
        if let cached = lastByteCount.withLock({ $0?.key == key ? $0?.text : nil }) { return cached }
        let text = Int64(clamping: bytes).formatted(.byteCount(style: .file, spellsOutZero: false).locale(locale))
        lastByteCount.withLock { $0 = (key, text) }
        return text
    }

    private struct ByteCountKey: Equatable {
        let bytes: UInt64
        let locale: String
    }
    private static let lastByteCount = Mutex<(key: ByteCountKey, text: String)?>(nil)

    public static func statusLine(_ status: UInt16) -> String { appStatusLine(status: status) }
    public static func reasonPhrase(for status: UInt16) -> String { appReasonPhrase(status: status) }
    public static func completionSummary(status: UInt16?, totalTimeNS: UInt64?, bytes: UInt64, locale: Locale = .autoupdatingCurrent)
        -> String
    {
        appCompletionSummary(status: status, nanoseconds: totalTimeNS, formattedBytes: byteCount(bytes, locale: locale))
    }
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
        guard
            let result = appResponseBadge(
                status: status, failure: failure?.kind, issues: failure?.issues.map(\.responseIssue) ?? [], host: host)
        else { return nil }
        switch result {
        case .status(let status): self = .status(status)
        case .failed(let title): self = .failed(title: title)
        case .cancelled: self = .cancelled
        }
    }
    private var rust: ResponseBadge {
        switch self {
        case .status(let status): .status(status: status)
        case .failed(let title): .failed(title: title)
        case .cancelled: .cancelled
        }
    }
    public var label: String { appResponseBadgeLabel(badge: rust) }
    public var detail: String { appResponseBadgeDetail(badge: rust) }

}

public struct RunFailureMessage: Equatable, Sendable {
    public typealias Category = FailureCategory
    public let category: Category
    public let title: String
    public let message: String
    public let systemImage: String
    public let suggestsNetworkSettings: Bool
    public init(_ failure: RunFailure, host: String? = nil) {
        let result = appFailureMessage(kind: failure.kind, issues: failure.issues.map(\.responseIssue), host: host)
        category = result.category
        title = result.title
        message = result.message
        systemImage = result.systemImage
        suggestsNetworkSettings = result.suggestsNetworkSettings
    }
    public static func host(from url: String?) -> String? { appFailureHost(url: url) }
    static func describe(_ issue: RequestIssue) -> String { appDescribeIssue(issue: issue.responseIssue) }
    static func sentence(_ kind: String) -> String { appFailureSentence(kind: kind) }
}
extension RequestIssue {
    fileprivate var responseIssue: FailureIssue { FailureIssue(path: path, kind: kind, reference: reference) }
}
