import AppKit
import Foundation
import Observation

// Called only by temporary instrumented compiler inputs, including the generated
// draft writer. Keep this accounting entirely out of shipping types.
enum DraftWorkProbe {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var encodings = 0
    nonisolated(unsafe) private static var comparisons = 0
    static func encoded() { lock.withLock { encodings += 1 } }
    static func compared() { lock.withLock { comparisons += 1 } }
    static func take() -> (encodings: Int, comparisons: Int) {
        lock.withLock {
            defer { encodings = 0; comparisons = 0 }
            return (encodings, comparisons)
        }
    }
}

// The observable assignment/dirty-touch behavior on 5511d4a. This includes
// Observation's own equality checks, which a hand-counted bound would miss.
@MainActor @Observable private final class BaselineDraftSession {
    var draft: RequestDraft {
        didSet { if !isTouched, draft != savedDraft { isTouched = true } }
    }
    var isTouched = false
    var savedDraft: RequestDraft?
    init(_ draft: RequestDraft) { self.draft = draft; savedDraft = draft }
}

private struct AdapterRunner: RequestRunner {
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { $0.finish() }
    }
    func cancel(runID: RunID) {}
}

@main @MainActor enum NativeAdapterTests {
    static func typingAssignmentsAvoidDraftWork() {
        let body = String(repeating: "x", count: 1_048_576)
        let draft = RequestDraft(id: "typing", body: .json(value: body))
        let session = DocumentSession(draft: draft, savedDraft: draft)
        let baseline = BaselineDraftSession(draft)
        // Include typing, deleting back to clean, and no-op body writes.
        let edits = (0..<60).map { $0 % 3 == 0 ? body + "!" : body }
        let before = session.rust.bridgeTraffic()
        _ = DraftWorkProbe.take()
        for text in edits { session.draft.body = .json(value: text) }
        let actual = DraftWorkProbe.take()
        for text in edits { baseline.draft.body = .json(value: text) }
        let original = DraftWorkProbe.take()
        let after = session.rust.bridgeTraffic()
        FileHandle.standardError.write(Data("typingAssignmentsAvoidDraftWork: encodings=\(actual.encodings), comparisons=\(actual.comparisons), baseline_comparisons=\(original.comparisons)\n".utf8))
        precondition(actual.encodings == 0, "Editor assignments must never encode drafts")
        precondition(actual.comparisons <= original.comparisons, "No extra full-draft equality checks")
        precondition(after.documentReads == before.documentReads && after.draftWrites == before.draftWrites)
        precondition(session.draftSynchronizationCount == 0)
        session.synchronizeDraft()
        precondition(session.draftSynchronizationCount == 1, "Every assignment must mark the draft for sync")
    }

    static func lastTabDirtyConfirmAndReopen(root: URL, defaults: UserDefaults) {
        let model = WireboltModel(runner: AdapterRunner(),
            history: HistoryRepository(root: root.appending(path: "history")),
            cookieJar: CookieJar(storageURL: root.appending(path: "cookies.json")),
            proxyPreferences: ProxyPreferences(defaults: defaults))
        let interface = WorkspaceUIState(defaults: defaults)
        let session = model.sessions.open(draft: RequestDraft(id: "last", name: "Saved"))
        interface.synchronizeSelection(model: model)
        let presentation = interface.presentation(for: session)
        session.draft.note = "Unsaved"
        interface.closeTab(id: session.id, model: model)
        precondition(interface.dirtyCloseRequest?.documentIDs == [session.id])
        interface.isShowingDirtyClose = false
        precondition(model.sessions.activeSession === session && session.draft.note == "Unsaved")
        interface.closeTab(id: session.id, model: model)
        interface.confirmDirtyClose(model: model)
        precondition(model.sessions.sessions.isEmpty && interface.dirtyCloseRequest == nil)
        interface.reopenLastDocument(model: model)
        precondition(model.sessions.activeSession === session)
        precondition(interface.presentation(for: session) === presentation)
        precondition(session.draft.note.isEmpty && !session.isDirty)
        session.draft.note = "After reopen"
        session.synchronizeDraft()
        precondition(session.rust.document().draft.note == "After reopen")
        print("lastTabDirtyConfirmAndReopen: passed")
    }

    static func main() throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let root = FileManager.default.temporaryDirectory.appending(path: "wirebolt-adapters-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let domain = "wirebolt.adapter-tests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: domain)!
        defer {
            defaults.removePersistentDomain(forName: domain)
            try? FileManager.default.removeItem(at: root)
        }
        typingAssignmentsAvoidDraftWork()
        lastTabDirtyConfirmAndReopen(root: root, defaults: defaults)
    }
}
