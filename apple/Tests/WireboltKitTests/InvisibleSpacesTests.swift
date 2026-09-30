import AppKit
import Testing
@testable import WireboltKit

@Suite("Invisible spaces")
struct InvisibleSpacesTests {
    @Test("Space runs are found in UTF-16 offsets around non-ASCII text")
    func runs() {
        #expect(InvisibleSpaces.runs(in: "") == [])
        #expect(InvisibleSpaces.runs(in: "{\"a\":1}") == [])
        #expect(InvisibleSpaces.runs(in: "  \"café\":  \"東京 🙂\" ") == [
            NSRange(location: 0, length: 2), NSRange(location: 9, length: 2), NSRange(location: 14, length: 1), NSRange(location: 18, length: 1),
        ])
        #expect(InvisibleSpaces.runs(in: "a\tb") == [])
    }

    @Test("Marking keeps length, other characters and their attributes")
    func marking() {
        let source = NSMutableAttributedString(string: "  \"key\": \"a b\"")
        source.addAttribute(.foregroundColor, value: NSColor.systemRed, range: NSRange(location: 2, length: 5))
        let marked = InvisibleSpaces.marking(source, attributes: [.foregroundColor: NSColor.tertiaryLabelColor])
        #expect(marked.string == "··\"key\":·\"a·b\"")
        #expect(marked.length == source.length)
        #expect(marked.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor == .tertiaryLabelColor)
        #expect(marked.attribute(.foregroundColor, at: 3, effectiveRange: nil) as? NSColor == .systemRed)
        #expect(marked.attribute(.foregroundColor, at: 8, effectiveRange: nil) as? NSColor == .tertiaryLabelColor)
        let plain = NSAttributedString(string: "{}")
        #expect(InvisibleSpaces.marking(plain, attributes: [:]) === plain)
    }

    @Test("The marker keeps the space advance in the editor font, so columns do not move")
    func markerAdvance() {
        for size in [10.0, 12.0, 28.0] {
            let font = NSFont.monospacedSystemFont(ofSize: size, weight: .regular)
            let space = (" " as NSString).size(withAttributes: [.font: font]).width
            let marker = (InvisibleSpaces.marker as NSString).size(withAttributes: [.font: font]).width
            #expect(abs(space - marker) < 0.001)
        }
    }
}
