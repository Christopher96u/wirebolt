import AppKit
import Foundation
import Observation
import SwiftUI

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

@main @MainActor enum NativeAdapterTests {
    static func print(_ message: String) { FileHandle.standardOutput.write(Data((message + "\n").utf8)) }

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
        let model = WireboltModel(runner: FixtureRunner(),
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

    static func main() async throws {
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
        let captureDirectory = ProcessInfo.processInfo.environment["WIREBOLT_NATIVE_CAPTURE_DIRECTORY"].map { URL(fileURLWithPath: $0) }
        try await CustomThemeNativeTests.run(root: root, defaults: defaults, screenshots: captureDirectory)
        try await EnvironmentInspectorTests.run(root: root, defaults: defaults, screenshots: captureDirectory)
        try await auditNavigationAndSettings(root: root, defaults: defaults, screenshots: captureDirectory)
        try await auditRequestEditors(defaults: defaults)
        try await auditWebSocketComposer(root: root, defaults: defaults)
        try await auditCookieRemovalFailure(root: root, defaults: defaults)
        try await auditPreparedRequestCopy(defaults: defaults)
        try await workspaceIdentitySaveFreezesSelection(root: root, defaults: defaults)
        typingAssignmentsAvoidDraftWork()
        lastTabDirtyConfirmAndReopen(root: root, defaults: defaults)
    }
}


extension NativeAdapterTests {
    static func auditModel(root: URL, defaults: UserDefaults) -> WireboltModel {
        WireboltModel(runner: FixtureRunner(), persistence: FixtureWorkspaceStore(),
            history: HistoryRepository(root: root.appending(path: "audit-history")),
            cookieJar: CookieJar(storageURL: root.appending(path: "audit-cookies")), cookieDirectory: root,
            proxyPreferences: ProxyPreferences(defaults: defaults))
    }

    static func auditNavigationAndSettings(root: URL, defaults: UserDefaults, screenshots: URL?) async throws {
        let restoreAccessibility = NativeTestSupport.enableAccessibility()
        defer { restoreAccessibility() }
        let model = auditModel(root: root, defaults: defaults)
        let request = RequestDraft(id: "audit-request", name: "Fixture request", url: "https://example.invalid/")
        model.workspace.collections = [CollectionDraft(id: "audit", name: "Fixture collection", requests: [RequestLocation(collectionID: "audit", request: request)])]
        _ = model.sessions.open(draft: request, collectionID: "audit")
        let interface = WorkspaceUIState(defaults: defaults)
        interface.synchronizeSelection(model: model)
        interface.columnVisibility = .detailOnly
        let (window, host) = await NativeTestSupport.mount(ContentView(model: model, interface: interface, loadsWorkspace: false), defaults: defaults)
        interface.focusRequestSearch()
        await NativeTestSupport.settle(host)
        precondition(interface.columnVisibility == .all)
        let field = EnvironmentInspectorTests.fields(host).first { $0.placeholderString == "Search" }!
        precondition(field.currentEditor() === window.firstResponder, "Filter reveals and focuses the previously unmounted search field")
        let sidebar = NativeTestSupport.elements(host)
        precondition(!sidebar.contains { $0.role == "AXButton" && ["Filter Requests", "Network Settings"].contains($0.name) })
        window.close()
        print("filterRequestsRevealsSidebarAndFocusesSearch: passed")
        print("sidebarOmitsRedundantFilterAndNetworkControls: passed")

        model.settingsTab = "general"
        model.isShowingAppSettings = true
        let (settingsWindow, settingsHost) = await NativeTestSupport.mount(WireboltSettingsView(model: model), defaults: defaults, height: 1050)
        defer { settingsWindow.close() }
        let general = NativeTestSupport.elements(settingsHost)
        precondition(general.contains { $0.name == "History" } && general.contains { $0.name == "Clear All History…" })
        precondition(!general.contains { ["Privacy", "Privacy Policy", "Analytics", "Crash reports"].contains($0.name) })
        if let screenshots { try NativeTestSupport.capture(settingsHost, directory: screenshots, name: "audit-settings-general-history") }
        precondition(NativeTestSupport.press(settingsHost, named: "Clear All History…"))
        func historyConfirmation() -> NSWindow? {
            NSApp.windows.first { candidate in
                candidate.isVisible && NativeTestSupport.elements(candidate).contains { $0.role == "AXButton" && $0.name == "Clear All" }
            }
        }
        await NativeTestSupport.wait(settingsHost) { historyConfirmation() != nil }
        precondition(NativeTestSupport.press(historyConfirmation()!, named: "Cancel"))
        await NativeTestSupport.wait(settingsHost) { historyConfirmation() == nil }
        precondition(model.settingsTab == "general" && model.isShowingAppSettings)
        print("generalShowsHistoryWithoutPrivacyPage: passed")
        let search = EnvironmentInspectorTests.fields(settingsHost).first { $0.accessibilityLabel() == "Search settings" }!
        EnvironmentInspectorTests.replace(search, with: "history", window: settingsWindow)
        await NativeTestSupport.settle(settingsHost)
        precondition(NativeTestSupport.elements(settingsHost).contains { $0.role == "AXButton" && $0.name == "General" })
        EnvironmentInspectorTests.replace(search, with: "privacy", window: settingsWindow)
        await NativeTestSupport.settle(settingsHost)
        precondition(!NativeTestSupport.elements(settingsHost).contains { $0.role == "AXButton" && $0.name == "Privacy" })
        EnvironmentInspectorTests.replace(search, with: "", window: settingsWindow)
        model.settingsTab = "keyboard"
        await NativeTestSupport.settle(settingsHost)
        precondition(!NativeTestSupport.elements(settingsHost).contains { $0.name == "New tab" || $0.name == "⌘T" })
        print("keyboardListsSingleNewRequestCommand: passed")
        model.settingsTab = "appearance"
        await NativeTestSupport.settle(settingsHost)
        let appearance = NativeTestSupport.elements(settingsHost)
        precondition(!appearance.contains { ["Canvas & variables", "Contrast"].contains($0.name) })
        precondition(appearance.contains { $0.name == "Follow System" } && appearance.contains { $0.name == "Custom theme from 2 colors" })
        if let screenshots { try NativeTestSupport.capture(settingsHost, directory: screenshots, name: "audit-settings-appearance") }
        print("appearanceOmitsDiagnosticRows: passed")
    }
}


@MainActor @Observable private final class NativeEditorFixture {
    var method = HTTPMethod.get
    var fields = [RequestField(name: "X-Old", value: .literal("old"))]
    var body = RequestBody.empty
    var part = MultipartPart(name: "value", value: .literal("Current multipart value"))
}

extension NativeAdapterTests {
    static func auditRequestEditors(defaults: UserDefaults) async throws {
        let restoreAccessibility = NativeTestSupport.enableAccessibility()
        defer { restoreAccessibility() }
        let fixture = NativeEditorFixture()
        let (methodWindow, methodHost) = await NativeTestSupport.mount(
            CustomHTTPMethodEditor(method: Binding(get: { fixture.method }, set: { fixture.method = $0 }), text: "bad method"), defaults: defaults, width: 500, height: 250)
        precondition(NativeTestSupport.press(methodHost, named: "Set"))
        await NativeTestSupport.settle(methodHost)
        precondition(fixture.method == .get && NativeTestSupport.elements(methodHost).contains { $0.name.contains("Enter a valid HTTP method") })
        let methodField = EnvironmentInspectorTests.fields(methodHost).first { $0.accessibilityLabel() == "Custom HTTP method" }!
        EnvironmentInspectorTests.replace(methodField, with: "FOO_BAR", window: methodWindow)
        precondition(NativeTestSupport.press(methodHost, named: "Set"))
        await NativeTestSupport.settle(methodHost)
        precondition(fixture.method.rawValue == "FOO_BAR")
        methodWindow.close()
        print("customMethodKeepsInvalidInputAndAcceptsTransportToken: passed")

        let (bulkWindow, bulkHost) = await NativeTestSupport.mount(
            BulkFieldEditor(fields: Binding(get: { fixture.fields }, set: { fixture.fields = $0 })), defaults: defaults)
        let bulk = NativeTestSupport.textViews(bulkHost).first { $0.string.contains("X-Old: old") }!
        fixture.fields = []
        await NativeTestSupport.settle(bulkHost)
        precondition(bulk.string.isEmpty, "Clear All clears the native bulk buffer")
        bulk.insertText("X-New: new", replacementRange: NSRange(location: 0, length: 0))
        await NativeTestSupport.settle(bulkHost)
        precondition(fixture.fields.map(\.name) == ["X-New"], "Typing cannot resurrect a cleared row")
        fixture.fields = [RequestField(name: "X-Replacement", value: .literal("external"))]
        await NativeTestSupport.settle(bulkHost)
        precondition(bulk.string == "X-Replacement: external")
        bulk.insertText("\nX-Another: new", replacementRange: NSRange(location: bulk.string.utf16.count, length: 0))
        await NativeTestSupport.settle(bulkHost)
        precondition(fixture.fields.map(\.name) == ["X-Replacement", "X-Another"])
        bulkWindow.close()
        print("bulkClearAndExternalReplacementNeverResurrectRows: passed")

        let (headersWindow, headersHost) = await NativeTestSupport.mount(ResponseHeadersTable(headers: [ResponseHeader(name: "A-One", value: "one"), ResponseHeader(name: "B-Two", value: "two")]), defaults: defaults)
        precondition(NativeTestSupport.press(headersHost, named: "Find"))
        await NativeTestSupport.settle(headersHost)
        let find = EnvironmentInspectorTests.fields(headersHost).first { $0.placeholderString == "Find" }!
        EnvironmentInspectorTests.replace(find, with: "A-One", window: headersWindow)
        await NativeTestSupport.settle(headersHost)
        precondition(!NativeTestSupport.elements(headersHost).contains { $0.name == "B-Two" })
        precondition(NativeTestSupport.press(headersHost, named: "Find"))
        await NativeTestSupport.settle(headersHost)
        precondition(NativeTestSupport.elements(headersHost).contains { $0.name == "B-Two" })
        headersWindow.close()
        print("closingHeaderFindClearsHiddenFilter: passed")

        let (partWindow, partHost) = await NativeTestSupport.mount(MultipartPartEditor(part: Binding(get: { fixture.part }, set: { fixture.part = $0 }), remove: {}), defaults: defaults, width: 560, height: 400)
        let edit = NativeTestSupport.elements(partHost).first { $0.role == "AXButton" && $0.name == "Edit Value…" }!
        precondition(edit.object.value(forKey: "accessibilityValue") as? String == "Current multipart value")
        precondition(NativeTestSupport.press(partHost, named: "Edit Value…"))
        await NativeTestSupport.settle(partHost)
        precondition(partWindow.attachedSheet != nil, "The value editor is reachable through a standard button")
        if let sheet = partWindow.attachedSheet { precondition(NativeTestSupport.press(sheet, named: "Cancel")) }
        await NativeTestSupport.settle(partHost)
        partWindow.close()
        print("multipartValueHasAccessibleEditButtonAndPreview: passed")

        let (toolsWindow, toolsHost) = await NativeTestSupport.mount(BodyTools(requestBody: Binding(get: { fixture.body }, set: { fixture.body = $0 })), defaults: defaults)
        for body in [RequestBody.empty, .raw(contentType: nil, value: "text"), .xml(value: "<a/>"), .html(value: "<p>a</p>"), .formURLEncoded(fields: []), .file(path: "fixture", contentType: nil), .multipart(parts: []), .json(value: "{}") ] {
            fixture.body = body
            await NativeTestSupport.settle(toolsHost)
            precondition(NativeTestSupport.elements(toolsHost).contains { $0.name == "Format Body" } == (body.composerKind == .json))
        }
        toolsWindow.close()
        print("formatBodyAppearsOnlyForJSON: passed")
    }
}


extension NativeAdapterTests {
    static func auditWebSocketComposer(root: URL, defaults: UserDefaults) async throws {
        let restoreAccessibility = NativeTestSupport.enableAccessibility()
        defer { restoreAccessibility() }
        let model = auditModel(root: root, defaults: defaults)
        let draft = RequestDraft(id: "fixture-socket", name: "Socket fixture", url: "ws://example.invalid/", webSocket: true)
        let session = model.sessions.open(draft: draft, kind: .webSocket)
        let interface = WorkspaceUIState(defaults: defaults)
        interface.synchronizeSelection(model: model)
        interface.requestSection = .body
        let (window, host) = await NativeTestSupport.mount(ContentView(model: model, interface: interface, loadsWorkspace: false), defaults: defaults)
        defer { session.socket.disconnect(); window.close() }
        let socket = FixtureSocket()
        session.socket.connect(input: RunInput(draft: draft, variables: [:]), connector: FixtureConnector(socket: socket))
        session.draft.url = ""
        await NativeTestSupport.settle(host)
        func assertDisconnectEnabled() {
            let disconnect = NativeTestSupport.elements(host).first { $0.name == "Disconnect" }!
            precondition(disconnect.object.value(forKey: "accessibilityEnabled") as? Bool == true)
        }
        assertDisconnectEnabled()
        socket.continuation.yield(.connected([]))
        await NativeTestSupport.wait(host) { session.socket.status == .connected }
        await NativeTestSupport.settle(host)
        assertDisconnectEnabled()
        for body in [RequestBody.json(value: "{"), .webSocketMessage(.binary).editingMessage("not base64"), .file(path: root.appending(path: "missing-message").path, contentType: nil)] {
            await session.socket.send(body: body)
            await NativeTestSupport.settle(host)
            let error = session.socket.errorMessage!
            precondition(NativeTestSupport.elements(host).contains { $0.name.contains(error) }, "Message errors remain beside the composer while connected")
        }
        socket.acceptsMessages = false
        await session.socket.send(body: .raw(contentType: nil, value: "fixture"))
        session.socket.clearMessages()
        await NativeTestSupport.settle(host)
        precondition(NativeTestSupport.elements(host).contains { $0.name.contains("could not be queued") })
        precondition(NativeTestSupport.press(host, named: "Disconnect"))
        await NativeTestSupport.settle(host)
        precondition(session.socket.status == .disconnected)
        print("webSocketSendErrorsRemainVisibleAcrossMessageStates: passed")
        print("disconnectRemainsEnabledWithoutURLWhileConnectingAndConnected: passed")
    }
}


extension NativeAdapterTests {
    static func auditCookieRemovalFailure(root: URL, defaults: UserDefaults) async throws {
        let restoreAccessibility = NativeTestSupport.enableAccessibility()
        defer { restoreAccessibility() }
        let directory = root.appending(path: "cookie-failure")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let jar = CookieJar(storageURL: directory.appending(path: "cookies.json"))
        await jar.store(headers: [ResponseHeader(name: "Set-Cookie", value: "fixture=value; Max-Age=600; Path=/")], requestURL: URL(string: "https://example.invalid/")!)
        let model = WireboltModel(runner: FixtureRunner(), persistence: FixtureWorkspaceStore(), history: HistoryRepository(root: root.appending(path: "cookie-failure-history")), cookieJar: jar, cookieDirectory: nil, proxyPreferences: ProxyPreferences(defaults: defaults))
        let (window, host) = await NativeTestSupport.mount(CookiesView(model: model), defaults: defaults)
        defer { window.close() }
        let backup = root.appending(path: "cookie-failure-backup")
        try FileManager.default.moveItem(at: directory, to: backup)
        try Data("blocked".utf8).write(to: directory)
        precondition(NativeTestSupport.press(host, named: "Clear All…"))
        await NativeTestSupport.settle(host)
        let confirmation = NSApp.windows.first { $0.isVisible && NativeTestSupport.elements($0).contains { $0.role == "AXButton" && $0.name == "Clear All" } }!
        precondition(NativeTestSupport.press(confirmation, named: "Clear All"))
        await NativeTestSupport.settle(host)
        precondition(NativeTestSupport.elements(host).contains { $0.name.hasPrefix("Cookie error:") })
        let retainedCookies = await model.cookies()
        precondition(retainedCookies.count == 1)
        precondition(NativeTestSupport.elements(host).contains { $0.name == "1 cookie" })
        try FileManager.default.removeItem(at: directory)
        try FileManager.default.moveItem(at: backup, to: directory)
        print("cookieClearFailureRemainsVisibleAndRetainsRows: passed")
    }
}


extension NativeAdapterTests {
    static func auditPreparedRequestCopy(defaults: UserDefaults) async throws {
        let restoreAccessibility = NativeTestSupport.enableAccessibility()
        defer { restoreAccessibility() }
        let (emptyWindow, emptyHost) = await NativeTestSupport.mount(PreparedRequestViewer(snapshot: nil), defaults: defaults)
        precondition(NativeTestSupport.elements(emptyHost).contains { $0.name.contains("redacted prepared details and body preview") })
        precondition(!NativeTestSupport.elements(emptyHost).contains { $0.name.contains("exactly what was transmitted") })
        emptyWindow.close()
        let snapshot = PreparedRunSnapshot(method: "POST", url: "https://example.invalid/", headers: [], body: PreparedBodySnapshot(byteCount: 100_000, contentType: "text/plain", textPreview: "fixture preview", redacted: false), transport: TransportSettings())
        let (window, host) = await NativeTestSupport.mount(PreparedRequestViewer(snapshot: snapshot), defaults: defaults)
        let names = NativeTestSupport.elements(host).map(\.name)
        precondition(names.contains("Prepared Request") && names.contains { $0.contains("Reconstructed from the prepared request") && $0.contains("body may be a preview") })
        precondition(!names.contains("Raw Request"))
        window.close()
        print("preparedRequestExplainsReconstructionRedactionAndPreview: passed")
    }
}


extension NativeAdapterTests {
    static func workspaceIdentitySaveFreezesSelection(root: URL, defaults: UserDefaults) async throws {
        let restoreAccessibility = NativeTestSupport.enableAccessibility()
        defer { restoreAccessibility() }
        let store = FixtureIdentityStore()
        let model = WireboltModel(runner: FixtureRunner(), persistence: store,
            history: HistoryRepository(root: root.appending(path: "identity-history")),
            cookieJar: CookieJar(storageURL: root.appending(path: "identity-cookies")),
            proxyPreferences: ProxyPreferences(defaults: defaults))
        let name = CredentialReference.unique(role: "client-identity", scope: "tls")
        model.workspace.transport.clientCertificateReference = name
        let staged = AppSessionSecrets()
        staged.edit(name: name, value: "fixture private key")
        var closeCount = 0
        let (window, host) = await NativeTestSupport.mount(NetworkSettingsPage(model: model, scope: .workspace,
            onClose: { closeCount += 1 }, stagedIdentities: staged), defaults: defaults, width: 700, height: 1100)
        defer { store.gate.release(); window.close() }
        precondition(NativeTestSupport.press(host, named: "Timeouts, redirects & TLS"))
        await NativeTestSupport.settle(host)
        precondition(NativeTestSupport.press(host, named: "Validate TLS certificates"))
        await NativeTestSupport.settle(host)
        precondition(NativeTestSupport.press(host, named: "Save"))
        await NativeTestSupport.wait(host) { store.gate.started }
        await NativeTestSupport.settle(host)
        let controls = NativeTestSupport.elements(host)
        for title in ["Save", "Cancel", "Remove", "Replace…", "Validate TLS certificates"] {
            let control = controls.first { $0.name == title }!
            precondition(control.object.value(forKey: "accessibilityEnabled") as? Bool == false, "Saving freezes \(title)")
        }
        _ = NativeTestSupport.press(host, named: "Cancel")
        _ = NativeTestSupport.press(host, named: "Remove")
        precondition(closeCount == 0 && model.workspace.transport.clientCertificateReference == name)
        store.gate.release()
        await NativeTestSupport.wait(host) { closeCount == 1 }
        precondition(model.workspace.transport.clientCertificateReference == name && !model.workspace.transport.validateTLS)
        let saved = await store.saved
        precondition(saved == [name])
        print("workspaceIdentitySaveFreezesSelectionUntilKeychainCommit: passed")
    }
}
