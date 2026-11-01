import Foundation

/// Native UTF-16 projections of Rust template spans.
public struct VariableReference: Equatable, Sendable {
    public let name: String
    public let range: NSRange
}
public struct OpenVariableReference: Equatable, Sendable {
    public let prefix: String
    public let replacementRange: NSRange
    public let isClosed: Bool
}
public enum VariableTemplate {
    public static func references(in text: String) -> [VariableReference] {
        domainTemplateReferences(text: text).map {
            VariableReference(name: $0.name, range: NSRange(location: Int($0.location), length: Int($0.length)))
        }
    }
    public static func openReference(in text: String, caret: Int) -> OpenVariableReference? {
        guard caret >= 0 else { return nil }
        return domainOpenReference(text: text, caret: UInt64(caret)).map {
            OpenVariableReference(
                prefix: $0.prefix, replacementRange: NSRange(location: Int($0.location), length: Int($0.length)), isClosed: $0.isClosed)
        }
    }
}
public struct VariableCatalog: Equatable, Sendable {
    public struct Entry: Equatable, Sendable {
        let domain: CatalogEntry
        public var name: String { domain.name }
        public var value: ValueSource { ValueSource(domain: domain.value) }
        public var environmentName: String { domain.environmentName }
        public var displayValue: String { domain.displayValue }
    }
    public static let secretPlaceholder = "•••• (secret)"
    public static let empty = VariableCatalog(environments: [], selectedEnvironmentID: nil)
    public let entries: [Entry]
    private let index: DomainCatalog
    private let byName: [String: Entry]
    public init(environments: [EnvironmentDraft], selectedEnvironmentID: String?) {
        let index = DomainCatalog(
            environments: environments.map(\.domain), selected: selectedEnvironmentID, comparison: NativeVariableComparison())
        self.index = index
        entries = index.entries().map { Entry(domain: $0) }
        byName = Dictionary(entries.map { ($0.name, $0) }, uniquingKeysWith: { _, newest in newest })
    }
    public static func == (lhs: Self, rhs: Self) -> Bool { lhs.entries == rhs.entries }
    public func entry(named name: String) -> Entry? { byName[name] }
    public func completions(matching prefix: String) -> [Entry] {
        index.completions(prefix: prefix).map { entries[Int($0)] }
    }
    public func summary(for text: String) -> String? {
        index.summary(text: text)
    }
}
public enum VariableDisplaySegment: Equatable, Sendable {
    case text(String)
    case variable(String)
    public static func segments(of text: String) -> [Self] {
        domainTemplateSegments(text: text).map { segment in
            switch segment {
            case .text(let value): .text(value)
            case .variable(let name): .variable(name)
            }
        }
    }
}
extension VariableReference {
    /// Native ranges used to hide chip delimiters while preserving editable source text.
    public func delimiterRanges(in text: String) -> (open: NSRange, close: NSRange) {
        let ranges = domainDelimiterRanges(text: text, location: UInt64(range.location), length: UInt64(range.length), name: name)
        return (NSRange(location: Int(ranges[0]), length: Int(ranges[1])), NSRange(location: Int(ranges[2]), length: Int(ranges[3])))
    }
}

/// Foundation supplies the user's locale; Rust owns completion ranking and catalog state.
private final class NativeVariableComparison: VariableComparisonServices, Sendable {
    func sortedNames(names: [String]) -> [String] {
        names.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }
    func nameMatches(names: [String], prefix: String) -> [NameMatch] {
        names.map {
            NameMatch(
                hasPrefix: $0.range(of: prefix, options: [.caseInsensitive, .anchored]) != nil,
                contains: $0.localizedCaseInsensitiveContains(prefix))
        }
    }
}
