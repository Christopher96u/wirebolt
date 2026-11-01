import Foundation
import CFNetwork
import Network
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


extension OAuthFormTests {
    @Test("OAuth system proxies preserve destination routing, bypass and endpoint encoding")
    func systemProxyEndpoints() throws {
        let target = try #require(URL(string: "https://example.com/token"))
        let http = [kCFProxyTypeKey as String: kCFProxyTypeHTTP as String,
            kCFProxyHostNameKey as String: "proxy.example", kCFProxyPortNumberKey as String: 8080] as [String: Any]
        let https = [kCFProxyTypeKey as String: kCFProxyTypeHTTPS as String,
            kCFProxyHostNameKey as String: "proxy.example", kCFProxyPortNumberKey as String: 8443] as [String: Any]
        let socks = [kCFProxyTypeKey as String: kCFProxyTypeSOCKS as String,
            kCFProxyHostNameKey as String: "::1", kCFProxyPortNumberKey as String: 1080] as [String: Any]
        let direct = [kCFProxyTypeKey as String: kCFProxyTypeNone as String]
        #expect(OAuth2Service.proxyRoutes([http], target: target) == [.proxy(endpoint: "http://proxy.example:8080")])
        #expect(OAuth2Service.proxyRoutes([https], target: target) == [.proxy(endpoint: "http://proxy.example:8443")])
        #expect(OAuth2Service.proxyRoutes([socks], target: target) == [.proxy(endpoint: "socks5h://[::1]:1080")])
        #expect(OAuth2Service.proxyRoutes([direct, http], target: target) == [.direct, .proxy(endpoint: "http://proxy.example:8080")])
    }

    @Test("OAuth system PAC scripts resolve proxies and direct destinations")
    func systemProxyScripts() throws {
        let script = "function FindProxyForURL(url, host) { return host == 'direct.example' ? 'DIRECT' : 'PROXY pac.example:3128; PROXY backup.example:3129; DIRECT'; }"
        let configuration = [kCFProxyTypeKey as String: kCFProxyTypeAutoConfigurationJavaScript as String,
            kCFProxyAutoConfigurationJavaScriptKey as String: script]
        let proxied = try #require(URL(string: "http://localhost/token?route=proxy"))
        let direct = try #require(URL(string: "https://direct.example/token"))
        #expect(OAuth2Service.proxyRoutes([configuration], target: proxied) == [.proxy(endpoint: "http://pac.example:3128"), .proxy(endpoint: "http://backup.example:3129"), .direct])
        #expect(OAuth2Service.proxyRoutes([configuration], target: direct) == [.direct])
    }
}

extension OAuthFormTests {
    @Test("OAuth resolves system PAC URLs on the calling worker run loop")
    func systemProxyConfigurationURL() throws {
        let queue = DispatchQueue(label: "wirebolt.oauth-proxy-test")
        let ready = DispatchSemaphore(value: 0)
        let listener = try NWListener(using: .tcp, on: .any)
        listener.stateUpdateHandler = { state in
            if case .ready = state { ready.signal() }
            if case .failed = state { ready.signal() }
        }
        listener.newConnectionHandler = { connection in
            connection.start(queue: queue)
            connection.receive(minimumIncompleteLength: 1, maximumLength: 4096) { _, _, _, _ in
                let script = "function FindProxyForURL(url, host) { return 'PROXY url-proxy.example:3128'; }"
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/x-ns-proxy-autoconfig\r\nContent-Length: \(script.utf8.count)\r\nConnection: close\r\n\r\n\(script)"
                connection.send(content: Data(response.utf8), completion: .contentProcessed { _ in connection.cancel() })
            }
        }
        listener.start(queue: queue)
        defer { listener.cancel() }
        #expect(ready.wait(timeout: .now() + 5) == .success)
        let port = try #require(listener.port)
        let file = try #require(URL(string: "http://127.0.0.1:\(port.rawValue)/proxy.pac"))
        let configuration = [kCFProxyTypeKey as String: kCFProxyTypeAutoConfigurationURL as String,
            kCFProxyAutoConfigurationURLKey as String: file] as [String: Any]
        let target = try #require(URL(string: "https://example.com/token"))
        #expect(OAuth2Service.proxyRoutes([configuration], target: target) == [.proxy(endpoint: "http://url-proxy.example:3128")])
        let unsupported = [kCFProxyTypeKey as String: kCFProxyTypeAutoConfigurationURL as String,
            kCFProxyAutoConfigurationURLKey as String: URL(fileURLWithPath: "/missing.pac")] as [String: Any]
        #expect(OAuth2Service.proxyRoutes([unsupported], target: target).isEmpty)
    }
}
