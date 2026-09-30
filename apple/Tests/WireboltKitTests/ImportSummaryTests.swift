import Foundation
import Testing
@testable import WireboltKit

@Suite("Import summary and failures")
struct ImportSummaryTests {
    @Test("A committed import decodes its delta and summary from one bridge document")
    func decodesCommitDocument() throws {
        let json = #"""
        {"version":7,"kind":"collection","affected_ids":["import-1-0"],
         "summary":{"collection_name":"Shop API","collection_names":["Shop API"],"request_count":9,"group_count":3,
                    "environments":[{"id":"import-1-env-0","name":"Shop API"}],
                    "warnings":["Scripts aren't supported and weren't imported: Shop API (pre-request)."]}}
        """#
        let result = try JSONDecoder().decode(ImportResult.self, from: Data(json.utf8))

        #expect(result.delta.affectedIDs == ["import-1-0"])
        #expect(result.summary.requestCount == 9)
        #expect(result.summary.environments == [.init(id: "import-1-env-0", name: "Shop API")])
        #expect(result.summary.needsReview)
        #expect(result.summary.headline == "Imported 9 requests in 3 folders into “Shop API”.")
        #expect(result.summary.environmentLine == "Created the environment “Shop API” for its variables.")
    }

    @Test("Summaries without warnings or environments need no review")
    func cleanSummary() {
        let summary = ImportSummary(collectionNames: ["Imported cURL"], requestCount: 1, groupCount: 0)
        #expect(!summary.needsReview)
        #expect(summary.headline == "Imported 1 request into “Imported cURL”.")
        #expect(summary.environmentLine == nil)
        #expect(ImportSummary(collectionNames: ["A", "B"], requestCount: 2, groupCount: 1).headline
            == "Imported 2 requests in 1 folder into 2 collections.")
    }

    @Test("Failure messages carry the importer's reason as a sentence")
    func failureMessages() {
        struct Reason: LocalizedError { var errorDescription: String? }
        #expect(ImportFormat.failureMessage(for: Reason(errorDescription: "legacy collection has no nodes"), fileName: "old.json")
            == "“old.json” couldn’t be imported. Legacy collection has no nodes.")
        #expect(ImportFormat.failureMessage(for: WorkspaceMutationError.unsupported, fileName: nil)
            == "The import failed. Check its format and contents.")
    }

    @Test("Unrecognized files say why, including OpenAPI documents")
    func unrecognizedFiles() {
        #expect(ImportFormat.unrecognizedMessage(fileName: "pets.yaml", contents: "openapi: 3.1.0\ninfo:\n  title: Pets")
            .contains("OpenAPI import isn’t supported yet"))
        #expect(ImportFormat.unrecognizedMessage(fileName: "pets.json", contents: #"{"swagger":"2.0"}"#)
            .contains("OpenAPI"))
        let other = ImportFormat.unrecognizedMessage(fileName: "notes.txt", contents: "hello")
        #expect(other.hasPrefix("“notes.txt” isn’t a format Wirebolt can import."))
        #expect(other.contains("Postman Collection v2"))
    }

    @Test("A successful import publishes its summary and selects the environment it created")
    @MainActor func successfulImportPublishesSummary() async {
        let persistence = ImportPersistence(outcome: .success)
        let model = WireboltModel(runner: ImportRunner(), persistence: persistence)
        model.selectedEnvironmentID = nil

        await model.importDocument(source: "{}", format: .postmanV2, url: URL(fileURLWithPath: "/tmp/shop.json"))

        #expect(model.importFailureMessage == nil)
        #expect(model.importSummary?.warnings == ["Scripts weren't imported."])
        #expect(model.selectedEnvironmentID == "import-env-0")
        #expect(await persistence.importedName == "shop")
        #expect(model.sessions.activeSession?.draft.id == "request-0")
    }

    @Test("A failed import reports the reason and leaves no summary")
    @MainActor func failedImportReportsReason() async {
        let model = WireboltModel(runner: ImportRunner(), persistence: ImportPersistence(outcome: .failure))

        await model.importDocument(source: "{}", format: .har, url: URL(fileURLWithPath: "/tmp/capture.har"))

        #expect(model.importFailureMessage == "“capture.har” couldn’t be imported. The HAR file has no HTTP requests to import.")
        #expect(model.importSummary == nil)
        #expect(await model.importPastedDocument(source: "curl", format: .curl)
            == "The import failed. The HAR file has no HTTP requests to import.")
    }
}

private struct ImportRunner: RequestRunner {
    func events(for _: RunInput, runID _: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { $0.finish() }
    }

    func cancel(runID _: RunID) {}
}

private actor ImportPersistence: WorkspacePersistence {
    enum Outcome { case success, failure }
    struct Failure: LocalizedError {
        var errorDescription: String? { "The HAR file has no HTTP requests to import." }
    }

    let outcome: Outcome
    private(set) var importedName: String?
    private var committed = false

    init(outcome: Outcome) { self.outcome = outcome }

    func load() async throws -> WorkspaceDraft {
        guard committed else { return WorkspaceDraft(name: "Imports") }
        let request = RequestLocation(collectionID: "import-0", request: RequestDraft(id: "request-0", name: "List"))
        return WorkspaceDraft(
            name: "Imports",
            collections: [CollectionDraft(id: "import-0", name: "Shop API", requests: [request])],
            environments: [
                EnvironmentDraft(id: "existing", name: "Existing"),
                EnvironmentDraft(id: "import-env-0", name: "Shop API"),
            ]
        )
    }

    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}

    func commitImport(format _: ImportFormat, source _: String) async throws -> ImportResult {
        guard outcome == .success else { throw Failure() }
        committed = true
        return ImportResult(
            delta: WorkspaceDelta(version: 1, kind: .collection, affectedIDs: ["import-0"]),
            summary: ImportSummary(
                collectionNames: ["Shop API"],
                requestCount: 1,
                groupCount: 0,
                environments: [.init(id: "import-env-0", name: "Shop API")],
                warnings: ["Scripts weren't imported."]
            )
        )
    }

    func commitImportFile(format: ImportFormat, source: String, name: String) async throws -> ImportResult {
        importedName = name
        return try await commitImport(format: format, source: source)
    }
}
