import Foundation
import Testing
@testable import WireboltKit

/// Placeholder PEM blocks: structurally valid, but not real key material.
enum ClientIdentityFixture {
    static let leaf = "-----BEGIN CERTIFICATE-----\nZml4dHVyZS1sZWFmLWNlcnRpZmljYXRl\n-----END CERTIFICATE-----"
    static let intermediate = "-----BEGIN CERTIFICATE-----\nZml4dHVyZS1pbnRlcm1lZGlhdGU=\n-----END CERTIFICATE-----"
    static let key = "-----BEGIN PRIVATE KEY-----\nZml4dHVyZS1wcml2YXRlLWtleQ==\n-----END PRIVATE KEY-----"
}

@Suite("Client identity")
struct ClientIdentityTests {
    typealias Fixture = ClientIdentityFixture

    @Test("A certificate chain and key combine from one file or two, certificates first")
    func combine() throws {
        let separate = try ClientIdentityPEM(parsing: [Fixture.leaf + "\r\n" + Fixture.intermediate + "\n", "Bag Attributes\n" + Fixture.key])
        #expect(separate.certificates == [Fixture.leaf, Fixture.intermediate])
        #expect(separate.privateKey == Fixture.key)
        #expect(separate.combined == Fixture.leaf + "\n" + Fixture.intermediate + "\n" + Fixture.key + "\n")
        let single = try ClientIdentityPEM(parsing: [Fixture.key + "\n" + Fixture.leaf])
        #expect(single.combined == Fixture.leaf + "\n" + Fixture.key + "\n")
        #expect(try ClientIdentityPEM(parsing: [single.combined]) == single)
        let rsa = Fixture.key.replacingOccurrences(of: "PRIVATE KEY", with: "RSA PRIVATE KEY")
        #expect(try ClientIdentityPEM(parsing: [Fixture.leaf, rsa]).privateKey == rsa)
    }

    @Test("Missing, duplicate and encrypted keys are rejected with an explanation")
    func failures() {
        #expect(throws: ClientIdentityPEM.Failure.noCertificate) { try ClientIdentityPEM(parsing: [Fixture.key]) }
        #expect(throws: ClientIdentityPEM.Failure.noPrivateKey) { try ClientIdentityPEM(parsing: [Fixture.leaf]) }
        #expect(throws: ClientIdentityPEM.Failure.noCertificate) { try ClientIdentityPEM(parsing: ["not a certificate"]) }
        #expect(throws: ClientIdentityPEM.Failure.multiplePrivateKeys) {
            try ClientIdentityPEM(parsing: [Fixture.leaf, Fixture.key, Fixture.key])
        }
        let pkcs8 = Fixture.key.replacingOccurrences(of: "PRIVATE KEY", with: "ENCRYPTED PRIVATE KEY")
        #expect(throws: ClientIdentityPEM.Failure.encryptedPrivateKey) { try ClientIdentityPEM(parsing: [Fixture.leaf, pkcs8]) }
        let legacy = "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: AES-128-CBC,00\n\nZml4dHVyZQ==\n-----END RSA PRIVATE KEY-----"
        #expect(throws: ClientIdentityPEM.Failure.encryptedPrivateKey) { try ClientIdentityPEM(parsing: [Fixture.leaf, legacy]) }
        #expect(ClientIdentityPEM.Failure.encryptedPrivateKey.errorDescription?.contains("passphrase") == true)
    }
}
