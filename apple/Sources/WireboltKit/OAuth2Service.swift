import AppKit
import AuthenticationServices
import Foundation
import Security

public struct OAuth2TokenReceipt: Equatable, Sendable {
    public let expiresAt: Date?
    public let scope: String?
}
public struct OAuth2ServiceError: LocalizedError, Equatable, Sendable {
    let domain: DomainOAuthError
    public var errorDescription: String? {
        guard case let .TransportFailure(kind) = domain else { return domainOauthErrorMessage(error: domain) }
        let code: URLError.Code = switch kind {
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
            return (receipt.token, OAuth2TokenReceipt(expiresAt: receipt.expiresAt.map(Date.init(timeIntervalSince1970:)), scope: receipt.scope))
        } catch let error as DomainOAuthError { throw OAuth2ServiceError(domain: error) }
    }
    public func presentationAnchor(for _: ASWebAuthenticationSession) -> ASPresentationAnchor {
        NSApplication.shared.keyWindow ?? NSApplication.shared.windows.first ?? ASPresentationAnchor()
    }
    nonisolated public func authorize(url: String, callbackScheme: String, completion: OAuthAuthorization) {
        Task { @MainActor [weak self] in
            guard let self, let url = URL(string: url) else { completion.complete(callback: nil, cancelled: false); return }
            let session = ASWebAuthenticationSession(url: url, callbackURLScheme: callbackScheme) { [weak self] callback, error in
                self?.browserSession = nil
                let cancelled = (error as? ASWebAuthenticationSessionError)?.code == .canceledLogin
                completion.complete(callback: error == nil ? callback?.absoluteString : nil, cancelled: cancelled)
            }
            session.presentationContextProvider = self
            session.prefersEphemeralWebBrowserSession = true
            browserSession = session
            if !session.start() { browserSession = nil; completion.complete(callback: nil, cancelled: false) }
        }
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
