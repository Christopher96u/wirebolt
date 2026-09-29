import Foundation
import Testing
@testable import WireboltKit

@Suite("JSON response presentation")
struct JSONResponseDocumentTests {
    @Test("Pretty JSON preserves exact keys, numeric tokens, and string escapes")
    func preservesTokens() async throws {
        let raw = #"{"z":9007199254740993,"z":1e+999,"nested":[true,false,null,{},[]],"s":"a\"b\\c\n\u6771京🚀"}"#
        let store = try await makeStore(Data(raw.utf8))
        let document = try #require(try await store.formattedJSON())
        let expected = #"""
        {
          "z": 9007199254740993,
          "z": 1e+999,
          "nested": [
            true,
            false,
            null,
            {},
            []
          ],
          "s": "a\"b\\c\n\u6771京🚀"
        }
        """#
        #expect(document.preview == expected)
        #expect(try String(contentsOf: document.url, encoding: .utf8) == expected)
        #expect(try await store.viewport(length: raw.utf8.count) == Data(raw.utf8))
        #expect(try await store.formattedJSON() === document)
    }

    @Test("Invalid JSON falls back without changing the response", arguments: [
        "", "{", "[1,]", #"{"a":}"#, #"{"a":1,}"#, "[01]", "[1.]", "[1e]",
        "[-]", "[+1]", "true false", #"["\x"]"#, #"["\uZZZZ"]"#, "[\"a\nb\"]", "[NaN]",
    ])
    func invalidJSON(raw: String) async throws {
        let store = try await makeStore(Data(raw.utf8))
        #expect(try await store.formattedJSON() == nil)
        #expect(try await store.viewport(length: raw.utf8.count) == Data(raw.utf8))
    }

    @Test("Scalars and already formatted JSON are supported", arguments: ["null", "-12.50e-9", "true", #""hello""#, "{}", "[]"])
    func scalars(raw: String) async throws {
        let store = try await makeStore(Data((" \n" + raw + "\r\t").utf8))
        #expect(try await store.formattedJSON()?.preview == raw)
    }

    @Test("Large strings and UTF-8 cross input chunk boundaries without data loss")
    func chunkBoundaries() async throws {
        let value = String(repeating: "a", count: 65_525) + "🚀東京" + String(repeating: "b", count: 70_000)
        let raw = "{\"value\":\"\(value)\",\"n\":-0.002E+04}"
        let store = try await makeStore(Data(raw.utf8))
        let document = try #require(try await store.formattedJSON())
        #expect(document.preview == "{\n  \"value\": \"\(value)\",\n  \"n\": -0.002E+04\n}")
    }

    @Test("Large formatted responses stay on disk with a bounded preview")
    func largeResponse() async throws {
        let raw = "[" + Array(repeating: #"{"id":9007199254740993,"ok":true}"#, count: 35_000).joined(separator: ",") + "]"
        let store = try await makeStore(Data(raw.utf8))
        let document = try #require(try await store.formattedJSON())
        #expect(document.byteCount > 1024 * 1024)
        #expect(document.preview.utf8.count <= ResponseBodyStore.viewportByteCount)
        let pretty = try String(contentsOf: document.url, encoding: .utf8)
        #expect(pretty.hasSuffix("\n  }\n]"))
        #expect(pretty.components(separatedBy: "9007199254740993").count - 1 == 35_000)
    }

    @Test("Malformed UTF-8 and pathological nesting retain the original body")
    func invalidEncodingAndDepth() async throws {
        for data in [Data([34, 0xC0, 0xAF, 34]), Data([34, 0xED, 0xA0, 0x80, 34]),
                     Data((String(repeating: "[", count: 258) + "0" + String(repeating: "]", count: 258)).utf8)] {
            let store = try await makeStore(data)
            #expect(try await store.formattedJSON() == nil)
            #expect(try await store.viewport(length: data.count) == data)
        }
    }

    @Test("Display decoding turns printable \\u escapes into characters and keeps unsafe ones escaped")
    func decodesUnicodeEscapes() async throws {
        let raw = #"{"name":"caf\u00e9","rocket":"\ud83d\ude80","quote":"\u0022\u005c","control":"\u0000\u202e","lone":"\ud800x","mixed":"\ud83d\n"}"#
        let store = try await makeStore(Data(raw.utf8))
        let decoded = try #require(try await store.formattedJSON(decodingUnicodeEscapes: true))
        #expect(decoded.containsUnicodeEscapes)
        #expect(decoded.preview == #"""
        {
          "name": "café",
          "rocket": "🚀",
          "quote": "\u0022\u005c",
          "control": "\u0000\u202e",
          "lone": "\ud800x",
          "mixed": "\ud83d\n"
        }
        """#)
        let exact = try #require(try await store.formattedJSON())
        #expect(exact.preview.contains(#""caf\u00e9""#))
        #expect(try await store.viewport(length: raw.utf8.count) == Data(raw.utf8))
    }

    @Test("Responses without escapes are formatted once for both presentations")
    func sharesPresentationWithoutEscapes() async throws {
        let store = try await makeStore(Data(#"{"a":"café"}"#.utf8))
        let decoded = try #require(try await store.formattedJSON(decodingUnicodeEscapes: true))
        #expect(!decoded.containsUnicodeEscapes)
        #expect(try await store.formattedJSON() === decoded)
    }

    private func makeStore(_ data: Data) async throws -> ResponseBodyStore {
        let store = try ResponseBodyStore(runID: RunID())
        try await store.append(data)
        try await store.finish()
        return store
    }
}
