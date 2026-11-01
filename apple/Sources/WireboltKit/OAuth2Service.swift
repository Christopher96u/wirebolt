import AppKit
import AuthenticationServices
import Foundation
import CFNetwork
import Darwin
import Synchronization

public struct OAuth2TokenReceipt: Equatable, Sendable {
    public let expiresAt: Date?
    public let scope: String?
}
public struct OAuth2ServiceError: LocalizedError, Equatable, Sendable {
    let domain: DomainOAuthError
    public var errorDescription: String? {
        guard case .TransportFailure(let kind, let host) = domain else { return domainOauthErrorMessage(error: domain) }
        let code: URLError.Code =
            switch kind {
            case .timeout: .timedOut
            case .hostNotFound: .cannotFindHost
            case .cannotConnect: .cannotConnectToHost
            case .offline: .notConnectedToInternet
            case .connectionLost: .networkConnectionLost
            case .certificate: .serverCertificateUntrusted
            case .tls: .secureConnectionFailed
            case .decode: .cannotDecodeContentData
            case .cancelled: .cancelled
            case .tooManyRedirects: .httpTooManyRedirects
            case .unsupportedUrl: .unsupportedURL
            case .noPermissionsToReadFile: .noPermissionsToReadFile
            case .cannotParseResponse: .cannotParseResponse
            case .certificateExpired: .serverCertificateHasBadDate
            case .certificateNotYetValid: .serverCertificateNotYetValid
            case .unknown: .unknown
            }
        let bundle = Bundle(identifier: "com.apple.CFNetwork")
        let key = "Err\(code == .unknown ? -998 : code.rawValue)"
        if let host, let warning = bundle?.localizedString(forKey: key + ".w", value: "", table: nil), !warning.isEmpty {
            return String(format: warning, host)
        }
        let description = bundle?.localizedString(forKey: key, value: "", table: nil) ?? ""
        return description.isEmpty ? URLError(code).localizedDescription : description
    }
}

@MainActor
public protocol OAuth2Authorizing: AnyObject {
    func acquireToken(configuration: OAuth2Configuration) async throws -> (String, OAuth2TokenReceipt)
}

/// Native browser presentation, proxy routing, HTTP defaults and localized diagnostics.
@MainActor
public final class OAuth2Service: NSObject, OAuth2Authorizing, OAuthPlatform, ASWebAuthenticationPresentationContextProviding {
    private var browserSession: ASWebAuthenticationSession?
    public func acquireToken(configuration: OAuth2Configuration) async throws -> (String, OAuth2TokenReceipt) {
        do {
            let receipt = try await domainAcquireToken(configuration: configuration, platform: self)
            return (
                receipt.token, OAuth2TokenReceipt(expiresAt: receipt.expiresAt.map(Date.init(timeIntervalSince1970:)), scope: receipt.scope)
            )
        } catch let error as DomainOAuthError { throw OAuth2ServiceError(domain: error) }
    }

    public func presentationAnchor(for _: ASWebAuthenticationSession) -> ASPresentationAnchor {
        NSApplication.shared.keyWindow ?? NSApplication.shared.windows.first ?? ASPresentationAnchor()
    }
    nonisolated public func authorize(url: String, callbackScheme: String, completion: OAuthAuthorization) {
        Task { @MainActor [weak self] in
            guard let self, let url = URL(string: url) else {
                completion.complete(callback: nil, cancelled: false)
                return
            }
            let session = ASWebAuthenticationSession(url: url, callbackURLScheme: callbackScheme) { [weak self] callback, error in
                self?.browserSession = nil
                let cancelled = (error as? ASWebAuthenticationSessionError)?.code == .canceledLogin
                completion.complete(callback: error == nil ? callback?.absoluteString : nil, cancelled: cancelled)
            }
            session.presentationContextProvider = self
            session.prefersEphemeralWebBrowserSession = true
            browserSession = session
            if !session.start() {
                browserSession = nil
                completion.complete(callback: nil, cancelled: false)
            }
        }
    }
    nonisolated public func systemProxies(url: String) -> [DomainOAuthProxy] {
        guard let target = URL(string: url), let settings = CFNetworkCopySystemProxySettings()?.takeRetainedValue() else { return [.direct] }
        let proxies = CFNetworkCopyProxiesForURL(target as CFURL, settings).takeRetainedValue() as NSArray
        return Self.proxyRoutes(proxies, target: target)
    }
    nonisolated static func proxyRoutes(_ proxies: NSArray, target: URL) -> [DomainOAuthProxy] {
        var routes: [DomainOAuthProxy] = []
        for case let proxy as [String: Any] in proxies {
            guard let type = proxy[kCFProxyTypeKey as String] as? String else { continue }
            if type == kCFProxyTypeNone as String { routes.append(.direct); continue }
            if type == kCFProxyTypeAutoConfigurationJavaScript as String,
                let script = proxy[kCFProxyAutoConfigurationJavaScriptKey as String] as? String,
                let resolved = CFNetworkCopyProxiesForAutoConfigurationScript(script as CFString, target as CFURL, nil)?.takeRetainedValue()
            {
                routes.append(contentsOf: proxyRoutes(resolved as NSArray, target: target))
                continue
            }
            if type == kCFProxyTypeAutoConfigurationURL as String,
                let url = proxy[kCFProxyAutoConfigurationURLKey as String] as? URL,
                let resolved = resolveProxyConfiguration(url, target: target)
            {
                routes.append(contentsOf: proxyRoutes(resolved, target: target))
                continue
            }
            let scheme: String
            if type == kCFProxyTypeHTTP as String { scheme = "http" }
            else if type == kCFProxyTypeHTTPS as String { scheme = "http" }
            else if type == kCFProxyTypeSOCKS as String { scheme = "socks5h" }
            else { continue }
            guard let host = proxy[kCFProxyHostNameKey as String] as? String else { continue }
            var endpoint = URLComponents()
            endpoint.scheme = scheme
            endpoint.host = host.contains(":") && !host.hasPrefix("[") ? "[\(host)]" : host
            endpoint.port = proxy[kCFProxyPortNumberKey as String] as? Int
            endpoint.user = proxy[kCFProxyUsernameKey as String] as? String
            endpoint.password = proxy[kCFProxyPasswordKey as String] as? String
            if let endpoint = endpoint.string { routes.append(.proxy(endpoint: endpoint)) }
        }
        return routes
    }
    private final class ProxyConfigurationResult {
        var proxies: NSArray?
        var completed = false
    }
    nonisolated private static func resolveProxyConfiguration(_ url: URL, target: URL) -> NSArray? {
        let result = ProxyConfigurationResult()
        var context = CFStreamClientContext(version: 0, info: Unmanaged.passUnretained(result).toOpaque(), retain: nil, release: nil, copyDescription: nil)
        let source = CFNetworkExecuteProxyAutoConfigurationURL(url as CFURL, target as CFURL, { context, proxies, error in
            let result = Unmanaged<ProxyConfigurationResult>.fromOpaque(context).takeUnretainedValue()
            if error == nil { result.proxies = proxies as NSArray }
            result.completed = true
        }, &context)
        let runLoop = CFRunLoopGetCurrent()
        CFRunLoopAddSource(runLoop, source, .defaultMode)
        defer { CFRunLoopSourceInvalidate(source) }
        let deadline = Date().addingTimeInterval(30)
        while !result.completed && deadline.timeIntervalSinceNow > 0 {
            CFRunLoopRunInMode(.defaultMode, deadline.timeIntervalSinceNow, true)
        }
        return result.proxies
    }
    nonisolated public func requestHeaders(completion: OAuthHeadersReply) {
        Task { completion.complete(headers: await Self.nativeHeaders()) }
    }
    nonisolated static func nativeHeaders() async -> DomainOAuthHeaders {
        let bundle = Bundle.main
        let name = bundle.object(forInfoDictionaryKey: "CFBundleName") as? String ?? ProcessInfo.processInfo.processName
        let version = (bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String).map { "/" + $0 } ?? " (unknown version)"
        let network = Bundle(identifier: "com.apple.CFNetwork")?.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "0"
        var system = utsname()
        uname(&system)
        let release = withUnsafeBytes(of: &system.release) { bytes in
            String(decoding: bytes.prefix(while: { $0 != 0 }), as: UTF8.self)
        }
        let capture = OAuthHeaderCapture()
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        configuration.waitsForConnectivity = false
        configuration.timeoutIntervalForRequest = 1
        let session = URLSession(configuration: configuration, delegate: capture, delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        // Port zero cannot reach a service; task metrics expose Foundation's generated headers.
        if let url = URL(string: "http://[::1]:0") { _ = try? await session.data(from: url) }
        return DomainOAuthHeaders(userAgent: "\(name)\(version) CFNetwork/\(network) Darwin/\(release)",
            acceptLanguage: capture.acceptLanguage.withLock { $0 })
    }
    nonisolated static func encodeForm(_ form: [String: String]) -> Data { Data(domainEncodeOauthForm(form: form).utf8) }
}

private final class OAuthHeaderCapture: NSObject, URLSessionTaskDelegate, Sendable {
    let acceptLanguage = Mutex("")
    func urlSession(_ session: URLSession, task: URLSessionTask, didFinishCollecting metrics: URLSessionTaskMetrics) {
        if let value = metrics.transactionMetrics.last?.request.value(forHTTPHeaderField: "Accept-Language") {
            acceptLanguage.withLock { $0 = value }
        }
    }
}
