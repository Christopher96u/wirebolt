import Foundation

public extension RequestDraft {
    var curlValueSources: [ValueSource] {
        var result = headers.filter(\.enabled).map(\.value) + query.filter(\.enabled).map(\.value)
        switch authentication {
        case .none: break
        case let .basic(username, password): result += [username, password]
        case let .bearer(token): result.append(token)
        case let .apiKey(_, _, value): result.append(value)
        case let .oauth2(configuration): result.append(.secret(configuration.accessTokenReference))
        }
        switch body {
        case let .formURLEncoded(fields): result += fields.filter(\.enabled).map(\.value)
        case let .multipart(parts): result += parts.filter(\.enabled).map(\.value)
        default: break
        }
        return result
    }

    /// Produces a shell command for the current draft. The caller decides whether
    /// to resolve credentials for an explicit copy action or retain placeholders.
    func curlCommand(resolve: (ValueSource) -> String) -> String {
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
        for header in headers where header.enabled {
            if case .multipart = body, header.name.caseInsensitiveCompare("Content-Type") == .orderedSame { continue }
            arguments += ["--header", quote(header.name + ": " + resolve(header.value))]
        }
        let hasContentType = headers.contains { $0.enabled && $0.name.caseInsensitiveCompare("Content-Type") == .orderedSame }
        func contentType(_ value: String?) {
            if !hasContentType, let value { arguments += ["--header", quote("Content-Type: " + value)] }
        }
        var streams: [String] = []
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
                case .binary:
                    let descriptor = streams.count + 3
                    value = "@/dev/fd/\(descriptor)"
                    streams.append("\(descriptor)< <(printf %s \(quote(resolve(part.value))) | base64 --decode)")
                }
                if let mime = part.contentType, !mime.isEmpty { value += ";type=" + formQuote(mime) }
                if let name = part.fileName { value += ";filename=" + formQuote(name) }
                arguments += ["--form", quote(part.name + "=" + value)]
            }
        }
        return (arguments + streams).joined(separator: " ")
    }
}
