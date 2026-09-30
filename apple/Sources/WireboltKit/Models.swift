import Foundation

public struct HTTPMethod: RawRepresentable, CaseIterable, Codable, Hashable, Sendable {
    public let rawValue: String

    public init?(rawValue: String) {
        let normalized = rawValue.trimmingCharacters(in: .whitespacesAndNewlines).uppercased()
        guard normalized.isEmpty == false,
              normalized.utf8.allSatisfy({ byte in
                  (65 ... 90).contains(byte) || (48 ... 57).contains(byte) || byte == 45
              })
        else { return nil }
        self.rawValue = normalized
    }

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
    public static let allCases: [HTTPMethod] = [.get, .post, .put, .patch, .delete, .head, .options]
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
    public var id: String
    public var name: String
    public var value: ValueSource
    public var enabled: Bool
    public var sensitive: Bool

    enum CodingKeys: String, CodingKey { case id, name, value, enabled, sensitive }

    public init(
        id: String = UUID().uuidString.lowercased(),
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
            ?? UUID().uuidString.lowercased()
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
    public var grant: OAuth2Grant
    public var authorizationURL: String
    public var tokenURL: String
    public var clientID: String
    public var clientSecretReference: String
    public var scopes: String
    public var audience: String
    public var redirectURI: String
    public var accessTokenReference: String

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
        String("\(scope).\(UUID().uuidString.lowercased()).\(sanitized(role))".prefix(128))
    }

    /// Names earlier versions gave every API Key and OAuth request. Other workspaces may still
    /// use them, so Wirebolt copies their values to per-request names and never deletes them.
    public static let legacySharedNames: Set<String> = ["auth.api-key", "oauth.client-secret", "oauth.access-token"]

    public static func isLegacyShared(_ name: String) -> Bool { legacySharedNames.contains(name) }

    /// Whether Wirebolt generated `name` for exactly one request or variable, so the Keychain
    /// item can be deleted once nothing references it. Names people chose (such as proxy
    /// credentials or `{{secret}}` references) and legacy shared names never qualify.
    public static func isOwned(_ name: String) -> Bool {
        name.wholeMatch(of: ownedPattern) != nil
    }

    // `auth.<uuid>.<role>` and `env.<uuid>.<environment>` from this version, the uppercase
    // `auth.<UUID>.<role>` and `request.<uuid>.<role>` from earlier ones, and names created
    // by an import (`import-<time>-<process>-<sequence>-request-<n>-<role>`).
    private nonisolated(unsafe) static let ownedPattern =
        /(?:(?:auth|env|request)\.[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\.[A-Za-z0-9._-]+)|(?:import-[0-9a-f]+-[0-9a-f]+-[0-9a-f]+-request-[0-9]+-[a-z-]+)/

    static func sanitized(_ text: String) -> String {
        let allowed = Set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-_")
        let clean = String(text.map { allowed.contains($0) ? $0 : "-" })
        return clean.isEmpty ? "value" : clean
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
        switch self {
        case .none: []
        case let .basic(username, password): [username.secretName, password.secretName].compactMap { $0 }
        case let .bearer(token): [token.secretName].compactMap { $0 }
        case let .apiKey(_, _, value): [value.secretName].compactMap { $0 }
        case let .oauth2(configuration):
            [configuration.clientSecretReference, configuration.accessTokenReference].filter { !$0.isEmpty }
        }
    }

    /// The same authentication with each Keychain name passed through `rename`.
    func renamingSecrets(_ rename: (String) -> String) -> RequestAuthentication {
        func renamed(_ source: ValueSource) -> ValueSource {
            source.secretName.map { .secret(rename($0)) } ?? source
        }
        switch self {
        case .none: return .none
        case let .basic(username, password): return .basic(username: renamed(username), password: renamed(password))
        case let .bearer(token): return .bearer(token: renamed(token))
        case let .apiKey(placement, name, value): return .apiKey(placement: placement, name: name, value: renamed(value))
        case var .oauth2(configuration):
            if !configuration.clientSecretReference.isEmpty {
                configuration.clientSecretReference = rename(configuration.clientSecretReference)
            }
            if !configuration.accessTokenReference.isEmpty {
                configuration.accessTokenReference = rename(configuration.accessTokenReference)
            }
            return .oauth2(configuration: configuration)
        }
    }

    /// Fresh per-request names for every credential, for a copy of a request (`all`) or for
    /// migrating legacy shared names only. Returns the new authentication and old → new names.
    func withOwnKeychainNames(onlyLegacy: Bool) -> (RequestAuthentication, [String: String]) {
        var renames: [String: String] = [:]
        let authentication = renamingSecrets { name in
            guard !onlyLegacy || CredentialReference.isLegacyShared(name) else { return name }
            if let existing = renames[name] { return existing }
            let role = name.contains(".") ? String(name.split(separator: ".").last ?? "secret") : "secret"
            let fresh = CredentialReference.unique(role: role)
            renames[name] = fresh
            return fresh
        }
        return (authentication, renames)
    }
}

public enum AuthenticationKind: CaseIterable, Sendable {
    case none, basic, bearer, apiKey, oauth2
}

public extension RequestAuthentication {
    var kind: AuthenticationKind {
        switch self {
        case .none: .none
        case .basic: .basic
        case .bearer: .bearer
        case .apiKey: .apiKey
        case .oauth2: .oauth2
        }
    }

    /// An empty authentication of `kind` whose secrets use fresh Keychain names.
    static func new(_ kind: AuthenticationKind) -> RequestAuthentication {
        switch kind {
        case .none: .none
        case .basic: .basic(username: .literal(""), password: .secret(CredentialReference.unique(role: "password")))
        case .bearer: .bearer(token: .secret(CredentialReference.unique(role: "token")))
        case .apiKey:
            .apiKey(placement: .header, name: "X-API-Key", value: .secret(CredentialReference.unique(role: "api-key")))
        case .oauth2:
            .oauth2(configuration: OAuth2Configuration())
        }
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
    public var id: String
    public var name: String
    public var kind: MultipartPartKind
    public var value: ValueSource
    public var filePath: String?
    public var fileName: String?
    public var contentType: String?
    public var enabled: Bool

    public init(
        id: String = UUID().uuidString.lowercased(),
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
}

public enum ProxySelection: Hashable, Sendable {
    case inherit
    case system
    case direct
    case manual(ProxyDocument)
}

public struct TransportSettings: Codable, Equatable, Sendable {
    public var validateTLS: Bool
    public var followRedirects: Bool
    public var maximumRedirects: Int
    public var totalTimeoutMS: UInt64
    public var readTimeoutMS: UInt64
    public var clientCertificateReference: String?
    public var customCAPath: String?

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
        self.maximumRedirects = min(max(maximumRedirects, 0), 10)
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
}

public struct RequestDraft: Equatable, Sendable {
    public var id: String
    public var name: String
    public var method: HTTPMethod
    public var url: String
    public var webSocket: Bool
    public var note: String
    public var query: [RequestField]
    public var headers: [RequestField]
    public var authentication: RequestAuthentication
    public var body: RequestBody
    public var proxy: ProxySelection
    public var transport: TransportSettings
    public var inheritsWorkspaceTransport: Bool

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
        let explicitFields = query
        result.query = []
        result.editURL(url)
        result.query += explicitFields
        return result
    }

    /// The URL field combines the base URL and the editable query table. Transport
    /// receives them separately so each query parameter is encoded exactly once.
    var displayURL: String {
        let enabled = query.filter(\.enabled)
        guard !enabled.isEmpty else { return url }
        var components = URLComponents()
        components.queryItems = enabled.map { URLQueryItem(name: $0.name, value: $0.value.editableValue) }
        let fragment = url.firstIndex(of: "#") ?? url.endIndex
        let prefix = String(url[..<fragment])
        let separator = prefix.contains("?") ? "&" : "?"
        return prefix + separator + (components.percentEncodedQuery ?? "") + url[fragment...]
    }

    mutating func editURL(_ text: String) {
        let fragment = text.firstIndex(of: "#") ?? text.endIndex
        let prefix = text[..<fragment]
        guard let start = prefix.firstIndex(of: "?") else {
            url = text
            query.removeAll(where: \.enabled)
            return
        }
        let rawQuery = String(prefix[prefix.index(after: start)...])
        var components = URLComponents()
        // URLComponents(string:) accepts partially typed and Unicode query values.
        components = URLComponents(string: "https://query.invalid/?" + rawQuery) ?? components
        guard let items = components.queryItems else { return }
        var previous = query.filter(\.enabled)
        let disabled = query.filter { !$0.enabled }
        query = items.map { item in
            if let index = previous.firstIndex(where: { $0.name == item.name }) {
                var field = previous.remove(at: index)
                if field.value.editableValue != (item.value ?? "") {
                    field.value = .literal(item.value ?? "")
                }
                return field
            }
            return RequestField(name: item.name, value: .literal(item.value ?? ""))
        } + disabled
        url = String(prefix[..<start]) + text[fragment...]
    }
}

public struct EnvironmentVariableDraft: Identifiable, Codable, Equatable, Sendable {
    public var id: String
    public var key: String
    public var value: ValueSource
    public var enabled: Bool
    public var order: Int

    public init(
        id: String = UUID().uuidString.lowercased(),
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
    /// Every Keychain name the request refers to: authentication, fields, bodies and proxy.
    var secretReferences: Set<String> {
        var names = Set(authentication.secretReferences)
        names.formUnion((query + headers).compactMap(\.value.secretName))
        switch body {
        case let .formURLEncoded(fields): names.formUnion(fields.compactMap(\.value.secretName))
        case let .multipart(parts): names.formUnion(parts.compactMap(\.value.secretName))
        default: break
        }
        if case let .manual(.manual(routes)) = proxy {
            names.formUnion(routes.compactMap(\.credentials).flatMap { [$0.username, $0.password] })
        }
        return names
    }
}

public extension WorkspaceDraft {
    var secretReferences: Set<String> {
        var names = Set(environments.flatMap(\.secretReferences))
        for collection in collections {
            for location in collection.requests { names.formUnion(location.request.secretReferences) }
        }
        if case let .manual(routes) = proxy {
            names.formUnion(routes.compactMap(\.credentials).flatMap { [$0.username, $0.password] })
        }
        return names
    }
}

public struct EnvironmentDraft: Identifiable, Codable, Equatable, Sendable {
    public var id: String
    public var name: String
    public var variables: [EnvironmentVariableDraft]

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
        self.id = id
        self.name = name
        variables = legacyVariables.sorted(by: { $0.key < $1.key }).enumerated().map {
            EnvironmentVariableDraft(
                id: "variable-\($0.offset)",
                key: $0.element.key,
                value: $0.element.value,
                order: $0.offset
            )
        }
    }

    public var enabledValues: [String: ValueSource] {
        Dictionary(
            variables
                .filter { $0.enabled && $0.key.isEmpty == false }
                .sorted { ($0.order, $0.key) < ($1.order, $1.key) }
                .map { ($0.key, $0.value) },
            uniquingKeysWith: { _, newest in newest }
        )
    }
}

public struct RequestLocation: Identifiable, Equatable, Sendable {
    public var id: String { "\(collectionID)/\(request.id)" }
    public let collectionID: String
    public var groupID: String?
    public var order: Int
    public var request: RequestDraft

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
    public var id: String
    public var name: String
    public var parentID: String?
    public var order: Int

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
}

public struct CollectionDraft: Identifiable, Equatable, Sendable {
    public var id: String
    public var name: String
    public var order: Int
    public var groups: [GroupDraft]
    public var requests: [RequestLocation]

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
        var text = "Imported \(Self.counted(requestCount, "request"))"
        if groupCount > 0 { text += " in \(Self.counted(groupCount, "folder"))" }
        if collectionNames.count == 1, let name = collectionNames.first {
            text += " into “\(name)”."
        } else if collectionNames.count > 1 {
            text += " into \(collectionNames.count) collections."
        } else {
            text += "."
        }
        return text
    }

    /// Describes the environments the import created, if any.
    public var environmentLine: String? {
        switch environments.count {
        case 0: nil
        case 1: "Created the environment “\(environments[0].name)” for its variables."
        default:
            "Created \(environments.count) environments: "
                + ListFormatter.localizedString(byJoining: environments.map { "“\($0.name)”" }) + "."
        }
    }

    private static func counted(_ count: Int, _ noun: String) -> String {
        "\(count) \(noun)\(count == 1 ? "" : "s")"
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
    public static let globalEnvironmentID = "global"
    public static let rootCollectionID = "workspace-root"
    public var name: String
    public var proxy: ProxyDocument?
    public var transport: TransportSettings
    public var collections: [CollectionDraft]
    public var environments: [EnvironmentDraft]

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
        var result: Set<String> = [rootID]
        var frontier = [rootID]
        while let parent = frontier.popLast() {
            for group in groups where group.parentID == parent && !result.contains(group.id) {
                result.insert(group.id)
                frontier.append(group.id)
            }
        }
        return result
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
        guard let branch else { return .unavailable(reason: "Check out a branch to push. HEAD is detached.") }
        guard upstream != nil else { return .publish(branch: branch) }
        guard ahead > 0 else { return .unavailable(reason: "No local commits to push.") }
        return .push
    }
}

extension GitChangeSnapshot {
    public var kind: GitChangeKind {
        if conflicted { return .conflicted }
        let deltas = [staged, unstaged]
        if deltas.contains(where: { $0 == .unmerged }) { return .conflicted }
        // A file added in the index stays "new" even with further unstaged edits.
        if deltas.contains(where: { $0 == .added || $0 == .untracked }) { return .new }
        if deltas.contains(.deleted) { return .deleted }
        if deltas.contains(.renamed) { return .renamed }
        if deltas.contains(.copied) { return .copied }
        if deltas.contains(.typeChanged) { return .typeChanged }
        return .modified
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
        switch kind {
        case "not_repository": self = .notRepository
        case "workspace_not_repository_root": self = .notRepositoryRoot
        case "git_unavailable": self = .gitUnavailable
        default: return nil
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
    public let method: String
    public let url: String
    public let query: [RequestField]
    public let headers: [RequestField]
    public let authentication: RequestAuthentication
    public let body: RequestBody
    public let variables: [String: ValueSource]
    public let appProxy: ProxyDocument?
    public let workspaceProxy: ProxyDocument?
    public let requestProxy: ProxyDocument?
    public let totalTimeoutMS: UInt64
    public let readTimeoutMS: UInt64
    public let maxResponseBytes: UInt64?
    public let validateTLS: Bool
    public let followRedirects: Bool
    public let maximumRedirects: UInt8
    public let clientCertificateReference: String?
    public let customCAPath: String?

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

    public init(
        draft: RequestDraft,
        variables: [String: ValueSource],
        workspaceProxy: ProxyDocument? = nil,
        appProxy: ProxyDocument? = nil
    ) {
        method = draft.method.rawValue
        url = draft.url
        query = draft.query
        headers = draft.headers
        authentication = draft.authentication
        body = draft.body
        self.variables = variables
        self.appProxy = appProxy
        self.workspaceProxy = workspaceProxy
        requestProxy = switch draft.proxy {
        case .inherit: nil
        case .system: .system
        case .direct: .direct
        case let .manual(document): document
        }
        totalTimeoutMS = draft.transport.totalTimeoutMS
        readTimeoutMS = draft.transport.readTimeoutMS
        maxResponseBytes = nil
        validateTLS = draft.transport.validateTLS
        followRedirects = draft.transport.followRedirects
        maximumRedirects = UInt8(clamping: draft.transport.maximumRedirects)
        clientCertificateReference = draft.transport.clientCertificateReference
        customCAPath = draft.transport.customCAPath
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
    public let destination: String
    public let endpoint: String
    public let credentials: ProxyCredentialsDocument?

    public init(
        destination: String,
        endpoint: String,
        credentials: ProxyCredentialsDocument? = nil
    ) {
        self.destination = destination
        self.endpoint = endpoint
        self.credentials = credentials
    }
}

public struct ProxyCredentialsDocument: Codable, Hashable, Sendable {
    public let username: String
    public let password: String

    public init(username: String, password: String) {
        self.username = username
        self.password = password
    }
}

public extension RequestField {
    /// Bulk editing changes values, not the credential storage or redaction policy.
    static func parseBulk(_ source: String, preserving previous: [RequestField]) -> [RequestField] {
        var unused = previous
        return source.split(separator: "\n", omittingEmptySubsequences: true).compactMap { line in
            var text = String(line)
            let enabled = !text.hasPrefix("#")
            if !enabled { text.removeFirst(); text = text.trimmingCharacters(in: .whitespaces) }
            guard let separator = text.firstIndex(of: ":") else { return nil }
            let name = String(text[..<separator]).trimmingCharacters(in: .whitespaces)
            let value = String(text[text.index(after: separator)...]).trimmingCharacters(in: .whitespaces)
            let match = unused.firstIndex { $0.name == name && $0.value.editableValue == value }
                ?? unused.firstIndex { $0.name == name }
                ?? unused.firstIndex { field in
                    guard field.value.editableValue == value else { return false }
                    if case .secret = field.value { return true }
                    return field.sensitive
                }
            var field = match.map { unused.remove(at: $0) } ?? RequestField()
            field.name = name
            field.enabled = enabled
            if case .secret = field.value { field.value = .secret(value) }
            else { field.value = .literal(value) }
            return field
        }
    }
}

/// Runtime cookie updates are deliberately separate from persisted response heads.
public struct ResponseCookies: Decodable, Equatable, Sendable {
    public let url: String
    public let headers: [ResponseHeader]
}
