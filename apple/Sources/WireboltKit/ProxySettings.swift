import Foundation
import Observation

public typealias ProxyScope = ProxySource

extension ProxySource: RawRepresentable, CaseIterable {
    public static var app: Self { .appDefault }
    public static var allCases: [Self] { [.app, .workspace, .request, .systemDefault] }
    public static var editableScopes: [Self] { [.app, .workspace, .request] }
    public var rawValue: String {
        switch self { case .appDefault: "app"; case .workspace: "workspace"; case .request: "request"; case .systemDefault: "systemDefault" }
    }
    public init?(rawValue: String) {
        guard let value = Self.allCases.first(where: { $0.rawValue == rawValue }) else { return nil }
        self = value
    }
    public var title: String {
        domainProxyScopeTitle(scope: self)
    }
}

public typealias EffectiveProxy = ResolvedDomainProxy

extension EffectiveProxy {
    public static func resolve(request: ProxyDocument?, workspace: ProxyDocument?, app: ProxyDocument) -> Self {
        domainResolveProxy(request: request, workspace: workspace, app: app)
    }

    public func summary(for url: String? = nil) -> String {
        domainProxySummary(configuration: configuration, url: url)
    }
}

extension ProxySelection {
    public var document: ProxyDocument? {
        switch self {
        case .inherit: nil
        case .direct: .direct
        case .system: .system
        case .manual(let value): value
        }
    }
    public init(document: ProxyDocument?) {
        switch document {
        case nil: self = .inherit
        case .direct: self = .direct
        case .system: self = .system
        case .manual(let routes): self = .manual(.manual(routes: routes))
        }
    }
}

@MainActor @Observable
public final class ProxyPreferences {
    public private(set) var configuration: ProxyDocument
    @ObservationIgnored private let defaults: UserDefaults?
    public static let key = "network.appProxy"
    public init(defaults: UserDefaults? = nil) {
        self.defaults = defaults
        configuration =
            defaults?.data(forKey: Self.key).flatMap { try? sessionDecodeProxy(data: $0) } ?? .system
    }
    public func save(_ configuration: ProxyDocument) throws {
        try configuration.validate()
        let data = try sessionEncodeProxy(value: configuration)
        defaults?.set(data, forKey: Self.key)
        self.configuration = configuration
    }
}

public struct ProxyValidationError: LocalizedError, Equatable {
    public let message: String
    public var errorDescription: String? { message }
    public init(_ message: String) { self.message = message }
}

extension ProxyDocument {
    public func validate() throws {
        do { try domainValidateProxy(configuration: self) } catch let DomainError.Invalid(reason) { throw ProxyValidationError(reason) }
    }
}

public struct ProxyRouteDraft: Identifiable, Equatable {
    var domain: ProxyRouteInput
    public static let protocols = ["http", "https", "socks4", "socks4a", "socks5", "socks5h"]
    public let id: UUID
    public var destination: ProxyDestination? {
        get { domain.destination }
        set { domain.destination = newValue }
    }
    public var scheme: String {
        get { domain.scheme }
        set { domain.scheme = newValue }
    }
    public var host: String {
        get { domain.host }
        set { domain.host = newValue }
    }
    public var port: String {
        get { domain.port }
        set { domain.port = newValue }
    }
    public var authenticated: Bool {
        get { domain.authenticated }
        set { domain.authenticated = newValue }
    }
    public var replaceCredentials: Bool {
        get { domain.replaceCredentials }
        set { domain.replaceCredentials = newValue }
    }
    public var username: String {
        get { domain.username }
        set { domain.username = newValue }
    }
    public var password: String {
        get { domain.password }
        set { domain.password = newValue }
    }
    public var credentials: ProxyCredentialsDocument? {
        get { domain.credentials }
        set { domain.credentials = newValue }
    }
    init(domain: ProxyRouteInput) {
        var domain = domain
        let parsedID = UUID(uuidString: domain.id)
        id = parsedID ?? UUID()
        if parsedID == nil { domain.id = id.uuidString.lowercased() }
        self.domain = domain
    }
    public init(route: ProxyRouteDocument? = nil) { self.init(domain: domainProxyRouteInput(route: route)) }
    public var needsCredentials: Bool { domainProxyNeedsCredentials(input: domain) }
    public var credentialReferences: ProxyCredentialsDocument? {
        domainProxyCredentialReferences(input: domain)
    }
    public func document() throws -> ProxyRouteDocument {
        do { return try domainProxyRouteDocument(input: domain) } catch let DomainError.Invalid(reason) {
            throw ProxyValidationError(reason)
        }
    }
    public var secrets: [String: String] { domainProxyRouteSecrets(input: domain) }
}

public struct ProxyFormDraft: Equatable {
    public typealias Mode = ProxyFormMode
    private var domain: ProxyFormInput
    public var mode: Mode {
        get { domain.mode }
        set { domain.mode = newValue }
    }
    public var routes: [ProxyRouteDraft] {
        get { domain.routes.map { ProxyRouteDraft(domain: $0) } }
        set { domain.routes = newValue.map(\.domain) }
    }
    public init(configuration: ProxyDocument?) { domain = domainProxyForm(configuration: configuration) }
    public func document() throws -> ProxyDocument? {
        do { return try domainProxyFormDocument(input: domain) } catch let DomainError.Invalid(reason) {
            throw ProxyValidationError(reason)
        }
    }
    public var secrets: [String: String] { domainProxyFormSecrets(input: domain) }
}


/// A TLS client identity in the PEM form the transport accepts: one or more certificates
/// (leaf first) and one unencrypted private key. The combined text is stored in Keychain;
/// workspaces keep only its reference.
public struct ClientIdentityPEM: Equatable, Sendable {
    public typealias Failure = PemError

    private let domain: DomainPemIdentity
    public var certificates: [String] { domain.certificates }
    public var privateKey: String { domain.privateKey }

    /// Certificates followed by the key, the order the transport expects.
    public var combined: String { domain.combined }

    /// Native error and value projections of Rust's PEM parser.
    public init(parsing sources: [String]) throws {
        domain = try domainParsePem(sources: sources)
    }
}

extension PemError: CustomDebugStringConvertible {
    public static var noCertificate: Self { .NoCertificate }
    public static var noPrivateKey: Self { .NoPrivateKey }
    public static var multiplePrivateKeys: Self { .MultiplePrivateKeys }
    public static var encryptedPrivateKey: Self { .EncryptedPrivateKey }
    public var debugDescription: String { domainPemErrorMessage(kind: self) }
}

extension ProxyFormMode: RawRepresentable, CaseIterable {
    public static var allCases: [Self] { [.inherit, .system, .direct, .manual] }
    public var rawValue: String {
        switch self { case .inherit: "inherit"; case .system: "system"; case .direct: "direct"; case .manual: "manual" }
    }
    public init?(rawValue: String) {
        guard let value = Self.allCases.first(where: { $0.rawValue == rawValue }) else { return nil }
        self = value
    }
}
