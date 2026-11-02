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

    @Test("Owned names are generated ones; proxy and chosen names are never owned")
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
            if case let .saveRequest(location) = command { RequestLocation(domain: location) } else { nil }
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

    private func writeWorkspace(at root: URL, requestSecret: String?, environmentSecret: String?) throws {
        let manager = FileManager.default
        let requests = root.appending(path: "collections/api/requests", directoryHint: .isDirectory)
        let environments = root.appending(path: "environments", directoryHint: .isDirectory)
        try manager.createDirectory(at: requests, withIntermediateDirectories: true)
        try manager.createDirectory(at: environments, withIntermediateDirectories: true)
        try Data("schema_version = 3\nname = \"Clone\"\n".utf8).write(to: root.appending(path: "wirebolt.toml"))
        let auth = requestSecret.map { "\n[authentication]\nkind = \"bearer\"\n\n[authentication.token]\nsecret = \"\($0)\"\n" } ?? ""
        try Data("""
        schema_version = 3
        id = "orders"
        name = "Orders"
        method = "GET"
        url = "https://api.example.com/orders"
        \(auth)
        """.utf8).write(to: requests.appending(path: "orders.toml"))
        let variable = environmentSecret.map { "\n[[variables]]\nid = \"token-row\"\nkey = \"token\"\nvalue = { secret = '\($0)' }\nenabled = true\norder = 0\n" } ?? ""
        try Data("schema_version = 3\nid = \"staging\"\nname = \"Staging\"\n\(variable)".utf8)
            .write(to: environments.appending(path: "staging.toml"))
        // Text outside request and environment documents does not count as a reference.
        try Data("\"\(tokenName)\" \"\(apiKeyName)\"".utf8).write(to: root.appending(path: "notes.txt"))
    }

    @Test("Scanning finds names in other workspaces' request and environment documents only")
    func scansOtherWorkspaces() throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "wirebolt-scan-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        try writeWorkspace(at: root.appending(path: "Checkout"), requestSecret: tokenName, environmentSecret: nil)
        try writeWorkspace(at: root.appending(path: "Payments"), requestSecret: nil, environmentSecret: apiKeyName)
        let other = "auth.9b2e6a64-5c1f-4d0e-8f7a-2b3c4d5e6f70.password"

        let found = WorkspaceSecretScan.referencedNames([tokenName, apiKeyName, other], in: [
            root.appending(path: "Checkout"), root.appending(path: "Payments"), root.appending(path: "Missing"),
        ])

        #expect(found == [tokenName, apiKeyName])
        #expect(WorkspaceSecretScan.referencedNames([other], in: [root.appending(path: "Checkout")]).isEmpty)
    }

    @Test("An item another known workspace still references is not deleted")
    func sharedWithAnotherWorkspace() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "wirebolt-clones-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let clone = root.appending(path: "Checkout clone")
        try writeWorkspace(at: clone, requestSecret: tokenName, environmentSecret: nil)
        let first = RequestDraft(id: "orders", name: "Orders", authentication: .bearer(token: .secret(tokenName)))
        let second = RequestDraft(id: "refunds", name: "Refunds", authentication: .apiKey(placement: .header, name: "X-Key", value: .secret(apiKeyName)))
        let keychain = KeychainRecorder(location: root.appending(path: "Checkout"), workspace: workspace([first, second]),
                                        keychain: [tokenName: "a", apiKeyName: "b"])
        let (model, _) = await makeModel(keychain)
        model.knownWorkspaceLocations = { [clone, root.appending(path: "Checkout"), root.appending(path: "Deleted")] }

        await model.deleteRequest(collectionID: "api", requestID: "orders")
        await model.deleteRequest(collectionID: "api", requestID: "refunds")
        await model.purgeReleasedSecrets()

        // The clone still uses the token; nothing else uses the API key; a missing folder doesn't block.
        #expect(await keychain.deleted == [apiKeyName])
        #expect(await keychain.saved[tokenName] == "a")
        #expect(!model.hasReleasableSecrets)
    }

    @Test("Removing a secret variable releases its item; purging twice deletes once")
    func environmentVariableDeletion() async throws {
        let reference = CredentialReference.unique(role: "staging", scope: "env")
        let environment = EnvironmentDraft(id: "staging", name: "Staging", variables: [
            EnvironmentVariableDraft(key: "token", value: .secret(reference)),
            EnvironmentVariableDraft(key: "host", value: .literal("api.example.com")),
        ])
        let keychain = KeychainRecorder(workspace: workspace([], environments: [environment]), keychain: [reference: "fixture"])
        let (model, _) = await makeModel(keychain)
        #expect(await model.commitEnvironment(id: environment.id, commit: .deleteVariable(id: environment.variables[0].id)) == nil)
        await model.purgeReleasedSecrets()
        await model.purgeReleasedSecrets()

        #expect(await keychain.deleted == [reference])
        #expect(!model.hasReleasableSecrets)
    }



}

@MainActor
private final class FixedTokenAuthorizer: OAuth2Authorizing {
    func acquireToken(configuration _: OAuth2Configuration) async throws -> (String, OAuth2TokenReceipt) {
        ("fixture-new-token", OAuth2TokenReceipt(expiresAt: nil, scope: nil))
    }
}
