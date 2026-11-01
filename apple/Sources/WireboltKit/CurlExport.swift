import Foundation

/// Shell exports contain Keychain references, never credential material.
public struct CurlSecretReferences: Sendable {
    public static let keychainService = "io.github.christopher96u.wirebolt"

    public let variables: [String: ValueSource]
    public var hexEncoded: Set<String> = []

    public init(variables: [String: ValueSource], hexEncoded: Set<String> = []) {
        self.variables = variables
        self.hexEncoded = hexEncoded
    }
    public static func printsAsHex(_ value: String) -> Bool { domainPrintsAsHex(text: value) }
}

func shellQuote(_ value: String) -> String { domainShellQuote(text: value) }

extension RequestDraft {
    public var curlValueSources: [ValueSource] {
        domainCurlValueSources(draft: domain)
    }
    public func curlSecretNames(variables: [String: ValueSource]) -> [String] {
        domainCurlExport(draft: domain, variables: variables, hexEncodedSecrets: []).secretNames
    }
    public func curlCommand(secrets: CurlSecretReferences) -> String {
        domainCurlExport(draft: domain, variables: secrets.variables, hexEncodedSecrets: Array(secrets.hexEncoded)).command
    }
}

extension ProxyDocument {
    func curlRoute(for url: String) -> ProxyRouteDocument? {
        domainCurlProxyRoute(configuration: self, url: url)
    }
    func curlArguments(for url: String, secrets: CurlSecretReferences) -> String {
        domainCurlProxyArguments(configuration: self, url: url, hexEncodedSecrets: Array(secrets.hexEncoded))
    }
}
