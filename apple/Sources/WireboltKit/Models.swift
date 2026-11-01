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

public typealias ValueSource = DomainValueSource

extension ValueSource {
    public static func literal(_ value: String) -> Self { .literal(value: value) }
    public static func secret(_ name: String) -> Self { .secret(secret: name) }
    public var editableValue: String {
        switch self { case .literal(let value), .secret(let value): value }
    }
}

public typealias RequestField = DomainField

extension RequestField: Identifiable {
    public init(id: String = domainUniqueId(), name: String = "", value: ValueSource = .literal(""), sensitive: Bool = false, enabled: Bool = true) {
        self.init(id: id, name: name, value: value, enabled: enabled, sensitive: sensitive)
    }
}

public typealias APIKeyPlacement = ApiKeyPlacement

public typealias OAuth2Grant = Oauth2Grant

extension OAuth2Grant {
    public static let authorizationCodePKCE = Self.authorizationCodePkce
}

public typealias OAuth2Configuration = DomainOAuthConfiguration

extension OAuth2Configuration {
    public var authorizationURL: String { get { authorizationUrl } set { authorizationUrl = newValue } }
    public var tokenURL: String { get { tokenUrl } set { tokenUrl = newValue } }
    public var clientID: String { get { clientId } set { clientId = newValue } }
    public var redirectURI: String { get { redirectUri } set { redirectUri = newValue } }
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
        self.init(grant: grant, authorizationUrl: authorizationURL, tokenUrl: tokenURL, clientId: clientID,
                  clientSecretReference: clientSecretReference, scopes: scopes, audience: audience, redirectUri: redirectURI,
                  accessTokenReference: accessTokenReference)
    }
}

public typealias RequestAuthentication = DomainAuthentication

/// Keychain names for credentials a request stores. Every new credential gets its own
/// name so two requests never read or overwrite each other's secret material.
public enum CredentialReference {
    public static func unique(role: String, scope: String = "auth") -> String {
        domainCredentialReference(role: role, scope: scope)
    }

    /// Names earlier versions gave every API Key and OAuth request. Other workspaces may still
    /// use them, so Wirebolt copies their values to per-request names and never deletes them.
    public static func isLegacyShared(_ name: String) -> Bool { domainLegacyReference(name: name) }

    public static func editableName(for source: ValueSource, role: String) -> String {
        domainEditableCredentialReference(source: source, role: role)
    }

    /// Whether Wirebolt generated `name` for exactly one request or variable, so the Keychain
    /// item can be deleted once nothing references it. Names people chose (such as proxy
    /// credentials or `{{secret}}` references) and legacy shared names never qualify.
    public static func isOwned(_ name: String) -> Bool {
        domainOwnedReference(name: name)
    }

    static func sanitized(_ text: String) -> String {
        domainSanitizeCredentialReference(text: text)
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
        domainAuthReferences(authentication: self)
    }

    /// The same authentication with each Keychain name passed through `rename`.
    public func renamingSecrets(_ rename: (String) -> String) -> RequestAuthentication {
        let names = Dictionary(secretReferences.map { ($0, rename($0)) }, uniquingKeysWith: { first, _ in first })
        return domainRenameAuth(authentication: self, names: names)
    }

    /// Fresh per-request names for every credential, for a copy of a request (`all`) or for
    /// migrating legacy shared names only. Returns the new authentication and old → new names.
    public func withOwnKeychainNames(onlyLegacy: Bool) -> (RequestAuthentication, [String: String]) {
        let result = domainOwnAuth(authentication: self, onlyLegacy: onlyLegacy)
        return (result.authentication, result.renames)
    }
}

public typealias AuthenticationKind = DomainAuthenticationKind

extension DomainAuthenticationKind: CaseIterable {
    public static var allCases: [Self] { [.none, .basic, .bearer, .apiKey, .oauth2] }
}

public extension RequestAuthentication {
    var kind: AuthenticationKind {
        domainAuthKind(authentication: self)
    }

    /// An empty authentication of `kind` whose secrets use fresh Keychain names.
    static func new(_ kind: AuthenticationKind) -> RequestAuthentication {
        domainNewAuth(kind: kind)
    }
}

public typealias RequestBody = DomainBody

public typealias MultipartPart = DomainMultipartPart

extension MultipartPart: Identifiable {
    public init(
        name: String = "", kind: MultipartPartKind = .text, value: ValueSource = .literal(""),
        filePath: String? = nil, fileName: String? = nil, contentType: String? = nil, enabled: Bool = true, id: String = domainUniqueId()
    ) {
        self.init(id: id, name: name, kind: kind, value: value, filePath: filePath, fileName: fileName, contentType: contentType, enabled: enabled)
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
        get { domain.clientCertificateReference }
        set { domain.clientCertificateReference = newValue }
    }
    public var customCAPath: String? {
        get { domain.customCaPath }
        set { domain.customCaPath = newValue }
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
        get { domain.query }
        set { domain.query = newValue }
    }
    public var headers: [RequestField] {
        get { domain.headers }
        set { domain.headers = newValue }
    }
    public var authentication: RequestAuthentication {
        get { domain.authentication }
        set { domain.authentication = newValue }
    }
    public var body: RequestBody {
        get { domain.body }
        set { domain.body = newValue }
    }
    public var proxy: ProxySelection {
        get { ProxySelection(document: domain.proxy) }
        set { domain.proxy = newValue.document }
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
    // Native String equality normalizes Unicode; the Rust wire representation
    // preserves bytes. Only the canonically-equal case needs the codec fallback.
    // This runs on editor mutations, never while projecting unchanged tabs.
    func hasSameBridgeBytes(as other: RequestDraft) -> Bool {
        guard self == other else { return false }
        var left: [UInt8] = []
        var right: [UInt8] = []
        FfiConverterTypeDomainRequestDraft.write(domain, into: &left)
        FfiConverterTypeDomainRequestDraft.write(other.domain, into: &right)
        return left == right
    }

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
        get { domain.value }
        set { domain.value = newValue }
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
        self.init(domain: domainLegacyEnvironment(id: id, name: name, values: legacyVariables))
    }

    public var enabledValues: [String: ValueSource] {
        domainEnabledValues(environment: domain)
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
        get { domain.groupId }
        set { domain.groupId = newValue }
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
        get { domain.parentId }
        set { domain.parentId = newValue }
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
        get { domain.proxy }
        set { domain.proxy = newValue }
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

public typealias GitDeltaSnapshot = GitDelta
public typealias GitOperationOutcome = WorkspaceGitOutcome

public typealias GitChangeSnapshot = WorkspaceGitChange

extension GitChangeSnapshot: Identifiable {
    public var id: String { previousPath.map { "\($0)\u{0}\(path)" } ?? path }
    public init(path: String, staged: GitDeltaSnapshot, unstaged: GitDeltaSnapshot, conflicted: Bool) {
        self.init(path: path, previousPath: nil, staged: staged, unstaged: unstaged, conflicted: conflicted)
    }
}

/// A readable summary of one changed file, combining its staged and unstaged deltas.
public typealias GitChangeKind = DomainGitChangeKind
extension GitChangeKind {
    public var rawValue: String { domainGitChangeTitle(kind: self) }
}

/// What the Push button can do for the current status.
public typealias GitPushAvailability = PushAvailability

extension GitStatusSnapshot {
    public var pushAvailability: GitPushAvailability {
        domainPushAvailability(branch: branch, upstream: upstream, ahead: ahead)
    }
}

extension GitChangeSnapshot {
    public var kind: GitChangeKind {
        sessionGitChangeKind(staged: staged, unstaged: unstaged, conflicted: conflicted)
    }
}

public typealias GitStatusSnapshot = WorkspaceGitStatus

extension GitStatusSnapshot {
    public init(branch: String?, upstream: String?, ahead: UInt64, behind: UInt64, changes: [GitChangeSnapshot]) {
        self.init(branch: branch, upstream: upstream, ahead: ahead, behind: behind, changes: changes, merging: false)
    }
    public var conflictedChanges: [GitChangeSnapshot] { changes.filter { $0.kind == .conflicted } }
}

public typealias GitOperationSnapshot = WorkspaceGitOperation

/// Git failures that mean the workspace cannot use Git yet, each with its own guidance.
public typealias GitRepositoryProblem = WorkspaceRepositoryProblem

extension GitRepositoryProblem {
    public init?(kind: WorkspaceFailureKind) {
        guard let problem = workspaceRepositoryProblem(kind: kind) else { return nil }
        self = problem
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

public typealias ResponseHeader = ResponseHeaderValue
extension ResponseHeader: Identifiable {
    public var id: String { name + "\u{0}" + value }
}

public typealias ResponseHead = SessionHead
extension ResponseHead {
    public var timeToHeadersNS: UInt64 { timeToHeadersNs }
    public init(status: UInt16, version: String, headers: [ResponseHeader], timeToHeadersNS: UInt64) {
        self.init(status: status, version: version, headers: headers, timeToHeadersNs: timeToHeadersNS)
    }
}

public typealias RunCompletion = SessionCompletion
extension RunCompletion {
    public var totalTimeNS: UInt64 { totalTimeNs }
    public init(bytesReceived: UInt64, totalTimeNS: UInt64) { self.init(bytesReceived: bytesReceived, totalTimeNs: totalTimeNS) }
}

public typealias RequestIssue = FailureIssue

public typealias RunFailure = SessionFailure
extension RunFailure: Error {}

public typealias PreparedHeaderSnapshot = SessionPreparedHeader

public typealias PreparedBodySnapshot = SessionPreparedBody

public struct PreparedRunSnapshot: Equatable, Sendable {
    var record: SessionPrepared
    public var proxy: EffectiveProxy? {
        get { record.proxy }
        set { record.proxy = newValue }
    }
    public var method: String { record.method }
    public var url: String { record.url }
    public var headers: [PreparedHeaderSnapshot] { record.headers }
    public var body: PreparedBodySnapshot { record.body }
    public var transport: TransportSettings { TransportSettings(domain: record.transport) }
    public init(proxy: EffectiveProxy? = nil, method: String, url: String, headers: [PreparedHeaderSnapshot], body: PreparedBodySnapshot, transport: TransportSettings) {
        record = SessionPrepared(proxy: proxy,
                                 method: method, url: url, headers: headers, body: body, transport: transport.domain)
    }
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
    public var query: [RequestField] { domain.query }
    public var headers: [RequestField] { domain.headers }
    public var authentication: RequestAuthentication { domain.authentication }
    public var body: RequestBody { domain.body }
    public var variables: [String: ValueSource] { domain.variables }
    public var appProxy: ProxyDocument? { domain.appProxy }
    public var workspaceProxy: ProxyDocument? { domain.workspaceProxy }
    public var requestProxy: ProxyDocument? { domain.requestProxy }
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
            draft: draft.domain, variables: variables, workspaceProxy: workspaceProxy,
            appProxy: appProxy)
    }

}

public typealias ProxyDocument = DomainProxy

public typealias ProxyRouteDocument = DomainProxyRoute

extension ProxyRouteDocument {
    public init(destination: ProxyDestination, endpoint: String) { self.init(destination: destination, endpoint: endpoint, credentials: nil) }
}

public typealias ProxyCredentialsDocument = DomainProxyCredentials

extension RequestField {
    /// Bulk editing changes values, not the credential storage or redaction policy.
    public static func parseBulk(_ source: String, preserving previous: [RequestField]) -> [RequestField] {
        domainParseBulk(source: source, previous: previous)
    }
}

/// Runtime cookie updates are deliberately separate from persisted response heads.
public struct ResponseCookies: Equatable, Sendable {
    public let url: String
    public let headers: [ResponseHeader]
}

public extension RequestBody {
    var composerKind: DomainBodyKind { domainBodyKind(body: domainKind) }
    var canFormat: Bool { domainBodyCanFormat(kind: domainKind) }
    func switching(to kind: DomainBodyKind) -> RequestBody {
        domainSwitchBody(body: self, kind: kind)
    }
    func formatted() -> RequestBody {
        domainFormatBody(body: self, ordering: NativeBodyKeyOrdering())
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
