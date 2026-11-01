import Foundation
import Observation

public enum ProxyScope: String, CaseIterable, Sendable {
    case app, workspace, request
    public var title: String {
        domainProxyScopeTitle(scope: domain)
    }
}

public struct EffectiveProxy: Codable, Equatable, Sendable {
    public enum Source: String, Codable, Sendable {
        case request, workspace, appDefault = "app_default", systemDefault = "system_default"
        public var title: String {
            domainProxyScopeTitle(scope: domain)
        }
    }
    public let configuration: ProxyDocument
    public let source: Source

    public static func resolve(request: ProxyDocument?, workspace: ProxyDocument?, app: ProxyDocument) -> Self {
        let result = domainResolveProxy(request: request?.domain, workspace: workspace?.domain, app: app.domain)
        return Self(configuration: ProxyDocument(domain: result.configuration), source: Source(domain: result.source))
    }

    public func summary(for url: String? = nil) -> String {
        domainProxySummary(configuration: configuration.domain, url: url)
    }
}

public extension ProxySelection {
    var document: ProxyDocument? {
        switch self { case .inherit: nil; case .direct: .direct; case .system: .system; case let .manual(value): value }
    }
    init(document: ProxyDocument?) {
        switch document { case nil: self = .inherit; case .direct: self = .direct; case .system: self = .system; case let .manual(routes): self = .manual(.manual(routes: routes)) }
    }
}

@MainActor @Observable
public final class ProxyPreferences {
    public private(set) var configuration: ProxyDocument
    @ObservationIgnored private let defaults: UserDefaults?
    public static let key = "network.appProxy"
    public init(defaults: UserDefaults? = nil) {
        self.defaults = defaults
        configuration = defaults?.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(ProxyDocument.self, from: $0) } ?? .system
    }
    public func save(_ configuration: ProxyDocument) throws {
        try configuration.validate()
        let data = try JSONEncoder().encode(configuration)
        defaults?.set(data, forKey: Self.key)
        self.configuration = configuration
    }
}

public struct ProxyValidationError: LocalizedError, Equatable {
    public let message: String
    public var errorDescription: String? { message }
    public init(_ message: String) { self.message = message }
}

public extension ProxyDocument {
    func validate() throws {
        do { try domainValidateProxy(configuration: domain) }
        catch let DomainError.Invalid(reason) { throw ProxyValidationError(reason) }
    }
}

public struct ProxyRouteDraft: Identifiable, Equatable {
    var domain: ProxyRouteInput
    public static let protocols = ["http", "https", "socks4", "socks4a", "socks5", "socks5h"]
    public var id: UUID { UUID(uuidString: domain.id)! }
    public var destination: String { get { domain.destination } set { domain.destination = newValue } }
    public var scheme: String { get { domain.scheme } set { domain.scheme = newValue } }
    public var host: String { get { domain.host } set { domain.host = newValue } }
    public var port: String { get { domain.port } set { domain.port = newValue } }
    public var authenticated: Bool { get { domain.authenticated } set { domain.authenticated = newValue } }
    public var replaceCredentials: Bool { get { domain.replaceCredentials } set { domain.replaceCredentials = newValue } }
    public var username: String { get { domain.username } set { domain.username = newValue } }
    public var password: String { get { domain.password } set { domain.password = newValue } }
    public var credentials: ProxyCredentialsDocument? {
        get { domain.credentials.map(ProxyCredentialsDocument.init(domain:)) }
        set { domain.credentials = newValue?.domain }
    }
    init(domain: ProxyRouteInput) { self.domain = domain }
    public init(route: ProxyRouteDocument? = nil) { domain = domainProxyRouteInput(route: route?.domain) }
    public var needsCredentials: Bool { domainProxyNeedsCredentials(input: domain) }
    public var credentialReferences: ProxyCredentialsDocument? {
        domainProxyCredentialReferences(input: domain).map(ProxyCredentialsDocument.init(domain:))
    }
    public func document() throws -> ProxyRouteDocument {
        do { return ProxyRouteDocument(domain: try domainProxyRouteDocument(input: domain)) }
        catch let DomainError.Invalid(reason) { throw ProxyValidationError(reason) }
    }
    public var secrets: [String: String] { domainProxyRouteSecrets(input: domain) }
}

public struct ProxyFormDraft: Equatable {
    public enum Mode: String, CaseIterable { case inherit, system, direct, manual }
    private var domain: ProxyFormInput
    public var mode: Mode { get { Mode(domain: domain.mode) } set { domain.mode = newValue.domain } }
    public var routes: [ProxyRouteDraft] {
        get { domain.routes.map { ProxyRouteDraft(domain: $0) } }
        set { domain.routes = newValue.map(\.domain) }
    }
    public init(configuration: ProxyDocument?) { domain = domainProxyForm(configuration: configuration?.domain) }
    public func document() throws -> ProxyDocument? {
        do { return try domainProxyFormDocument(input: domain).map(ProxyDocument.init(domain:)) }
        catch let DomainError.Invalid(reason) { throw ProxyValidationError(reason) }
    }
    public var secrets: [String: String] { domainProxyFormSecrets(input: domain) }
}


/// A TLS client identity in the PEM form the transport accepts: one or more certificates
/// (leaf first) and one unencrypted private key. The combined text is stored in Keychain;
/// workspaces keep only its reference.
public struct ClientIdentityPEM: Equatable, Sendable {
    public enum Failure: LocalizedError, Equatable {
        case noCertificate, noPrivateKey, multiplePrivateKeys, encryptedPrivateKey

        public var errorDescription: String? {
            domainPemErrorMessage(kind: domain)
        }
    }

    private let domain: DomainPemIdentity
    public var certificates: [String] { domain.certificates }
    public var privateKey: String { domain.privateKey }

    /// Certificates followed by the key, the order the transport expects.
    public var combined: String { domain.combined }

    /// Native error and value projections of Rust's PEM parser.
    public init(parsing sources: [String]) throws {
        do {
            domain = try domainParsePem(sources: sources)
        } catch let error as PemError {
            throw Failure(domain: error)
        }
    }
}

private extension ClientIdentityPEM.Failure {
    var domain: PemError {
        switch self {
        case .noCertificate: .NoCertificate
        case .noPrivateKey: .NoPrivateKey
        case .multiplePrivateKeys: .MultiplePrivateKeys
        case .encryptedPrivateKey: .EncryptedPrivateKey
        }
    }
    init(domain: PemError) {
        self = switch domain {
        case .NoCertificate: .noCertificate
        case .NoPrivateKey: .noPrivateKey
        case .MultiplePrivateKeys: .multiplePrivateKeys
        case .EncryptedPrivateKey: .encryptedPrivateKey
        }
    }
}

extension EffectiveProxy.Source {
    init(domain: DomainProxySource) {
        self = switch domain {
        case .request: .request
        case .workspace: .workspace
        case .appDefault: .appDefault
        case .systemDefault: .systemDefault
        }
    }
    var domain: DomainProxySource {
        switch self {
        case .request: .request
        case .workspace: .workspace
        case .appDefault: .appDefault
        case .systemDefault: .systemDefault
        }
    }
}

private extension ProxyScope {
    var domain: DomainProxySource {
        switch self {
        case .app: .appDefault
        case .workspace: .workspace
        case .request: .request
        }
    }
}

private extension ProxyFormDraft.Mode {
    init(domain: ProxyFormMode) {
        self = switch domain {
        case .inherit: .inherit
        case .system: .system
        case .direct: .direct
        case .manual: .manual
        }
    }
    var domain: ProxyFormMode {
        switch self {
        case .inherit: .inherit
        case .system: .system
        case .direct: .direct
        case .manual: .manual
        }
    }
}
