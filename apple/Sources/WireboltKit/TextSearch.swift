import Foundation

public struct TextSearchQuery: Equatable, Sendable {
    public var text: String
    public var matchCase: Bool
    public var wholeWord: Bool
    public var regularExpression: Bool
    public static let maximumMatches = 20_000

    public init(text: String = "", matchCase: Bool = false, wholeWord: Bool = false, regularExpression: Bool = false) {
        self.text = text
        self.matchCase = matchCase
        self.wholeWord = wholeWord
        self.regularExpression = regularExpression
    }

    public func matches(in source: String, range: NSRange? = nil) throws -> [NSRange] {
        guard !text.isEmpty else { return [] }
        var pattern = regularExpression ? text : NSRegularExpression.escapedPattern(for: text)
        if wholeWord { pattern = #"(?<![\p{L}\p{N}_])(?:"# + pattern + #")(?![\p{L}\p{N}_])"# }
        let expression = try NSRegularExpression(pattern: pattern, options: matchCase ? [.anchorsMatchLines] : [.caseInsensitive, .anchorsMatchLines])
        let length = (source as NSString).length
        let scope = range.map { NSIntersectionRange($0, NSRange(location: 0, length: length)) } ?? NSRange(location: 0, length: length)
        var matches: [NSRange] = []
        expression.enumerateMatches(in: source, options: .reportProgress, range: scope) { result, _, stop in
            if Task.isCancelled { stop.pointee = true; return }
            if let result { matches.append(result.range) }
            if matches.count == Self.maximumMatches { stop.pointee = true }
        }
        try Task.checkCancellation()
        return matches
    }
}
