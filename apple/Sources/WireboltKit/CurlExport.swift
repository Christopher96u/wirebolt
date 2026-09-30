import Foundation

/// Keychain-backed values in a copied cURL command. The shell reads each secret with
/// `security find-generic-password` when the command runs, from the Keychain item the app
/// stores it in, so the copied text never contains secret material.
public struct CurlSecretReferences: Sendable {
    /// The Keychain service of every Wirebolt secret; the item's account is the secret name.
    public static let keychainService = "io.github.christopher96u.wirebolt"

    public let variables: [String: ValueSource]
    /// Secrets that `security … -w` prints as hex because they contain a byte outside
    /// printable ASCII, such as a newline in a PEM identity. The command decodes them.
    public var hexEncoded: Set<String> = []

    public init(variables: [String: ValueSource], hexEncoded: Set<String> = []) {
        self.variables = variables
        self.hexEncoded = hexEncoded
    }

    public static func printsAsHex(_ value: String) -> Bool {
        !value.utf8.allSatisfy { (0x20...0x7E).contains($0) }
    }

    /// Expands `{{name}}` references like request preparation, keeping secrets as references.
    /// Unknown or cyclic references stay as text; the export validates values in Rust first.
    func expand(_ source: ValueSource) -> CurlText {
        switch source {
        case let .secret(name): CurlText([.secret(name)])
        case let .literal(text): expand(template: text, resolving: [])
        }
    }

    func expand(template text: String, resolving: Set<String> = []) -> CurlText {
        let references = VariableTemplate.references(in: text)
        guard !references.isEmpty else { return CurlText(text) }
        let source = text as NSString
        var result = CurlText()
        var location = 0
        for reference in references {
            result += CurlText(source.substring(with: NSRange(location: location, length: reference.range.location - location)))
            let whole = source.substring(with: reference.range)
            switch variables[reference.name] {
            case let .secret(name)?: result += CurlText([.secret(name)])
            case let .literal(value)? where !resolving.contains(reference.name) && resolving.count < 64:
                result += expand(template: value, resolving: resolving.union([reference.name]))
            default: result += CurlText(whole)
            }
            location = NSMaxRange(reference.range)
        }
        return result + CurlText(source.substring(from: location))
    }

    /// A shell word: literal runs in single quotes, secrets as double-quoted command substitutions.
    func word(_ text: CurlText) -> String {
        let rendered = text.parts.map { part in
            switch part {
            case let .text(value): shellQuote(value)
            case let .secret(name): "\"$(" + keychainRead(name) + ")\""
            }
        }.joined()
        return rendered.isEmpty ? "''" : rendered
    }

    /// Prints a secret's exact value, for process substitution.
    func keychainRead(_ name: String) -> String {
        "security find-generic-password -s " + shellQuote(Self.keychainService) + " -a " + shellQuote(name) + " -w"
            + (hexEncoded.contains(name) ? " | xxd -r -p" : "")
    }
}

/// Text for one cURL argument: literal runs and Keychain secret references.
struct CurlText: Equatable {
    enum Part: Equatable {
        case text(String)
        case secret(String)
    }

    private(set) var parts: [Part] = []

    init() {}
    init(_ text: String) { if !text.isEmpty { parts = [.text(text)] } }
    init(_ parts: [Part]) { for part in parts { append(part) } }

    /// The text when it references no secret.
    var plain: String? {
        var text = ""
        for part in parts {
            guard case let .text(value) = part else { return nil }
            text += value
        }
        return text
    }

    var secretNames: [String] {
        parts.compactMap { if case let .secret(name) = $0 { name } else { nil } }
    }

    private mutating func append(_ part: Part) {
        if case let .text(value) = part {
            guard !value.isEmpty else { return }
            if case let .text(previous)? = parts.last { parts[parts.count - 1] = .text(previous + value); return }
        }
        parts.append(part)
    }

    static func + (lhs: CurlText, rhs: CurlText) -> CurlText { var result = lhs; result += rhs; return result }
    static func + (lhs: CurlText, rhs: String) -> CurlText { lhs + CurlText(rhs) }
    static func + (lhs: String, rhs: CurlText) -> CurlText { CurlText(lhs) + rhs }
    static func += (lhs: inout CurlText, rhs: CurlText) { for part in rhs.parts { lhs.append(part) } }
}

func shellQuote(_ value: String) -> String { "'" + value.replacingOccurrences(of: "'", with: "'\"'\"'") + "'" }

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

    /// Every Keychain secret the command reads, in order, including through variables.
    func curlSecretNames(variables: [String: ValueSource]) -> [String] {
        var names: [String] = []
        _ = curlCommand(secrets: CurlSecretReferences(variables: variables), recordingSecrets: &names)
        var seen: Set<String> = []
        return names.filter { seen.insert($0).inserted }
    }

    /// Produces a shell command for the draft. Secrets, including secret variables and the
    /// client identity, are read from Keychain by the shell; their values are never included.
    func curlCommand(secrets: CurlSecretReferences) -> String {
        var names: [String] = []
        return curlCommand(secrets: secrets, recordingSecrets: &names)
    }

    private func curlCommand(secrets: CurlSecretReferences, recordingSecrets names: inout [String]) -> String {
        func expand(_ source: ValueSource) -> CurlText {
            let text = secrets.expand(source); names += text.secretNames; return text
        }
        func expand(_ template: String) -> CurlText { expand(.literal(template)) }
        func word(_ text: CurlText) -> String { secrets.word(text) }
        func formQuote(_ value: String) -> String {
            "\"" + value.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
        }
        var streams: [String] = []
        // Returns a `/dev/fd` path that reads the output of a shell command.
        func stream(_ command: String) -> String {
            let descriptor = streams.count + 3
            streams.append("\(descriptor)< <(\(command))")
            return "/dev/fd/\(descriptor)"
        }

        var queryFields = query.filter(\.enabled).map { (name: expand($0.name), value: expand($0.value)) }
        var auth: [String] = []
        switch authentication {
        case .none: break
        case let .basic(username, password): auth += ["--user", word(expand(username) + ":" + expand(password))]
        case let .bearer(token): auth += ["--header", word("Authorization: Bearer " + expand(token))]
        case let .apiKey(placement, name, value):
            if placement == .query { queryFields.append((expand(name), expand(value))) }
            else { auth += ["--header", word(expand(name) + ": " + expand(value))] }
        case let .oauth2(configuration): auth += ["--header", word("Authorization: Bearer " + expand(.secret(configuration.accessTokenReference)))]
        }
        var arguments = ["curl", "--request", shellQuote(method.rawValue)]
        let address = expand(url)
        if queryFields.isEmpty {
            arguments.append(word(address))
        } else if let base = address.plain, case let plainQuery = queryFields.compactMap({ field in
                      field.name.plain.flatMap { name in field.value.plain.map { RequestField(name: name, value: .literal($0)) } }
                  }), plainQuery.count == queryFields.count {
            var request = self
            request.url = base
            request.query = plainQuery
            arguments.append(shellQuote(request.displayURL))
        } else {
            // curl encodes each `--url-query` value, so secret values can stay in Keychain.
            arguments.append(word(address))
            for field in queryFields {
                let name = field.name.plain.map { $0.addingPercentEncoding(withAllowedCharacters: Self.queryNameAllowed) ?? $0 }
                arguments += ["--url-query", word((name.map(CurlText.init) ?? field.name) + "=" + field.value)]
            }
        }
        arguments += auth
        if transport.followRedirects { arguments += ["--location", "--max-redirs", String(transport.maximumRedirects)] }
        if !transport.validateTLS { arguments.append("--insecure") }
        if transport.totalTimeoutMS != 30_000 { arguments += ["--max-time", String(Double(transport.totalTimeoutMS) / 1000)] }
        if let path = transport.customCAPath { arguments += ["--cacert", shellQuote(path)] }
        if let identity = transport.clientCertificateReference {
            // The stored identity holds the certificates and key; curl picks each from its own copy.
            names.append(identity)
            arguments += ["--cert", stream(secrets.keychainRead(identity)), "--key", stream(secrets.keychainRead(identity))]
        }
        let enabledHeaders = headers.filter(\.enabled).map { (name: expand($0.name), value: expand($0.value)) }
        func isContentType(_ name: CurlText) -> Bool { name.plain?.caseInsensitiveCompare("Content-Type") == .orderedSame }
        for header in enabledHeaders {
            if case .multipart = body, isContentType(header.name) { continue }
            arguments += ["--header", word(header.name + ": " + header.value)]
        }
        let hasContentType = enabledHeaders.contains { isContentType($0.name) }
        func contentType(_ value: CurlText?) {
            if !hasContentType, let value { arguments += ["--header", word("Content-Type: " + value)] }
        }
        switch body {
        case .empty: break
        case let .json(value): contentType(CurlText("application/json")); arguments += ["--data-raw", word(expand(value))]
        case let .xml(value): contentType(CurlText("application/xml")); arguments += ["--data-raw", word(expand(value))]
        case let .html(value): contentType(CurlText("text/html")); arguments += ["--data-raw", word(expand(value))]
        case let .text(mime, value), let .raw(mime, value):
            contentType(mime.map(expand) ?? CurlText()); arguments += ["--data-raw", word(expand(value))]
        case let .file(path, mime): contentType(mime.map(expand)); arguments += ["--data-binary", shellQuote("@" + path)]
        case let .formURLEncoded(fields):
            contentType(CurlText("application/x-www-form-urlencoded"))
            for field in fields where field.enabled { arguments += ["--data-urlencode", word(expand(field.name) + "=" + expand(field.value))] }
        case let .multipart(parts):
            for part in parts where part.enabled {
                let value = expand(part.value)
                var form: CurlText
                switch part.kind {
                case .text:
                    // `<` makes curl read a text field's value from the stream.
                    if let plain = value.plain { form = CurlText(formQuote(plain)) }
                    else { form = CurlText("<" + stream("printf %s " + word(value))) }
                case .file: form = CurlText("@" + formQuote(part.filePath ?? ""))
                case .binary: form = CurlText("@" + stream("printf %s " + word(value) + " | base64 --decode"))
                }
                if let mime = part.contentType, !mime.isEmpty { form += CurlText(";type=" + formQuote(mime)) }
                if let name = part.fileName { form += CurlText(";filename=" + formQuote(name)) }
                arguments += ["--form", word(expand(part.name) + "=" + form)]
            }
        }
        return (arguments + streams).joined(separator: " ")
    }

    private static let queryNameAllowed = CharacterSet.urlQueryAllowed.subtracting(CharacterSet(charactersIn: "&=+#"))
}

internal extension ProxyDocument {
    func curlRoute(for url: String) -> ProxyRouteDocument? {
        guard case let .manual(routes) = self,
              let scheme = URL(string: url.trimmingCharacters(in: .whitespacesAndNewlines))?.scheme?.lowercased()
        else { return nil }
        return routes.first { $0.destination == "all" || $0.destination == scheme }
    }

    /// Proxy credentials are read from Keychain by the shell, like request secrets.
    func curlArguments(for url: String, secrets: CurlSecretReferences) -> String {
        switch self {
        case .system: return ""
        case .direct: return " --noproxy '*'"
        case .manual:
            guard let route = curlRoute(for: url) else { return " --noproxy '*'" }
            var arguments = " --proxy " + shellQuote(route.endpoint) + " --noproxy ''"
            if let credentials = route.credentials {
                arguments += " --proxy-user " + secrets.word(CurlText([.secret(credentials.username), .text(":"), .secret(credentials.password)]))
            }
            return arguments
        }
    }
}
