import AppKit
import AuthenticationServices
import Foundation
import CFNetwork
import Security

public struct OAuth2TokenReceipt: Equatable, Sendable {
    public let expiresAt: Date?
    public let scope: String?
}
public struct OAuth2ServiceError: LocalizedError, Equatable, Sendable {
    let domain: DomainOAuthError
    public var errorDescription: String? {
        guard case .TransportFailure(let kind) = domain else { return domainOauthErrorMessage(error: domain) }
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
            }
        return URLError(code).localizedDescription
    }
}
@MainActor
public protocol OAuth2Authorizing: AnyObject {
    func acquireToken(configuration: OAuth2Configuration) async throws -> (String, OAuth2TokenReceipt)
}

/// Apple adapters only: the browser presentation and Keychain implement Rust's foreign trait.
@MainActor
public final class OAuth2Service: NSObject, OAuth2Authorizing, OAuthPlatform, ASWebAuthenticationPresentationContextProviding {
    private var browserSession: ASWebAuthenticationSession?
    public func acquireToken(configuration: OAuth2Configuration) async throws -> (String, OAuth2TokenReceipt) {
        do {
            let receipt = try await domainAcquireToken(configuration: configuration.domain, platform: self)
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
    nonisolated public func readSecret(name: String) -> String? {
        guard !name.isEmpty else { return nil }
        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: "io.github.christopher96u.wirebolt",
            kSecAttrAccount: name,
            kSecReturnData: true,
            kSecMatchLimit: kSecMatchLimitOne,
        ]
        var value: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &value) == errSecSuccess, let data = value as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }
    nonisolated static func encodeForm(_ form: [String: String]) -> Data { Data(domainEncodeOauthForm(form: form).utf8) }
}
