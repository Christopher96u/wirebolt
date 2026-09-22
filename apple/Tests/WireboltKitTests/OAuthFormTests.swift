import Foundation
import Testing
@testable import WireboltKit

struct OAuthFormTests {
    @Test func formRoundTripsSpecialCharacters() throws {
        let original = ["client+id": "qa+client & café=東京%", "client_secret": "a+b c&d=e"]
        let body = String(decoding: OAuth2Service.encodeForm(original), as: UTF8.self)
        var decoded: [String: String] = [:]
        for pair in body.split(separator: "&") {
            let parts = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            let key = try #require(String(parts[0]).replacingOccurrences(of: "+", with: " ").removingPercentEncoding)
            let value = try #require(String(parts[1]).replacingOccurrences(of: "+", with: " ").removingPercentEncoding)
            decoded[key] = value
        }
        #expect(decoded == original)
    }
}
