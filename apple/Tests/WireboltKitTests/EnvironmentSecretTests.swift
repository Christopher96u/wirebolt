import Foundation
import Testing
@testable import WireboltKit

struct EnvironmentSecretTests {
    @Test("Saving environment secrets writes Keychain first and the workspace keeps only the reference")
    @MainActor
    func savesSecretValuesOutsideTheWorkspace() async throws {
        let keychain = KeychainRecorder()
        let model = WireboltModel(runner: SilentRunner(), persistence: keychain)
        let row = EnvironmentVariableDraft(id: "token-row", key: "token", value: .literal(""))
        let environment = EnvironmentDraft(id: "staging", name: "Staging", variables: [row])
        model.workspace.environments = [environment]
        model.selectedEnvironmentID = environment.id
        #expect(await model.commitEnvironment(id: environment.id, commit: .variableValue(id: row.id, value: "fixture-environment-token")) == nil)
        #expect(await model.commitEnvironment(id: environment.id, commit: .variableSecret(id: row.id, secret: true)) == nil)
        let saved = try #require(model.workspace.environments.first)
        let savedReference = try #require(saved.variables.first?.value.secretName)
        #expect(savedReference.hasPrefix("env.") && savedReference.hasSuffix(".staging"))
        #expect(CredentialReference.isOwned(savedReference))

        #expect(await keychain.saved[savedReference] == "fixture-environment-token")
        #expect(model.secretMaterial(for: .secret(savedReference)) == "fixture-environment-token")
        let stored = try #require(await keychain.environments.last)
        #expect(stored.domain.variables.first?.value == .secret(savedReference))
        #expect(model.activeVariables["token"] == .secret(savedReference))
    }

    @Test("A Keychain failure is reported and nothing is marked saved")
    @MainActor
    func keychainFailure() async {
        let keychain = KeychainRecorder()
        await keychain.failSecrets()
        let model = WireboltModel(runner: SilentRunner(), persistence: keychain)

        let environment = EnvironmentDraft(id: "staging", name: "Staging", variables: [
            EnvironmentVariableDraft(id: "row", key: "token", value: .literal("fixture")),
        ])
        model.workspace.environments = [environment]
        #expect(await model.commitEnvironment(id: environment.id, commit: .variableSecret(id: "row", secret: true)) != nil)
        #expect(model.operationFailure == nil)
        #expect(model.workspace.environments == [environment])
    }
}
