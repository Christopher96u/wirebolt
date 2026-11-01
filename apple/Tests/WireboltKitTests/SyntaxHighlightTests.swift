import Foundation
import Testing
@testable import WireboltKit

@Suite("Batched editor syntax parity")
struct SyntaxHighlightTests {
    @Test func tokenPatternsKeepNativeRangesAndPrecedence() throws {
        let fixtures: [(SyntaxLanguage, String, [(String, SyntaxTokenKind)])] = [
            (.json, #"{"🚀": true, "null": false, "n": -12.5, "s": "escaped \" true", "u": "https://x/a?b=1"}"#, [
                (#"\b(true|false)\b"#, .keyword), (#"\bnull\b"#, .keyword), (#"-?\b\d+(?:\.\d+)?\b"#, .number),
                (#"\"(?:\\.|[^\"\\])*\""#, .string), (#"\"(?:\\.|[^\"\\])*\"(?=\s*:)"#, .key), (#"https?://[^\"\s]+"#, .url),
            ]),
            (.xml, "<node x=\"🚀\">text</node>\n<!-- comment -->", [(#"</?[A-Za-z][^>]*>"#, .key), (#"\"[^\"]*\""#, .string)]),
            (.http, "GET /🚀 HTTP/1.1\nHost: example.com\nSecret: •••\ninvalid: x", [
                (#"(?m)^[A-Z]+\s+\S+\s+HTTP/\d(?:\.\d)?$"#, .keyword), (#"(?m)^[A-Za-z0-9-]+(?=:)"#, .key), (#"•+"#, .string),
            ]),
        ]
        let http = fixtures[2]
        let lineEndings = ["\r\n", "\r", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}"].map { (http.0, http.1.replacingOccurrences(of: "\n", with: $0), http.2) }
        let json = fixtures[0]
        let escapes = ["\r\n", "\r", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}", "\n"].map { (json.0, "\"escaped " + "\\" + $0 + "\": false cafétrue étrue a\u{200d}false", json.2) }
        for (language, source, patterns) in fixtures + lineEndings + escapes {
            let chunk = try #require(domainSyntaxHighlight(source: source, language: language).first)
            let native = try patterns.flatMap { pattern, kind in
                try NSRegularExpression(pattern: pattern).matches(in: source, range: NSRange(location: 0, length: (source as NSString).length)).map {
                    SyntaxToken(range: ResponseTextRange(location: UInt64($0.range.location), length: UInt64($0.range.length)), kind: kind)
                }
            }
            #expect(chunk.tokens == native)
        }
    }

    @Test func nativeLineRangesIndentAndChunkBoundaries() throws {
        for source in ["", "🚀\r\n \tx\r  y\u{85}\t z\u{2028} end\u{2029}last", String(repeating: "  {\"🚀\": true}\r\n", count: 4_000)] {
            let native = source as NSString
            let chunks = domainSyntaxHighlight(source: source, language: .json)
            var offset = 0
            for chunk in chunks {
                let range = source.utf8.count > 16 * 1024
                    ? native.lineRange(for: NSRange(location: offset, length: min(16 * 1024, native.length - offset)))
                    : NSRange(location: 0, length: native.length)
                #expect(chunk.range.nsRange == range)
                let part = native.substring(with: range) as NSString
                var cursor = 0
                var indents: [SyntaxIndent] = []
                while cursor < part.length {
                    let line = part.lineRange(for: NSRange(location: cursor, length: 0))
                    var index = cursor
                    var columns: UInt64 = 0
                    while index < NSMaxRange(line) {
                        if part.character(at: index) == 32 { columns += 1 }
                        else if part.character(at: index) == 9 { columns += 4 - columns % 4 }
                        else { break }
                        index += 1
                    }
                    if columns > 0 { indents.append(SyntaxIndent(range: ResponseTextRange(location: UInt64(line.location), length: UInt64(line.length)), columns: columns)) }
                    cursor = NSMaxRange(line)
                }
                #expect(chunk.indents == indents)
                offset = NSMaxRange(range)
            }
            #expect(offset == native.length)
        }
    }
}
