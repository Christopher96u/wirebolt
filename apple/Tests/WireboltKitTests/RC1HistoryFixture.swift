import Foundation

// Frozen v1.0.0-rc.1 definitions. Keep independent of shipping bridge types.
enum RC1History {

    public struct RunHistoryEntry: Codable, Equatable, Identifiable, Sendable {
        public let id: String
        public let requestID: String
        public let createdAt: Date
        public let prepared: PreparedRunSnapshot
        public let responseHead: ResponseHead?
        public let completion: RunCompletion?
        public let failure: RunFailure?
        public let bodyPath: String
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

    public struct EffectiveProxy: Codable, Equatable, Sendable {
        public enum Source: String, Codable, Sendable {
            case request, workspace, appDefault = "app_default", systemDefault = "system_default"
            public var title: String {
                switch self { case .request: "Request"; case .workspace: "Workspace"; case .appDefault: "App default"; case .systemDefault: "System default" }
            }
        }
        public let configuration: ProxyDocument
        public let source: Source
    
        public static func resolve(request: ProxyDocument?, workspace: ProxyDocument?, app: ProxyDocument) -> Self {
            if let request { return Self(configuration: request, source: .request) }
            if let workspace { return Self(configuration: workspace, source: .workspace) }
            return Self(configuration: app, source: .appDefault)
        }
    
        public func summary(for url: String? = nil) -> String {
            switch configuration {
            case .system: return "System proxy"
            case .direct: return "Direct · No proxy"
            case let .manual(routes):
                let scheme = url.flatMap { URL(string: $0)?.scheme?.lowercased() }.map { $0 == "wss" ? "https" : $0 == "ws" ? "http" : $0 }
                let route = scheme.flatMap { scheme in routes.first { $0.destination == "all" || $0.destination == scheme } } ?? (scheme == nil ? routes.first : nil)
                guard let route else { return "Direct · No matching proxy route" }
                guard let endpoint = URLComponents(string: route.endpoint), let host = endpoint.host else { return "Proxy configuration error" }
                return "\(endpoint.scheme?.uppercased() ?? "Proxy") · \(host)\(endpoint.port.map { ":\($0)" } ?? "")"
            }
        }
    }
}
