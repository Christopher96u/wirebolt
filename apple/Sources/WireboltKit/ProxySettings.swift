import Foundation
import Observation

public enum ProxyScope: String, CaseIterable, Sendable {
    case app, workspace, request
    public var title: String {
        switch self { case .app: "App default"; case .workspace: "Workspace"; case .request: "Request" }
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
        guard case let .manual(routes) = self else { return }
        guard !routes.isEmpty else { throw ProxyValidationError("Add at least one proxy route.") }
        var destinations = Set<String>()
        for route in routes {
            guard ["all", "http", "https"].contains(route.destination) else { throw ProxyValidationError("Choose All, HTTP or HTTPS destinations.") }
            let covered: Set<String> = route.destination == "all" ? ["http", "https"] : [route.destination]
            guard destinations.isDisjoint(with: covered) else { throw ProxyValidationError("Proxy routes overlap. Use one route for all traffic, or separate HTTP and HTTPS routes.") }
            destinations.formUnion(covered)
            guard let parts = URLComponents(string: route.endpoint),
                  let scheme = parts.scheme, ProxyRouteDraft.protocols.contains(scheme),
                  let host = parts.host, !host.isEmpty,
                  !host.contains(where: { $0.isWhitespace }),
                  parts.user == nil, parts.password == nil,
                  parts.path.isEmpty || parts.path == "/", parts.query == nil, parts.fragment == nil,
                  parts.port.map({ (1...65535).contains($0) }) ?? true
            else { throw ProxyValidationError("Use a valid proxy host and port, without a path or embedded credentials.") }
            if route.credentials != nil && ["socks4", "socks4a"].contains(scheme) {
                throw ProxyValidationError("SOCKS4 does not support username/password authentication. Choose SOCKS5.")
            }
        }
    }
}

public struct ProxyRouteDraft: Identifiable, Equatable {
    public static let protocols = ["http", "https", "socks4", "socks4a", "socks5", "socks5h"]
    public let id = UUID()
    public var destination = "all"
    public var scheme = "http"
    public var host = ""
    public var port = "8080"
    public var authenticated = false
    public var replaceCredentials = false
    public var username = ""
    public var password = ""
    public var credentials: ProxyCredentialsDocument?
    public init(route: ProxyRouteDocument? = nil) {
        guard let route else { return }
        destination = route.destination
        let parts = URLComponents(string: route.endpoint)
        scheme = parts?.scheme ?? "http"
        host = parts?.host ?? ""
        port = parts?.port.map(String.init) ?? ""
        authenticated = route.credentials != nil
        credentials = route.credentials
    }
    public var needsCredentials: Bool { authenticated && (credentials == nil || replaceCredentials) }
    public var credentialReferences: ProxyCredentialsDocument? {
        guard authenticated else { return nil }
        if !needsCredentials { return credentials }
        let prefix = "proxy.\(id.uuidString.lowercased())"
        return ProxyCredentialsDocument(username: "\(prefix).user", password: "\(prefix).password")
    }
    public func document() throws -> ProxyRouteDocument {
        let cleanHost = host.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleanHost.isEmpty, !cleanHost.contains(where: { "/@?#".contains($0) || $0.isWhitespace }), !cleanHost.contains("://") else { throw ProxyValidationError("Enter a host name or IP address, without a protocol or path.") }
        if needsCredentials && (username.isEmpty || password.isEmpty) { throw ProxyValidationError("Enter both the proxy username and password.") }
        guard port.isEmpty || Int(port).map({ (1...65535).contains($0) }) == true else { throw ProxyValidationError("Port must be between 1 and 65535.") }
        let hostPart = cleanHost.contains(":") && !cleanHost.hasPrefix("[") ? "[\(cleanHost)]" : cleanHost
        let route = ProxyRouteDocument(destination: destination, endpoint: "\(scheme)://\(hostPart)\(port.isEmpty ? "" : ":" + port)", credentials: credentialReferences)
        try ProxyDocument.manual(routes: [route]).validate()
        return route
    }
    public var secrets: [String: String] {
        guard needsCredentials, let refs = credentialReferences else { return [:] }
        return [refs.username: username, refs.password: password]
    }
}

public struct ProxyFormDraft: Equatable {
    public enum Mode: String, CaseIterable { case inherit, system, direct, manual }
    public var mode: Mode
    public var routes: [ProxyRouteDraft]
    public init(configuration: ProxyDocument?) {
        switch configuration {
        case nil: mode = .inherit; routes = [ProxyRouteDraft()]
        case .system: mode = .system; routes = [ProxyRouteDraft()]
        case .direct: mode = .direct; routes = [ProxyRouteDraft()]
        case let .manual(values): mode = .manual; routes = values.map { ProxyRouteDraft(route: $0) }
        }
    }
    public func document() throws -> ProxyDocument? {
        switch mode {
        case .inherit: return nil
        case .system: return .system
        case .direct: return .direct
        case .manual:
            let value = try ProxyDocument.manual(routes: routes.map { try $0.document() })
            try value.validate()
            return value
        }
    }
    public var secrets: [String: String] {
        guard mode == .manual else { return [:] }
        return routes.reduce(into: [:]) { $0.merge($1.secrets, uniquingKeysWith: { _, new in new }) }
    }
}

