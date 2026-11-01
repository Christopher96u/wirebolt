import Foundation

public struct HTTPMethod: RawRepresentable, CaseIterable, Hashable, Sendable {
    public let rawValue: String

    public init?(rawValue: String) {
        guard let normalized = domainNormalizeMethod(value: rawValue) else { return nil }
        self.rawValue = normalized
    }

    init(domainValue: String) { rawValue = domainValue }

    private init(_ value: String) {
        rawValue = value
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

public enum ValueSource: Equatable, Hashable, Sendable {
    case literal(String)
    case secret(String)

    public var editableValue: String {
        switch self {
        case .literal(let value), .secret(let value): value
        }
    }
}

public struct RequestField: Identifiable, Equatable, Sendable {
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

}

public enum APIKeyPlacement: String, CaseIterable, Sendable {
    case header
    case query
}

public enum OAuth2Grant: String, CaseIterable, Sendable {
    case authorizationCodePKCE = "authorization_code_pkce"
    case clientCredentials = "client_credentials"
}

public struct OAuth2Configuration: Equatable, Sendable {
    var domain: DomainOAuthConfiguration = DomainOAuthConfiguration(
        grant: .authorizationCodePkce, authorizationUrl: "", tokenUrl: "", clientId: "", clientSecretReference: "", scopes: "",
        audience: "", redirectUri: "", accessTokenReference: "")
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

}

public enum RequestAuthentication: Equatable, Sendable {
    case none
    case basic(username: ValueSource, password: ValueSource)
    case bearer(token: ValueSource)
    case apiKey(placement: APIKeyPlacement, name: String, value: ValueSource)
    case oauth2(configuration: OAuth2Configuration)

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

extension ValueSource {
    public var secretName: String? {
        if case .secret(let name) = self { name } else { nil }
    }
}

extension RequestAuthentication {
    /// Keychain names this authentication reads or writes.
    public var secretReferences: [String] {
        domainAuthReferences(authentication: domain)
    }

    /// The same authentication with each Keychain name passed through `rename`.
    public func renamingSecrets(_ rename: (String) -> String) -> RequestAuthentication {
        let names = Dictionary(secretReferences.map { ($0, rename($0)) }, uniquingKeysWith: { first, _ in first })
        return RequestAuthentication(domain: domainRenameAuth(authentication: domain, names: names))
    }

    /// Fresh per-request names for every credential, for a copy of a request (`all`) or for
    /// migrating legacy shared names only. Returns the new authentication and old → new names.
    public func withOwnKeychainNames(onlyLegacy: Bool) -> (RequestAuthentication, [String: String]) {
        let result = domainOwnAuth(authentication: domain, onlyLegacy: onlyLegacy)
        return (RequestAuthentication(domain: result.authentication), result.renames)
    }
}

public enum AuthenticationKind: CaseIterable, Sendable {
    case none, basic, bearer, apiKey, oauth2
}

public extension RequestAuthentication {
    var kind: AuthenticationKind {
        AuthenticationKind(domain: domainAuthKind(authentication: domain))
    }

    /// An empty authentication of `kind` whose secrets use fresh Keychain names.
    static func new(_ kind: AuthenticationKind) -> RequestAuthentication {
        RequestAuthentication(domain: domainNewAuth(kind: kind.domain))
    }
}

public enum RequestBody: Equatable, Sendable {
    case empty
    case text(contentType: String?, value: String)
    case json(value: String)
    case xml(value: String)
    case html(value: String)
    case raw(contentType: String?, value: String)
    case formURLEncoded(fields: [RequestField])
    case multipart(parts: [MultipartPart])
    case file(path: String, contentType: String?)

}

public enum MultipartPartKind: String, CaseIterable, Sendable {
    case text
    case binary
    case file
}

public struct MultipartPart: Identifiable, Equatable, Sendable {
    var domain: DomainMultipartPart = DomainMultipartPart(
        id: "", name: "", kind: .text, value: .literal(value: ""), filePath: nil, fileName: nil, contentType: nil, enabled: false)
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

}

public enum ProxySelection: Hashable, Sendable {
    case inherit
    case system
    case direct
    case manual(ProxyDocument)
}

public struct TransportSettings: Equatable, Sendable {
    var domain: DomainTransport = DomainTransport(
        validateTls: false, followRedirects: false, maximumRedirects: 0, totalTimeoutMs: 0, readTimeoutMs: 0,
        clientCertificateReference: nil, customCaPath: nil)
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

}

public struct RequestDraft: Equatable, Sendable {
    private var displayCache: UrlQueryEdit?
    public static func == (lhs: Self, rhs: Self) -> Bool { lhs.domain == rhs.domain }
    var domain: DomainRequestDraft = DomainRequestDraft(
        id: "", name: "", method: "", url: "", webSocket: false, note: "", query: [], headers: [], authentication: .none, body: .empty,
        proxy: nil,
        transport: DomainTransport(
            validateTls: false, followRedirects: false, maximumRedirects: 0, totalTimeoutMs: 0, readTimeoutMs: 0,
            clientCertificateReference: nil, customCaPath: nil), inheritsWorkspaceTransport: false)
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

extension RequestDraft {
    public var separatingURLQuery: RequestDraft {
        var result = self
        result.apply(domainSeparateUrlQuery(url: domain.url, query: domain.query))
        return result
    }

    /// The URL field combines the base URL and the editable query table. Transport
    /// receives them separately so each query parameter is encoded exactly once.
    public var displayURL: String {
        if let cache = displayCache, cache.url == domain.url, cache.query == domain.query { return cache.displayUrl }
        return domainDisplayUrl(url: domain.url, query: domain.query)
    }

    public mutating func editURL(_ text: String) {
        apply(domainEditUrl(url: domain.url, query: domain.query, text: text))
    }

    private mutating func apply(_ edit: UrlQueryEdit) {
        domain.url = edit.url
        domain.query = edit.query
        displayCache = edit
    }
}

public struct EnvironmentVariableDraft: Identifiable, Equatable, Sendable {
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

}

extension EnvironmentVariableDraft {
    public var isSecret: Bool {
        if case .secret = value { true } else { false }
    }

    /// A new Keychain name for this variable's secret value. It is stored in the variable, so
    /// renaming the key keeps its value, and it is unique, so no other environment,
    /// workspace or Mac user of the same Keychain shares it.
    public func makeSecretReference(environmentID: String) -> String {
        CredentialReference.unique(role: environmentID, scope: "env")
    }
}

extension EnvironmentDraft {
    public var secretReferences: Set<String> { Set(variables.compactMap(\.value.secretName)) }
}

extension RequestDraft {
    /// Keychain references are collected in Rust, with no secret material.
    public var secretReferences: Set<String> { Set(domainRequestReferences(draft: domain)) }
}

extension WorkspaceDraft {
    public var secretReferences: Set<String> { Set(domainWorkspaceReferences(workspace: domain)) }
}

public struct EnvironmentDraft: Identifiable, Equatable, Sendable {
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
        set {
            projectedVariables = newValue
            domain.variables = newValue.map(\.domain)
        }
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

}

public struct RequestLocation: Identifiable, Equatable, Sendable {
    var domain: DomainLocation = DomainLocation(
        collectionId: "", groupId: nil, order: 0,
        request: DomainRequestDraft(
            id: "", name: "", method: "", url: "", webSocket: false, note: "", query: [], headers: [], authentication: .none, body: .empty,
            proxy: nil,
            transport: DomainTransport(
                validateTls: false, followRedirects: false, maximumRedirects: 0, totalTimeoutMs: 0, readTimeoutMs: 0,
                clientCertificateReference: nil, customCaPath: nil), inheritsWorkspaceTransport: false))
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

public struct GroupDraft: Identifiable, Equatable, Sendable {
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
        set {
            projectedGroups = newValue
            domain.groups = newValue.map(\.domain)
        }
    }
    public var requests: [RequestLocation] {
        get { projectedRequests }
        set {
            projectedRequests = newValue
            domain.requests = newValue.map(\.domain)
        }
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

public typealias WorkspaceNodeKind = SessionWorkspaceNodeKind

public struct WorkspaceDelta: Equatable, Sendable {
    public let version: UInt64
    public let kind: WorkspaceNodeKind
    public let affectedIDs: [String]

    public init(version: UInt64, kind: WorkspaceNodeKind, affectedIDs: [String]) {
        self.version = version
        self.kind = kind
        self.affectedIDs = affectedIDs
    }

}

public typealias ImportFormat = DomainImportFormat

/// What an import created and everything it could not carry over exactly.
public struct ImportSummary: Equatable, Identifiable, Sendable {
    public struct Environment: Equatable, Sendable {
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

    /// Warnings or new environments deserve a closer look than a transient banner.
    public var needsReview: Bool { !warnings.isEmpty || !environments.isEmpty }

    /// One sentence describing what was imported, such as
    /// "Imported 9 requests in 3 folders into “Shop API”."
    public var headline: String {
        domainImportHeadline(names: collectionNames, requests: Int64(requestCount), groups: Int64(groupCount))
    }

    /// Describes the environments the import created, if any.
    public var environmentLine: String? {
        domainImportEnvironmentLine(
            names: environments.map(\.name), localizedList: ListFormatter.localizedString(byJoining: environments.map { "“\($0.name)”" }))
    }

}

/// A typed import result supplied by an injected workspace store.
public struct ImportResult: Equatable, Sendable {
    public let delta: WorkspaceDelta
    public let summary: ImportSummary

    public init(delta: WorkspaceDelta, summary: ImportSummary) {
        self.delta = delta
        self.summary = summary
    }

}

public struct WorkspaceDraft: Equatable, Sendable {
    var domain: DomainWorkspace = DomainWorkspace(
        name: "", proxy: nil,
        transport: DomainTransport(
            validateTls: false, followRedirects: false, maximumRedirects: 0, totalTimeoutMs: 0, readTimeoutMs: 0,
            clientCertificateReference: nil, customCaPath: nil), collections: [], environments: [])
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
        set {
            projectedCollections = newValue
            domain.collections = newValue.map(\.domain)
        }
    }
    public var environments: [EnvironmentDraft] {
        get { projectedEnvironments }
        set {
            projectedEnvironments = newValue
            domain.environments = newValue.map(\.domain)
        }
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

extension CollectionDraft {
    public func descendantGroupIDs(of rootID: String) -> Set<String> {
        Set(domainSubtree(collection: domain, root: rootID).map { groups[Int($0)].id }).union([rootID])
    }
}

public typealias GitDeltaSnapshot = WorkspaceGitDelta
public typealias GitOperationOutcome = WorkspaceGitOutcome

public struct GitChangeSnapshot: Equatable, Identifiable, Sendable {
    public var id: String { previousPath.map { "\($0)\u{0}\(path)" } ?? path }
    public let path: String
    public let previousPath: String?
    public let staged: GitDeltaSnapshot
    public let unstaged: GitDeltaSnapshot
    public let conflicted: Bool

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
public typealias GitChangeKind = DomainGitChangeKind
extension GitChangeKind {
    public var rawValue: String { domainGitChangeTitle(kind: self) }
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
        case .publish(let branch): .publish(branch: branch)
        case .unavailable(let reason): .unavailable(reason: reason)
        }
    }
}

extension GitChangeSnapshot {
    public var kind: GitChangeKind {
        sessionGitChangeKind(staged: staged, unstaged: unstaged, conflicted: conflicted)
    }
}

public struct GitStatusSnapshot: Equatable, Sendable {
    public let branch: String?
    public let upstream: String?
    public let ahead: UInt64
    public let behind: UInt64
    public let changes: [GitChangeSnapshot]
    /// A pull stopped mid-merge; Abort Merge returns to the state before the pull.
    public let merging: Bool

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

    public var conflictedChanges: [GitChangeSnapshot] {
        changes.filter { $0.kind == .conflicted }
    }
}

public struct GitOperationSnapshot: Equatable, Sendable {
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

    public init?(kind: WorkspaceFailureKind) {
        switch workspaceRepositoryProblem(kind: kind) {
        case .notRepository: self = .notRepository
        case .notRepositoryRoot: self = .notRepositoryRoot
        case .gitUnavailable: self = .gitUnavailable
        case nil: return nil
        }
    }
}

public struct GitFailure: Error, Equatable, Sendable {
    public let kind: WorkspaceFailureKind
    public let reason: String

    public init(kind: WorkspaceFailureKind, reason: String) {
        self.kind = kind
        self.reason = reason
    }
}

public struct ResponseHeader: Equatable, Identifiable, Sendable {
    public let name: String
    public let value: String
    public var id: String { name + "\u{0}" + value }
}

public struct ResponseHead: Equatable, Sendable {
    public let status: UInt16
    public let version: String
    public let headers: [ResponseHeader]
    public let timeToHeadersNS: UInt64

}

public struct RunCompletion: Equatable, Sendable {
    public let bytesReceived: UInt64
    public let totalTimeNS: UInt64

}

public struct RequestIssue: Equatable, Sendable {
    public let path: String
    public let kind: String
    public let reference: String?
}

public struct RunFailure: Error, Equatable, Sendable {
    public let kind: ResponseFailureKind
    public let issues: [RequestIssue]
}

public struct PreparedHeaderSnapshot: Equatable, Sendable {
    public let name: String
    public let value: String
    public let redacted: Bool
}

public struct PreparedBodySnapshot: Equatable, Sendable {
    public let byteCount: UInt64
    public let contentType: String?
    public let textPreview: String?
    public let redacted: Bool

}

public struct PreparedRunSnapshot: Equatable, Sendable {
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

public struct RunInput: Sendable {
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

    init(domain: DomainRunInput) { self.domain = domain }

    public init(draft: RequestDraft, variables: [String: ValueSource], workspaceProxy: ProxyDocument? = nil, appProxy: ProxyDocument? = nil)
    {
        domain = domainRunInput(
            draft: draft.domain, variables: variables.mapValues(\.domain), workspaceProxy: workspaceProxy?.domain,
            appProxy: appProxy?.domain)
    }

}

public enum ProxyDocument: Hashable, Sendable {
    case system
    case direct
    case manual(routes: [ProxyRouteDocument])

}

public struct ProxyRouteDocument: Hashable, Sendable {
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

}

public struct ProxyCredentialsDocument: Hashable, Sendable {
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

}

extension RequestField {
    /// Bulk editing changes values, not the credential storage or redaction policy.
    public static func parseBulk(_ source: String, preserving previous: [RequestField]) -> [RequestField] {
        domainParseBulk(source: source, previous: previous.map(\.domain)).map(RequestField.init(domain:))
    }
}

/// Runtime cookie updates are deliberately separate from persisted response heads.
public struct ResponseCookies: Equatable, Sendable {
    public let url: String
    public let headers: [ResponseHeader]
}

extension ValueSource {
    var domain: DomainValueSource {
        switch self {
        case .literal(let v0): .literal(value: v0)
        case .secret(let v0): .secret(secret: v0)
        }
    }
    init(domain: DomainValueSource) {
        self =
            switch domain {
            case .literal(let v0): .literal(v0)
            case .secret(let v0): .secret(v0)
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
        self =
            switch domain {
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
        self =
            switch domain {
            case .authorizationCodePkce: .authorizationCodePKCE
            case .clientCredentials: .clientCredentials
            }
    }
}

extension RequestAuthentication {
    var domain: DomainAuthentication {
        switch self {
        case .none: .none
        case .basic(let v0, let v1): .basic(username: v0.domain, password: v1.domain)
        case .bearer(let v0): .bearer(token: v0.domain)
        case .apiKey(let v0, let v1, let v2): .apiKey(placement: v0.domain, name: v1, value: v2.domain)
        case .oauth2(let v0): .oauth2(configuration: v0.domain)
        }
    }
    init(domain: DomainAuthentication) {
        self =
            switch domain {
            case .none: .none
            case .basic(let v0, let v1): .basic(username: ValueSource(domain: v0), password: ValueSource(domain: v1))
            case .bearer(let v0): .bearer(token: ValueSource(domain: v0))
            case .apiKey(let v0, let v1, let v2): .apiKey(placement: APIKeyPlacement(domain: v0), name: v1, value: ValueSource(domain: v2))
            case .oauth2(let v0): .oauth2(configuration: OAuth2Configuration(domain: v0))
            }
    }
}

extension RequestBody {
    var domain: DomainBody {
        switch self {
        case .empty: .empty
        case .text(let v0, let v1): .text(contentType: v0.map { $0 }, value: v1)
        case .json(let v0): .json(value: v0)
        case .xml(let v0): .xml(value: v0)
        case .html(let v0): .html(value: v0)
        case .raw(let v0, let v1): .raw(contentType: v0.map { $0 }, value: v1)
        case .formURLEncoded(let v0): .formUrlEncoded(fields: v0.map { $0.domain })
        case .multipart(let v0): .multipart(parts: v0.map { $0.domain })
        case .file(let v0, let v1): .file(path: v0, contentType: v1.map { $0 })
        }
    }
    init(domain: DomainBody) {
        self =
            switch domain {
            case .empty: .empty
            case .text(let v0, let v1): .text(contentType: v0.map { $0 }, value: v1)
            case .json(let v0): .json(value: v0)
            case .xml(let v0): .xml(value: v0)
            case .html(let v0): .html(value: v0)
            case .raw(let v0, let v1): .raw(contentType: v0.map { $0 }, value: v1)
            case .formUrlEncoded(let v0): .formURLEncoded(fields: v0.map { RequestField(domain: $0) })
            case .multipart(let v0): .multipart(parts: v0.map { MultipartPart(domain: $0) })
            case .file(let v0, let v1): .file(path: v0, contentType: v1.map { $0 })
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
        self =
            switch domain {
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
        case .manual(let v0): .manual(routes: v0.map { $0.domain })
        }
    }
    init(domain: DomainProxy) {
        self =
            switch domain {
            case .system: .system
            case .direct: .direct
            case .manual(let v0): .manual(routes: v0.map { ProxyRouteDocument(domain: $0) })
            }
    }
}

extension AuthenticationKind {
    var domain: DomainAuthenticationKind {
        switch self {
        case .none: .none
        case .basic: .basic
        case .bearer: .bearer
        case .apiKey: .apiKey
        case .oauth2: .oauth2
        }
    }
    init(domain: DomainAuthenticationKind) {
        self = switch domain {
        case .none: .none
        case .basic: .basic
        case .bearer: .bearer
        case .apiKey: .apiKey
        case .oauth2: .oauth2
        }
    }
}

public extension RequestBody {
    var composerKind: DomainBodyKind { domainBodyKind(body: domainKind) }
    var canFormat: Bool { domainBodyCanFormat(kind: domainKind) }
    func switching(to kind: DomainBodyKind) -> RequestBody {
        RequestBody(domain: domainSwitchBody(body: domain, kind: kind))
    }
    func formatted() -> RequestBody {
        RequestBody(domain: domainFormatBody(body: domain, ordering: NativeBodyKeyOrdering()))
    }
}

private final class NativeBodyKeyOrdering: AppBodyKeyOrdering, Sendable {
    func sortedKeys(keys: [String]) -> [String] {
        keys.sorted {
            $0.compare($1, options: [.numeric, .caseInsensitive, .forcedOrdering], locale: NSLocale.system)
                == .orderedAscending
        }
    }
}

private extension RequestBody {
    var domainKind: DomainBodyKind {
        switch self {
        case .empty: .empty
        case .text: .text
        case .json: .json
        case .xml: .xml
        case .html: .html
        case .raw: .raw
        case .formURLEncoded: .form
        case .multipart: .multipart
        case .file: .file
        }
    }
}
