import Foundation
import Testing
@testable import WireboltKit

@Suite("Resolved exports")
struct ResolvedExportTests {
    @Test("Export resolves all inputs with active variables and inherits redirect settings")
    @MainActor
    func resolvedExport() async throws {
        let runner = ExportResolver()
        let model = WireboltModel(runner: runner)
        model.workspace.environments = [EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Global",
            legacyVariables: ["host": .literal("example.com"), "payload": .literal("resolved body")])]
        model.workspace.transport.followRedirects = true
        let draft = RequestDraft(method: .post, url: "https://{{host}}/echo", headers: [RequestField(name: "X-Token", value: .secret("token"))], body: .text(contentType: nil, value: "{{payload}}"))
        let command = try #require(await model.curlCommand(for: draft))
        #expect(command.contains("https://example.com/echo"))
        #expect(command.contains("'X-Token: '\"$(security find-generic-password -s 'io.github.christopher96u.wirebolt' -a 'token' -w)\""))
        #expect(!command.contains("fixture-secret"))
        #expect(command.contains("resolved body"))
        #expect(command.contains("--location"))
        #expect(!command.contains("{{"))
        #expect(await runner.variables["host"] == .literal("example.com"))
        #expect(draft.url == "https://{{host}}/echo")
        #expect(draft.headers[0].value == .secret("token"))
    }

    @Test("Export honors inherited proxy, request override and resolved URL scheme")
    @MainActor
    func proxyRouting() async throws {
        let model = WireboltModel(runner: ExportResolver())
        model.workspace.proxy = .manual(routes: [ProxyRouteDocument(destination: "https", endpoint: "http://proxy.test:8080", credentials: ProxyCredentialsDocument(username: "token", password: "token"))])
        var draft = RequestDraft(url: "https://{{host}}/echo")
        let command = try #require(await model.curlCommand(for: draft))
        #expect(command.contains("--proxy 'http://proxy.test:8080' --noproxy ''"))
        #expect(command.contains("--proxy-user \"$(security find-generic-password -s 'io.github.christopher96u.wirebolt' -a 'token' -w)\"':'"))
        #expect(!command.contains("fixture-secret"))
        draft.proxy = .direct
        let direct = try #require(await model.curlCommand(for: draft))
        #expect(direct.contains("--noproxy '*'"))
        #expect(!direct.contains("--proxy "))
        draft.proxy = .inherit; draft.url = "http://example.com/echo"
        let unmatched = try #require(await model.curlCommand(for: draft))
        #expect(unmatched.contains("--noproxy '*'"))
        #expect(!unmatched.contains("--proxy-user"))
    }

    @Test("Secret values never reach the command; multi-line ones are decoded from hex by the shell")
    @MainActor
    func secretsAreReferenced() async throws {
        let model = WireboltModel(runner: ExportResolver())
        model.workspace.environments = [EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Global",
            legacyVariables: ["pem": .secret("multiline")])]
        var draft = RequestDraft(url: "https://example.com/", headers: [RequestField(name: "X-Token", value: .secret("token"))])
        draft.body = .text(contentType: nil, value: "{{pem}}")
        let command = try #require(await model.curlCommand(for: draft))
        #expect(!command.contains("fixture-secret"))
        #expect(!command.contains("fixture-line"))
        #expect(command.contains("-a 'multiline' -w | xxd -r -p"))
        #expect(command.contains("-a 'token' -w)"))
        let arguments = try runCopiedCurl(command, keychain: ["token": "fixture-secret", "multiline": "fixture-line\nsecond"])
        #expect(arguments.contains("X-Token: fixture-secret"))
        #expect(arguments.last == "fixture-line\nsecond")
    }

    @Test("Failed resolution cannot produce an executable partial export")
    @MainActor
    func failedExport() async {
        let model = WireboltModel(runner: ExportResolver())
        let command = await model.curlCommand(for: RequestDraft(url: "{{missing}}"))
        #expect(command == nil)
        #expect(model.operationFailure?.kind == "invalid_request")
    }
}

private actor ExportResolver: RequestRunner {
    var variables: [String: ValueSource] = [:]
    func resolveValues(_ values: [ValueSource], variables: [String: ValueSource]) async throws -> [String] {
        self.variables = variables
        return try values.map { value in
            switch value {
            case .literal("https://{{host}}/echo"): return "https://example.com/echo"
            case .literal("{{payload}}"): return "resolved body"
            case .secret("token"): return "fixture-secret"
            case .secret("multiline"): return "fixture-line\nsecond"
            case .literal("{{pem}}"): return "fixture-line\nsecond"
            case .literal(let text) where !text.contains("{{"): return text
            default: throw RunFailure(kind: "invalid_request", issues: [])
            }
        }
    }
    nonisolated func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { $0.finish() }
    }
    nonisolated func cancel(runID: RunID) {}
}
