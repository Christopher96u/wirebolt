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
