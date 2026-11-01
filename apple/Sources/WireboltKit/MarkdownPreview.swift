import AppKit

public final class RenderedMarkdown: @unchecked Sendable {
    public let text: NSAttributedString
    public let cost: Int
    init(_ text: NSAttributedString, cost: Int) {
        self.text = text
        self.cost = cost
    }
}

/// Rust parses and chooses cache retention. Swift only builds native attributes.
public actor MarkdownPreviewCache {
    public static let shared = MarkdownPreviewCache()
    private let rust: AppMarkdownCache
    private var rendered: [UInt64: RenderedMarkdown] = [:]
    public var retainedBytes: Int { Int(clamping: rust.stats().retainedBytes) }
    public var parseCount: Int { Int(clamping: rust.stats().parseCount) }
    public init(byteLimit: Int = 8 * 1_024 * 1_024) { rust = AppMarkdownCache(byteLimit: UInt64(max(0, byteLimit))) }
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
