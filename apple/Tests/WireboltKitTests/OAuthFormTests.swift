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

extension OAuthFormTests {
    @Test("Rust transport failures retain native localized network diagnostics")
    func nativeNetworkMessages() {
        let cases: [(DomainNetworkFailure, URLError.Code)] = [(.timeout, .timedOut), (.hostNotFound, .cannotFindHost), (.cannotConnect, .cannotConnectToHost), (.offline, .notConnectedToInternet), (.connectionLost, .networkConnectionLost), (.certificate, .serverCertificateUntrusted), (.tls, .secureConnectionFailed), (.decode, .cannotDecodeContentData)]
        for (kind, code) in cases {
            let error = OAuth2ServiceError(domain: .TransportFailure(kind: kind))
            #expect(error.errorDescription == URLError(code).localizedDescription)
        }
    }
}
