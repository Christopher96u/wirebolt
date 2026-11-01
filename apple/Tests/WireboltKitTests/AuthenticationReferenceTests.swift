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

    @Test("A new API key request projects its typed reference")
    func apiKeyProjectsOnlyReference() throws {
        var draft = RequestDraft(url: "https://api.example.com/v1/orders")
        draft.authentication = .new(.apiKey)
        let input = RunInput(draft: draft, variables: [:]).domain
        let name = try #require(references(draft.authentication).first)
        guard case let .apiKey(placement, key, value) = input.authentication else { Issue.record("Expected API key"); return }
        #expect(placement == .header)
        #expect(key == "X-API-Key")
        #expect(value == .secret(name))
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

actor KeychainRecorder: WorkspacePersistence {
    private(set) var saved: [String: String] = [:]
    private(set) var deleted: [String] = []
    private(set) var environments: [EnvironmentDraft] = []
    private(set) var commands: [SessionWorkspaceCommand] = []
    var failsSecrets = false
    nonisolated let location: URL?
    private let workspace: WorkspaceDraft

    init(location: URL? = nil, workspace: WorkspaceDraft = WorkspaceDraft(name: "Fixture"), keychain: [String: String] = [:]) {
        self.location = location
        self.workspace = workspace
        saved = keychain
    }

    func load() async throws -> WorkspaceDraft { workspace }
    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment: EnvironmentDraft) async throws { environments.append(environment) }
    func readSecret(name: String) async throws -> String? { saved[name] }
    func saveSecret(name: String, value: String) async throws {
        if failsSecrets { throw RunFailure(kind: .keychain, issues: []) }
        saved[name] = value
    }
    func deleteSecret(name: String) async throws {
        deleted.append(name)
        saved[name] = nil
    }
    func failSecrets() { failsSecrets = true }

    func apply(_ command: SessionWorkspaceCommand) async throws -> WorkspaceDelta {
        commands.append(command)
        if case let .saveEnvironment(environment) = command { environments.append(EnvironmentDraft(domain: environment)) }
        return WorkspaceDelta(version: UInt64(commands.count), kind: .workspace, affectedIDs: [])
    }
}

struct SilentRunner: RequestRunner {
    func events(for _: RunInput, runID _: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { $0.finish() }
    }

    func cancel(runID _: RunID) {}
}

extension AuthenticationReferenceTests {
    @Test func credentialNamePolicyComesFromTheDomain() {
        for name in ["auth.api-key", "oauth.client-secret", "oauth.access-token", ""] {
            let replacement = CredentialReference.editableName(for: .secret(name), role: "token")
            #expect(replacement != name)
            #expect(CredentialReference.isOwned(replacement))
        }
        #expect(CredentialReference.editableName(for: .secret("personal.token"), role: "token") == "personal.token")
    }
}
