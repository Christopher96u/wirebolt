import AppKit
import SwiftUI

@MainActor enum CommandPaletteNativeTests {
    static func print(_ message: String) { FileHandle.standardOutput.write(Data((message + "\n").utf8)) }
    static func key(_ code: UInt16, _ window: NSWindow, characters: String = "", modifiers: NSEvent.ModifierFlags = []) -> NSEvent {
        NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
            timestamp: 0, windowNumber: window.windowNumber, context: nil, characters: characters,
            charactersIgnoringModifiers: characters, isARepeat: false, keyCode: code)!
    }
    static func open(_ interface: WorkspaceUIState, model: WireboltModel, window: NSWindow, host: NSView) async -> CommandPaletteController {
        window.makeKey()
        interface.toggleCommandPalette(model: model, in: window)
        await NativeTestSupport.wait(host) { interface.commandPalette?.presentation == .presented }
        await NativeTestSupport.settle(host)
        let controller = interface.commandPalette!
        precondition((window.firstResponder as? NSTextView)?.isFieldEditor == true)
        return controller
    }
    static func choose(_ command: WorkspaceCommand, controller: CommandPaletteController) {
        let title = workspaceCommandDescriptor(command: command, context: CommandContext(hasSession: true, webSocket: false, hasName: true, hasCollections: true, canGoBack: true, canGoForward: true, tabCount: 2, sidebarHidden: false)).title
        controller.query(">" + title)
        let result = controller.projection.snapshot!.results.first { $0.destination == .command(command: command) }!
        controller.confirm(result.id)
    }
    static func run(root: URL, defaults: UserDefaults, screenshots: URL?) async throws {
        let restoreAX = NativeTestSupport.enableAccessibility()
        defer { restoreAX() }
        let globals = EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Globals")
        let request = RequestDraft(id: "list", name: "List products", url: "https://example.invalid/secret-url-sentinel", note: "Fixture notes", body: .json(value: "{\"private\":true}"))
        let create = RequestDraft(id: "create", name: "Create product", method: .post)
        let workspace = WorkspaceDraft(name: "Catalog API", collections: [CollectionDraft(id: "catalog", name: "Catalog API",
            groups: [GroupDraft(id: "products", name: "Products")], requests: [
                RequestLocation(collectionID: "catalog", groupID: "products", request: request),
                RequestLocation(collectionID: "catalog", groupID: "products", request: create)])],
            environments: [globals, EnvironmentDraft(id: "staging", name: "Staging", variables: [EnvironmentVariableDraft(id: "base", key: "base_url", value: .literal("environment-sentinel"))]), EnvironmentDraft(id: "production", name: "Production")])
        let store = PaletteFixtureStore(workspace)
        let transport = PaletteCountingRunner()
        let model = WireboltModel(runner: transport, persistence: store,
            history: HistoryRepository(root: root.appending(path: "palette-history")),
            cookieJar: CookieJar(storageURL: root.appending(path: "palette-cookies")), cookieDirectory: root,
            proxyPreferences: ProxyPreferences(defaults: defaults))
        await model.loadWorkspace()
        let interface = WorkspaceUIState(defaults: defaults)
        let themes = ThemeStore(libraryURL: root.appending(path: "palette-themes.json"))
        let custom = await themes.save(name: "Harbor", preview: domainDeriveTheme(background: PaletteColor(hex: 0x101923), accent: PaletteColor(hex: 0x64cec7)))!
        interface.commandPalette = CommandPaletteController(model: model, interface: interface, defaults: defaults, themes: themes)
        interface.activateSavedRequest(workspace.location(collectionID: "catalog", requestID: "list")!, model: model, preview: false)
        let (window, host) = await NativeTestSupport.mount(ContentView(model: model, interface: interface, loadsWorkspace: false), defaults: defaults)
        interface.isWorkspaceWindowActive = { [weak window] in $0 === window }
        WorkspaceWindowRegistry.primary = window
        defer { interface.commandPalette?.dismiss(restoringFocus: false); window.close(); WorkspaceWindowRegistry.primary = nil }
        let active = model.sessions.activeSession!
        // Exercise a running session without contacting a server or Keychain.
        let run = await RuntimeRunFixture.start(active, directory: root)
        let runID = active.activeRunID
        let beforeTraffic = active.rust.bridgeTraffic()
        let beforeSync = active.draftSynchronizationCount
        let initialGroups = model.sessions.groups.map { ($0.id, $0.tabIDs, $0.selectedTabID, $0.backwardTabIDs, $0.forwardTabIDs) }
        let initialDraft = active.draft
        let originalAppearance = defaults.string(forKey: "interfaceAppearance")
        model.selectedEnvironmentID = "staging"
        interface.sidebarFilter = "hidden by sidebar filter"
        interface.collapsedSidebarCollections = ["catalog"]
        let controller = await open(interface, model: model, window: window, host: host)
        if let screenshots { try NativeTestSupport.capture(host, directory: screenshots, name: "command-palette-root") }
        for query in ["products", ">theme", "secret-url-sentinel", "environment-sentinel", ""] {
            controller.query(query)
            controller.projection.move(1)
        }
        precondition(!interface.dispatch(.newHttp, model: model))
        interface.closeActiveTab(model: model)
        interface.selectTab(offset: 1, model: model)
        interface.addKey()
        precondition(controller.handleKey(key(48, window, modifiers: .control)))
        precondition(transport.executionCount == 0 && transport.cancellationCount == 0)
        precondition(active.draft == initialDraft && active.activeRunID == runID)
        precondition(model.selectedEnvironmentID == "staging" && defaults.string(forKey: "interfaceAppearance") == originalAppearance)
        precondition(active.draftSynchronizationCount == beforeSync)
        let afterTraffic = active.rust.bridgeTraffic()
        precondition(afterTraffic.documentReads == beforeTraffic.documentReads && afterTraffic.draftWrites == beforeTraffic.draftWrites)
        for (group, initial) in zip(model.sessions.groups, initialGroups) {
            precondition(group.id == initial.0 && group.tabIDs == initial.1 && group.selectedTabID == initial.2 && group.backwardTabIDs == initial.3 && group.forwardTabIDs == initial.4)
        }
        precondition(controller.handleKey(key(13, window, characters: "w", modifiers: .command)))
        await NativeTestSupport.settle(host)
        precondition(!controller.isOpen && model.sessions.activeSession === active)
        print("paletteHighlightCancelAndDispatchPreserveDocumentsEnvironmentAppearanceHistoryAndRun: passed")

        // Native focus and selection restoration from all editable surface types.
        for section in [RequestPanelSection.params, .body, .note] {
            interface.requestSection = section
            await NativeTestSupport.settle(host)
            let responder: NSView
            if section == .params {
                responder = EnvironmentInspectorTests.fields(host).first { $0.placeholderString == "URL" || $0.stringValue.contains("example.invalid") } ?? EnvironmentInspectorTests.fields(host).first { $0.isEditable }!
            } else {
                responder = NativeTestSupport.textViews(host).first { $0.isEditable && !$0.isFieldEditor }!
            }
            window.makeFirstResponder(responder)
            (window.firstResponder as? NSTextView)?.setSelectedRange(NSRange(location: 1, length: 2))
            let previous = window.firstResponder
            let selection = (previous as? NSTextView)?.selectedRange()
            _ = await open(interface, model: model, window: window, host: host)
            precondition(controller.handleKey(key(53, window)))
            await NativeTestSupport.settle(host)
            precondition(window.firstResponder === previous)
            if let selection { precondition((window.firstResponder as? NSTextView)?.selectedRange() == selection) }
        }
        interface.requestSection = .headers
        active.draft.headers = [RequestField(id: "header", name: "Accept", value: .literal("application/json"))]
        await NativeTestSupport.settle(host)
        if let field = EnvironmentInspectorTests.fields(host).first(where: { $0.stringValue == "Accept" }) {
            window.makeFirstResponder(field)
            _ = await open(interface, model: model, window: window, host: host)
            controller.dismiss(restoringFocus: true)
            await NativeTestSupport.settle(host)
            precondition(field.currentEditor() === window.firstResponder)
        } else { preconditionFailure("Table cell must be available") }
        interface.isShowingEnvironmentInspector = true
        await NativeTestSupport.settle(host)
        let field = EnvironmentInspectorTests.fields(host).first { $0.accessibilityLabel() == "Variable name" }!
        window.makeFirstResponder(field)
        _ = await open(interface, model: model, window: window, host: host)
        controller.dismiss(restoringFocus: true)
        await NativeTestSupport.settle(host)
        precondition(field.currentEditor() === window.firstResponder)
        print("paletteRestoresURLBodyNotesTableAndInspectorFocus: passed")

        _ = await open(interface, model: model, window: window, host: host)
        controller.query("product")
        await NativeTestSupport.settle(host)
        if let screenshots { try NativeTestSupport.capture(host, directory: screenshots, name: "command-palette-requests") }
        let requestResult = controller.projection.snapshot!.results.first { $0.destination == .request(collectionId: "catalog", requestId: "create") }!
        controller.confirm(requestResult.id)
        await NativeTestSupport.settle(host)
        precondition(model.sessions.activeSession?.requestID == "create" && !model.sessions.isPreview(tabID: model.sessions.activeSession!.id))
        // A request in another split focuses that exact existing session and pins previews.
        interface.openInNewSplit(tabID: active.id, model: model)
        let splitSession = model.sessions.activeSession!
        splitSession.draft.name = "Unsaved split title"
        splitSession.draft.body = .json(value: "split draft")
        let splitGroup = model.sessions.activeGroupID
        let splitDraft = splitSession.draft
        interface.activateTab(id: active.id, groupID: initialGroups[0].0, model: model)
        _ = await open(interface, model: model, window: window, host: host)
        controller.query("unsaved split")
        controller.confirm()
        precondition(model.sessions.activeSession === splitSession && model.sessions.activeGroupID == splitGroup && splitSession.draft == splitDraft)
        _ = await open(interface, model: model, window: window, host: host)
        choose(.filterRequests, controller: controller)
        await NativeTestSupport.settle(host)
        precondition(interface.columnVisibility == .all && !controller.isOpen)
        print("paletteConfirmsSavedRequestAndExactSplitTabWithCurrentDraftMetadata: passed")

        _ = await open(interface, model: model, window: window, host: host)
        controller.query(">")
        await NativeTestSupport.settle(host)
        if let screenshots { try NativeTestSupport.capture(host, directory: screenshots, name: "command-palette-actions") }
        controller.query(">environment")
        let parent = controller.projection.snapshot!
        choose(.switchEnvironment, controller: controller)
        precondition(controller.projection.snapshot?.page == .environments)
        await NativeTestSupport.settle(host)
        if let screenshots { try NativeTestSupport.capture(host, directory: screenshots, name: "command-palette-environments") }
        controller.query("")
        precondition(controller.handleKey(key(51, window)))
        precondition(controller.projection.snapshot?.page == .root)
        // choose() changes the query; verify the actual parent preserved by the core.
        precondition(controller.projection.snapshot?.selectedId == parent.selectedId || controller.projection.snapshot?.query == ">Switch environment…")
        choose(.switchEnvironment, controller: controller)
        controller.confirm("environment:")
        await NativeTestSupport.wait(host) { !controller.isOpen }
        precondition(model.selectedEnvironmentID == nil)
        _ = await open(interface, model: model, window: window, host: host)
        choose(.changeTheme, controller: controller)
        await NativeTestSupport.settle(host)
        if let screenshots { try NativeTestSupport.capture(host, directory: screenshots, name: "command-palette-themes") }
        controller.highlight("theme:paper")
        precondition(defaults.string(forKey: "interfaceAppearance") == originalAppearance)
        controller.confirm("theme:paper")
        await NativeTestSupport.settle(host)
        precondition(defaults.string(forKey: "interfaceAppearance") == "paper")
        print("paletteEnvironmentAndThemeSubmenusApplyOnlyOnConfirmAndBackRestoresParent: passed")

        _ = await open(interface, model: model, window: window, host: host)
        controller.query("no-matching-fixture")
        await NativeTestSupport.settle(host)
        if let screenshots { try NativeTestSupport.capture(host, directory: screenshots, name: "command-palette-no-results") }
        controller.query(">go forward")
        await NativeTestSupport.settle(host)
        let unavailable = controller.projection.snapshot!.results.first!
        precondition(unavailable.unavailableReason != nil)
        controller.confirm(unavailable.id)
        precondition(controller.isOpen && controller.failure == unavailable.unavailableReason)
        if let screenshots { try NativeTestSupport.capture(host, directory: screenshots, name: "command-palette-unavailable") }
        let ax = NativeTestSupport.elements(host)
        precondition(ax.contains { $0.name == "Command Palette" })
        precondition(ax.contains { $0.name == "Search requests or actions" })
        precondition(!ax.contains { $0.name.contains("secret-url-sentinel") })
        let editor = window.firstResponder as! NSTextView
        editor.setMarkedText("東京", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        precondition(editor.hasMarkedText())
        precondition(!controller.handleKey(key(36, window)) && !controller.handleKey(key(53, window)))
        precondition(controller.isOpen)
        editor.unmarkText()
        controller.dismiss(restoringFocus: true)
        await NativeTestSupport.settle(host)
        precondition(active.activeRunID == runID)
        await run.complete()
        print("paletteIMEUnavailableReasonAndModalAccessibility: passed")
        _ = await open(interface, model: model, window: window, host: host)
        choose(.settings, controller: controller)
        await NativeTestSupport.settle(host)
        precondition(model.isShowingAppSettings && !controller.isOpen)
        precondition(!controller.canOpen(in: window))
        model.isShowingAppSettings = false
        await NativeTestSupport.settle(host)
        print("paletteSettingsHandoffDoesNotRestoreSearchFocus: passed")

        // Preview request confirmation pins; tab confirmation preserves a touched preview.
        let previewDraft = RequestDraft(id: "preview", name: "Preview fixture", url: "https://example.invalid/preview")
        model.workspace.collections[0].requests.append(RequestLocation(collectionID: "catalog", request: previewDraft))
        let preview = model.sessions.openPreview(draft: previewDraft, collectionID: "catalog")
        interface.synchronizeSelection(model: model)
        precondition(model.sessions.isPreview(tabID: preview.id))
        _ = await open(interface, model: model, window: window, host: host)
        controller.query("preview fixture")
        controller.confirm("request:catalog:preview")
        precondition(model.sessions.activeSession === preview && !model.sessions.isPreview(tabID: preview.id))
        let touched = model.sessions.openPreview(draft: RequestDraft(id: "touched", name: "Touched preview"), collectionID: "catalog")
        touched.draft.note = "Keep this draft"
        let previewID = model.sessions.activeGroup?.previewTabID
        interface.synchronizeSelection(model: model)
        _ = await open(interface, model: model, window: window, host: host)
        controller.query("touched preview")
        controller.confirm()
        precondition(model.sessions.activeSession === touched && touched.isTouched && touched.draft.note == "Keep this draft" && model.sessions.activeGroup?.previewTabID == previewID)
        print("paletteRequestPinsPreviewAndTabPreservesTouchedPreview: passed")

        _ = await open(interface, model: model, window: window, host: host)
        choose(.newWebSocket, controller: controller)
        await NativeTestSupport.wait(host) { model.sessions.activeSession?.kind == .webSocket }
        _ = await open(interface, model: model, window: window, host: host)
        choose(.save, controller: controller)
        await NativeTestSupport.wait(host) { !controller.isOpen }
        precondition(model.sessions.activeSession?.savedDraft != nil)
        print("paletteCreatesWebSocketAndSavesThroughExistingActions: passed")

        await store.setFailing(true)
        let priorTab = model.sessions.activeSession?.id
        _ = await open(interface, model: model, window: window, host: host)
        choose(.newHttp, controller: controller)
        await NativeTestSupport.wait(host) { controller.presentation == .presented }
        precondition(controller.failure != nil && model.sessions.activeSession?.id == priorTab)
        choose(.save, controller: controller)
        await NativeTestSupport.wait(host) { controller.presentation == .presented }
        precondition(controller.failure != nil && model.sessions.activeSession?.id == priorTab)
        controller.dismiss(restoringFocus: true)
        await store.setFailing(false)
        print("paletteCreationAndSaveFailuresRemainInlineWithoutChangingTabs: passed")

        // A failed environment commit preserves selection and hands off to its error.
        _ = await open(interface, model: model, window: window, host: host)
        choose(.switchEnvironment, controller: controller)
        model.environmentEditing.edit(.key(environmentId: "staging", id: "base"), text: "bad name")
        controller.confirm("environment:staging")
        await NativeTestSupport.wait(host) { !controller.isOpen }
        precondition(model.selectedEnvironmentID == nil && interface.isShowingEnvironmentInspector)
        precondition(!model.environmentEditing.problems.isEmpty)
        model.environmentEditing.edit(.key(environmentId: "staging", id: "base"), text: "base_url")
        let flushed = await model.environmentEditing.flush()
        precondition(flushed)
        print("paletteEnvironmentFlushFailurePreservesSelectionAndHandsOffError: passed")

        _ = await open(interface, model: model, window: window, host: host)
        controller.query("preview fixture")
        let catalogIndex = model.workspace.collections.firstIndex { $0.id == "catalog" }!
        model.workspace.collections[catalogIndex].requests.removeAll { $0.request.id == "preview" }
        controller.confirm("request:catalog:preview")
        precondition(controller.isOpen && controller.failure != nil)
        controller.dismiss(restoringFocus: false)
        _ = await open(interface, model: model, window: window, host: host)
        choose(.changeTheme, controller: controller)
        precondition(controller.projection.snapshot!.results.contains { $0.title == "Harbor" })
        controller.confirm("theme:" + ThemeSelection.custom(id: custom.id).rawValue)
        precondition(defaults.string(forKey: "interfaceAppearance") == custom.id)
        _ = await open(interface, model: model, window: window, host: host)
        choose(.changeTheme, controller: controller)
        _ = await themes.delete(id: custom.id, selection: .system)
        controller.confirm("theme:" + custom.id)
        precondition(controller.isOpen && controller.failure != nil)
        interface.toggleCommandPalette(model: model, in: window)
        precondition(!controller.isOpen)
        model.isShowingWorkspaceSettings = true
        precondition(!controller.canOpen(in: window))
        model.isShowingWorkspaceSettings = false
        interface.renamingRequestID = "catalog:list"
        precondition(!controller.canOpen(in: window))
        interface.renamingRequestID = nil
        model.importSummary = ImportSummary(collectionNames: ["Imported fixture"], requestCount: 1, groupCount: 0)
        precondition(controller.canOpen(in: window))
        model.importSummary = ImportSummary(collectionNames: ["Imported fixture"], requestCount: 1, groupCount: 0, warnings: ["Fixture warning"])
        precondition(!controller.canOpen(in: window))
        model.importSummary = nil
        _ = await open(interface, model: model, window: window, host: host)
        let otherGroup = model.sessions.groups.first { $0.id != model.sessions.activeGroupID }!
        model.sessions.activeGroupID = otherGroup.id
        interface.synchronizeSelection(model: model)
        let beforeFocus = interface.focusURLTrigger
        controller.dismiss(restoringFocus: true)
        await NativeTestSupport.settle(host)
        precondition(interface.focusURLTrigger > beforeFocus)
        print("paletteAllowsNonmodalImportBannerAndRestoresLogicalFocusAfterSelectionChanges: passed")
        precondition(transport.executionCount == 0 && transport.cancellationCount == 0)
        print("paletteRevalidatesRemovedDestinationsCustomThemesAndOpeningGuards: passed")
    }
}

private final class PaletteCountingRunner: RequestRunner, @unchecked Sendable {
    private let lock = NSLock()
    private var executions = 0
    private var cancellations = 0
    var executionCount: Int { lock.withLock { executions } }
    var cancellationCount: Int { lock.withLock { cancellations } }
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        lock.withLock { executions += 1 }
        return AsyncThrowingStream { $0.finish() }
    }
    func cancel(runID: RunID) { lock.withLock { cancellations += 1 } }
}
