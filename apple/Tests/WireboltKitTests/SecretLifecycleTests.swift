import Foundation
import Testing
@testable import WireboltKit

@Suite("Keychain item lifecycle")
@MainActor
struct SecretLifecycleTests {
    private let tokenName = "auth.0f8fad5b-d9cb-469f-a165-70867728950e.token"
    private let apiKeyName = "auth.7c9e6679-7425-40de-944b-e07fc1f90ae7.api-key"

    private func workspace(_ requests: [RequestDraft], environments: [EnvironmentDraft] = []) -> WorkspaceDraft {
        WorkspaceDraft(name: "Fixture", collections: [CollectionDraft(
            id: "api", name: "API",
            requests: requests.enumerated().map { RequestLocation(collectionID: "api", order: $0.offset, request: $0.element) }
        )], environments: environments)
    }

    private func makeModel(_ persistence: KeychainRecorder) async -> (WireboltModel, UndoManager) {
        let model = WireboltModel(runner: SilentRunner(), persistence: persistence, cookieDirectory: nil)
        let undoManager = UndoManager()
        undoManager.groupsByEvent = false
        model.undoManager = undoManager
        await model.loadWorkspace()
        return (model, undoManager)
    }

    @Test("Owned names are generated ones; legacy, proxy and chosen names are never owned")
    func ownership() {
        #expect(CredentialReference.isOwned(tokenName))
        #expect(CredentialReference.isOwned(CredentialReference.unique(role: "password")))
        #expect(CredentialReference.isOwned("auth.0F8FAD5B-D9CB-469F-A165-70867728950E.password"))
        #expect(CredentialReference.isOwned("import-18a2b3c4d5e6f-1f3-2-request-0-password"))
        for name in ["auth.api-key", "oauth.client-secret", "oauth.access-token", "demo.proxy.user", "github-token", "request.request-0.token"] {
            #expect(!CredentialReference.isOwned(name), "\(name)")
        }
    }

    @Test("Duplicating a request copies its credentials into the copy's own Keychain items")
    func duplicateGetsOwnSecrets() async throws {
        let source = RequestDraft(id: "orders", name: "Orders", url: "https://api.example.com/orders",
                                  authentication: .bearer(token: .secret(tokenName)))
        let keychain = KeychainRecorder(workspace: workspace([source]), keychain: [tokenName: "fixture-token"])
        let (model, _) = await makeModel(keychain)

        await model.duplicateRequest(collectionID: "api", requestID: "orders")

        let copy = try #require(model.workspace.collections[0].requests.first { $0.request.id != "orders" })
        guard case let .bearer(.secret(copyName)) = copy.request.authentication else {
            Issue.record("The copy lost its bearer reference"); return
        }
        #expect(copyName != tokenName)
        #expect(CredentialReference.isOwned(copyName))
        #expect(await keychain.saved[copyName] == "fixture-token")
        #expect(await keychain.saved[tokenName] == "fixture-token")
        #expect(model.workspace.location(collectionID: "api", requestID: "orders")?.request.authentication == source.authentication)
        let persisted = await keychain.commands.compactMap { command -> RequestLocation? in
            if case let .saveRequest(_, location) = command { location } else { nil }
        }
        #expect(persisted.last?.request.id == copy.request.id)
        #expect(persisted.last?.request.authentication == copy.request.authentication)
    }

    @Test("A deleted request keeps its Keychain item while undo can restore it")
    func deletionWaitsForUndo() async throws {
        let request = RequestDraft(id: "orders", name: "Orders", authentication: .bearer(token: .secret(tokenName)))
        let keychain = KeychainRecorder(workspace: workspace([request]), keychain: [tokenName: "fixture-token"])
        let (model, undoManager) = await makeModel(keychain)

        await model.deleteRequest(collectionID: "api", requestID: "orders")
        #expect(await keychain.deleted.isEmpty)
        undoManager.undo()
        await model.finishUndoWork()
        #expect(model.workspace.location(collectionID: "api", requestID: "orders") != nil)

        // Still referenced after the undo: switching workspaces keeps the value.
        #expect(await model.openWorkspace(using: KeychainRecorder()))
        #expect(await keychain.deleted.isEmpty)
        #expect(await keychain.saved[tokenName] == "fixture-token")
    }

    @Test("Once the undo stack is discarded, released items are deleted and shared ones stay")
    func purgeAfterDeletion() async throws {
        let shared = "github-token"
        let deleted = RequestDraft(id: "orders", name: "Orders", headers: [RequestField(name: "X-Token", value: .secret(shared))],
                                   authentication: .apiKey(placement: .header, name: "X-Key", value: .secret(apiKeyName)))
        let kept = RequestDraft(id: "legacy", name: "Legacy", headers: [RequestField(name: "X-Token", value: .secret(shared))],
                                authentication: .bearer(token: .secret(tokenName)))
        let keychain = KeychainRecorder(workspace: workspace([deleted, kept]),
                                        keychain: [apiKeyName: "a", tokenName: "b", shared: "c"])
        let (model, _) = await makeModel(keychain)

        await model.deleteRequest(collectionID: "api", requestID: "orders")
        #expect(await model.openWorkspace(using: KeychainRecorder()))

        #expect(await keychain.deleted == [apiKeyName])
        #expect(await keychain.saved[tokenName] == "b")
        #expect(await keychain.saved[shared] == "c")
    }

    @Test("Removing a secret variable releases its item; purging twice deletes once")
    func environmentVariableDeletion() async throws {
        let reference = EnvironmentVariableDraft(key: "token").makeSecretReference(environmentID: "staging")
        let environment = EnvironmentDraft(id: "staging", name: "Staging", variables: [
            EnvironmentVariableDraft(key: "token", value: .secret(reference)),
            EnvironmentVariableDraft(key: "host", value: .literal("api.example.com")),
        ])
        let keychain = KeychainRecorder(workspace: workspace([], environments: [environment]), keychain: [reference: "fixture"])
        let (model, _) = await makeModel(keychain)
        var edited = environment
        edited.variables.removeFirst()

        #expect(await model.saveEnvironment(edited))
        await model.purgeReleasedSecrets()
        await model.purgeReleasedSecrets()

        #expect(await keychain.deleted == [reference])
        #expect(!model.hasReleasableSecrets)
    }

    @Test("Saving a request with legacy shared names moves it to its own names and keeps the shared items")
    func migratesLegacyNamesOnSave() async throws {
        let request = RequestDraft(id: "orders", name: "Orders", url: "https://api.example.com/orders",
                                   authentication: .apiKey(placement: .header, name: "X-API-Key", value: .secret("auth.api-key")))
        let keychain = KeychainRecorder(workspace: workspace([request]), keychain: ["auth.api-key": "fixture-shared-key"])
        let (model, _) = await makeModel(keychain)
        let session = try #require(model.sessions.activeSession)

        #expect(await model.save(session, fallbackCollectionID: "api"))

        guard case let .apiKey(_, _, .secret(migrated)) = session.draft.authentication else {
            Issue.record("API key reference missing"); return
        }
        #expect(migrated != "auth.api-key" && CredentialReference.isOwned(migrated))
        #expect(await keychain.saved[migrated] == "fixture-shared-key")
        #expect(!session.isDirty)
        #expect(model.workspace.location(collectionID: "api", requestID: "orders")?.request.authentication == session.draft.authentication)

        await model.purgeReleasedSecrets()
        #expect(await keychain.saved["auth.api-key"] == "fixture-shared-key")
        #expect(await keychain.deleted.isEmpty)
    }

    @Test("A new OAuth token never overwrites the legacy shared token item")
    func migratesLegacyOAuthBeforeTokenRequest() async throws {
        let configuration = OAuth2Configuration(grant: .clientCredentials, tokenURL: "https://id.example.com/token", clientID: "wirebolt",
                                                clientSecretReference: "oauth.client-secret", accessTokenReference: "oauth.access-token")
        let request = RequestDraft(id: "orders", name: "Orders", authentication: .oauth2(configuration: configuration))
        let keychain = KeychainRecorder(workspace: workspace([request]),
                                        keychain: ["oauth.client-secret": "fixture-client-secret", "oauth.access-token": "fixture-old-token"])
        let model = WireboltModel(runner: SilentRunner(), persistence: keychain, cookieDirectory: nil,
                                  oauth2: FixedTokenAuthorizer())
        await model.loadWorkspace()
        let session = try #require(model.sessions.activeSession)

        await model.acquireOAuthToken(for: session)

        guard case let .oauth2(migrated) = session.draft.authentication else { Issue.record("OAuth missing"); return }
        #expect(!CredentialReference.isLegacyShared(migrated.accessTokenReference))
        #expect(await keychain.saved[migrated.accessTokenReference] == "fixture-new-token")
        #expect(await keychain.saved[migrated.clientSecretReference] == "fixture-client-secret")
        #expect(await keychain.saved["oauth.access-token"] == "fixture-old-token")
    }

    @Test("Migration renames only legacy shared names")
    func legacyEditsNeverWriteSharedItems() {
        let (authentication, renames) = RequestAuthentication.basic(username: .secret("auth.api-key"), password: .secret(tokenName))
            .withOwnKeychainNames(onlyLegacy: true)
        #expect(renames.keys.sorted() == ["auth.api-key"])
        #expect(authentication.secretReferences.contains(tokenName))
        #expect(!authentication.secretReferences.contains("auth.api-key"))
    }
}

@MainActor
private final class FixedTokenAuthorizer: OAuth2Authorizing {
    func acquireToken(configuration _: OAuth2Configuration) async throws -> (String, OAuth2TokenReceipt) {
        ("fixture-new-token", OAuth2TokenReceipt(expiresAt: nil, scope: nil))
    }
}
