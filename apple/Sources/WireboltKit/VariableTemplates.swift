import Foundation

/// One `{{name}}` reference in an editable value.
public struct VariableReference: Equatable, Sendable {
    /// The name with surrounding whitespace removed, as the Rust resolver looks it up.
    public let name: String
    /// UTF-16 range of the whole reference, braces included, for AppKit text APIs.
    public let range: NSRange
}

/// An unfinished `{{partial` before the insertion point, offered for completion.
public struct OpenVariableReference: Equatable, Sendable {
    public let prefix: String
    /// UTF-16 range from just after `{{` to the insertion point; a completion replaces it.
    public let replacementRange: NSRange
    /// True when `}}` already follows the insertion point, so a completion must not add it.
    public let isClosed: Bool
}

/// Locates variable references with the same rules as request preparation in Rust:
/// `{{` opens, the next `}}` closes, and the trimmed name must not be empty.
public enum VariableTemplate {
    private static let maximumNameLength = 128

    public static func references(in text: String) -> [VariableReference] {
        guard text.contains("{{") else { return [] }
        let source = text as NSString
        var references: [VariableReference] = []
        var location = 0
        while location < source.length {
            let open = source.range(of: "{{", range: NSRange(location: location, length: source.length - location))
            guard open.location != NSNotFound else { break }
            let afterOpen = open.location + 2
            let close = source.range(of: "}}", range: NSRange(location: afterOpen, length: source.length - afterOpen))
            guard close.location != NSNotFound else { break }
            let name = source.substring(with: NSRange(location: afterOpen, length: close.location - afterOpen))
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if !name.isEmpty {
                references.append(VariableReference(name: name, range: NSRange(location: open.location, length: close.location + 2 - open.location)))
            }
            location = close.location + 2
        }
        return references
    }

    /// The unfinished reference that ends at `caret` (a UTF-16 offset), if any.
    public static func openReference(in text: String, caret: Int) -> OpenVariableReference? {
        let source = text as NSString
        guard caret >= 2, caret <= source.length else { return nil }
        let searchStart = max(0, caret - maximumNameLength - 2)
        let open = source.range(of: "{{", options: .backwards, range: NSRange(location: searchStart, length: caret - searchStart))
        guard open.location != NSNotFound else { return nil }
        var start = open.location + 2
        // `{{{` is treated as a literal brace followed by an opening `{{`.
        while start < caret, source.character(at: start) == 0x7B { start += 1 }
        let partial = source.substring(with: NSRange(location: start, length: caret - start))
        guard !partial.contains("}"), !partial.contains("{"),
              partial.rangeOfCharacter(from: .whitespacesAndNewlines) == nil else { return nil }
        let rest = NSRange(location: caret, length: source.length - caret)
        let isClosed = source.range(of: "}}", options: .anchored, range: rest).location != NSNotFound
        return OpenVariableReference(prefix: partial, replacementRange: NSRange(location: start, length: caret - start), isClosed: isClosed)
    }
}

/// Variables visible to a request: the global environment, overridden by the selected
/// environment. Used for highlighting, hover details and completion; never exposes
/// secret material, only whether a value is a Keychain reference.
public struct VariableCatalog: Equatable, Sendable {
    public struct Entry: Equatable, Sendable {
        public let name: String
        public let value: ValueSource
        public let environmentName: String

        /// The value as shown in help tags and completion rows.
        public var displayValue: String {
            switch value {
            case .literal(let text): text
            case .secret: VariableCatalog.secretPlaceholder
            }
        }
    }

    public static let secretPlaceholder = "•••• (secret)"
    public static let empty = VariableCatalog(entries: [])

    /// Sorted by name.
    public let entries: [Entry]
    private let byName: [String: Entry]

    private init(entries: [Entry]) {
        self.entries = entries.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
        byName = Dictionary(entries.map { ($0.name, $0) }, uniquingKeysWith: { _, newest in newest })
    }

    public init(environments: [EnvironmentDraft], selectedEnvironmentID: String?) {
        var merged: [String: Entry] = [:]
        let global = environments.first { $0.id == WorkspaceDraft.globalEnvironmentID }
        let selected = environments.first { $0.id == selectedEnvironmentID && $0.id != WorkspaceDraft.globalEnvironmentID }
        for environment in [global, selected].compactMap(\.self) {
            for (name, value) in environment.enabledValues {
                merged[name] = Entry(name: name, value: value, environmentName: environment.name)
            }
        }
        self.init(entries: Array(merged.values))
    }

    public func entry(named name: String) -> Entry? { byName[name] }

    /// Names starting with `prefix` first, then names containing it; case-insensitive.
    public func completions(matching prefix: String) -> [Entry] {
        guard !prefix.isEmpty else { return entries }
        let starts = entries.filter { $0.name.range(of: prefix, options: [.caseInsensitive, .anchored]) != nil }
        let contains = entries.filter {
            $0.name.range(of: prefix, options: [.caseInsensitive, .anchored]) == nil && $0.name.localizedCaseInsensitiveContains(prefix)
        }
        return starts + contains
    }

    /// One line per distinct reference, e.g. `host = api.example.com (Staging)`, or nil
    /// when `text` has no references.
    public func summary(for text: String) -> String? {
        var seen: Set<String> = []
        let lines = VariableTemplate.references(in: text).compactMap { reference -> String? in
            guard seen.insert(reference.name).inserted else { return nil }
            guard let entry = entry(named: reference.name) else { return "\(reference.name): not defined in the active environments" }
            return "\(reference.name) = \(entry.displayValue) (\(entry.environmentName))"
        }
        return lines.isEmpty ? nil : lines.joined(separator: "\n")
    }
}
