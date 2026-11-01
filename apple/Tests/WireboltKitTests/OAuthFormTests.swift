import Foundation
import Darwin
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
        let cases: [(DomainNetworkFailure, URLError.Code)] = [(.timeout, .timedOut), (.hostNotFound, .cannotFindHost), (.cannotConnect, .cannotConnectToHost), (.offline, .notConnectedToInternet), (.connectionLost, .networkConnectionLost), (.certificate, .serverCertificateUntrusted), (.tls, .secureConnectionFailed), (.decode, .cannotDecodeContentData), (.cancelled, .cancelled), (.tooManyRedirects, .httpTooManyRedirects), (.unsupportedUrl, .unsupportedURL), (.noPermissionsToReadFile, .noPermissionsToReadFile), (.cannotParseResponse, .cannotParseResponse), (.certificateExpired, .serverCertificateHasBadDate), (.certificateNotYetValid, .serverCertificateNotYetValid), (.unknown, .unknown)]
        for (kind, code) in cases {
            let error = OAuth2ServiceError(domain: .TransportFailure(kind: kind, host: nil))
            let key = "Err\(code == .unknown ? -998 : code.rawValue)"
            #expect(error.errorDescription == Bundle(identifier: "com.apple.CFNetwork")?.localizedString(forKey: key, value: nil, table: nil))
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

extension OAuthFormTests {
    nonisolated private static func waitForReady(_ ready: DispatchSemaphore) -> Bool {
        ready.wait(timeout: .now() + 5) == .success
    }
    @Test("OAuth headers match native URLSession defaults")
    @MainActor
    func nativeRequestHeaders() async throws {
        let queue = DispatchQueue(label: "wirebolt.oauth-headers-test")
        let ready = DispatchSemaphore(value: 0)
        let listener = try NWListener(using: .tcp, on: .any)
        listener.stateUpdateHandler = { state in
            if case .ready = state { ready.signal() }
            if case .failed = state { ready.signal() }
        }
        listener.newConnectionHandler = { connection in
            connection.start(queue: queue)
            connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) { data, _, _, _ in
                let headers = data ?? Data()
                let head = "HTTP/1.1 200 OK\r\nContent-Length: \(headers.count)\r\nConnection: close\r\n\r\n"
                connection.send(content: Data(head.utf8) + headers, completion: .contentProcessed { _ in connection.cancel() })
            }
        }
        listener.start(queue: queue)
        defer { listener.cancel() }
        #expect(await Task.detached { Self.waitForReady(ready) }.value)
        let port = try #require(listener.port)
        let url = try #require(URL(string: "http://127.0.0.1:\(port.rawValue)/token"))
        let configuration = URLSessionConfiguration.ephemeral
        configuration.waitsForConnectivity = false
        configuration.timeoutIntervalForRequest = 30
        let (data, _) = try await URLSession(configuration: configuration).data(from: url)
        let lines = String(decoding: data, as: UTF8.self).components(separatedBy: "\r\n")
        let headers = await OAuth2Service.nativeHeaders()
        #expect(lines.contains("User-Agent: \(headers.userAgent)"), "Expected \(headers.userAgent); native headers: \(lines)")
        #expect(lines.contains("Accept-Language: \(headers.acceptLanguage)"))

    }
}

extension OAuthFormTests {
    @Test("OAuth certificate diagnostics match URLSession, including the hostname and expiration")
    @MainActor
    func nativeCertificateDiagnostics() async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        try Self.openssl(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", directory.appending(path: "key.pem").path,
            "-out", directory.appending(path: "cert.pem").path, "-days", "1", "-subj", "/CN=localhost"])
        try Data().write(to: directory.appending(path: "index"))
        try "01".write(to: directory.appending(path: "serial"), atomically: true, encoding: .utf8)
        let authority = """
        [ca]
        default_ca=fixture
        [fixture]
        database=\(directory.path)/index
        serial=\(directory.path)/serial
        new_certs_dir=\(directory.path)
        certificate=\(directory.path)/cert.pem
        private_key=\(directory.path)/key.pem
        default_md=sha256
        policy=subject
        [subject]
        commonName=supplied
        """
        try authority.write(to: directory.appending(path: "ca.cnf"), atomically: true, encoding: .utf8)
        try Self.openssl(["req", "-new", "-key", directory.appending(path: "key.pem").path,
            "-out", directory.appending(path: "request.csr").path, "-subj", "/CN=localhost"])
        try Self.openssl(["ca", "-selfsign", "-batch", "-config", directory.appending(path: "ca.cnf").path,
            "-in", directory.appending(path: "request.csr").path, "-out", directory.appending(path: "expired.pem").path,
            "-startdate", "20000101000000Z", "-enddate", "20010101000000Z"])
        for certificate in ["cert.pem", "expired.pem"] {
            let port = try await Task.detached { try Self.availablePort() }.value
            let server = Process()
            server.executableURL = URL(fileURLWithPath: "/usr/bin/openssl")
            server.arguments = ["s_server", "-quiet", "-accept", "\(port)", "-cert", directory.appending(path: certificate).path,
                "-key", directory.appending(path: "key.pem").path, "-www"]
            server.standardOutput = FileHandle.nullDevice
            server.standardError = FileHandle.nullDevice
            try server.run()
            defer { server.terminate() }
            let url = try #require(URL(string: "https://localhost:\(port)/token"))
            let configuration = URLSessionConfiguration.ephemeral
            configuration.waitsForConnectivity = false
            configuration.timeoutIntervalForRequest = 1
            let native = URLSession(configuration: configuration)
            defer { native.invalidateAndCancel() }
            var nativeError: NSError?
            for _ in 0..<100 {
                do { _ = try await native.data(from: url) } catch {
                    let failure = error as NSError
                    if failure.code != URLError.cannotConnectToHost.rawValue { nativeError = failure; break }
                }
                try await Task.sleep(for: .milliseconds(5))
            }
            let expected = try #require(nativeError)
            let oauth = DomainOAuthConfiguration(grant: .authorizationCodePkce, authorizationUrl: "https://localhost/authorize",
                tokenUrl: url.absoluteString, clientId: "fixture", clientSecretReference: "", scopes: "", audience: "",
                redirectUri: "wirebolt://oauth/callback", accessTokenReference: "fixture.token")
            do {
                _ = try await domainAcquireToken(configuration: oauth, platform: CertificateOAuthPlatform())
                Issue.record("The untrusted TLS server must be rejected")
            } catch let failure as DomainOAuthError {
                #expect(OAuth2ServiceError(domain: failure).errorDescription == expected.localizedDescription)
            }
        }
    }
    nonisolated private static func openssl(_ arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/openssl")
        process.arguments = arguments
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
    }
    nonisolated private static func availablePort() throws -> UInt16 {
        let descriptor = socket(AF_INET, SOCK_STREAM, 0)
        try #require(descriptor >= 0)
        defer { close(descriptor) }
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_addr = in_addr(s_addr: inet_addr("127.0.0.1"))
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { Darwin.bind(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        try #require(bound == 0)
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        let status = withUnsafeMutablePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(descriptor, $0, &length) }
        }
        try #require(status == 0)
        return UInt16(bigEndian: address.sin_port)
    }
}
private final class CertificateOAuthPlatform: OAuthPlatform {
    func systemProxies(url: String) -> [DomainOAuthProxy] { [.direct] }
    func requestHeaders(completion: OAuthHeadersReply) {
        Task { completion.complete(headers: await OAuth2Service.nativeHeaders()) }
    }
    func authorize(url: String, callbackScheme: String, completion: OAuthAuthorization) {
        let state = URLComponents(string: url)?.queryItems?.first(where: { $0.name == "state" })?.value ?? ""
        completion.complete(callback: "wirebolt://oauth/callback?state=\(state)&code=fixture", cancelled: false)
    }
}
