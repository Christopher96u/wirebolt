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
        #expect(try await cache.render("```\nunclosed code").text.string.contains("unclosed code"))
    }
}
