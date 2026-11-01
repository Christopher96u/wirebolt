import Foundation

public struct HTTPMethod: RawRepresentable, CaseIterable, Codable, Hashable, Sendable {
    public let rawValue: String

    public init?(rawValue: String) {
        guard let normalized = domainNormalizeMethod(value: rawValue) else { return nil }
        self.rawValue = normalized
    }

    init(domainValue: String) { rawValue = domainValue }

    private init(_ value: String) {
        rawValue = value
    }

    public init(from decoder: any Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        guard let method = Self(rawValue: raw) else {
            throw DecodingError.dataCorruptedError(
                in: try decoder.singleValueContainer(),
                debugDescription: "Invalid HTTP method"
            )
        }
        self = method
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }

    public static let get = HTTPMethod("GET")
    public static let post = HTTPMethod("POST")
    public static let put = HTTPMethod("PUT")
    public static let patch = HTTPMethod("PATCH")
    public static let delete = HTTPMethod("DELETE")
    public static let head = HTTPMethod("HEAD")
    public static let options = HTTPMethod("OPTIONS")
    /// RFC 10008: a safe, idempotent request whose content (with its media type) is the query.
    public static let query = HTTPMethod("QUERY")
    public static let allCases: [HTTPMethod] = [.get, .post, .put, .patch, .delete, .head, .options, .query]
}

public enum ValueSource: Codable, Equatable, Hashable, Sendable {
    case literal(String)
    case secret(String)

    private enum CodingKeys: String, CodingKey { case secret }

    public init(from decoder: any Decoder) throws {
        if let literal = try? decoder.singleValueContainer().decode(String.self) {
            self = .literal(literal)
            return
        }
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self = try .secret(container.decode(String.self, forKey: .secret))
    }

    public func encode(to encoder: any Encoder) throws {
        switch self {
        case let .literal(value):
            var container = encoder.singleValueContainer()
            try container.encode(value)
        case let .secret(name):
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(name, forKey: .secret)
        }
    }

    public var editableValue: String {
        switch self {
        case let .literal(value), let .secret(value): value
        }
    }
}

public struct RequestField: Identifiable, Codable, Equatable, Sendable {
    var domain: DomainField = DomainField(id: "", name: "", value: .literal(value: ""), enabled: false, sensitive: false)
    init(domain: DomainField) { self.domain = domain }

    public var id: String {
        get { domain.id }
        set { domain.id = newValue }
    }
    public var name: String {
        get { domain.name }
        set { domain.name = newValue }
    }
    public var value: ValueSource {
        get { ValueSource(domain: domain.value) }
        set { domain.value = newValue.domain }
    }
    public var enabled: Bool {
        get { domain.enabled }
        set { domain.enabled = newValue }
    }
    public var sensitive: Bool {
        get { domain.sensitive }
        set { domain.sensitive = newValue }
    }
    enum CodingKeys: String, CodingKey { case id, name, value, enabled, sensitive }

    public init(
        id: String = domainUniqueId(),
        name: String = "",
        value: ValueSource = .literal(""),
        enabled: Bool = true,
        sensitive: Bool = false
    ) {
        self.id = id
        self.name = name
        self.value = value
        self.enabled = enabled
        self.sensitive = sensitive
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decodeIfPresent(String.self, forKey: .id)
            ?? domainUniqueId()
        name = try container.decode(String.self, forKey: .name)
        value = try container.decode(ValueSource.self, forKey: .value)
        enabled = try container.decode(Bool.self, forKey: .enabled)
        sensitive = try container.decodeIfPresent(Bool.self, forKey: .sensitive) ?? false
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(name, forKey: .name)
        try container.encode(value, forKey: .value)
        try container.encode(enabled, forKey: .enabled)
        if sensitive { try container.encode(true, forKey: .sensitive) }
    }
}

public enum APIKeyPlacement: String, Codable, CaseIterable, Sendable {
    case header
    case query
}

public enum OAuth2Grant: String, Codable, CaseIterable, Sendable {
    case authorizationCodePKCE = "authorization_code_pkce"
    case clientCredentials = "client_credentials"
}

public struct OAuth2Configuration: Codable, Equatable, Sendable {
    var domain: DomainOAuthConfiguration = DomainOAuthConfiguration(grant: .authorizationCodePkce, authorizationUrl: "", tokenUrl: "", clientId: "", clientSecretReference: "", scopes: "", audience: "", redirectUri: "", accessTokenReference: "")
    init(domain: DomainOAuthConfiguration) { self.domain = domain }

    public var grant: OAuth2Grant {
        get { OAuth2Grant(domain: domain.grant) }
        set { domain.grant = newValue.domain }
    }
    public var authorizationURL: String {
        get { domain.authorizationUrl }
        set { domain.authorizationUrl = newValue }
    }
    public var tokenURL: String {
        get { domain.tokenUrl }
        set { domain.tokenUrl = newValue }
    }
    public var clientID: String {
        get { domain.clientId }
        set { domain.clientId = newValue }
    }
    public var clientSecretReference: String {
        get { domain.clientSecretReference }
        set { domain.clientSecretReference = newValue }
    }
    public var scopes: String {
        get { domain.scopes }
        set { domain.scopes = newValue }
    }
    public var audience: String {
        get { domain.audience }
        set { domain.audience = newValue }
    }
    public var redirectURI: String {
        get { domain.redirectUri }
        set { domain.redirectUri = newValue }
    }
    public var accessTokenReference: String {
        get { domain.accessTokenReference }
        set { domain.accessTokenReference = newValue }
    }
    public init(
        grant: OAuth2Grant = .authorizationCodePKCE,
        authorizationURL: String = "",
        tokenURL: String = "",
        clientID: String = "",
        clientSecretReference: String = CredentialReference.unique(role: "oauth-client-secret"),
        scopes: String = "",
        audience: String = "",
        redirectURI: String = "wirebolt://oauth/callback",
        accessTokenReference: String = CredentialReference.unique(role: "oauth-access-token")
    ) {
        self.grant = grant
        self.authorizationURL = authorizationURL
        self.tokenURL = tokenURL
        self.clientID = clientID
        self.clientSecretReference = clientSecretReference
        self.scopes = scopes
        self.audience = audience
        self.redirectURI = redirectURI
        self.accessTokenReference = accessTokenReference
    }

    enum CodingKeys: String, CodingKey {
        case grant, scopes, audience
        case authorizationURL = "authorization_url"
        case tokenURL = "token_url"
        case clientID = "client_id"
        case clientSecretReference = "client_secret_reference"
        case redirectURI = "redirect_uri"
        case accessTokenReference = "access_token_reference"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        grant = try c.decode(OAuth2Grant.self, forKey: .grant)
        authorizationURL = try c.decode(String.self, forKey: .authorizationURL)
        tokenURL = try c.decode(String.self, forKey: .tokenURL)
        clientID = try c.decode(String.self, forKey: .clientID)
        clientSecretReference = try c.decode(String.self, forKey: .clientSecretReference)
        scopes = try c.decode(String.self, forKey: .scopes)
        audience = try c.decode(String.self, forKey: .audience)
        redirectURI = try c.decode(String.self, forKey: .redirectURI)
        accessTokenReference = try c.decode(String.self, forKey: .accessTokenReference)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(grant, forKey: .grant)
        try c.encode(authorizationURL, forKey: .authorizationURL)
        try c.encode(tokenURL, forKey: .tokenURL)
        try c.encode(clientID, forKey: .clientID)
        try c.encode(clientSecretReference, forKey: .clientSecretReference)
        try c.encode(scopes, forKey: .scopes)
        try c.encode(audience, forKey: .audience)
        try c.encode(redirectURI, forKey: .redirectURI)
        try c.encode(accessTokenReference, forKey: .accessTokenReference)
    }
}

public enum RequestAuthentication: Codable, Equatable, Sendable {
    case none
    case basic(username: ValueSource, password: ValueSource)
    case bearer(token: ValueSource)
    case apiKey(placement: APIKeyPlacement, name: String, value: ValueSource)
    case oauth2(configuration: OAuth2Configuration)

    private enum CodingKeys: String, CodingKey {
        case kind, username, password, token, placement, name, value, configuration
    }
    private enum Kind: String, Codable { case none, basic, bearer, apiKey = "api_key", oauth2 }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(Kind.self, forKey: .kind) {
        case .none: self = .none
        case .basic:
            self = try .basic(
                username: container.decode(ValueSource.self, forKey: .username),
                password: container.decode(ValueSource.self, forKey: .password)
            )
        case .bearer: self = try .bearer(token: container.decode(ValueSource.self, forKey: .token))
        case .apiKey:
            self = try .apiKey(
                placement: container.decode(APIKeyPlacement.self, forKey: .placement),
                name: container.decode(String.self, forKey: .name),
                value: container.decode(ValueSource.self, forKey: .value)
            )
        case .oauth2:
            self = try .oauth2(configuration: container.decode(OAuth2Configuration.self, forKey: .configuration))
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .none:
            try container.encode(Kind.none, forKey: .kind)
        case let .basic(username, password):
            try container.encode(Kind.basic, forKey: .kind)
            try container.encode(username, forKey: .username)
            try container.encode(password, forKey: .password)
        case let .bearer(token):
            try container.encode(Kind.bearer, forKey: .kind)
            try container.encode(token, forKey: .token)
        case let .apiKey(placement, name, value):
            try container.encode(Kind.apiKey, forKey: .kind)
            try container.encode(placement, forKey: .placement)
            try container.encode(name, forKey: .name)
            try container.encode(value, forKey: .value)
        case let .oauth2(configuration):
            try container.encode(Kind.oauth2, forKey: .kind)
            try container.encode(configuration, forKey: .configuration)
        }
    }
}

/// Keychain names for credentials a request stores. Every new credential gets its own
/// name so two requests never read or overwrite each other's secret material.
public enum CredentialReference {
    public static func unique(role: String, scope: String = "auth") -> String {
        domainCredentialReference(role: role, scope: scope)
    }

    /// Names earlier versions gave every API Key and OAuth request. Other workspaces may still
    /// use them, so Wirebolt copies their values to per-request names and never deletes them.
    public static let legacySharedNames: Set<String> = ["auth.api-key", "oauth.client-secret", "oauth.access-token"]

    public static func isLegacyShared(_ name: String) -> Bool { legacySharedNames.contains(name) }

    /// Whether Wirebolt generated `name` for exactly one request or variable, so the Keychain
    /// item can be deleted once nothing references it. Names people chose (such as proxy
    /// credentials or `{{secret}}` references) and legacy shared names never qualify.
    public static func isOwned(_ name: String) -> Bool {
        domainOwnedReference(name: name)
    }


    static func sanitized(_ text: String) -> String {
        domainSanitized(text: text)
    }
}

public extension ValueSource {
    var secretName: String? {
        if case let .secret(name) = self { name } else { nil }
    }
}

public extension RequestAuthentication {
    /// Keychain names this authentication reads or writes.
    var secretReferences: [String] {
        domainAuthReferences(authentication: domain)
    }

    /// The same authentication with each Keychain name passed through `rename`.
    func renamingSecrets(_ rename: (String) -> String) -> RequestAuthentication {
        let names = Dictionary(secretReferences.map { ($0, rename($0)) }, uniquingKeysWith: { first, _ in first })
        return RequestAuthentication(domain: domainRenameAuth(authentication: domain, names: names))
    }

    /// Fresh per-request names for every credential, for a copy of a request (`all`) or for
    /// migrating legacy shared names only. Returns the new authentication and old → new names.
    func withOwnKeychainNames(onlyLegacy: Bool) -> (RequestAuthentication, [String: String]) {
        let result = domainOwnAuth(authentication: domain, onlyLegacy: onlyLegacy)
        return (RequestAuthentication(domain: result.authentication), result.renames)
    }
}

public enum AuthenticationKind: CaseIterable, Sendable {
    case none, basic, bearer, apiKey, oauth2
}

public extension RequestAuthentication {
    var kind: AuthenticationKind {
        let kind = domainAuthKind(authentication: domain)
        return AuthenticationKind.allCases.first { String(describing: $0) == kind } ?? .none
    }

    /// An empty authentication of `kind` whose secrets use fresh Keychain names.
    static func new(_ kind: AuthenticationKind) -> RequestAuthentication {
        RequestAuthentication(domain: domainNewAuth(kind: String(describing: kind)))
    }
}

public enum RequestBody: Codable, Equatable, Sendable {
    case empty
    case text(contentType: String?, value: String)
    case json(value: String)
    case xml(value: String)
    case html(value: String)
    case raw(contentType: String?, value: String)
    case formURLEncoded(fields: [RequestField])
    case multipart(parts: [MultipartPart])
    case file(path: String, contentType: String?)

    private enum CodingKeys: String, CodingKey {
        case kind, value, fields, parts, path
        case contentType = "content_type"
    }
    private enum Kind: String, Codable {
        case empty, text, json, xml, html, raw, multipart, file
        case formURLEncoded = "form_url_encoded"
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .empty:
            try container.encode(Kind.empty, forKey: .kind)
        case let .text(contentType, value):
            try container.encode(Kind.text, forKey: .kind)
            try container.encodeIfPresent(contentType, forKey: .contentType)
            try container.encode(value, forKey: .value)
        case let .json(value):
            try container.encode(Kind.json, forKey: .kind)
            try container.encode(value, forKey: .value)
        case let .xml(value):
            try container.encode(Kind.xml, forKey: .kind)
            try container.encode(value, forKey: .value)
        case let .html(value):
            try container.encode(Kind.html, forKey: .kind)
            try container.encode(value, forKey: .value)
        case let .raw(contentType, value):
            try container.encode(Kind.raw, forKey: .kind)
            try container.encodeIfPresent(contentType, forKey: .contentType)
            try container.encode(value, forKey: .value)
        case let .formURLEncoded(fields):
            try container.encode(Kind.formURLEncoded, forKey: .kind)
            try container.encode(fields, forKey: .fields)
        case let .multipart(parts):
            try container.encode(Kind.multipart, forKey: .kind)
            try container.encode(parts, forKey: .parts)
        case let .file(path, contentType):
            try container.encode(Kind.file, forKey: .kind)
            try container.encode(path, forKey: .path)
            try container.encodeIfPresent(contentType, forKey: .contentType)
        }
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(Kind.self, forKey: .kind) {
        case .empty: self = .empty
        case .text:
            self = try .text(
                contentType: container.decodeIfPresent(String.self, forKey: .contentType),
                value: container.decode(String.self, forKey: .value)
            )
        case .json: self = try .json(value: container.decode(String.self, forKey: .value))
        case .xml: self = try .xml(value: container.decode(String.self, forKey: .value))
        case .html: self = try .html(value: container.decode(String.self, forKey: .value))
        case .raw:
            self = try .raw(
                contentType: container.decodeIfPresent(String.self, forKey: .contentType),
                value: container.decode(String.self, forKey: .value)
            )
        case .formURLEncoded:
            self = try .formURLEncoded(fields: container.decode([RequestField].self, forKey: .fields))
        case .multipart:
            self = try .multipart(parts: container.decode([MultipartPart].self, forKey: .parts))
        case .file:
            self = try .file(
                path: container.decode(String.self, forKey: .path),
                contentType: container.decodeIfPresent(String.self, forKey: .contentType)
            )
        }
    }
}

public enum MultipartPartKind: String, Codable, CaseIterable, Sendable {
    case text
    case binary
    case file
}

public struct MultipartPart: Identifiable, Codable, Equatable, Sendable {
    var domain: DomainMultipartPart = DomainMultipartPart(id: "", name: "", kind: .text, value: .literal(value: ""), filePath: nil, fileName: nil, contentType: nil, enabled: false)
    init(domain: DomainMultipartPart) { self.domain = domain }

    public var id: String {
        get { domain.id }
        set { domain.id = newValue }
    }
    public var name: String {
        get { domain.name }
        set { domain.name = newValue }
    }
    public var kind: MultipartPartKind {
        get { MultipartPartKind(domain: domain.kind) }
        set { domain.kind = newValue.domain }
    }
    public var value: ValueSource {
        get { ValueSource(domain: domain.value) }
        set { domain.value = newValue.domain }
    }
    public var filePath: String? {
        get { domain.filePath.map { $0 } }
        set { domain.filePath = newValue.map { $0 } }
    }
    public var fileName: String? {
        get { domain.fileName.map { $0 } }
        set { domain.fileName = newValue.map { $0 } }
    }
    public var contentType: String? {
        get { domain.contentType.map { $0 } }
        set { domain.contentType = newValue.map { $0 } }
    }
    public var enabled: Bool {
        get { domain.enabled }
        set { domain.enabled = newValue }
    }
    public init(
        id: String = domainUniqueId(),
        name: String = "",
        kind: MultipartPartKind = .text,
        value: ValueSource = .literal(""),
        filePath: String? = nil,
        fileName: String? = nil,
        contentType: String? = nil,
        enabled: Bool = true
    ) {
        self.id = id
        self.name = name
        self.kind = kind
        self.value = value
        self.filePath = filePath
        self.fileName = fileName
        self.contentType = contentType
        self.enabled = enabled
    }

    enum CodingKeys: String, CodingKey {
        case id, name, kind, value, enabled
        case filePath = "file_path"
        case fileName = "file_name"
        case contentType = "content_type"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        kind = try c.decode(MultipartPartKind.self, forKey: .kind)
        value = try c.decode(ValueSource.self, forKey: .value)
        filePath = try c.decodeIfPresent(String.self, forKey: .filePath)
        fileName = try c.decodeIfPresent(String.self, forKey: .fileName)
        contentType = try c.decodeIfPresent(String.self, forKey: .contentType)
        enabled = try c.decode(Bool.self, forKey: .enabled)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(name, forKey: .name)
        try c.encode(kind, forKey: .kind)
        try c.encode(value, forKey: .value)
        try c.encodeIfPresent(filePath, forKey: .filePath)
        try c.encodeIfPresent(fileName, forKey: .fileName)
        try c.encodeIfPresent(contentType, forKey: .contentType)
        try c.encode(enabled, forKey: .enabled)
    }
}

public enum ProxySelection: Hashable, Sendable {
    case inherit
    case system
    case direct
    case manual(ProxyDocument)
}

public struct TransportSettings: Codable, Equatable, Sendable {
    var domain: DomainTransport = DomainTransport(validateTls: false, followRedirects: false, maximumRedirects: 0, totalTimeoutMs: 0, readTimeoutMs: 0, clientCertificateReference: nil, customCaPath: nil)
    init(domain: DomainTransport) { self.domain = domain }

    public var validateTLS: Bool {
        get { domain.validateTls }
        set { domain.validateTls = newValue }
    }
    public var followRedirects: Bool {
        get { domain.followRedirects }
        set { domain.followRedirects = newValue }
    }
    public var maximumRedirects: Int {
        get { Int(domain.maximumRedirects) }
        set { domain.maximumRedirects = Int64(newValue) }
    }
    public var totalTimeoutMS: UInt64 {
        get { domain.totalTimeoutMs }
        set { domain.totalTimeoutMs = newValue }
    }
    public var readTimeoutMS: UInt64 {
        get { domain.readTimeoutMs }
        set { domain.readTimeoutMs = newValue }
    }
    public var clientCertificateReference: String? {
        get { domain.clientCertificateReference.map { $0 } }
        set { domain.clientCertificateReference = newValue.map { $0 } }
    }
    public var customCAPath: String? {
        get { domain.customCaPath.map { $0 } }
        set { domain.customCaPath = newValue.map { $0 } }
    }
    public init(
        validateTLS: Bool = true,
        followRedirects: Bool = false,
        maximumRedirects: Int = 10,
        totalTimeoutMS: UInt64 = 30_000,
        readTimeoutMS: UInt64 = 10_000,
        clientCertificateReference: String? = nil,
        customCAPath: String? = nil
    ) {
        self.validateTLS = validateTLS
        self.followRedirects = followRedirects
        self.maximumRedirects = Int(domainClampRedirects(value: Int64(maximumRedirects)))
        self.totalTimeoutMS = totalTimeoutMS
        self.readTimeoutMS = readTimeoutMS
        self.clientCertificateReference = clientCertificateReference
        self.customCAPath = customCAPath
    }

    enum CodingKeys: String, CodingKey {
        case validateTLS = "validate_tls"
        case followRedirects = "follow_redirects"
        case maximumRedirects = "maximum_redirects"
        case totalTimeoutMS = "total_timeout_ms"
        case readTimeoutMS = "read_timeout_ms"
        case clientCertificateReference = "client_certificate_reference"
        case customCAPath = "custom_ca_path"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        validateTLS = try c.decode(Bool.self, forKey: .validateTLS)
        followRedirects = try c.decode(Bool.self, forKey: .followRedirects)
        maximumRedirects = try c.decode(Int.self, forKey: .maximumRedirects)
        totalTimeoutMS = try c.decode(UInt64.self, forKey: .totalTimeoutMS)
        readTimeoutMS = try c.decode(UInt64.self, forKey: .readTimeoutMS)
        clientCertificateReference = try c.decodeIfPresent(String.self, forKey: .clientCertificateReference)
        customCAPath = try c.decodeIfPresent(String.self, forKey: .customCAPath)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(validateTLS, forKey: .validateTLS)
        try c.encode(followRedirects, forKey: .followRedirects)
        try c.encode(maximumRedirects, forKey: .maximumRedirects)
        try c.encode(totalTimeoutMS, forKey: .totalTimeoutMS)
        try c.encode(readTimeoutMS, forKey: .readTimeoutMS)
        try c.encodeIfPresent(clientCertificateReference, forKey: .clientCertificateReference)
        try c.encodeIfPresent(customCAPath, forKey: .customCAPath)
    }
}

public struct RequestDraft: Equatable, Sendable {
    private var displayCache: UrlQueryEdit?
    public static func == (lhs: Self, rhs: Self) -> Bool { lhs.domain == rhs.domain }
    var domain: DomainRequestDraft = DomainRequestDraft(id: "", name: "", method: "", url: "", webSocket: false, note: "", query: [], headers: [], authentication: .none, body: .empty, proxy: nil, transport: DomainTransport(validateTls: false, followRedirects: false, maximumRedirects: 0, totalTimeoutMs: 0, readTimeoutMs: 0, clientCertificateReference: nil, customCaPath: nil), inheritsWorkspaceTransport: false)
    init(domain: DomainRequestDraft) { self.domain = domain }

    public var id: String {
        get { domain.id }
        set { domain.id = newValue }
    }
    public var name: String {
        get { domain.name }
        set { domain.name = newValue }
    }
    public var method: HTTPMethod {
        get { HTTPMethod(domainValue: domain.method) }
        set { domain.method = newValue.rawValue }
    }
    public var url: String {
        get { domain.url }
        set { domain.url = newValue }
    }
    public var webSocket: Bool {
        get { domain.webSocket }
        set { domain.webSocket = newValue }
    }
    public var note: String {
        get { domain.note }
        set { domain.note = newValue }
    }
    public var query: [RequestField] {
        get { domain.query.map { RequestField(domain: $0) } }
        set { domain.query = newValue.map { $0.domain } }
    }
    public var headers: [RequestField] {
        get { domain.headers.map { RequestField(domain: $0) } }
        set { domain.headers = newValue.map { $0.domain } }
    }
    public var authentication: RequestAuthentication {
        get { RequestAuthentication(domain: domain.authentication) }
        set { domain.authentication = newValue.domain }
    }
    public var body: RequestBody {
        get { RequestBody(domain: domain.body) }
        set { domain.body = newValue.domain }
    }
    public var proxy: ProxySelection {
        get { ProxySelection(document: domain.proxy.map(ProxyDocument.init(domain:))) }
        set { domain.proxy = newValue.document?.domain }
    }
    public var transport: TransportSettings {
        get { TransportSettings(domain: domain.transport) }
        set { domain.transport = newValue.domain }
    }
    public var inheritsWorkspaceTransport: Bool {
        get { domain.inheritsWorkspaceTransport }
        set { domain.inheritsWorkspaceTransport = newValue }
    }
    public init(
        id: String = "draft",
        name: String = "Untitled Request",
        method: HTTPMethod = .get,
        url: String = "",
        webSocket: Bool = false,
        note: String = "",
        query: [RequestField] = [],
        headers: [RequestField] = [],
        authentication: RequestAuthentication = .none,
        body: RequestBody = .empty,
        proxy: ProxySelection = .inherit,
        transport: TransportSettings = TransportSettings(),
        inheritsWorkspaceTransport: Bool = true
    ) {
        self.id = id
        self.name = name
        self.method = method
        self.url = url
        self.webSocket = webSocket
        self.note = note
        self.query = query
        self.headers = headers
        self.authentication = authentication
        self.body = body
        self.proxy = proxy
        self.transport = transport
        self.inheritsWorkspaceTransport = inheritsWorkspaceTransport
    }
}

public extension RequestDraft {
    var separatingURLQuery: RequestDraft {
        var result = self
        result.apply(domainSeparateUrlQuery(url: domain.url, query: domain.query))
        return result
    }

    /// The URL field combines the base URL and the editable query table. Transport
    /// receives them separately so each query parameter is encoded exactly once.
    var displayURL: String {
        if let cache = displayCache, cache.url == domain.url, cache.query == domain.query { return cache.displayUrl }
        return domainDisplayUrl(url: domain.url, query: domain.query)
    }

    mutating func editURL(_ text: String) {
        apply(domainEditUrl(url: domain.url, query: domain.query, text: text))
    }

    private mutating func apply(_ edit: UrlQueryEdit) {
        domain.url = edit.url
        domain.query = edit.query
        displayCache = edit
    }
}

public struct EnvironmentVariableDraft: Identifiable, Codable, Equatable, Sendable {
    var domain: DomainVariable = DomainVariable(id: "", key: "", value: .literal(value: ""), enabled: false, order: 0)
    init(domain: DomainVariable) { self.domain = domain }

    public var id: String {
        get { domain.id }
        set { domain.id = newValue }
    }
    public var key: String {
        get { domain.key }
        set { domain.key = newValue }
    }
    public var value: ValueSource {
        get { ValueSource(domain: domain.value) }
        set { domain.value = newValue.domain }
    }
    public var enabled: Bool {
        get { domain.enabled }
        set { domain.enabled = newValue }
    }
    public var order: Int {
        get { Int(domain.order) }
        set { domain.order = Int64(newValue) }
    }
    public init(
        id: String = domainUniqueId(),
        key: String = "",
        value: ValueSource = .literal(""),
        enabled: Bool = true,
        order: Int = 0
    ) {
        self.id = id
        self.key = key
        self.value = value
        self.enabled = enabled
        self.order = order
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        key = try c.decode(String.self, forKey: .key)
        value = try c.decode(ValueSource.self, forKey: .value)
        enabled = try c.decode(Bool.self, forKey: .enabled)
        order = try c.decode(Int.self, forKey: .order)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(key, forKey: .key)
        try c.encode(value, forKey: .value)
        try c.encode(enabled, forKey: .enabled)
        try c.encode(order, forKey: .order)
    }

    private enum CodingKeys: String, CodingKey { case id, key, value, enabled, order }
}

public extension EnvironmentVariableDraft {
    var isSecret: Bool {
        if case .secret = value { true } else { false }
    }

    /// A new Keychain name for this variable's secret value. It is stored in the variable, so
    /// renaming the key keeps its value, and it is unique, so no other environment,
    /// workspace or Mac user of the same Keychain shares it.
    func makeSecretReference(environmentID: String) -> String {
        CredentialReference.unique(role: environmentID, scope: "env")
    }
}

public extension EnvironmentDraft {
    var secretReferences: Set<String> { Set(variables.compactMap(\.value.secretName)) }
}

public extension RequestDraft {
    /// Keychain references are collected in Rust, with no secret material.
    var secretReferences: Set<String> { Set(domainRequestReferences(draft: domain)) }
}

public extension WorkspaceDraft {
    var secretReferences: Set<String> { Set(domainWorkspaceReferences(workspace: domain)) }
}

public struct EnvironmentDraft: Identifiable, Codable, Equatable, Sendable {
    var domain: DomainEnvironment = DomainEnvironment(id: "", name: "", variables: [])
    private var projectedVariables: [EnvironmentVariableDraft] = []
    init(domain: DomainEnvironment) {
        self.domain = domain
        projectedVariables = domain.variables.map(EnvironmentVariableDraft.init(domain:))
    }

    public var id: String {
        get { domain.id }
        set { domain.id = newValue }
    }
    public var name: String {
        get { domain.name }
        set { domain.name = newValue }
    }
    public var variables: [EnvironmentVariableDraft] {
        get { projectedVariables }
        set { projectedVariables = newValue; domain.variables = newValue.map(\.domain) }
    }
    public init(
        id: String,
        name: String,
        variables: [EnvironmentVariableDraft] = []
    ) {
        self.id = id
        self.name = name
        self.variables = variables
    }

    public init(id: String, name: String, legacyVariables: [String: ValueSource]) {
        self.init(domain: domainLegacyEnvironment(id: id, name: name, values: legacyVariables.mapValues(\.domain)))
    }

    public var enabledValues: [String: ValueSource] {
        domainEnabledValues(environment: domain).mapValues(ValueSource.init(domain:))
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        variables = try c.decode([EnvironmentVariableDraft].self, forKey: .variables)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(name, forKey: .name)
        try c.encode(variables, forKey: .variables)
    }

    private enum CodingKeys: String, CodingKey { case id, name, variables }
}

public struct RequestLocation: Identifiable, Equatable, Sendable {
    var domain: DomainLocation = DomainLocation(collectionId: "", groupId: nil, order: 0, request: DomainRequestDraft(id: "", name: "", method: "", url: "", webSocket: false, note: "", query: [], headers: [], authentication: .none, body: .empty, proxy: nil, transport: DomainTransport(validateTls: false, followRedirects: false, maximumRedirects: 0, totalTimeoutMs: 0, readTimeoutMs: 0, clientCertificateReference: nil, customCaPath: nil), inheritsWorkspaceTransport: false))
    init(domain: DomainLocation) { self.domain = domain }

    public var id: String { "\(collectionID)/\(request.id)" }
    public var collectionID: String {
        get { domain.collectionId }
        set { domain.collectionId = newValue }
    }
    public var groupID: String? {
        get { domain.groupId.map { $0 } }
        set { domain.groupId = newValue.map { $0 } }
    }
    public var order: Int {
        get { Int(domain.order) }
        set { domain.order = Int64(newValue) }
    }
    public var request: RequestDraft {
        get { RequestDraft(domain: domain.request) }
        set { domain.request = newValue.domain }
    }
    public init(
        collectionID: String,
        groupID: String? = nil,
        order: Int = 0,
        request: RequestDraft
    ) {
        self.collectionID = collectionID
        self.groupID = groupID
        self.order = order
        self.request = request
    }
}

public struct GroupDraft: Identifiable, Codable, Equatable, Sendable {
    var domain: DomainGroup = DomainGroup(id: "", name: "", parentId: nil, order: 0)
    init(domain: DomainGroup) { self.domain = domain }

    public var id: String {
        get { domain.id }
        set { domain.id = newValue }
    }
    public var name: String {
        get { domain.name }
        set { domain.name = newValue }
    }
    public var parentID: String? {
        get { domain.parentId.map { $0 } }
        set { domain.parentId = newValue.map { $0 } }
    }
    public var order: Int {
        get { Int(domain.order) }
        set { domain.order = Int64(newValue) }
    }
    public init(id: String, name: String, parentID: String? = nil, order: Int = 0) {
        self.id = id
        self.name = name
        self.parentID = parentID
        self.order = order
    }

    enum CodingKeys: String, CodingKey {
        case id, name, order
        case parentID = "parent_id"
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        parentID = try c.decodeIfPresent(String.self, forKey: .parentID)
        order = try c.decode(Int.self, forKey: .order)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(name, forKey: .name)
        try c.encodeIfPresent(parentID, forKey: .parentID)
        try c.encode(order, forKey: .order)
    }
}

public struct CollectionDraft: Identifiable, Equatable, Sendable {
    var domain: DomainCollection = DomainCollection(id: "", name: "", order: 0, groups: [], requests: [])
    private var projectedGroups: [GroupDraft] = []
    private var projectedRequests: [RequestLocation] = []
    init(domain: DomainCollection) {
        self.domain = domain
        projectedGroups = domain.groups.map(GroupDraft.init(domain:))
        projectedRequests = domain.requests.map(RequestLocation.init(domain:))
    }

    public var id: String {
        get { domain.id }
        set { domain.id = newValue }
    }
    public var name: String {
        get { domain.name }
        set { domain.name = newValue }
    }
    public var order: Int {
        get { Int(domain.order) }
        set { domain.order = Int64(newValue) }
    }
    public var groups: [GroupDraft] {
        get { projectedGroups }
        set { projectedGroups = newValue; domain.groups = newValue.map(\.domain) }
    }
    public var requests: [RequestLocation] {
        get { projectedRequests }
        set { projectedRequests = newValue; domain.requests = newValue.map(\.domain) }
    }
    public init(
        id: String,
        name: String,
        order: Int = 0,
        groups: [GroupDraft] = [],
        requests: [RequestLocation] = []
    ) {
        self.id = id
        self.name = name
        self.order = order
        self.groups = groups
        self.requests = requests
    }
}

public enum WorkspaceNodeKind: String, Codable, Equatable, Sendable {
    case workspace
    case collection
    case group
    case request
    case environment
}

public enum WorkspaceCommand: Equatable, Sendable {
    case reorderChildren(collectionID: String, parentID: String?, items: [String])
    case saveWorkspaceProxy(ProxyDocument?)
    case saveWorkspaceSettings(TransportSettings)
    case renameWorkspace(name: String)
    case createCollection(CollectionDraft)
    case renameCollection(id: String, name: String)
    case deleteCollection(id: String)
    case createGroup(collectionID: String, group: GroupDraft)
    case renameGroup(collectionID: String, id: String, name: String)
    case deleteGroup(collectionID: String, id: String)
    case moveGroup(collectionID: String, id: String, parentID: String?, order: Int)
    case saveRequest(collectionID: String, location: RequestLocation)
    case deleteRequest(collectionID: String, id: String)
    case duplicateRequest(collectionID: String, id: String, newID: String, name: String)
    case moveRequest(
        fromCollectionID: String,
        requestID: String,
        toCollectionID: String,
        groupID: String?,
        order: Int
    )
    case saveEnvironment(EnvironmentDraft)
    case deleteEnvironment(id: String)
}

public struct WorkspaceDelta: Codable, Equatable, Sendable {
    public let version: UInt64
    public let kind: WorkspaceNodeKind
    public let affectedIDs: [String]

    public init(version: UInt64, kind: WorkspaceNodeKind, affectedIDs: [String]) {
        self.version = version
        self.kind = kind
        self.affectedIDs = affectedIDs
    }

    enum CodingKeys: String, CodingKey {
        case version, kind
        case affectedIDs = "affected_ids"
    }
}

public enum ImportFormat: String, Codable, CaseIterable, Sendable {
    case curl
    case har
    case legacyWorkspaceV1 = "legacy_workspace_v1"
    case postmanV2 = "postman_v2"
    /// Insomnia v4 export (JSON or YAML) or v5 YAML collection.
    case insomnia
    /// Bruno "Export collection" JSON.
    case bruno
    /// A Bruno collection folder or `.bru` file, bundled by `BrunoCollectionSource`.
    case brunoFolder = "bruno_folder"
}

public struct ImportPreview: Codable, Equatable, Identifiable, Sendable {
    public let collectionName: String
    public let requestCount: Int
    public let groupCount: Int
    public let warnings: [String]
    public var id: String { "\(collectionName)-\(requestCount)-\(groupCount)" }

        enum CodingKeys: String, CodingKey {
        case warnings
        case collectionName = "collection_name"
        case requestCount = "request_count"
        case groupCount = "group_count"
    }
}

/// What an import created and everything it could not carry over exactly.
public struct ImportSummary: Codable, Equatable, Identifiable, Sendable {
    public struct Environment: Codable, Equatable, Sendable {
        public let id: String
        public let name: String

        public init(id: String, name: String) {
            self.id = id
            self.name = name
        }
    }

    /// Distinguishes consecutive imports so presentations restart.
    public var id = UUID()
    public let collectionNames: [String]
    public let requestCount: Int
    public let groupCount: Int
    public let environments: [Environment]
    public let warnings: [String]

    public init(
        collectionNames: [String],
        requestCount: Int,
        groupCount: Int,
        environments: [Environment] = [],
        warnings: [String] = []
    ) {
        self.collectionNames = collectionNames
        self.requestCount = requestCount
        self.groupCount = groupCount
        self.environments = environments
        self.warnings = warnings
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        collectionNames = try container.decode([String].self, forKey: .collectionNames)
        requestCount = try container.decode(Int.self, forKey: .requestCount)
        groupCount = try container.decode(Int.self, forKey: .groupCount)
        environments = try container.decodeIfPresent([Environment].self, forKey: .environments) ?? []
        warnings = try container.decodeIfPresent([String].self, forKey: .warnings) ?? []
    }

        /// Warnings or new environments deserve a closer look than a transient banner.
    public var needsReview: Bool { !warnings.isEmpty || !environments.isEmpty }

    /// One sentence describing what was imported, such as
    /// "Imported 9 requests in 3 folders into “Shop API”."
    public var headline: String {
        domainImportHeadline(names: collectionNames, requests: Int64(requestCount), groups: Int64(groupCount))
    }

    /// Describes the environments the import created, if any.
    public var environmentLine: String? {
        domainImportEnvironmentLine(names: environments.map(\.name), localizedList: ListFormatter.localizedString(byJoining: environments.map { "“\($0.name)”" }))
    }


    enum CodingKeys: String, CodingKey {
        case warnings, environments
        case collectionNames = "collection_names"
        case requestCount = "request_count"
        case groupCount = "group_count"
    }
}

/// A committed import: the workspace delta and its summary, decoded from one bridge document.
public struct ImportResult: Decodable, Equatable, Sendable {
    public let delta: WorkspaceDelta
    public let summary: ImportSummary

    public init(delta: WorkspaceDelta, summary: ImportSummary) {
        self.delta = delta
        self.summary = summary
    }

    public init(from decoder: any Decoder) throws {
        delta = try WorkspaceDelta(from: decoder)
        summary = try decoder.container(keyedBy: CodingKeys.self).decode(ImportSummary.self, forKey: .summary)
    }

    enum CodingKeys: String, CodingKey { case summary }
}

public struct WorkspaceDraft: Equatable, Sendable {
    var domain: DomainWorkspace = DomainWorkspace(name: "", proxy: nil, transport: DomainTransport(validateTls: false, followRedirects: false, maximumRedirects: 0, totalTimeoutMs: 0, readTimeoutMs: 0, clientCertificateReference: nil, customCaPath: nil), collections: [], environments: [])
    private var projectedCollections: [CollectionDraft] = []
    private var projectedEnvironments: [EnvironmentDraft] = []
    init(domain: DomainWorkspace) {
        self.domain = domain
        projectedCollections = domain.collections.map(CollectionDraft.init(domain:))
        projectedEnvironments = domain.environments.map(EnvironmentDraft.init(domain:))
    }

    public static let globalEnvironmentID = "global"
    public static let rootCollectionID = "workspace-root"
    public var name: String {
        get { domain.name }
        set { domain.name = newValue }
    }
    public var proxy: ProxyDocument? {
        get { domain.proxy.map { ProxyDocument(domain: $0) } }
        set { domain.proxy = newValue.map { $0.domain } }
    }
    public var transport: TransportSettings {
        get { TransportSettings(domain: domain.transport) }
        set { domain.transport = newValue.domain }
    }
    public var collections: [CollectionDraft] {
        get { projectedCollections }
        set { projectedCollections = newValue; domain.collections = newValue.map(\.domain) }
    }
    public var environments: [EnvironmentDraft] {
        get { projectedEnvironments }
        set { projectedEnvironments = newValue; domain.environments = newValue.map(\.domain) }
    }
    public init(
        name: String,
        proxy: ProxyDocument? = nil,
        transport: TransportSettings = TransportSettings(),
        collections: [CollectionDraft] = [],
        environments: [EnvironmentDraft] = []
    ) {
        self.name = name
        self.proxy = proxy
        self.transport = transport
        self.collections = collections
        self.environments = environments
    }

    public func location(collectionID: String, requestID: String) -> RequestLocation? {
        collections
            .first(where: { $0.id == collectionID })?
            .requests
            .first(where: { $0.request.id == requestID })
    }
}

public extension CollectionDraft {
    func descendantGroupIDs(of rootID: String) -> Set<String> {
        Set(domainSubtree(collection: domain, root: rootID).map { groups[Int($0)].id }).union([rootID])
    }
}

public enum GitDeltaSnapshot: String, Codable, Equatable, Sendable {
    case none
    case added
    case modified
    case deleted
    case renamed
    case copied
    case typeChanged = "type_changed"
    case untracked
    case unmerged
}

public struct GitChangeSnapshot: Codable, Equatable, Identifiable, Sendable {
    public var id: String { previousPath.map { "\($0)\u{0}\(path)" } ?? path }
    public let path: String
    public let previousPath: String?
    public let staged: GitDeltaSnapshot
    public let unstaged: GitDeltaSnapshot
    public let conflicted: Bool

    enum CodingKeys: String, CodingKey {
        case path, staged, unstaged, conflicted
        case previousPath = "previous_path"
    }

    public init(
        path: String,
        previousPath: String? = nil,
        staged: GitDeltaSnapshot,
        unstaged: GitDeltaSnapshot,
        conflicted: Bool
    ) {
        self.path = path
        self.previousPath = previousPath
        self.staged = staged
        self.unstaged = unstaged
        self.conflicted = conflicted
    }
}

/// A readable summary of one changed file, combining its staged and unstaged deltas.
public enum GitChangeKind: String, Equatable, Sendable {
    case new = "New"
    case modified = "Modified"
    case deleted = "Deleted"
    case renamed = "Renamed"
    case copied = "Copied"
    case typeChanged = "Type Changed"
    case conflicted = "Conflict"
}

/// What the Push button can do for the current status.
public enum GitPushAvailability: Equatable, Sendable {
    /// Push commits to the configured upstream.
    case push
    /// No upstream yet: pushing publishes the branch to `origin` and tracks it.
    case publish(branch: String)
    /// The button stays visible but disabled, with this reason as help.
    case unavailable(reason: String)
}

extension GitStatusSnapshot {
    public var pushAvailability: GitPushAvailability {
        switch domainPushAvailability(branch: branch, upstream: upstream, ahead: ahead) {
        case .push: .push
        case let .publish(branch): .publish(branch: branch)
        case let .unavailable(reason): .unavailable(reason: reason)
        }
    }
}

extension GitChangeSnapshot {
    public var kind: GitChangeKind {
        GitChangeKind(rawValue: domainGitChangeKind(staged: staged.rawValue, unstaged: unstaged.rawValue, conflicted: conflicted))!
    }
}

public struct GitStatusSnapshot: Codable, Equatable, Sendable {
    public let branch: String?
    public let upstream: String?
    public let ahead: UInt64
    public let behind: UInt64
    public let changes: [GitChangeSnapshot]
    /// A pull stopped mid-merge; Abort Merge returns to the state before the pull.
    public let merging: Bool

    enum CodingKeys: String, CodingKey { case branch, upstream, ahead, behind, changes, merging }

    public init(
        branch: String?,
        upstream: String?,
        ahead: UInt64,
        behind: UInt64,
        changes: [GitChangeSnapshot],
        merging: Bool = false
    ) {
        self.branch = branch
        self.upstream = upstream
        self.ahead = ahead
        self.behind = behind
        self.changes = changes
        self.merging = merging
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        branch = try container.decodeIfPresent(String.self, forKey: .branch)
        upstream = try container.decodeIfPresent(String.self, forKey: .upstream)
        ahead = try container.decode(UInt64.self, forKey: .ahead)
        behind = try container.decode(UInt64.self, forKey: .behind)
        changes = try container.decode([GitChangeSnapshot].self, forKey: .changes)
        merging = try container.decodeIfPresent(Bool.self, forKey: .merging) ?? false
    }

    public var conflictedChanges: [GitChangeSnapshot] {
        changes.filter { $0.kind == .conflicted }
    }
}

public enum GitOperationOutcome: String, Codable, Equatable, Sendable {
    case nothingToCommit = "nothing_to_commit"
    case committed
    case updated
    case upToDate = "up_to_date"
    case pushed
    case conflicted
    case mergeAborted = "merge_aborted"
}

public struct GitOperationSnapshot: Codable, Equatable, Sendable {
    public let outcome: GitOperationOutcome
    public let revision: String?
    public let status: GitStatusSnapshot

    public init(
        outcome: GitOperationOutcome,
        revision: String?,
        status: GitStatusSnapshot
    ) {
        self.outcome = outcome
        self.revision = revision
        self.status = status
    }
}

/// Git failures that mean the workspace cannot use Git yet, each with its own guidance.
public enum GitRepositoryProblem: Equatable, Sendable {
    case notRepository
    case notRepositoryRoot
    case gitUnavailable

    public init?(kind: String) {
        switch workspaceRepositoryProblem(kind: kind) {
        case .notRepository: self = .notRepository
        case .notRepositoryRoot: self = .notRepositoryRoot
        case .gitUnavailable: self = .gitUnavailable
        case nil: return nil
        }
    }
}

public struct GitFailure: Error, Equatable, Sendable {
    public let kind: String
    public let reason: String

    public init(kind: String, reason: String) {
        self.kind = kind
        self.reason = reason
    }
}

public struct ResponseHeader: Codable, Equatable, Identifiable, Sendable {
    public let name: String
    public let value: String
    public var id: String { name + "\u{0}" + value }
}

public struct ResponseHead: Codable, Equatable, Sendable {
    public let status: UInt16
    public let version: String
    public let headers: [ResponseHeader]
    public let timeToHeadersNS: UInt64

    enum CodingKeys: String, CodingKey {
        case status, version, headers
        case timeToHeadersNS = "time_to_headers_ns"
    }
}

public struct RunCompletion: Codable, Equatable, Sendable {
    public let bytesReceived: UInt64
    public let totalTimeNS: UInt64

    enum CodingKeys: String, CodingKey {
        case bytesReceived = "bytes_received"
        case totalTimeNS = "total_time_ns"
    }
}

public struct RequestIssue: Codable, Equatable, Sendable {
    public let path: String
    public let kind: String
    public let reference: String?
}

public struct RunFailure: Error, Codable, Equatable, Sendable {
    public let kind: String
    public let issues: [RequestIssue]
}

public struct PreparedHeaderSnapshot: Codable, Equatable, Sendable {
    public let name: String
    public let value: String
    public let redacted: Bool
}

public struct PreparedBodySnapshot: Codable, Equatable, Sendable {
    public let byteCount: UInt64
    public let contentType: String?
    public let textPreview: String?
    public let redacted: Bool

    enum CodingKeys: String, CodingKey {
        case redacted
        case byteCount = "byte_count"
        case contentType = "content_type"
        case textPreview = "text_preview"
    }
}

public struct PreparedRunSnapshot: Codable, Equatable, Sendable {
    public var proxy: EffectiveProxy? = nil
    public let method: String
    public let url: String
    public let headers: [PreparedHeaderSnapshot]
    public let body: PreparedBodySnapshot
    public let transport: TransportSettings
}

public enum RunEvent: Equatable, Sendable {
    case prepared(PreparedRunSnapshot)
    case head(ResponseHead)
    case cookies(ResponseCookies)
    case chunk(Data)
    case complete(RunCompletion)
}

public struct RunInput: Encodable, Sendable {
    let domain: DomainRunInput
    public var method: String { domain.method }
    public var url: String { domain.url }
    public var query: [RequestField] { domain.query.map(RequestField.init(domain:)) }
    public var headers: [RequestField] { domain.headers.map(RequestField.init(domain:)) }
    public var authentication: RequestAuthentication { RequestAuthentication(domain: domain.authentication) }
    public var body: RequestBody { RequestBody(domain: domain.body) }
    public var variables: [String: ValueSource] { domain.variables.mapValues(ValueSource.init(domain:)) }
    public var appProxy: ProxyDocument? { domain.appProxy.map(ProxyDocument.init(domain:)) }
    public var workspaceProxy: ProxyDocument? { domain.workspaceProxy.map(ProxyDocument.init(domain:)) }
    public var requestProxy: ProxyDocument? { domain.requestProxy.map(ProxyDocument.init(domain:)) }
    public var totalTimeoutMS: UInt64 { domain.totalTimeoutMs }
    public var readTimeoutMS: UInt64 { domain.readTimeoutMs }
    public var maxResponseBytes: UInt64? { domain.maxResponseBytes }
    public var validateTLS: Bool { domain.validateTls }
    public var followRedirects: Bool { domain.followRedirects }
    public var maximumRedirects: UInt8 { domain.maximumRedirects }
    public var clientCertificateReference: String? { domain.clientCertificateReference }
    public var customCAPath: String? { domain.customCaPath }
    enum CodingKeys: String, CodingKey {
        case method, url, query, headers, authentication, body, variables
        case appProxy = "app_proxy"
        case workspaceProxy = "workspace_proxy"
        case requestProxy = "request_proxy"
        case totalTimeoutMS = "total_timeout_ms"
        case readTimeoutMS = "read_timeout_ms"
        case maxResponseBytes = "max_response_bytes"
        case validateTLS = "validate_tls"
        case followRedirects = "follow_redirects"
        case maximumRedirects = "maximum_redirects"
        case clientCertificateReference = "client_certificate_reference"
        case customCAPath = "custom_ca_path"
    }

    public init(draft: RequestDraft, variables: [String: ValueSource], workspaceProxy: ProxyDocument? = nil, appProxy: ProxyDocument? = nil) {
        domain = domainRunInput(draft: draft.domain, variables: variables.mapValues(\.domain), workspaceProxy: workspaceProxy?.domain, appProxy: appProxy?.domain)
    }
    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(method, forKey: .method)
        try container.encode(url, forKey: .url)
        try container.encode(query, forKey: .query)
        try container.encode(headers, forKey: .headers)
        try container.encode(authentication, forKey: .authentication)
        try container.encode(body, forKey: .body)
        try container.encode(variables, forKey: .variables)
        try container.encodeIfPresent(appProxy, forKey: .appProxy)
        try container.encodeIfPresent(workspaceProxy, forKey: .workspaceProxy)
        try container.encodeIfPresent(requestProxy, forKey: .requestProxy)
        try container.encode(totalTimeoutMS, forKey: .totalTimeoutMS)
        try container.encode(readTimeoutMS, forKey: .readTimeoutMS)
        try container.encodeIfPresent(maxResponseBytes, forKey: .maxResponseBytes)
        try container.encode(validateTLS, forKey: .validateTLS)
        try container.encode(followRedirects, forKey: .followRedirects)
        try container.encode(maximumRedirects, forKey: .maximumRedirects)
        try container.encodeIfPresent(clientCertificateReference, forKey: .clientCertificateReference)
        try container.encodeIfPresent(customCAPath, forKey: .customCAPath)
    }
}

public enum ProxyDocument: Codable, Hashable, Sendable {
    case system
    case direct
    case manual(routes: [ProxyRouteDocument])

    private enum CodingKeys: String, CodingKey { case mode, routes }
    private enum Mode: String, Codable { case system, direct, manual }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(Mode.self, forKey: .mode) {
        case .system: self = .system
        case .direct: self = .direct
        case .manual:
            self = try .manual(routes: container.decode([ProxyRouteDocument].self, forKey: .routes))
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .system: try container.encode(Mode.system, forKey: .mode)
        case .direct: try container.encode(Mode.direct, forKey: .mode)
        case let .manual(routes):
            try container.encode(Mode.manual, forKey: .mode)
            try container.encode(routes, forKey: .routes)
        }
    }
}

public struct ProxyRouteDocument: Codable, Hashable, Sendable {
    var domain: DomainProxyRoute = DomainProxyRoute(destination: "", endpoint: "", credentials: nil)
    init(domain: DomainProxyRoute) { self.domain = domain }

    public var destination: String {
        get { domain.destination }
        set { domain.destination = newValue }
    }
    public var endpoint: String {
        get { domain.endpoint }
        set { domain.endpoint = newValue }
    }
    public var credentials: ProxyCredentialsDocument? {
        get { domain.credentials.map { ProxyCredentialsDocument(domain: $0) } }
        set { domain.credentials = newValue.map { $0.domain } }
    }
    public init(
        destination: String,
        endpoint: String,
        credentials: ProxyCredentialsDocument? = nil
    ) {
        self.destination = destination
        self.endpoint = endpoint
        self.credentials = credentials
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        destination = try c.decode(String.self, forKey: .destination)
        endpoint = try c.decode(String.self, forKey: .endpoint)
        credentials = try c.decodeIfPresent(ProxyCredentialsDocument.self, forKey: .credentials)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(destination, forKey: .destination)
        try c.encode(endpoint, forKey: .endpoint)
        try c.encodeIfPresent(credentials, forKey: .credentials)
    }

    private enum CodingKeys: String, CodingKey { case destination, endpoint, credentials }
}

public struct ProxyCredentialsDocument: Codable, Hashable, Sendable {
    var domain: DomainProxyCredentials = DomainProxyCredentials(username: "", password: "")
    init(domain: DomainProxyCredentials) { self.domain = domain }

    public var username: String {
        get { domain.username }
        set { domain.username = newValue }
    }
    public var password: String {
        get { domain.password }
        set { domain.password = newValue }
    }
    public init(username: String, password: String) {
        self.username = username
        self.password = password
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        username = try c.decode(String.self, forKey: .username)
        password = try c.decode(String.self, forKey: .password)
    }

    public func encode(to encoder: any Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(username, forKey: .username)
        try c.encode(password, forKey: .password)
    }

    private enum CodingKeys: String, CodingKey { case username, password }
}

public extension RequestField {
    /// Bulk editing changes values, not the credential storage or redaction policy.
    static func parseBulk(_ source: String, preserving previous: [RequestField]) -> [RequestField] {
        domainParseBulk(source: source, previous: previous.map(\.domain)).map(RequestField.init(domain:))
    }
}

/// Runtime cookie updates are deliberately separate from persisted response heads.
public struct ResponseCookies: Decodable, Equatable, Sendable {
    public let url: String
    public let headers: [ResponseHeader]
}

extension ValueSource {
    var domain: DomainValueSource {
        switch self {
        case let .literal(v0): .literal(value: v0)
        case let .secret(v0): .secret(secret: v0)
        }
    }
    init(domain: DomainValueSource) {
        self = switch domain {
        case let .literal(v0): .literal(v0)
        case let .secret(v0): .secret(v0)
        }
    }
}

extension APIKeyPlacement {
    var domain: DomainApiKeyPlacement {
        switch self {
        case .header: .header
        case .query: .query
        }
    }
    init(domain: DomainApiKeyPlacement) {
        self = switch domain {
        case .header: .header
        case .query: .query
        }
    }
}

extension OAuth2Grant {
    var domain: DomainOAuthGrant {
        switch self {
        case .authorizationCodePKCE: .authorizationCodePkce
        case .clientCredentials: .clientCredentials
        }
    }
    init(domain: DomainOAuthGrant) {
        self = switch domain {
        case .authorizationCodePkce: .authorizationCodePKCE
        case .clientCredentials: .clientCredentials
        }
    }
}

extension RequestAuthentication {
    var domain: DomainAuthentication {
        switch self {
        case .none: .none
        case let .basic(v0, v1): .basic(username: v0.domain, password: v1.domain)
        case let .bearer(v0): .bearer(token: v0.domain)
        case let .apiKey(v0, v1, v2): .apiKey(placement: v0.domain, name: v1, value: v2.domain)
        case let .oauth2(v0): .oauth2(configuration: v0.domain)
        }
    }
    init(domain: DomainAuthentication) {
        self = switch domain {
        case .none: .none
        case let .basic(v0, v1): .basic(username: ValueSource(domain: v0), password: ValueSource(domain: v1))
        case let .bearer(v0): .bearer(token: ValueSource(domain: v0))
        case let .apiKey(v0, v1, v2): .apiKey(placement: APIKeyPlacement(domain: v0), name: v1, value: ValueSource(domain: v2))
        case let .oauth2(v0): .oauth2(configuration: OAuth2Configuration(domain: v0))
        }
    }
}

extension RequestBody {
    var domain: DomainBody {
        switch self {
        case .empty: .empty
        case let .text(v0, v1): .text(contentType: v0.map { $0 }, value: v1)
        case let .json(v0): .json(value: v0)
        case let .xml(v0): .xml(value: v0)
        case let .html(v0): .html(value: v0)
        case let .raw(v0, v1): .raw(contentType: v0.map { $0 }, value: v1)
        case let .formURLEncoded(v0): .formUrlEncoded(fields: v0.map { $0.domain })
        case let .multipart(v0): .multipart(parts: v0.map { $0.domain })
        case let .file(v0, v1): .file(path: v0, contentType: v1.map { $0 })
        }
    }
    init(domain: DomainBody) {
        self = switch domain {
        case .empty: .empty
        case let .text(v0, v1): .text(contentType: v0.map { $0 }, value: v1)
        case let .json(v0): .json(value: v0)
        case let .xml(v0): .xml(value: v0)
        case let .html(v0): .html(value: v0)
        case let .raw(v0, v1): .raw(contentType: v0.map { $0 }, value: v1)
        case let .formUrlEncoded(v0): .formURLEncoded(fields: v0.map { RequestField(domain: $0) })
        case let .multipart(v0): .multipart(parts: v0.map { MultipartPart(domain: $0) })
        case let .file(v0, v1): .file(path: v0, contentType: v1.map { $0 })
        }
    }
}

extension MultipartPartKind {
    var domain: DomainMultipartKind {
        switch self {
        case .text: .text
        case .binary: .binary
        case .file: .file
        }
    }
    init(domain: DomainMultipartKind) {
        self = switch domain {
        case .text: .text
        case .binary: .binary
        case .file: .file
        }
    }
}

extension ProxyDocument {
    var domain: DomainProxy {
        switch self {
        case .system: .system
        case .direct: .direct
        case let .manual(v0): .manual(routes: v0.map { $0.domain })
        }
    }
    init(domain: DomainProxy) {
        self = switch domain {
        case .system: .system
        case .direct: .direct
        case let .manual(v0): .manual(routes: v0.map { ProxyRouteDocument(domain: $0) })
        }
    }
}
