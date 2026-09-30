import Foundation

public extension RequestDraft {
    var curlValueSources: [ValueSource] {
        var sources: [ValueSource] = []
        _ = resolvingCurlValues { source in sources.append(source); return source.editableValue }
        if let identity = transport.clientCertificateReference { sources.append(.secret(identity)) }
        return sources
    }

    private func resolvingCurlValues(_ resolve: (ValueSource) -> String) -> RequestDraft {
        var result = self
        func text(_ value: String) -> String { resolve(.literal(value)) }
        func fields(_ values: [RequestField]) -> [RequestField] {
            values.map { field in
                guard field.enabled else { return field }
                var field = field; field.name = text(field.name); field.value = .literal(resolve(field.value)); return field
            }
        }
        result.url = text(url)
        result.query = fields(query)
        result.headers = fields(headers)
        switch authentication {
        case .none: break
        case let .basic(username, password): result.authentication = .basic(username: .literal(resolve(username)), password: .literal(resolve(password)))
        case let .bearer(token): result.authentication = .bearer(token: .literal(resolve(token)))
        case let .apiKey(placement, name, value): result.authentication = .apiKey(placement: placement, name: text(name), value: .literal(resolve(value)))
        case let .oauth2(configuration): result.authentication = .bearer(token: .literal(resolve(.secret(configuration.accessTokenReference))))
        }
        switch body {
        case .empty: break
        case let .json(value): result.body = .json(value: text(value))
        case let .xml(value): result.body = .xml(value: text(value))
        case let .html(value): result.body = .html(value: text(value))
        case let .text(mime, value): result.body = .text(contentType: mime.map(text), value: text(value))
        case let .raw(mime, value): result.body = .raw(contentType: mime.map(text), value: text(value))
        case let .file(path, mime): result.body = .file(path: path, contentType: mime.map(text))
        case let .formURLEncoded(values): result.body = .formURLEncoded(fields: fields(values))
        case let .multipart(parts): result.body = .multipart(parts: parts.map { part in
            guard part.enabled else { return part }
            var part = part; part.name = text(part.name)
            if part.kind != .file { part.value = .literal(resolve(part.value)) }
            return part
        })
        }
        return result
    }

    /// Produces a shell command for the current draft. The caller decides whether
    /// to resolve credentials for an explicit copy action or retain placeholders.
    func curlCommand(resolve: (ValueSource) -> String) -> String {
        let identity = transport.clientCertificateReference.map { resolve(.secret($0)) }
        return resolvingCurlValues(resolve).renderCurlCommand(clientIdentity: identity)
    }

    /// The client identity is passed through process substitution, like binary multipart
    /// parts, so its private key is never written to a file.
    private func renderCurlCommand(clientIdentity: String?) -> String {
        func resolve(_ value: ValueSource) -> String { value.editableValue }
        func quote(_ value: String) -> String { "'" + value.replacingOccurrences(of: "'", with: "'\"'\"'") + "'" }
        func formQuote(_ value: String) -> String {
            "\"" + value.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
        }
        var request = self
        request.query = request.query.map { field in
            var field = field; field.value = .literal(resolve(field.value)); return field
        }
        var auth: [String] = []
        switch authentication {
        case .none: break
        case let .basic(username, password): auth += ["--user", quote(resolve(username) + ":" + resolve(password))]
        case let .bearer(token): auth += ["--header", quote("Authorization: Bearer " + resolve(token))]
        case let .apiKey(placement, name, value):
            if placement == .query { request.query.append(RequestField(name: name, value: .literal(resolve(value)))) }
            else { auth += ["--header", quote(name + ": " + resolve(value))] }
        case let .oauth2(configuration): auth += ["--header", quote("Authorization: Bearer " + resolve(.secret(configuration.accessTokenReference)))]
        }
        var arguments = ["curl", "--request", quote(method.rawValue), quote(request.displayURL)] + auth
        var streams: [String] = []
        // Returns a `/dev/fd` path that reads `text` from a process substitution.
        func stream(_ text: String, decoding: String? = nil) -> String {
            let descriptor = streams.count + 3
            streams.append("\(descriptor)< <(printf %s \(quote(text))\(decoding.map { " | " + $0 } ?? ""))")
            return "/dev/fd/\(descriptor)"
        }
        if transport.followRedirects { arguments += ["--location", "--max-redirs", String(transport.maximumRedirects)] }
        if !transport.validateTLS { arguments.append("--insecure") }
        if transport.totalTimeoutMS != 30_000 { arguments += ["--max-time", String(Double(transport.totalTimeoutMS) / 1000)] }
        if let path = transport.customCAPath { arguments += ["--cacert", quote(path)] }
        if let clientIdentity {
            if let identity = try? ClientIdentityPEM(parsing: [clientIdentity]) {
                arguments += ["--cert", stream(identity.certificates.joined(separator: "\n") + "\n"), "--key", stream(identity.privateKey + "\n")]
            } else {
                arguments += ["--cert", stream(clientIdentity)]
            }
        }
        for header in headers where header.enabled {
            if case .multipart = body, header.name.caseInsensitiveCompare("Content-Type") == .orderedSame { continue }
            arguments += ["--header", quote(header.name + ": " + resolve(header.value))]
        }
        let hasContentType = headers.contains { $0.enabled && $0.name.caseInsensitiveCompare("Content-Type") == .orderedSame }
        func contentType(_ value: String?) {
            if !hasContentType, let value { arguments += ["--header", quote("Content-Type: " + value)] }
        }
        switch body {
        case .empty: break
        case let .json(value): contentType("application/json"); arguments += ["--data-raw", quote(value)]
        case let .xml(value): contentType("application/xml"); arguments += ["--data-raw", quote(value)]
        case let .html(value): contentType("text/html"); arguments += ["--data-raw", quote(value)]
        case let .text(mime, value), let .raw(mime, value): contentType(mime ?? ""); arguments += ["--data-raw", quote(value)]
        case let .file(path, mime): contentType(mime); arguments += ["--data-binary", quote("@" + path)]
        case let .formURLEncoded(fields):
            contentType("application/x-www-form-urlencoded")
            for field in fields where field.enabled { arguments += ["--data-urlencode", quote(field.name + "=" + resolve(field.value))] }
        case let .multipart(parts):
            for part in parts where part.enabled {
                var value: String
                switch part.kind {
                case .text: value = formQuote(resolve(part.value))
                case .file: value = "@" + formQuote(part.filePath ?? "")
                case .binary: value = "@" + stream(resolve(part.value), decoding: "base64 --decode")
                }
                if let mime = part.contentType, !mime.isEmpty { value += ";type=" + formQuote(mime) }
                if let name = part.fileName { value += ";filename=" + formQuote(name) }
                arguments += ["--form", quote(part.name + "=" + value)]
            }
        }
        return (arguments + streams).joined(separator: " ")
    }
}

internal extension ProxyDocument {
    func curlRoute(for url: String) -> ProxyRouteDocument? {
        guard case let .manual(routes) = self,
              let scheme = URL(string: url.trimmingCharacters(in: .whitespacesAndNewlines))?.scheme?.lowercased()
        else { return nil }
        return routes.first { $0.destination == "all" || $0.destination == scheme }
    }

    func curlArguments(for url: String, credentials: [String]) -> String {
        func quote(_ value: String) -> String { "'" + value.replacingOccurrences(of: "'", with: "'\"'\"'") + "'" }
        switch self {
        case .system: return ""
        case .direct: return " --noproxy '*'"
        case .manual:
            guard let route = curlRoute(for: url) else { return " --noproxy '*'" }
            var arguments = " --proxy " + quote(route.endpoint) + " --noproxy ''"
            if credentials.count == 2 {
                arguments += " --proxy-user " + quote(credentials[0] + ":" + credentials[1])
            }
            return arguments
        }
    }
}
