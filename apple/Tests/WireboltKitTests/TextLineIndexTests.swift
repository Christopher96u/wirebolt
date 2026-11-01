import Foundation
import Testing
@testable import WireboltKit

struct TextLineIndexTests {
    @Test(arguments: ["", "abc", "one\ntwo\n", "café 東京 🚀\r\nsecond\n", "\n\n"])
    func editsMatchFreshIndex(_ initial: String) {
        let source = NSMutableString(string: initial)
        var index = TextLineIndex(initial)
        for replacement in ["\n", "👨‍👩‍👧‍👦", "x\ny\n", "", "\r\n", "東京"] {
            for position in [0, source.length] {
                let range = NSRange(location: min(position, source.length), length: 0)
                source.replaceCharacters(in: range, with: replacement)
                index.replace(range, with: replacement)
                let fresh = TextLineIndex(source as String)
                #expect(index.starts == fresh.starts)
                #expect(index.length == fresh.length)
            }
        }
        let range = NSRange(location: 0, length: source.length)
        index.replace(range, with: "")
        #expect(index.starts == [0])
        #expect(index.length == 0)
    }

    @Test func removesAndJoinsLinesWithoutDuplicateOffsets() {
        var index = TextLineIndex("one\ntwo\nthree\n")
        index.replace(NSRange(location: 3, length: 5), with: "!")
        #expect(index.starts == [0, 10])
        #expect(index.line(at: 9) == 0)
        #expect(index.line(at: 10) == 1)
        index.replace(NSRange(location: 10, length: 0), with: "x\ny")
        #expect(index.starts == [0, 10, 12])
    }

    @Test func repeatedMiddleEditsUseUTF16Coordinates() {
        let source = NSMutableString(string: String(repeating: "東京 🚀\n", count: 1000))
        var index = TextLineIndex(source as String)
        for step in 0..<80 {
            let line = (step * 17) % index.starts.count
            let start = index.starts[line]
            let end = line + 1 < index.starts.count ? index.starts[line + 1] : source.length
            let range = NSRange(location: start, length: end - start)
            let replacement = step.isMultiple(of: 3) ? "joined" : "café\n🚀\n"
            source.replaceCharacters(in: range, with: replacement)
            index.replace(range, with: replacement)
            #expect(index.starts == TextLineIndex(source as String).starts)
        }
    }
}

extension TextLineIndexTests {
    @Test func copiedFacadesKeepIndependentLineSnapshots() {
        let original = TextLineIndex("one\ntwo\n")
        var edited = original
        edited.replace(NSRange(location: 0, length: 0), with: "x\n")
        #expect(original.starts == [0, 4, 8])
        #expect(edited.starts == [0, 2, 6, 10])
        #expect(original.length == 8 && edited.length == 10)
    }
}

extension TextLineIndexTests {
    @Test func invalidBridgeRangesKeepTheIndexUnchanged() {
        var index = TextLineIndex("one\ntwo")
        for range in [NSRange(location: -1, length: 0), NSRange(location: 0, length: -1),
                      NSRange(location: NSNotFound, length: 0), NSRange(location: 2, length: Int.max),
                      NSRange(location: 8, length: 0)] {
            index.replace(range, with: "changed\n")
            #expect(index.starts == [0, 4])
            #expect(index.length == 7)
        }
        index.replace(NSRange(location: 7, length: 0), with: "\n")
        #expect(index.starts == [0, 4, 8])
    }
}
