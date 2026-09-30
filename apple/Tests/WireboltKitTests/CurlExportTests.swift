import Foundation
import Testing
@testable import WireboltKit

/// Runs a copied command with `curl` and `security` replaced by shell functions: `security`
/// answers from `keychain` the way `find-generic-password -w` prints (hex when a value has a
/// byte outside printable ASCII) and `curl` prints its arguments plus any `/dev/fd` contents.
func runCopiedCurl(_ command: String, keychain: [String: String] = [:]) throws -> [String] {
    let entries = keychain.map { name, value in
        let printed = CurlSecretReferences.printsAsHex(value) ? Data(value.utf8).map { String(format: "%02x", $0) }.joined() : value
        return "    \(shellQuote(name))) printf '%s\\n' \(shellQuote(printed));;"
    }.joined(separator: "\n")
    let stubs = """
    security() {
      local service="" account=""
      while [ $# -gt 0 ]; do case "$1" in -s) service="$2"; shift 2;; -a) account="$2"; shift 2;; *) shift;; esac; done
      [ "$service" = \(shellQuote(CurlSecretReferences.keychainService)) ] || return 44
      case "$account" in
    \(entries)
        *) return 44;;
      esac
    }
    curl() {
      for a in "$@"; do
        printf '%s\\0' "$a"
        if [[ $a =~ '/dev/fd/([0-9]+)' ]]; then printf 'fd%s=%s\\0' "$match[1]" "$(cat /dev/fd/$match[1])"; fi
      done
    }

    """
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/bin/zsh")
    process.arguments = ["-f", "-c", stubs + command]
    let output = Pipe()
    process.standardOutput = output
    try process.run()
    let data = output.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    #expect(process.terminationStatus == 0)
    return String(decoding: data, as: UTF8.self).split(separator: "\0", omittingEmptySubsequences: false).dropLast().map(String.init)
}

@Suite("cURL export")
struct CurlExportTests {
    private func command(_ request: RequestDraft, variables: [String: ValueSource] = [:], keychain: [String: String] = [:]) -> String {
        let hex = keychain.filter { CurlSecretReferences.printsAsHex($0.value) }.keys
        return request.curlCommand(secrets: CurlSecretReferences(variables: variables, hexEncoded: Set(hex)))
    }

    @Test("Shell quoting preserves apostrophes, substitutions, Unicode and credentials as literal arguments")
    func shellQuoting() throws {
        var request = RequestDraft(url: "https://example.com/echo")
        request.method = .post
        request.authentication = .basic(username: .literal("O'Brien"), password: .secret("test.password"))
        let payload = "café 東京 $(printf SHOULD_NOT_RUN) `printf SHOULD_NOT_RUN` 'quoted'"
        request.body = .json(value: payload)
        let keychain = ["test.password": "p@ss w'rd $(x)"]
        let command = command(request, keychain: keychain)
        #expect(!command.contains("p@ss"))
        #expect(try runCopiedCurl(command, keychain: keychain) == ["--request", "POST", "https://example.com/echo", "--user", "O'Brien:p@ss w'rd $(x)",
                                                                  "--header", "Content-Type: application/json", "--data-raw", payload])
    }

    @Test("Secrets in auth, headers, query, bodies and variables are read from Keychain when the command runs")
    func secretsStayInKeychain() throws {
        let keychain = [
            "env.token": "tok3n/+&=?", "auth.bearer": "bearer-value", "header.secret": "header-value",
            "form.secret": "form value", "part.secret": "part-value", "unicode.secret": "café ☕",
        ]
        let variables: [String: ValueSource] = [
            "token": .secret("env.token"), "host": .literal("api.example.com"), "wrapped": .literal("id-{{token}}"),
        ]
        var request = RequestDraft(url: "https://{{host}}/items?fixed=1")
        request.method = .post
        request.query = [RequestField(name: "q", value: .literal("a b")), RequestField(name: "key", value: .literal("{{wrapped}}"))]
        request.headers = [RequestField(name: "X-Secret", value: .secret("header.secret")),
                           RequestField(name: "X-Unicode", value: .secret("unicode.secret"))]
        request.authentication = .bearer(token: .secret("auth.bearer"))
        request.body = .formURLEncoded(fields: [RequestField(name: "f", value: .secret("form.secret")),
                                                RequestField(name: "t", value: .literal("{{token}}"))])
        let command = command(request, variables: variables, keychain: keychain)
        for value in keychain.values { #expect(!command.contains(value)) }
        #expect(command.contains("security find-generic-password -s 'io.github.christopher96u.wirebolt' -a 'env.token' -w"))
        #expect(command.contains("-a 'unicode.secret' -w | xxd -r -p"))
        #expect(!command.contains("-a 'env.token' -w | xxd"))
        #expect(try runCopiedCurl(command, keychain: keychain) == [
            "--request", "POST", "https://api.example.com/items?fixed=1",
            "--url-query", "q=a b", "--url-query", "key=id-tok3n/+&=?",
            "--header", "Authorization: Bearer bearer-value",
            "--header", "X-Secret: header-value", "--header", "X-Unicode: café ☕",
            "--header", "Content-Type: application/x-www-form-urlencoded",
            "--data-urlencode", "f=form value", "--data-urlencode", "t=tok3n/+&=?",
        ])
        #expect(request.curlSecretNames(variables: variables) == ["env.token", "auth.bearer", "header.secret", "unicode.secret", "form.secret"])
    }

    @Test("API keys, OAuth tokens and multipart parts reference Keychain instead of copying values")
    func advancedSecrets() throws {
        let keychain = ["api.key": "k3y", "oauth.token": "oauth-value", "part.secret": "part value", "binary.secret": "aGk="]
        var request = RequestDraft(url: "https://example.com/")
        request.authentication = .apiKey(placement: .query, name: "api_key", value: .secret("api.key"))
        #expect(try runCopiedCurl(command(request, keychain: keychain), keychain: keychain)
            == ["--request", "GET", "https://example.com/", "--url-query", "api_key=k3y"])
        request.authentication = .oauth2(configuration: OAuth2Configuration(accessTokenReference: "oauth.token"))
        request.body = .multipart(parts: [
            MultipartPart(name: "note", kind: .text, value: .secret("part.secret"), contentType: "text/plain"),
            MultipartPart(name: "plain", kind: .text, value: .literal("visible")),
            MultipartPart(name: "blob", kind: .binary, value: .secret("binary.secret"), fileName: "a.bin"),
        ])
        let command = command(request, keychain: keychain)
        for value in keychain.values { #expect(!command.contains(value)) }
        #expect(try runCopiedCurl(command, keychain: keychain) == [
            "--request", "GET", "https://example.com/", "--header", "Authorization: Bearer oauth-value",
            "--form", "note=</dev/fd/3;type=\"text/plain\"", "fd3=part value",
            "--form", "plain=\"visible\"",
            "--form", "blob=@/dev/fd/4;filename=\"a.bin\"", "fd4=hi",
        ])
    }

    @Test("Forms retain enabled repeated fields and binary multipart keeps its bytes")
    func forms() {
        var request = RequestDraft(url: "http://localhost/echo")
        request.body = .formURLEncoded(fields: [RequestField(name: "name", value: .literal("café")),
            RequestField(name: "name", value: .literal("東京")), RequestField(name: "off", value: .literal("hidden"), enabled: false)])
        let form = command(request)
        #expect(form.contains("--data-urlencode 'name=café' --data-urlencode 'name=東京'"))
        #expect(!form.contains("hidden"))
        request.headers = [RequestField(name: "Content-Type", value: .literal("multipart/form-data; boundary=stale"))]
        request.body = .multipart(parts: [MultipartPart(name: "file", kind: .binary, value: .literal("AAH/"), fileName: "a.bin")])
        let multipart = command(request)
        #expect(multipart.contains("@/dev/fd/3"))
        #expect(multipart.contains("printf %s 'AAH/' | base64 --decode"))
        #expect(!multipart.contains("boundary=stale"))
    }

    @Test("Client certificate and key are read from Keychain through file descriptors, never copied")
    func clientIdentity() throws {
        var request = RequestDraft(url: "https://mtls.example.com/")
        request.transport.clientCertificateReference = "tls.fixture.client-identity"
        request.transport.customCAPath = "/tmp/fixture ca.pem"
        #expect(request.curlValueSources.last == .secret("tls.fixture.client-identity"))
        let identity = try ClientIdentityPEM(parsing: [ClientIdentityFixture.leaf, ClientIdentityFixture.intermediate, ClientIdentityFixture.key]).combined
        let keychain = ["tls.fixture.client-identity": identity]
        let command = command(request, keychain: keychain)
        #expect(!command.contains("BEGIN"))
        #expect(command.contains("--cert /dev/fd/3 --key /dev/fd/4"))
        #expect(command.contains("3< <(security find-generic-password -s 'io.github.christopher96u.wirebolt' -a 'tls.fixture.client-identity' -w | xxd -r -p)"))
        let stored = String(identity.dropLast())
        #expect(try runCopiedCurl(command, keychain: keychain) == [
            "--request", "GET", "https://mtls.example.com/", "--cacert", "/tmp/fixture ca.pem",
            "--cert", "/dev/fd/3", "fd3=" + stored, "--key", "/dev/fd/4", "fd4=" + stored,
        ])
    }

    @Test("Binary multipart streams follow the client identity descriptors")
    func descriptorsDoNotCollide() {
        var request = RequestDraft(url: "https://mtls.example.com/")
        request.transport.clientCertificateReference = "tls.fixture.client-identity"
        request.body = .multipart(parts: [MultipartPart(name: "file", kind: .binary, value: .literal("AAH/"), fileName: "a.bin")])
        let command = command(request)
        #expect(command.contains("--cert /dev/fd/3 --key /dev/fd/4"))
        #expect(command.contains("--form 'file=@/dev/fd/5;filename=\"a.bin\"'"))
        #expect(command.contains("5< <(printf %s 'AAH/' | base64 --decode)"))
    }
}
