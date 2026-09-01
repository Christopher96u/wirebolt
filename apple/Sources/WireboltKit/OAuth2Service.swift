import AppKit
import AuthenticationServices
import CryptoKit
import Foundation
import Security

public struct OAuth2TokenReceipt: Equatable, Sendable {
    public let expiresAt: Date?
    public let scope: String?
}

public enum OAuth2ServiceError: LocalizedError, Equatable, Sendable {
    case invalidConfiguration
    case authorizationCancelled
    case authorizationFailed
    case stateMismatch
    case missingAuthorizationCode
    case clientSecretUnavailable
    case tokenEndpointRejected(Int)
    case invalidTokenResponse

    public var errorDescription: String? {
        switch self {
        case .invalidConfiguration: "OAuth 2 configuration is incomplete."
        case .authorizationCancelled: "Authorization was cancelled."
        case .authorizationFailed: "The authorization server did not complete the sign-in."
        case .stateMismatch: "The OAuth 2 state did not match."
        case .missingAuthorizationCode: "The authorization response did not include a code."
        case .clientSecretUnavailable: "The client secret is unavailable in Keychain."
        case let .tokenEndpointRejected(status): "The token endpoint returned HTTP \(status)."
        case .invalidTokenResponse: "The token endpoint returned an invalid response."
        }
    }
}

@MainActor
public protocol OAuth2Authorizing: AnyObject {
    func acquireToken(configuration: OAuth2Configuration) async throws -> (String, OAuth2TokenReceipt)
}

@MainActor
public final class OAuth2Service: NSObject, OAuth2Authorizing, ASWebAuthenticationPresentationContextProviding {
    private var browserSession: ASWebAuthenticationSession?

    public func acquireToken(
        configuration: OAuth2Configuration
    ) async throws -> (String, OAuth2TokenReceipt) {
        guard let tokenURL = URL(string: configuration.tokenURL),
              tokenURL.scheme == "https" || Self.isLoopback(tokenURL),
              configuration.clientID.isEmpty == false,
              configuration.accessTokenReference.isEmpty == false
        else { throw OAuth2ServiceError.invalidConfiguration }

        var form: [String: String] = [
            "client_id": configuration.clientID,
        ]
        if configuration.scopes.isEmpty == false { form["scope"] = configuration.scopes }
        if configuration.audience.isEmpty == false { form["audience"] = configuration.audience }

        switch configuration.grant {
        case .clientCredentials:
            form["grant_type"] = "client_credentials"
            guard let secret = Self.keychainSecret(named: configuration.clientSecretReference) else {
                throw OAuth2ServiceError.clientSecretUnavailable
            }
            form["client_secret"] = secret
        case .authorizationCodePKCE:
            let verifier = Self.randomURLSafeValue(byteCount: 48)
            let state = Self.randomURLSafeValue(byteCount: 24)
            let challenge = Self.base64URL(Data(SHA256.hash(data: Data(verifier.utf8))))
            let code = try await authorizationCode(
                configuration: configuration,
                challenge: challenge,
                state: state
            )
            form["grant_type"] = "authorization_code"
            form["code"] = code
            form["code_verifier"] = verifier
            form["redirect_uri"] = configuration.redirectURI
        }

        return try await exchange(form: form, tokenURL: tokenURL)
    }

    public func presentationAnchor(for _: ASWebAuthenticationSession) -> ASPresentationAnchor {
        NSApplication.shared.keyWindow ?? NSApplication.shared.windows.first ?? ASPresentationAnchor()
    }

    private func authorizationCode(
        configuration: OAuth2Configuration,
        challenge: String,
        state: String
    ) async throws -> String {
        guard var components = URLComponents(string: configuration.authorizationURL),
              components.scheme == "https" || components.host == "localhost",
              let callbackScheme = URL(string: configuration.redirectURI)?.scheme
        else { throw OAuth2ServiceError.invalidConfiguration }
        var items = components.queryItems ?? []
        items += [
            URLQueryItem(name: "response_type", value: "code"),
            URLQueryItem(name: "client_id", value: configuration.clientID),
            URLQueryItem(name: "redirect_uri", value: configuration.redirectURI),
            URLQueryItem(name: "code_challenge", value: challenge),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
            URLQueryItem(name: "state", value: state),
        ]
        if configuration.scopes.isEmpty == false {
            items.append(URLQueryItem(name: "scope", value: configuration.scopes))
        }
        if configuration.audience.isEmpty == false {
            items.append(URLQueryItem(name: "audience", value: configuration.audience))
        }
        components.queryItems = items
        guard let url = components.url else { throw OAuth2ServiceError.invalidConfiguration }

        return try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(url: url, callbackURLScheme: callbackScheme) {
                [weak self] callback, error in
                self?.browserSession = nil
                if let authenticationError = error as? ASWebAuthenticationSessionError,
                   authenticationError.code == .canceledLogin
                {
                    continuation.resume(throwing: OAuth2ServiceError.authorizationCancelled)
                    return
                }
                guard error == nil, let callback,
                      let callbackComponents = URLComponents(url: callback, resolvingAgainstBaseURL: false)
                else {
                    continuation.resume(throwing: OAuth2ServiceError.authorizationFailed)
                    return
                }
                let values = Dictionary(
                    callbackComponents.queryItems?.compactMap { item in
                        item.value.map { (item.name, $0) }
                    } ?? [],
                    uniquingKeysWith: { first, _ in first }
                )
                guard values["state"] == state else {
                    continuation.resume(throwing: OAuth2ServiceError.stateMismatch)
                    return
                }
                guard let code = values["code"], code.isEmpty == false else {
                    continuation.resume(throwing: OAuth2ServiceError.missingAuthorizationCode)
                    return
                }
                continuation.resume(returning: code)
            }
            session.presentationContextProvider = self
            session.prefersEphemeralWebBrowserSession = true
            browserSession = session
            guard session.start() else {
                browserSession = nil
                continuation.resume(throwing: OAuth2ServiceError.authorizationFailed)
                return
            }
        }
    }

    private func exchange(
        form: [String: String],
        tokenURL: URL
    ) async throws -> (String, OAuth2TokenReceipt) {
        var request = URLRequest(url: tokenURL)
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        var components = URLComponents()
        components.queryItems = form.sorted(by: { $0.key < $1.key }).map {
            URLQueryItem(name: $0.key, value: $0.value)
        }
        request.httpBody = Data((components.percentEncodedQuery ?? "").utf8)
        let configuration = URLSessionConfiguration.ephemeral
        configuration.waitsForConnectivity = false
        configuration.timeoutIntervalForRequest = 30
        let (data, response) = try await URLSession(configuration: configuration).data(for: request)
        guard let response = response as? HTTPURLResponse else {
            throw OAuth2ServiceError.invalidTokenResponse
        }
        guard (200 ..< 300).contains(response.statusCode) else {
            throw OAuth2ServiceError.tokenEndpointRejected(response.statusCode)
        }
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let token = object["access_token"] as? String,
              token.isEmpty == false
        else { throw OAuth2ServiceError.invalidTokenResponse }
        let seconds = (object["expires_in"] as? NSNumber)?.doubleValue
        let receipt = OAuth2TokenReceipt(
            expiresAt: seconds.map { Date().addingTimeInterval($0) },
            scope: object["scope"] as? String
        )
        return (token, receipt)
    }

    private static func keychainSecret(named name: String) -> String? {
        guard name.isEmpty == false else { return nil }
        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: "local.wirebolt.app",
            kSecAttrAccount: name,
            kSecReturnData: true,
            kSecMatchLimit: kSecMatchLimitOne,
        ]
        var value: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &value) == errSecSuccess,
              let data = value as? Data
        else { return nil }
        return String(data: data, encoding: .utf8)
    }

    private static func randomURLSafeValue(byteCount: Int) -> String {
        var bytes = [UInt8](repeating: 0, count: byteCount)
        guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
            return UUID().uuidString.replacingOccurrences(of: "-", with: "")
        }
        return base64URL(Data(bytes))
    }

    private static func base64URL(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    private static func isLoopback(_ url: URL) -> Bool {
        ["localhost", "127.0.0.1", "::1"].contains(url.host?.lowercased() ?? "")
    }
}
