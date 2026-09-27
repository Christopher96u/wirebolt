import AppKit
import CryptoKit

/// Immutable after construction; NSTextView receives its own mutable storage copy.
public final class RenderedMarkdown: @unchecked Sendable {
    public let text: NSAttributedString
    public let cost: Int
    init(_ text: NSAttributedString, cost: Int) { self.text = text; self.cost = cost }
}

/// Serial background parsing bounds concurrent work and retained preview memory.
public actor MarkdownPreviewCache {
    public static let shared = MarkdownPreviewCache()
    private let byteLimit: Int
    private var entries: [String: RenderedMarkdown] = [:]
    private var recency: [String] = []
    public private(set) var retainedBytes = 0
    public private(set) var parseCount = 0

    public init(byteLimit: Int = 8 * 1_024 * 1_024) { self.byteLimit = max(0, byteLimit) }

    public func render(_ source: String) throws -> RenderedMarkdown {
        try Task.checkCancellation()
        let key = SHA256.hash(data: Data(source.utf8)).map { String(format: "%02x", $0) }.joined()
        if let cached = entries[key] {
            recency.removeAll { $0 == key }; recency.append(key)
            return cached
        }
        parseCount += 1
        let rendered = try Self.parse(source)
        try Task.checkCancellation()
        if rendered.cost <= byteLimit {
            while retainedBytes + rendered.cost > byteLimit || entries.count >= 8 {
                guard !recency.isEmpty else { break }
                let oldest = recency.removeFirst()
                retainedBytes -= entries.removeValue(forKey: oldest)?.cost ?? 0
            }
            entries[key] = rendered; recency.append(key); retainedBytes += rendered.cost
        }
        return rendered
    }

    private static func parse(_ source: String) throws -> RenderedMarkdown {
        let markdown = try AttributedString(markdown: source, options: .init(interpretedSyntax: .full))
        try Task.checkCancellation()
        let output = NSMutableAttributedString(string: "")
        var previousBlock: Int?
        var previousItem: Int?
        var runs = 0
        for run in markdown.runs {
            try Task.checkCancellation()
            runs += 1
            let components = run.presentationIntent?.components ?? []
            let block = components.first?.identity
            var heading: Int?
            var code = false
            var quote = false
            var item: (id: Int, number: Int)?
            var ordered: Bool?
            var depth = 0
            for component in components {
                switch component.kind {
                case .header(let level): heading = level
                case .codeBlock: code = true
                case .blockQuote: quote = true
                case .listItem(let number): if item == nil { item = (component.identity, number) }
                case .orderedList: if ordered == nil { ordered = true }; depth += 1
                case .unorderedList: if ordered == nil { ordered = false }; depth += 1
                default: break
                }
            }
            let paragraph = NSMutableParagraphStyle()
            paragraph.lineSpacing = 3
            paragraph.paragraphSpacing = 6
            if depth > 0 {
                paragraph.firstLineHeadIndent = CGFloat(max(0, depth - 1)) * 20
                paragraph.headIndent = CGFloat(depth) * 20
            }
            if quote { paragraph.firstLineHeadIndent += 16; paragraph.headIndent += 16 }
            var attributes: [NSAttributedString.Key: Any] = [
                .font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.labelColor,
                .paragraphStyle: paragraph,
            ]
            let inline = run.inlinePresentationIntent ?? []
            let monospace = code || inline.contains(.code)
            var font = monospace ? NSFont.monospacedSystemFont(ofSize: 12, weight: .regular) : NSFont.systemFont(ofSize: 13)
            if let heading { font = NSFont.systemFont(ofSize: CGFloat(max(14, 26 - heading * 2)), weight: .bold) }
            var traits = font.fontDescriptor.symbolicTraits
            if inline.contains(.stronglyEmphasized) { traits.insert(.bold) }
            if inline.contains(.emphasized) { traits.insert(.italic) }
            font = NSFont(descriptor: font.fontDescriptor.withSymbolicTraits(traits), size: font.pointSize) ?? font
            attributes[.font] = font
            if monospace { attributes[.backgroundColor] = NSColor.quaternaryLabelColor }
            if quote { attributes[.foregroundColor] = NSColor.secondaryLabelColor }
            if inline.contains(.strikethrough) { attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
            if let link = run.link, ["http", "https", "mailto"].contains(link.scheme?.lowercased() ?? "") {
                attributes[.link] = link
            }
            if block != previousBlock && output.length > 0 {
                output.append(NSAttributedString(string: "\n", attributes: [.font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.labelColor]))
            }
            if let item, item.id != previousItem {
                output.append(NSAttributedString(string: ordered == true ? "\(item.number).  " : "•  ", attributes: attributes))
            }
            output.append(NSAttributedString(string: String(markdown[run.range].characters), attributes: attributes))
            previousBlock = block
            previousItem = item?.id
        }
        return RenderedMarkdown(NSAttributedString(attributedString: output), cost: output.length * 2 + runs * 256)
    }
}
