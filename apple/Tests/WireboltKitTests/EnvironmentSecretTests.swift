import Foundation
import Testing
@testable import WireboltKit

struct EnvironmentSecretTests {
    @Test("Secret variable references are unique per environment and row and valid for Keychain")
    func secretReferences() {
        let row = EnvironmentVariableDraft(id: "variable-0", key: "apiToken", value: .literal(""))
        let staging = row.secretReference(environmentID: "staging")
        let production = row.secretReference(environmentID: "production")
        #expect(staging == "env.staging.variable-0")
        #expect(staging != production)
        var renamed = row
        renamed.key = "token"
        #expect(renamed.secretReference(environmentID: "staging") == staging)

        let odd = EnvironmentVariableDraft(id: String(repeating: "é/", count: 100), key: "x")
        let name = odd.secretReference(environmentID: "team env")
        #expect(name.count <= 128)
        #expect(name.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || ".-_".contains($0)) })
    }

    @Test("Saving environment secrets writes Keychain first and the workspace keeps only the reference")
    @MainActor
    func savesSecretValuesOutsideTheWorkspace() async throws {
        let keychain = KeychainRecorder()
        let model = WireboltModel(runner: SilentRunner(), persistence: keychain)
        let row = EnvironmentVariableDraft(id: "token-row", key: "token", value: .literal(""))
        let reference = row.secretReference(environmentID: "staging")
        var environment = EnvironmentDraft(id: "staging", name: "Staging", variables: [row])
        environment.variables[0].value = .secret(reference)

        #expect(await model.saveSecrets([reference: "fixture-environment-token"]))
        #expect(await model.saveEnvironment(environment))

        #expect(await keychain.saved[reference] == "fixture-environment-token")
        #expect(model.secretMaterial(for: .secret(reference)) == "fixture-environment-token")
        let stored = try #require(await keychain.environments.last)
        let encoded = String(decoding: try JSONEncoder().encode(stored), as: UTF8.self)
        #expect(encoded.contains(#"{"secret":"env.staging.token-row"}"#))
        #expect(!encoded.contains("fixture-environment-token"))
        #expect(model.activeVariables["token"] == .secret(reference))
    }

    @Test("A Keychain failure is reported and nothing is marked saved")
    @MainActor
    func keychainFailure() async {
        let keychain = KeychainRecorder()
        await keychain.failSecrets()
        let model = WireboltModel(runner: SilentRunner(), persistence: keychain)

        #expect(await model.saveSecrets(["env.staging.row": "fixture"]) == false)
        #expect(model.operationFailure?.kind == "keychain")
        #expect(model.secretMaterial(for: .secret("env.staging.row")).isEmpty)
    }
}
