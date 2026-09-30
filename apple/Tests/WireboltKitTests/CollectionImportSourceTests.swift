import Foundation
import Testing
@testable import WireboltKit

@Suite("Insomnia and Bruno import sources")
struct CollectionImportSourceTests {
    @Test("Insomnia and Bruno exports are routed to their importers")
    func detectsInsomniaAndBrunoExports() {
        let insomniaV4 = #"{"_type":"export","__export_format":4,"resources":[]}"#
        #expect(ImportFormat.detect(fileExtension: "json", contents: insomniaV4) == .insomnia)
        let insomniaV5 = """
        type: collection.insomnia.rest/5.0
        schema_version: "5.1"
        name: Payments API
        """
        #expect(ImportFormat.detect(fileExtension: "yaml", contents: insomniaV5) == .insomnia)
        #expect(ImportFormat.detect(fileExtension: "yaml", contents: "_type: export\n__export_format: 4\n") == .insomnia)
        let bruno = #"{"name":"Library","version":"1","items":[],"environments":[]}"#
        #expect(ImportFormat.detect(fileExtension: "json", contents: bruno) == .bruno)
        #expect(ImportFormat.detect(fileExtension: "bru", contents: "meta {\n  name: Ping\n}\n") == .brunoFolder)
        // Postman and Wirebolt documents keep their importers.
        #expect(ImportFormat.detect(fileExtension: "json", contents: #"{"info":{"_postman_id":"x"},"item":[]}"#) == .postmanV2)
        #expect(ImportFormat.detect(fileExtension: "json", contents: #"{"version":1,"nodes":[]}"#) == .legacyWorkspaceV1)
        #expect(ImportFormat.detect(fileExtension: "yaml", contents: "openapi: 3.1.0\n") == nil)
    }

    @Test("A Bruno folder bundles only its collection files")
    func bundlesBrunoFolder() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bruno-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let files = [
            "bruno.json": #"{"version":"1","name":"Library","type":"collection"}"#,
            "Users/folder.bru": "meta {\n  name: Members\n}\n",
            "Users/Get member.bru": "get {\n  url: {{baseUrl}}/members\n}\n",
            "environments/Local.bru": "vars {\n  baseUrl: http://127.0.0.1\n}\n",
            "node_modules/dependency/ignored.bru": "get {\n}\n",
            ".git/config.bru": "get {\n}\n",
            "assets/cover.png": "not read",
        ]
        for (path, contents) in files {
            let url = root.appendingPathComponent(path)
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try contents.write(to: url, atomically: true, encoding: .utf8)
        }

        #expect(BrunoCollectionSource.isCollection(root))
        #expect(!BrunoCollectionSource.isCollection(root.appendingPathComponent("assets")))
        let bundle = try BrunoCollectionSource.bundle(at: root)
        let document = try #require(try JSONSerialization.jsonObject(with: Data(bundle.utf8)) as? [String: Any])
        let paths = (document["files"] as? [[String: String]] ?? []).compactMap { $0["path"] }
        #expect(paths == ["Users/Get member.bru", "Users/folder.bru", "bruno.json", "environments/Local.bru"])
        #expect(document["root"] as? String == root.path)

        let single = try BrunoCollectionSource.bundle(at: root.appendingPathComponent("Users/Get member.bru"))
        #expect(single.contains(#""path":"Get member.bru""#))
        #expect(throws: BrunoCollectionSource.ReadError.notACollection) {
            try BrunoCollectionSource.bundle(at: root.appendingPathComponent("assets"))
        }
    }
}
