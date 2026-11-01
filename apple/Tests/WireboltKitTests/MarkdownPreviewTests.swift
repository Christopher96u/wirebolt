import AppKit
import Testing
@testable import WireboltKit

@Suite("Markdown preview")
struct MarkdownPreviewTests {
    @Test func blocksAndInlineFormatting() async throws {
        let result = try await MarkdownPreviewCache().render("# Title\n\n- **Bold** item\n- second\n\n> quote\n\n```swift\nlet x = 1\n```\n\n[link](https://example.com)")
        #expect(result.text.string == "Title\n•  Bold item\n•  second\nquote\nlet x = 1\n\nlink")
        let titleFont = try #require(result.text.attribute(.font, at: 0, effectiveRange: nil) as? NSFont)
        #expect(titleFont.pointSize > 13)
        let bold = (result.text.string as NSString).range(of: "Bold")
        let font = try #require(result.text.attribute(.font, at: bold.location, effectiveRange: nil) as? NSFont)
        #expect(font.fontDescriptor.symbolicTraits.contains(.bold))
        let code = (result.text.string as NSString).range(of: "let x")
        #expect(result.text.attribute(.backgroundColor, at: code.location - 1, effectiveRange: nil) == nil)
        let link = (result.text.string as NSString).range(of: "link")
        #expect(result.text.attribute(.link, at: link.location, effectiveRange: nil) as? URL == URL(string: "https://example.com"))
    }

    @Test func nestedListsUnicodeAndNoRemoteResources() async throws {
        let source = "1. café 🚀\n2. 東京\n   - nested\n\n![alt text](https://example.invalid/tracking.png)\n\n[unsafe](file:///etc/passwd)"
        let result = try await MarkdownPreviewCache().render(source)
        #expect(result.text.string.contains("1.  café 🚀"))
        #expect(result.text.string.contains("2.  東京"))
        #expect(result.text.string.contains("•  nested"))
        #expect(result.text.string.contains("alt text"))
        result.text.enumerateAttributes(in: NSRange(location: 0, length: result.text.length)) { attributes, _, _ in
            #expect(attributes[.attachment] == nil)
            #expect(attributes[.link] == nil)
        }
    }

    @Test func cacheReuseInvalidationAndBudget() async throws {
        let cache = MarkdownPreviewCache(byteLimit: 1_024)
        let first = try await cache.render("**one**")
        let repeated = try await cache.render("**one**")
        #expect(first === repeated)
        #expect(await cache.parseCount == 1)
        let edited = try await cache.render("**two**")
        #expect(edited.text.string == "two")
        #expect(first.text.string == "one")
        for index in 0..<20 { _ = try await cache.render("Note \(index)") }
        #expect(await cache.retainedBytes <= 1_024)
        _ = try await cache.render(String(repeating: "large ", count: 1_000))
        #expect(await cache.retainedBytes <= 1_024)
    }

    @Test func cancelledWorkDoesNotParseOrPublish() async throws {
        let cache = MarkdownPreviewCache()
        let task = Task {
            withUnsafeCurrentTask { $0?.cancel() }
            return try await cache.render("# Cancel me")
        }
        do { _ = try await task.value; Issue.record("Expected cancellation") }
        catch is CancellationError { }
        #expect(await cache.parseCount == 0)
    }

    @Test func emptyAndUnfinishedMarkdownRemainReadable() async throws {
        let cache = MarkdownPreviewCache()
        #expect(try await cache.render("").text.length == 0)
        #expect(try await cache.render("**unfinished").text.string == "**unfinished")
        #expect(try await cache.render("```\nunclosed code").text.string == "unclosed code\n")
    }
}

// Frozen rendering oracle from 081c6d0. Compare text and all native styling,
// including the unstyled separators, rather than assuming CommonMark parity.
extension MarkdownPreviewTests {
    static let corpus: [String] = [
        "", "# One\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six",
        "Title\n=====\n\nSubtitle\n-----", "plain *em* **strong** ***both*** ~~strike~~ ~single~",
        "one `code` two ``a ` b`` and **`strong code`**", "```swift\nlet x = 1\n```\n\n    indented\n    code\n",
        "- first\n- second\n  - nested\n    3. ordered\n    4. next\n- last",
        "3. start\n4. **bold** item\n\n   paragraph\n\n5. last", "- [ ] todo\n- [x] done",
        "> quote\n>\n> second *paragraph*\n> > nested\n> - item",
        "[link](https://example.com/a?q=b) <https://example.com> <me@example.com>",
        "[relative](../page) [unsafe](file:///etc/passwd) [script](javascript:alert) [mail](mailto:a@b.com)",
        "![alt *text*](https://example.invalid/image.png) then ![empty]()",
        "| a | b |\n| - | - |\n| **x** | `y` |", "<div>\nhello\n</div>\n\n<b>inline</b> <!-- comment -->",
        "soft\nbreak  \nhard\\\nbreak", "before\n\n---\n\nafter\n\n***\n\nend",
        "footnote[^1]\n\n[^1]: definition", "\\*escaped\\* &amp; &#x1F680; \\[link] \\\\ end",
        "café é 東京 🚀 **🧑🏽‍💻**\n\nעברית العربية", "**unfinished", "```\nunclosed code",
        "- ![image](x) **bold**\n\n---\n\n> `quote`\n> next",
    ]

    @Test(arguments: corpus) func matchesFoundationRenderer(source: String) async throws {
        let old = try Self.legacyRender(source)
        let new = try await MarkdownPreviewCache().render(source)
        #expect(new.text.string == old.text.string)
        #expect(new.text.isEqual(to: old.text))
        #expect(new.cost == old.cost)
    }
    static func legacyRender(_ source: String) throws -> RenderedMarkdown {
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
