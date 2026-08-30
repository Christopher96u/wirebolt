import Foundation

public enum HTTPMethod: String, CaseIterable, Codable, Sendable {
    case get = "GET"
    case post = "POST"
    case put = "PUT"
    case patch = "PATCH"
    case delete = "DELETE"
    case head = "HEAD"
    case options = "OPTIONS"
}

public enum ValueSource: Codable, Equatable, Sendable {
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
    public var id: UUID
    public var name: String
    public var value: ValueSource
    public var enabled: Bool

    enum CodingKeys: String, CodingKey { case name, value, enabled }

    public init(
        id: UUID = UUID(),
        name: String = "",
        value: ValueSource = .literal(""),
        enabled: Bool = true
    ) {
        self.id = id
        self.name = name
        self.value = value
        self.enabled = enabled
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = UUID()
        name = try container.decode(String.self, forKey: .name)
        value = try container.decode(ValueSource.self, forKey: .value)
        enabled = try container.decode(Bool.self, forKey: .enabled)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(name, forKey: .name)
        try container.encode(value, forKey: .value)
        try container.encode(enabled, forKey: .enabled)
    }
}

public enum APIKeyPlacement: String, Codable, CaseIterable, Sendable {
    case header
    case query
}

public enum RequestAuthentication: Codable, Equatable, Sendable {
    case none
    case basic(username: ValueSource, password: ValueSource)
    case bearer(token: ValueSource)
    case apiKey(placement: APIKeyPlacement, name: String, value: ValueSource)

    private enum CodingKeys: String, CodingKey { case kind, username, password, token, placement, name, value }
    private enum Kind: String, Codable { case none, basic, bearer, apiKey = "api_key" }

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
        }
    }
}

public enum RequestBody: Codable, Equatable, Sendable {
    case empty
    case text(contentType: String?, value: String)
    case json(value: String)
    case formURLEncoded(fields: [RequestField])

    private enum CodingKeys: String, CodingKey { case kind, contentType = "content_type", value, fields }
    private enum Kind: String, Codable { case empty, text, json, formURLEncoded = "form_url_encoded" }

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
        case let .formURLEncoded(fields):
            try container.encode(Kind.formURLEncoded, forKey: .kind)
            try container.encode(fields, forKey: .fields)
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
        case .formURLEncoded:
            self = try .formURLEncoded(fields: container.decode([RequestField].self, forKey: .fields))
        }
    }
}

public enum ProxySelection: Hashable, Sendable {
    case inherit
    case system
    case direct
    case manual(ProxyDocument)
}

public struct RequestDraft: Equatable, Sendable {
    public var id: String
    public var name: String
    public var method: HTTPMethod
    public var url: String
    public var query: [RequestField]
    public var headers: [RequestField]
    public var authentication: RequestAuthentication
    public var body: RequestBody
    public var proxy: ProxySelection

    public init(
        id: String = "draft",
        name: String = "Untitled Request",
        method: HTTPMethod = .get,
        url: String = "",
        query: [RequestField] = [],
        headers: [RequestField] = [],
        authentication: RequestAuthentication = .none,
        body: RequestBody = .empty,
        proxy: ProxySelection = .inherit
    ) {
        self.id = id
        self.name = name
        self.method = method
        self.url = url
        self.query = query
        self.headers = headers
        self.authentication = authentication
        self.body = body
        self.proxy = proxy
    }
}

public struct EnvironmentDraft: Identifiable, Codable, Equatable, Sendable {
    public var id: String
    public var name: String
    public var variables: [String: ValueSource]

    public init(id: String, name: String, variables: [String: ValueSource] = [:]) {
        self.id = id
        self.name = name
        self.variables = variables
    }
}

public struct RequestLocation: Identifiable, Equatable, Sendable {
    public var id: String { "\(collectionID)/\(request.id)" }
    public let collectionID: String
    public var request: RequestDraft

    public init(collectionID: String, request: RequestDraft) {
        self.collectionID = collectionID
        self.request = request
    }
}

public struct CollectionDraft: Identifiable, Equatable, Sendable {
    public let id: String
    public var name: String
    public var requests: [RequestLocation]

    public init(id: String, name: String, requests: [RequestLocation] = []) {
        self.id = id
        self.name = name
        self.requests = requests
    }
}

public struct WorkspaceDraft: Equatable, Sendable {
    public var name: String
    public var proxy: ProxyDocument?
    public var collections: [CollectionDraft]
    public var environments: [EnvironmentDraft]

    public init(
        name: String,
        proxy: ProxyDocument? = nil,
        collections: [CollectionDraft] = [],
        environments: [EnvironmentDraft] = []
    ) {
        self.name = name
        self.proxy = proxy
        self.collections = collections
        self.environments = environments
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

public enum RunEvent: Equatable, Sendable {
    case head(ResponseHead)
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
    public let workspaceProxy: ProxyDocument?
    public let requestProxy: ProxyDocument?
    public let totalTimeoutMS: UInt64
    public let readTimeoutMS: UInt64
    public let maxResponseBytes: UInt64?

    enum CodingKeys: String, CodingKey {
        case method, url, query, headers, authentication, body, variables
        case workspaceProxy = "workspace_proxy"
        case requestProxy = "request_proxy"
        case totalTimeoutMS = "total_timeout_ms"
        case readTimeoutMS = "read_timeout_ms"
        case maxResponseBytes = "max_response_bytes"
    }

    public init(
        draft: RequestDraft,
        variables: [String: ValueSource],
        workspaceProxy: ProxyDocument? = nil
    ) {
        method = draft.method.rawValue
        url = draft.url
        query = draft.query
        headers = draft.headers
        authentication = draft.authentication
        body = draft.body
        self.variables = variables
        self.workspaceProxy = workspaceProxy
        requestProxy = switch draft.proxy {
        case .inherit: nil
        case .system: .system
        case .direct: .direct
        case let .manual(document): document
        }
        totalTimeoutMS = 30_000
        readTimeoutMS = 10_000
        maxResponseBytes = nil
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
