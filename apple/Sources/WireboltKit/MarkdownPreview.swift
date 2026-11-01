import AppKit

public final class RenderedMarkdown: @unchecked Sendable {
    public let text: NSAttributedString
    public let cost: Int
    init(_ text: NSAttributedString, cost: Int) {
        self.text = text
        self.cost = cost
    }
}

/// Rust assembles runs and owns cache retention; Foundation supplies Markdown intents.
public actor MarkdownPreviewCache {
    public static let shared = MarkdownPreviewCache()
    private let rust: AppMarkdownCache
    private var rendered: [UInt64: RenderedMarkdown] = [:]
    public var retainedBytes: Int { Int(clamping: rust.stats().retainedBytes) }
    public var parseCount: Int { Int(clamping: rust.stats().parseCount) }
    public init(byteLimit: Int = 8 * 1_024 * 1_024) { rust = AppMarkdownCache(byteLimit: UInt64(max(0, byteLimit)), parser: FoundationMarkdownParser()) }
    public func render(_ source: String) async throws -> RenderedMarkdown {
        let rust = rust
        let document = try await responseAsync { try responseCall { try rust.render(source: source, cancellation: $0) } }
        let id = document.id()
        if let cached = rendered[id] { return cached }
        let output = NSMutableAttributedString(string: "")
        for run in document.runs() {
            try Task.checkCancellation()
            output.append(NSAttributedString(string: run.text, attributes: Self.attributes(run.style)))
        }
        let result = RenderedMarkdown(NSAttributedString(attributedString: output), cost: Int(clamping: document.cost()))
        let retained = Set(rust.stats().retainedIds)
        rendered = rendered.filter { retained.contains($0.key) }
        if retained.contains(id) { rendered[id] = result }
        return result
    }
    private static func attributes(_ style: MarkdownStyle) -> [NSAttributedString.Key: Any] {
        if style.blockSeparator {
            return [.font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.labelColor]
        }
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = 3
        paragraph.paragraphSpacing = 6
        if style.depth > 0 {
            paragraph.firstLineHeadIndent = CGFloat(style.depth - 1) * 20
            paragraph.headIndent = CGFloat(style.depth) * 20
        }
        if style.quote {
            paragraph.firstLineHeadIndent += 16
            paragraph.headIndent += 16
        }
        var font = style.code ? NSFont.monospacedSystemFont(ofSize: 12, weight: .regular) : NSFont.systemFont(ofSize: 13)
        if style.heading > 0 { font = NSFont.systemFont(ofSize: CGFloat(max(14, 26 - Int(style.heading) * 2)), weight: .bold) }
        var traits = font.fontDescriptor.symbolicTraits
        if style.bold { traits.insert(.bold) }
        if style.italic { traits.insert(.italic) }
        font = NSFont(descriptor: font.fontDescriptor.withSymbolicTraits(traits), size: font.pointSize) ?? font
        var attributes: [NSAttributedString.Key: Any] = [
            .font: font, .foregroundColor: style.quote ? NSColor.secondaryLabelColor : NSColor.labelColor, .paragraphStyle: paragraph,
        ]
        if style.code { attributes[.backgroundColor] = NSColor.quaternaryLabelColor }
        if style.strikethrough { attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
        if let link = style.link, let url = URL(string: link) { attributes[.link] = url }
        return attributes
    }
}

/// Foundation's full Markdown interpretation is a platform service: its block,
/// image, HTML and extension behavior is part of the existing Notes appearance.
private final class FoundationMarkdownParser: AppMarkdownParser, Sendable {
    func parse(source: String, cancellation: AppResponseCancellation) throws -> [MarkdownInputRun] {
        try cancellation.check()
        let markdown = try AttributedString(markdown: source, options: .init(interpretedSyntax: .full))
        var output: [MarkdownInputRun] = []
        output.reserveCapacity(markdown.runs.count)
        for run in markdown.runs {
            if Task.isCancelled { throw ResponseError.Cancelled }
            let components = run.presentationIntent?.components ?? []
            var heading: UInt32 = 0
            var code = false
            var quote = false
            var item: (id: UInt64, number: UInt64)?
            var ordered: Bool?
            var depth: UInt32 = 0
            for component in components {
                switch component.kind {
                case .header(let level): heading = UInt32(level)
                case .codeBlock: code = true
                case .blockQuote: quote = true
                case .listItem(let number):
                    if item == nil { item = (UInt64(component.identity), UInt64(number)) }
                case .orderedList:
                    if ordered == nil { ordered = true }
                    depth += 1
                case .unorderedList:
                    if ordered == nil { ordered = false }
                    depth += 1
                default: break
                }
            }
            let inline = run.inlinePresentationIntent ?? []
            output.append(MarkdownInputRun(
                text: String(markdown[run.range].characters),
                style: MarkdownStyle(
                    heading: heading, code: code || inline.contains(.code), quote: quote, depth: depth,
                    bold: inline.contains(.stronglyEmphasized), italic: inline.contains(.emphasized),
                    strikethrough: inline.contains(.strikethrough), link: run.link?.absoluteString, blockSeparator: false),
                block: components.first.map { UInt64($0.identity) },
                item: item.map { MarkdownListItem(id: $0.id, number: $0.number, ordered: ordered == true) }))
        }
        try cancellation.check()
        return output
    }
}
