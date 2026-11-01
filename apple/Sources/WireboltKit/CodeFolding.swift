import Foundation

public struct CodeFold: Equatable, Sendable {
    public let line: Int
    public let range: NSRange
    var rust: Fold { Fold(line: UInt64(max(0, line)), range: range.responseRange) }
}
public struct CodeProjection: Sendable {
    public enum Syntax: Sendable {
        case json, xml, html, none
        var rust: CodeSyntax {
            switch self {
            case .json: .json
            case .xml: .xml
            case .html: .html
            case .none: .none
            }
        }
    }
    public let source: String
    public let text: String
    public let folds: [CodeFold]
    public let collapsed: Set<Int>
    private let rust: AppCodeProjection
    private init(source: String, rust: AppCodeProjection, unfolded: Bool) {
        self.source = source
        self.rust = rust
        let snapshot = rust.snapshot(includeText: !unfolded)
        text = snapshot.text ?? source
        folds = snapshot.folds.map { CodeFold(line: Int(clamping: $0.line), range: $0.range.nsRange) }
        collapsed = Set(snapshot.collapsed.map { Int(clamping: $0) })
    }
    public init(unfolded source: String, folds: [CodeFold] = []) {
        self.init(source: source, rust: AppCodeProjection.unfolded(source: source, folds: folds.map(\.rust)), unfolded: true)
    }
    public init(source: String, collapsed: Set<Int> = [], syntax: Syntax = .json) {
        let projection = try? responseCall {
            try AppCodeProjection(source: source, collapsed: collapsed.map { UInt64(max(0, $0)) }, syntax: syntax.rust, cancellation: $0)
        }
        self.init(source: source, rust: projection ?? AppCodeProjection.unfolded(source: source, folds: []), unfolded: collapsed.isEmpty)
    }
    public func updatingUnfoldedSource(_ source: String, range: NSRange, replacement: String, removed: String) -> CodeProjection? {
        rust.updatingEdit(range: range.responseRange, replacement: replacement, removed: removed).map {
            CodeProjection(source: source, rust: $0, unfolded: true)
        }
    }
    public func sourceOffset(_ display: Int, afterPlaceholder: Bool = false) -> Int {
        Int(clamping: rust.sourceOffset(display: UInt64(max(0, display)), afterPlaceholder: afterPlaceholder))
    }
    public func sourceOffsets(_ display: [Int]) -> [Int] {
        rust.sourceOffsets(offsets: display.map { UInt64(max(0, $0)) }).map { Int(clamping: $0) }
    }
    public func displayOffset(_ source: Int) -> Int { Int(clamping: rust.displayOffset(source: UInt64(max(0, source)))) }
    public func replacing(displayRange: NSRange, with replacement: String) -> String {
        rust.replacing(range: displayRange.responseRange, replacement: replacement)
    }
}
