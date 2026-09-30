import Foundation
import Testing
@testable import WireboltKit

struct AuthenticationReferenceTests {
    private func references(_ authentication: RequestAuthentication) -> [String] {
        switch authentication {
        case .none: []
        case let .basic(username, password): [username, password].compactMap(\.secretName)
        case let .bearer(token): [token].compactMap(\.secretName)
        case let .apiKey(_, _, value): [value].compactMap(\.secretName)
        case let .oauth2(configuration): [configuration.clientSecretReference, configuration.accessTokenReference]
        }
    }

    @Test("Every auth type offered in the picker creates its own Keychain names")
    func newAuthenticationUsesUniqueReferences() {
        for kind in AuthenticationKind.allCases where kind != .none {
            let first = RequestAuthentication.new(kind)
            let second = RequestAuthentication.new(kind)
            #expect(first.kind == kind)
            #expect(!references(first).isEmpty)
            #expect(Set(references(first)).isDisjoint(with: references(second)), "\(kind) shares a Keychain name")
        }
    }

    @Test("API Key and OAuth no longer default to names shared by every request")
    func noSharedDefaults() {
        let legacy: Set = ["auth.api-key", "oauth.client-secret", "oauth.access-token"]
        let names = references(.new(.apiKey)) + references(.new(.oauth2)) + references(.oauth2(configuration: OAuth2Configuration()))
        #expect(legacy.isDisjoint(with: names))
        let configuration = OAuth2Configuration()
        #expect(configuration.clientSecretReference != configuration.accessTokenReference)
    }

    @Test("Generated names are valid Keychain reference names")
    func referencesAreValidSecretNames() {
        let allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-_")
        for kind in AuthenticationKind.allCases {
            for name in references(.new(kind)) {
                #expect((1 ... 128).contains(name.count))
                #expect(name.unicodeScalars.allSatisfy(allowed.contains), "\(name)")
            }
        }
    }

    @Test("A new API key request encodes the reference, never material")
    func apiKeyEncodesOnlyReference() throws {
        var draft = RequestDraft(url: "https://api.example.com/v1/orders")
        draft.authentication = .new(.apiKey)
        let encoded = String(decoding: try JSONEncoder().encode(RunInput(draft: draft, variables: [:])), as: UTF8.self)
        let name = try #require(references(draft.authentication).first)
        #expect(encoded.contains(#""kind":"api_key""#))
        #expect(encoded.contains(name))
    }

    @Test("Editing the API key value stages Keychain material under the request's own name")
    @MainActor
    func apiKeyMaterialIsStagedPerRequest() async throws {
        let keychain = KeychainRecorder()
        let model = WireboltModel(runner: SilentRunner(), persistence: keychain)
        let first = model.makeNewRequest()
        let second = model.makeNewRequest()
        first.draft.url = "https://api.example.com/a"
        first.draft.authentication = .new(.apiKey)
        second.draft.authentication = .new(.apiKey)
        let firstName = try #require(references(first.draft.authentication).first)
        let secondName = try #require(references(second.draft.authentication).first)
        model.editSecret(name: firstName, value: "fixture-key-a")

        await model.send(first)

        #expect(await keychain.saved == [firstName: "fixture-key-a"])
        #expect(model.secretMaterial(for: .secret(secondName)).isEmpty)
    }
}

private extension ValueSource {
    var secretName: String? {
        if case let .secret(name) = self { name } else { nil }
    }
}

actor KeychainRecorder: WorkspacePersistence {
    private(set) var saved: [String: String] = [:]
    private(set) var environments: [EnvironmentDraft] = []
    private(set) var commands: [WorkspaceCommand] = []
    var failsSecrets = false
    nonisolated let location: URL?

    init(location: URL? = nil) { self.location = location }

    func load() async throws -> WorkspaceDraft { WorkspaceDraft(name: "Fixture") }
    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment: EnvironmentDraft) async throws { environments.append(environment) }
    func readSecret(name: String) async throws -> String? { saved[name] }
    func saveSecret(name: String, value: String) async throws {
        if failsSecrets { throw RunFailure(kind: "keychain", issues: []) }
        saved[name] = value
    }
    func failSecrets() { failsSecrets = true }

    func apply(_ command: WorkspaceCommand) async throws -> WorkspaceDelta {
        commands.append(command)
        if case let .saveEnvironment(environment) = command { environments.append(environment) }
        return WorkspaceDelta(version: UInt64(commands.count), kind: .workspace, affectedIDs: [])
    }
}

struct SilentRunner: RequestRunner {
    func events(for _: RunInput, runID _: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { $0.finish() }
    }

    func cancel(runID _: RunID) {}
}
