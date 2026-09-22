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
        model.workspace.environments = [EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Global", legacyVariables: ["host": .literal("example.com")])]
        model.workspace.transport.followRedirects = true
        let draft = RequestDraft(method: .post, url: "https://{{host}}/echo", headers: [RequestField(name: "X-Token", value: .secret("token"))], body: .text(contentType: nil, value: "{{payload}}"))
        let command = try #require(await model.curlCommand(for: draft))
        #expect(command.contains("https://example.com/echo"))
        #expect(command.contains("X-Token: fixture-secret"))
        #expect(command.contains("resolved body"))
        #expect(command.contains("--location"))
        #expect(!command.contains("{{"))
        #expect(await runner.variables["host"] == .literal("example.com"))
        #expect(draft.url == "https://{{host}}/echo")
        #expect(draft.headers[0].value == .secret("token"))
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
