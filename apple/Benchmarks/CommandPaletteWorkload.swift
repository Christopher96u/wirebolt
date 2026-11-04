import AppKit
import SwiftUI

extension NativeWorkloads {
    static func commandPalette(_ count: Int, root: URL) async throws {
        let requests = (0..<count).map { i in
            RequestLocation(collectionID: "fixture", request: RequestDraft(id: "r\(i)", name: "Request \(i)",
                url: "https://example.invalid/\(i)", body: .json(value: String(repeating: "x", count: i == 0 ? 1_048_576 : 0))))
        }
        let workspace = WorkspaceDraft(name: "Palette fixture", collections: [CollectionDraft(id: "fixture", name: "Fixture", requests: requests)],
            environments: [EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Globals"),
                EnvironmentDraft(id: "staging", name: "Staging", variables: [EnvironmentVariableDraft(id: "base", key: "fixture", value: .literal("initial"))])])
        let model = WireboltModel(runner: FixtureRunner(), persistence: PaletteFixtureStore(workspace),
            history: HistoryRepository(root: root.appending(path: "history")), cookieJar: CookieJar(storageURL: root.appending(path: "cookies.json")),
            cookieDirectory: root, proxyPreferences: ProxyPreferences(defaults: fixtureDefaults))
        await model.loadWorkspace()
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        for request in requests.prefix(20) { _ = model.sessions.open(draft: request.request, collectionID: "fixture") }
        interface.synchronizeSelection(model: model)
        let (window, host, _) = mount(ContentView(model: model, interface: interface, loadsWorkspace: false))
        interface.isWorkspaceWindowActive = { [weak window] in $0 === window }
        WorkspaceWindowRegistry.primary = window
        window.makeKey()
        defer { interface.commandPalette?.dismiss(restoringFocus: false); window.close(); WorkspaceWindowRegistry.primary = nil }
        await NativeTestSupport.settle(host)
        let coldStart = ContinuousClock.now
        interface.toggleCommandPalette(model: model, in: window)
        while interface.commandPalette?.presentation == .opening { await Task.yield() }
        flush(host)
        record("command_palette_cold_open", [ms(coldStart)], budget: windowFirstContentBudget,
            details: ["requests": count, "includes_index": true])
        let palette = interface.commandPalette!
        precondition(palette.presentation == .presented)
        palette.dismiss(restoringFocus: false)
        await NativeTestSupport.settle(host)
        var opens: [Double] = [], queries: [Double] = [], typing: [Double] = [], cancel: [Double] = []
        var afterEnvironmentEdit: [Double] = [], afterRequestRename: [Double] = []
        let traffic = model.sessions.sessions.values.map { ($0, $0.rust.bridgeTraffic(), $0.draftSynchronizationCount) }
        for index in 0..<samples {
            let start = ContinuousClock.now
            interface.toggleCommandPalette(model: model, in: window)
            while palette.presentation == .opening { await Task.yield() }
            flush(host)
            opens.append(ms(start))
            await NativeTestSupport.settle(host)
            let queryStart = ContinuousClock.now
            palette.query(index.isMultiple(of: 2) ? "Request 99" : "absent-fixture")
            flush(host)
            queries.append(ms(queryStart))
            await NativeTestSupport.settle(host)
            let field = findPaletteSearch(host)!
            window.makeFirstResponder(field)
            let editor = field.currentEditor() as! NSTextView
            let typingStart = ContinuousClock.now
            editor.insertText("x", replacementRange: NSRange(location: editor.string.utf16.count, length: 0))
            flush(host)
            typing.append(ms(typingStart))
            let cancelStart = ContinuousClock.now
            palette.dismiss(restoringFocus: false)
            flush(host)
            cancel.append(ms(cancelStart))
            await NativeTestSupport.settle(host)

            let beforeNormalization = palette.projection.rust.normalizedRequests()
            model.environmentEditing.edit(.value(environmentId: "staging", id: "base"), text: "fixture \(index)")
            let environmentSaved = await model.environmentEditing.flush()
            precondition(environmentSaved)
            let environmentStart = ContinuousClock.now
            interface.toggleCommandPalette(model: model, in: window)
            while palette.presentation == .opening { await Task.yield() }
            flush(host)
            afterEnvironmentEdit.append(ms(environmentStart))
            precondition(palette.projection.rust.normalizedRequests() == beforeNormalization)
            palette.dismiss(restoringFocus: false)
            await NativeTestSupport.settle(host)

            await model.renameRequest(collectionID: "fixture", requestID: "r\(count - 1)", name: "Renamed fixture \(index)")
            let renameStart = ContinuousClock.now
            interface.toggleCommandPalette(model: model, in: window)
            while palette.presentation == .opening { await Task.yield() }
            flush(host)
            afterRequestRename.append(ms(renameStart))
            precondition(palette.projection.rust.normalizedRequests() == beforeNormalization + 1)
            palette.dismiss(restoringFocus: false)
            await NativeTestSupport.settle(host)
        }
        for (session, before, sync) in traffic {
            let after = session.rust.bridgeTraffic()
            precondition(after.documentReads == before.documentReads && after.draftWrites == before.draftWrites && session.draftSynchronizationCount == sync)
        }
        let details: [String: Any] = ["requests": count, "tabs": 20, "path": "production overlay + Rust index/FFI + native layout/display", "full_draft_reads": 0, "full_draft_writes": 0, "calibrated_hardware": "Apple M5 Pro"]
        record("command_palette_warm_open", opens, budget: paletteOpenBudget, details: details)
        record("command_palette_warm_open_after_environment_edit", afterEnvironmentEdit, budget: paletteOpenBudget, details: details)
        record("command_palette_warm_open_after_request_rename", afterRequestRename, budget: paletteOpenBudget, details: details)
        record("command_palette_query", queries, budget: paletteQueryBudget, details: details)
        record("command_palette_typing", typing, budget: paletteTypingBudget, details: details)
        record("command_palette_cancel", cancel, budget: 50, details: details)
    }
    private static func findPaletteSearch(_ view: NSView) -> NSTextField? {
        if let field = view as? NSTextField, field.accessibilityLabel() == "Search requests or actions" { return field }
        return view.subviews.lazy.compactMap(findPaletteSearch).first
    }
}
