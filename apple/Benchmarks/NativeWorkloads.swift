import AppKit
import Darwin
import UniformTypeIdentifiers
import Foundation
import Observation
import SwiftUI
import os

private struct OfflineRunner: RequestRunner {
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> { AsyncThrowingStream { $0.finish() } }
    func cancel(runID: RunID) {}
}
@MainActor @Observable private final class TextFixture {
    var text: String
    var preview = false
    init(_ text: String) { self.text = text }
}
private struct EditorFixture: View {
    @Bindable var fixture: TextFixture
    var body: some View { NativeCodeEditor(text: $fixture.text, language: .json) }
}
private struct NotesFixture: View {
    @Bindable var fixture: TextFixture
    var body: some View { NotesEditor(text: $fixture.text, preview: $fixture.preview) }
}
@MainActor @Observable private final class TreeThemeFixture {
    var theme = ResolvedTheme(preset: .black)
    let nodes = JSONNode.makeRoot(from: #"{"group":{"message":"hello","count":2}}"#)
}
private struct TreeFixture: View {
    @Bindable var fixture: TreeThemeFixture
    var body: some View {
        JSONTreeView(nodes: fixture.nodes, revision: 1).environment(\.wireboltTheme, fixture.theme)
    }
}
final class ProbeWindow: NSWindow { override var canBecomeKey: Bool { true } }

@main @MainActor struct NativeWorkloads {
    static let log = OSLog(subsystem: "io.github.christopher96u.wirebolt", category: .pointsOfInterest)
    static var measurements: [[String: Any]] = []
    private struct Budgets: Decodable {
        let nativeColdWindowFirstContentMilliseconds: Double
        let nativeResponseRendererColdSwitchMilliseconds: Double
        let nativeResponseRendererSwitchP95Milliseconds: Double
        let nativeTabCloseCommandP95Milliseconds: Double
    }
    static var windowFirstContentBudget = 0.0
    /// Each of the first JSON→Raw and Raw→JSON switches mounts or reactivates a renderer.
    static var rendererColdSwitchBudget = 0.0
    /// Warm switches between mounted renderers: one 60 Hz frame for the complete native
    /// SwiftUI/AppKit path. The engine-only synthetic budget (4 ms) is gated by PerformanceContract.
    static var rendererSwitchBudget = 0.0
    static var tabCloseBudget = 0.0
    static var samples = 100
    static let defaultsName = "wirebolt-native-workloads-\(UUID().uuidString)"
    static let fixtureDefaults = UserDefaults(suiteName: defaultsName)!
    static func ms(_ start: ContinuousClock.Instant) -> Double {
        let d = start.duration(to: .now).components
        return Double(d.seconds)*1000 + Double(d.attoseconds)/1e15
    }
    static func spin(_ seconds: Double = 0.04) { RunLoop.current.run(until: Date().addingTimeInterval(seconds)) }
    static func flush(_ host: NSView) {
        host.layoutSubtreeIfNeeded()
        host.displayIfNeeded()
    }
    static func record(_ name: String, _ values: [Double], budget: Double, details: [String: Any] = [:]) {
        let ordered = values.sorted()
        let p95 = ordered[max(0, Int(ceil(Double(ordered.count)*0.95))-1)]
        let item: [String: Any] = ["name":name, "samples_ms":values,
            "p50_ms":ordered[ordered.count/2], "p95_ms":p95, "max_ms":ordered.last!,
            "budget_ms":budget, "over_budget":p95 > budget, "details":details]
        measurements.append(item)
        fputs("MEASURE \(name) p95=\(p95) ms max=\(ordered.last!)\n", stderr)
    }
    static func mount<V: View>(_ view: V, width: CGFloat = 1248, height: CGFloat = 800) -> (ProbeWindow, NSView, Double) {
        let start = ContinuousClock.now
        let window = ProbeWindow(contentRect: NSRect(x:100,y:100,width:width,height:height),styleMask:[.titled,.resizable,.closable],backing:.buffered,defer:false)
        window.isReleasedWhenClosed = false
        let appearance: ColorScheme? = switch ProcessInfo.processInfo.environment["WIREBOLT_BENCH_APPEARANCE"] {
        case "light": .light
        case "dark": .dark
        default: nil
        }
        let host = NSHostingView(rootView: view.defaultAppStorage(fixtureDefaults).preferredColorScheme(appearance))
        window.contentView = host
        window.orderFront(nil)
        flush(host)
        return (window,host,ms(start))
    }
    static func textViews(_ view: NSView) -> [NSTextView] {
        (view as? NSTextView).map { [$0] } ?? view.subviews.flatMap { textViews($0) }
    }
    static func editor(_ rows: Int) throws {
        let json = "[\n" + (0..<rows).map { "  {\"id\":\($0),\"active\":true,\"message\":\"café 東京 🚀\"}" }.joined(separator:",\n") + "\n]"
        let fixture = TextFixture(json)
        let (window, host, cold) = mount(EditorFixture(fixture: fixture))
        defer { window.close() }
        spin()
        guard let editor = textViews(host).first else { throw CocoaError(.coderValueNotFound) }
        record("editor_mount",[cold],budget:windowFirstContentBudget,details:["bytes":json.utf8.count,"rows":rows])
        var values: [Double] = []
        for _ in 0..<samples {
            os_signpost(.begin,log:log,name:"EditorInsert")
            let start = ContinuousClock.now
            editor.insertText(" ",replacementRange:NSRange(location:1,length:0))
            flush(host)
            values.append(ms(start))
            os_signpost(.end,log:log,name:"EditorInsert")
            spin(0.01)
        }
        precondition(fixture.text.utf16.count == json.utf16.count+samples)
        precondition(editor.string == fixture.text, "native and model text must stay identical")
        record("editor_insert",values,budget:16,details:["bytes":json.utf8.count,"rows":rows,"edit":"space at UTF16 offset 1","path":"native NSTextView.insertText + layout/display"])
        var resized: [Double] = []
        for i in 0..<samples {
            os_signpost(.begin,log:log,name:"EditorResize")
            let start=ContinuousClock.now
            window.setContentSize(NSSize(width:i.isMultiple(of:2) ? 800 : 1248,height:800))
            flush(host)
            resized.append(ms(start))
            os_signpost(.end,log:log,name:"EditorResize")
            spin(0.01)
        }
        record("editor_resize",resized,budget:16,details:["rows":rows])
        var scrolling: [Double] = []
        var scrollWall: [Double] = []
        if let scroll = editor.enclosingScrollView {
            for i in 0..<samples {
                let wallStart = ContinuousClock.now
                let start = clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID)
                let y = max(0, editor.bounds.height - scroll.contentView.bounds.height) * CGFloat(i % 10) / 9
                scroll.contentView.scroll(to: NSPoint(x: 0, y: y))
                scroll.reflectScrolledClipView(scroll.contentView)
                // Flush one viewport draw. Thread CPU time counts scroll/layout/highlight/
                // drawing work while excluding AppKit's wait for a display frame.
                flush(host)
                scrolling.append(Double(clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID) - start) / 1e6)
                scrollWall.append(ms(wallStart))
                spin(0.01)
            }
        }
        if !scrolling.isEmpty {
            let path = "clip-view scroll + scroller reflection + pending layout/display, one viewport draw"
            record("editor_scroll", scrolling, budget: 16, details: ["rows": rows,
                "clock": "main-thread CPU (CLOCK_THREAD_CPUTIME_ID)", "path": path])
            // Two 60 Hz frames tolerate AppKit's display wait, including on variable
            // refresh displays, while still gating locks, synchronous I/O and FFI waits.
            record("editor_scroll_wall", scrollWall, budget: 1000.0 / 30, details: ["rows": rows,
                "clock": "wall (ContinuousClock)", "path": path])
        }
        window.makeKey()
        window.makeFirstResponder(editor)
        let before = fixture.text
        editor.breakUndoCoalescing()
        let inserted = "\n  \"unicode\": \"👨‍👩‍👧‍👦 東京\",\n"
        editor.setSelectedRange(NSRange(location: 1, length: 0))
        editor.insertText(inserted, replacementRange: NSRange(location: 1, length: 0))
        flush(host)
        spin()
        precondition(fixture.text == (before as NSString).replacingCharacters(in: NSRange(location: 1, length: 0), with: inserted))
        precondition(editor.selectedRange().location == 1 + (inserted as NSString).length)
        editor.undoManager?.undo()
        flush(host)
        spin()
        precondition(fixture.text == before, "undo must restore the original Unicode document")
        for fraction in [0, 1, 2] {
            let old = fixture.text
            let target = (old as NSString).range(of: "message", options: [], range: NSRange(location: (old as NSString).length * fraction / 3, length: (old as NSString).length - (old as NSString).length * fraction / 3))
            precondition(target.location != NSNotFound)
            editor.setSelectedRange(target)
            editor.insertText("東京👨‍👩‍👧‍👦", replacementRange: target)
            flush(host)
            precondition(editor.string == fixture.text)
            precondition(fixture.text == (old as NSString).replacingCharacters(in: target, with: "東京👨‍👩‍👧‍👦"))
        }
        spin()
        precondition(editor.string == fixture.text)


    }
    static func workspace(_ count: Int, root: URL) throws {
        let model = WireboltModel(runner:OfflineRunner(),history:HistoryRepository(root:root.appending(path:"history")),cookieJar:CookieJar(storageURL:root.appending(path:"cookies.json")))
        let defaultsName="wirebolt-profiling-\(UUID().uuidString)"
        let defaults=UserDefaults(suiteName:defaultsName)!
        defer { defaults.removePersistentDomain(forName:defaultsName) }
        let interface=WorkspaceUIState(defaults:defaults)
        let requests=(0..<count).map { i in RequestLocation(collectionID:"fixture",request:RequestDraft(id:"r\(i)",name:"Request \(i)",url:"https://example.invalid/\(i)")) }
        model.workspace.collections=[CollectionDraft(id:"fixture",name:"Fixture",requests:requests)]
        let tabs=requests.prefix(20).map { model.sessions.open(draft:$0.request,collectionID:"fixture") }
        interface.synchronizeSelection(model:model)
        let (window,host,cold)=mount(ContentView(model:model,interface:interface,loadsWorkspace:false))
        defer { window.close() }
        spin(0.4)
        flush(host)
        record("workspace_mount",[cold],budget:windowFirstContentBudget,details:["requests":count,"tabs":tabs.count,"excludes_deferred_content":false])
        precondition(model.undoManager != nil && model.undoManager === window.undoManager,
                     "Workspace edits must register their undo on the window's undo manager")
        var switches:[Double]=[]
        for i in 0..<samples {
            os_signpost(.begin,log:log,name:"TabSwitch")
            let start=ContinuousClock.now
            interface.activateTab(id:tabs[i%tabs.count].id,model:model)
            flush(host)
            switches.append(ms(start))
            os_signpost(.end,log:log,name:"TabSwitch")
            spin(0.02)
        }
        record("tab_switch",switches,budget:50,details:["requests":count,"tabs":tabs.count,"path":"actual selection + ContentView layout/display"])
        var filters:[Double]=[]
        for i in 0..<samples {
            os_signpost(.begin,log:log,name:"SidebarFilter")
            let start=ContinuousClock.now
            interface.sidebarFilter=i.isMultiple(of:2) ? "Request 9" : ""
            flush(host)
            filters.append(ms(start))
            os_signpost(.end,log:log,name:"SidebarFilter")
            spin(0.02)
        }
        record("sidebar_filter",filters,budget:count > 1000 ? 50 : 25,details:["requests":count])
        var typing:[Double]=[]
        for i in 0..<samples {
            os_signpost(.begin,log:log,name:"URLChange")
            let start=ContinuousClock.now
            model.draft.url = "https://example.invalid/edited/\(i)"
            flush(host)
            typing.append(ms(start))
            os_signpost(.end,log:log,name:"URLChange")
            spin(0.02)
        }
        record("url_change",typing,budget:16,details:["requests":count,"note":"actual draft setter and view update, not physical keystroke latency"])
    }
    static func response(_ rows: Int) throws {
        let raw="["+(0..<rows).map { "{\"id\":\($0),\"active\":true,\"message\":\"café 東京 🚀\"}" }.joined(separator:",")+"]"
        let session=DocumentSession(draft:RequestDraft())
        let run=RunID()
        session.beginRun(run)
        var ready=false
        Task { @MainActor in
            await session.consume(.chunk(Data(raw.utf8)),runID:run)
            await session.consume(.complete(RunCompletion(bytesReceived:UInt64(raw.utf8.count),totalTimeNS:1)),runID:run)
            ready=true
        }
        for _ in 0..<1000 where !ready { spin(0.002) }
        precondition(ready)
        let state=DocumentPresentationState()
        os_signpost(.begin,log:log,name:"ResponseMount")
        let start=ContinuousClock.now
        let (window,host,_)=mount(ResponseViewer(interface:state,session:session))
        defer { window.close() }
        func contentReady() -> Bool {
            func indexed(_ view:NSView) -> Bool {
                if let indexed = view as? IndexedCodeView, indexed.hasDrawnViewport {
                    return state.responseRenderer == .json ? indexed.language == .json && indexed.index?.url.lastPathComponent.hasPrefix("wirebolt-json-") == true : indexed.language == .plain && indexed.index?.url == session.bodyStore?.url
                }
                return view.subviews.contains(where:indexed)
            }
            if state.responseRenderer == .json {
                return indexed(host) || textViews(host).contains { $0.string.hasPrefix("[\n  {") }
            }
            return indexed(host) || textViews(host).contains { $0.string == raw }
        }
        while !contentReady() && ms(start)<20000 { spin(0.002); flush(host) }
        precondition(contentReady())
        record("response_pretty_ready",[ms(start)],budget:windowFirstContentBudget,details:["rows":rows,"raw_bytes":raw.utf8.count,"poll_interval_ms":2,"includes_framework_and_window_construction":true])
        os_signpost(.end,log:log,name:"ResponseMount")
        var switches:[Double]=[]
        for i in 0..<(samples + 2) {
            os_signpost(.begin,log:log,name:"ResponseRendererSwitch")
            let start=ContinuousClock.now
            state.responseRenderer=i.isMultiple(of:2) ? .raw : .json
            flush(host)
            while !contentReady() && ms(start)<20000 { spin(0.002); flush(host) }
            precondition(contentReady())
            switches.append(ms(start))
            os_signpost(.end,log:log,name:"ResponseRendererSwitch")
            spin(0.02)
        }
        record("response_renderer_first_switch",[switches[0]],budget:rendererColdSwitchBudget,details:["rows":rows,"raw_bytes":raw.utf8.count,"from":"JSON","to":"Raw","budget_key":"nativeResponseRendererColdSwitchMilliseconds"])
        record("response_renderer_second_switch",[switches[1]],budget:rendererColdSwitchBudget,details:["rows":rows,"raw_bytes":raw.utf8.count,"from":"Raw","to":"JSON","budget_key":"nativeResponseRendererColdSwitchMilliseconds"])
        record("response_renderer_switch",Array(switches.dropFirst(2)),budget:rendererSwitchBudget,details:["rows":rows,"raw_bytes":raw.utf8.count,"alternating":"Raw,JSON","readiness":"expected text or indexed first viewport drawn; not GPU presentation","budget_key":"nativeResponseRendererSwitchP95Milliseconds"])
        if let editor = textViews(host).first(where: { $0.string.hasPrefix("[\n  {") }),
           let coordinator = editor.delegate as? NativeCodeEditor.Coordinator, let scroll = editor.enclosingScrollView {
            let pretty = editor.string
            coordinator.collapsed.insert(0)
            coordinator.render(in: scroll)
            precondition(editor.string == "[…]")
            state.responseRenderer = .raw
            flush(host)
            precondition(scroll.isHidden, "inactive editors must leave the responder and accessibility trees")
            state.responseRenderer = .json
            flush(host)
            precondition(textViews(host).contains { $0 === editor } && editor.string == "[…]", "renderer switches must preserve folding and editor identity")
            if let find = coordinator.findState {
                find.query.text = "café"
                find.isVisible = true
                let started = ContinuousClock.now
                while find.matchCount != rows && ms(started) < 10000 { spin(0.002); flush(host) }
                precondition(find.matchCount == rows && editor.string == pretty, "Find must unfold and search the entire response")
            } else { preconditionFailure("the active editor must handle Find") }
        }
        state.responseRenderer = .json
        flush(host)
        let waitStart = ContinuousClock.now
        while !contentReady() && ms(waitStart) < 20000 { spin(0.002); flush(host) }
        func indexedViews(_ v: NSView) -> [IndexedCodeView] {
            (v as? IndexedCodeView).map { [$0] } ?? v.subviews.flatMap { indexedViews($0) }
        }
        if fixtureDefaults.bool(forKey: "editor.wordWrap"), let source = indexedViews(host).first(where: { $0.language == .json }) {
            let fullStart = ContinuousClock.now
            while source.index?.isComplete != true && ms(fullStart) < 10000 { spin(0.002); flush(host) }
            precondition(source.index?.isComplete == true, "The complete response must remain available after the first viewport")
            // Exercise the actual viewport independently of the surrounding toolbar's minimum width.
            let (resizeWindow, resizeHost, _) = mount(IndexedResponseEditor(url: source.index!.url, preview: "", language: .json, search: ""))
            defer { resizeWindow.close() }
            let readyStart = ContinuousClock.now
            while (indexedViews(resizeHost).first?.index?.isComplete != true || indexedViews(resizeHost).first?.hasDrawnViewport != true) && ms(readyStart) < 10000 {
                spin(0.002); flush(resizeHost)
            }
            let v = indexedViews(resizeHost).first!
            let scroll = v.enclosingScrollView!
            let old = v.index!
            let targetRow = old.rowCount / 2
            scroll.contentView.scroll(to: NSPoint(x: 0, y: Double(targetRow) * v.lineHeight))
            scroll.reflectScrolledClipView(scroll.contentView); flush(resizeHost)
            let beforeRow = Int(scroll.contentView.bounds.minY / v.lineHeight)
            let beforeLine = try old.rows(start: beforeRow, count: 1).first!.line
            resizeWindow.setContentSize(NSSize(width: 260, height: 800)); flush(resizeHost)
            let resizeStart = ContinuousClock.now
            while (v.index?.wrapping == old.wrapping || v.index?.isComplete != true || !v.hasDrawnViewport) && ms(resizeStart) < 10000 { spin(0.002); flush(resizeHost) }
            spin(0.1); flush(resizeHost)
            let afterRow = Int(scroll.contentView.bounds.minY / v.lineHeight)
            let afterLine = try v.index!.rows(start: afterRow, count: 1).first!.line
            precondition(v.index!.rowCount > old.rowCount, "The resize fixture must exercise soft wrapping")
            precondition(beforeLine == afterLine, "Resize must preserve the logical line being read")
            print("RESIZE_ANCHOR beforeLine=\(beforeLine) afterLine=\(afterLine) oldRows=\(old.rowCount) newRows=\(v.index!.rowCount)")
        }
        let received = try Data(contentsOf: session.bodyStore!.url)
        precondition(received == Data(raw.utf8), "presentation must not change received bytes")
        try streamedResponsePresentation()
    }

    /// A pane mounted when the head arrives (before the body file is complete)
    /// must still present the completed body, in both JSON and Raw renderers.
    static func streamedResponsePresentation() throws {
        let body = #"{"zeta":1,"alpha":"caf\u00e9"}"#
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        session.beginRun(run)
        var step = 0
        Task { @MainActor in
            await session.consume(.head(ResponseHead(status: 200, version: "HTTP/1.1",
                headers: [ResponseHeader(name: "Content-Type", value: "application/json")], timeToHeadersNS: 1)), runID: run)
            step = 1
        }
        for _ in 0..<1000 where step < 1 { spin(0.002) }
        let state = DocumentPresentationState()
        let (window, host, _) = mount(ResponseViewer(interface: state, session: session))
        defer { window.close() }
        spin(0.05); flush(host)
        Task { @MainActor in
            await session.consume(.chunk(Data(body.utf8)), runID: run)
            await session.consume(.complete(RunCompletion(bytesReceived: UInt64(body.utf8.count), totalTimeNS: 1)), runID: run)
            step = 2
        }
        func shows(_ prefix: String) -> Bool {
            let started = ContinuousClock.now
            while !textViews(host).contains(where: { $0.string.hasPrefix(prefix) && !$0.isHiddenOrHasHiddenAncestor }) && ms(started) < 5000 {
                spin(0.002); flush(host)
            }
            return textViews(host).contains { $0.string.hasPrefix(prefix) && !$0.isHiddenOrHasHiddenAncestor }
        }
        precondition(shows("{\n  \"zeta\": 1,\n  \"alpha\": \"café\""), "a pane mounted while streaming must show the formatted body")
        state.responseRenderer = .raw
        precondition(shows(body), "Raw must show the completed body")
        print("streamed_response_presentation=passed")
    }
    struct Element {
        let role: String
        let name: String
        let frame: NSRect
    }
    static func elements(_ object: Any, depth: Int = 0) -> [Element] {
        guard depth < 30, let object = object as? NSObject else { return [] }
        // macOS 27 can return NSAttributedString for nominally String AX
        // labels. Preserve it rather than dropping names or force-bridging.
        func value(_ key: String) -> Any? {
            object.responds(to: NSSelectorFromString(key)) ? object.value(forKey: key) : nil
        }
        func text(_ key: String) -> String {
            let raw = value(key)
            return (raw as? NSAttributedString)?.string ?? (raw as? String) ?? ""
        }
        let name = ["accessibilityLabel", "accessibilityTitle", "accessibilityValue"]
            .map(text).first { !$0.isEmpty } ?? ""
        let current = Element(role: text("accessibilityRole"), name: name,
            frame: (value("accessibilityFrame") as? NSValue)?.rectValue ?? .zero)
        return [current] + (value("accessibilityChildren") as? [Any] ?? []).flatMap { elements($0, depth: depth + 1) }
    }

    static func missingBodyFilePresentation(root: URL) async throws {
        let blocked = root.appending(path: "not-a-body-directory")
        try Data("fixture".utf8).write(to: blocked)
        let session = DocumentSession(draft: RequestDraft())
        let run = RunID()
        session.rust.begin(runId: run.description, directory: blocked.path)
        session.mirror()
        let body = String(repeating: "preview survives 🌍\n", count: 4000)
        await session.consume(.head(ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1)), runID: run)
        await session.consume(.chunk(Data(body.utf8)), runID: run)
        await session.consume(.complete(RunCompletion(bytesReceived: UInt64(body.utf8.count), totalTimeNS: 1)), runID: run)
        precondition(session.bodyStore == nil && session.failure?.kind == .storage)
        precondition(session.responsePreviewData.count <= DocumentSession.previewByteLimit && session.responseWasTruncated)
        let state = DocumentPresentationState()
        state.responseRenderer = .raw
        let (window, host, _) = mount(ResponseViewer(interface: state, session: session))
        defer { window.close() }
        var shown = false
        for _ in 0..<200 {
            try await Task.sleep(for: .milliseconds(5)); flush(host)
            shown = textViews(host).contains { !$0.isHiddenOrHasHiddenAncestor && $0.string.hasPrefix("preview survives 🌍") }
            if shown { break }
        }
        precondition(shown, "a failed body-file creation must leave the memory preview visible after completion")
        let title = RunFailureMessage(RunFailure(kind: .storage, issues: [])).title
        precondition(elements(host).contains { $0.name.contains(title) }, "the partial preview must still report the storage failure")
        print("missing_body_file_presentation=passed")
    }

    static func verifyRedesignFlows(root: URL) async throws {
        try await missingBodyFilePresentation(root: root)
        let persistence = try RustWorkspacePersistence(path: root.appending(path: "redesign-flows"))
        let model = WireboltModel(runner: OfflineRunner(), persistence: persistence,
            history: HistoryRepository(root: root.appending(path: "flow-history")), cookieJar: CookieJar())
        await model.loadWorkspace()
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        let (window, host, _) = mount(ContentView(model: model, interface: interface, loadsWorkspace: false))
        defer { window.close() }
        window.makeKey()
        interface.makeNewRequest(model: model)
        var focusedURL = false
        for _ in 0..<200 {
            try await Task.sleep(for: .milliseconds(5)); flush(host)
            if let editor = window.firstResponder as? NSTextView, editor.isFieldEditor,
               let field = editor.delegate as? NSTextField, field.accessibilityLabel() == "Request URL" {
                focusedURL = true
                break
            }
        }
        precondition(focusedURL && interface.renamingRequestID == nil, "New Request must focus the URL without entering sidebar rename")
        guard let first = model.sessions.activeSession else { preconditionFailure("New request is missing") }
        first.draft.url = "https://example.invalid/saved-by-close"
        guard let other = await model.createRequest() else { preconditionFailure("Second request is missing") }
        other.draft.url = "https://example.invalid/keep-dirty"
        model.sessions.select(tabID: first.id)
        interface.synchronizeSelection(model: model)
        interface.close(.one(first.id), model: model)
        precondition(interface.dirtyCloseRequest?.documentIDs == [first.id])
        interface.saveDirtyClose(model: model)
        for _ in 0..<200 where model.sessions.session(id: first.id) != nil {
            try await Task.sleep(for: .milliseconds(5)); flush(host)
        }
        precondition(model.sessions.session(id: first.id) == nil, "Save and Close must close the successfully saved tab")
        precondition(model.sessions.session(id: other.id) === other && other.isDirty, "Save and Close must leave unrelated dirty tabs intact")
        precondition(model.workspace.location(collectionID: first.collectionID!, requestID: first.requestID)?.request.url == first.draft.url)
        print("new_request_url_focus_and_scoped_save_close=passed")
        func fields(_ view: NSView) -> [NSTextField] {
            (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap { fields($0) }
        }
        func stageVariable(_ key: String) async throws {
            interface.isShowingEnvironmentInspector = true
            for _ in 0..<100 where interface.saveEnvironmentEdits == nil {
                try await Task.sleep(for: .milliseconds(5)); flush(host)
            }
            guard let field = fields(host).first(where: { $0.placeholderString == "New key" }) else {
                preconditionFailure("Inspector's new-key field must remain reachable")
            }
            for label in ["Sidebar width", "Environment inspector width"] {
                // Split dividers deliberately keep a narrow grab strip so they do not
                // cover adjacent editor controls; their full-height AX target is named.
                precondition(elements(host).contains { $0.name == label && $0.frame.width > 0 && $0.frame.height >= 28 },
                    "Resize dividers must expose a named, full-height accessibility target")
            }
            window.makeFirstResponder(field)
            guard let editor = field.currentEditor() as? NSTextView else { preconditionFailure("Variable entry must accept keyboard focus") }
            editor.insertText(key, replacementRange: NSRange(location: 0, length: 0))
            for _ in 0..<100 where !interface.hasEnvironmentEdits {
                try await Task.sleep(for: .milliseconds(5)); flush(host)
            }
            precondition(interface.hasEnvironmentEdits, "Staged inspector edits must participate in exit protection")
        }
        try await stageVariable("preview_only")
        precondition(model.workspace.environments.isEmpty, "Typing must not persist staged variables")
        guard let save = interface.saveEnvironmentEdits else { preconditionFailure("Inspector apply action must be installed") }
        let applied = await save()
        precondition(applied, "Inspector must apply valid staged edits")
        flush(host)
        precondition(model.workspace.environments.first?.variables.first?.key == "preview_only")
        model.undoManager?.undo()
        await model.finishUndoWork()
        precondition(model.workspace.environments.isEmpty, "Applying environments must support workspace undo")
        // Let the old inspector detach before opening a fresh editing session.
        try await Task.sleep(for: .milliseconds(20)); flush(host)
        try await stageVariable("discard_me")
        for (response, expected) in [(NSApplication.ModalResponse.alertSecondButtonReturn, false), (.alertThirdButtonReturn, true)] {
            let decision = Task { await confirmEnvironmentEdits(interface: interface, in: window) }
            for _ in 0..<100 where window.attachedSheet == nil {
                try await Task.sleep(for: .milliseconds(5))
            }
            guard let sheet = window.attachedSheet else { preconditionFailure("Pending environment edits must show an exit decision") }
            window.endSheet(sheet, returnCode: response)
            spin(0.04)
            let result = await decision.value
            precondition(result == expected)
            if !expected { precondition(interface.hasEnvironmentEdits && interface.isShowingEnvironmentInspector) }
            try await Task.sleep(for: .milliseconds(20)); flush(host)
        }
        precondition(!interface.hasEnvironmentEdits && !interface.isShowingEnvironmentInspector)
        precondition(model.workspace.environments.isEmpty, "Discarding must not persist a staged variable")
        print("environment_staging_undo_exit_guards=passed")

    }

    static func verifyTreeTheme() {
        let fixture = TreeThemeFixture()
        let (window, host, _) = mount(TreeFixture(fixture: fixture))
        defer { window.close(); fixtureDefaults.set(12.0, forKey: "editor.fontSize") }
        func outline(in view: NSView) -> NSOutlineView? {
            (view as? NSOutlineView) ?? view.subviews.lazy.compactMap { outline(in: $0) }.first
        }
        spin(); flush(host)
        guard let tree = outline(in: host), let group = tree.item(atRow: 1) else { preconditionFailure("Tree must display the response") }
        tree.expandItem(group)
        tree.selectRowIndexes(IndexSet(integer: 2), byExtendingSelection: false)
        for (index, preset) in ThemePreset.allCases.enumerated() {
            fixture.theme = ResolvedTheme(preset: preset, revision: UInt64(200 + index))
            spin(); flush(host)
            let key = tree.view(atColumn: 0, row: 2, makeIfNecessary: true) as? NSTextField
            precondition(key?.textColor == fixture.theme.definition.syntaxKey.nsColor, "Tree cells must follow theme revisions")
            precondition(tree.backgroundColor == fixture.theme.definition.window.nsColor)
            precondition(tree.isItemExpanded(group) && tree.selectedRow == 2, "Theme changes must preserve tree expansion and selection")
        }
        fixtureDefaults.set(18.0, forKey: "editor.fontSize")
        spin(); flush(host)
        precondition((tree.view(atColumn: 1, row: 2, makeIfNecessary: true) as? NSTextField)?.font?.pointSize == 18,
            "Tree must honor the editor font-size preference")
        print("json_tree_theme_font_and_selection=passed")
    }

    static func verifyInterface(root: URL) throws {
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")),
            cookieJar: CookieJar(storageURL: root.appending(path: "cookies.json")))
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        interface.responseOrientation = .right
        let session = model.sessions.open(draft: RequestDraft(name: "JSON request", url: "https://example.invalid"))
        session.draft.body = .json(value: "{\"ok\":true}")
        interface.presentation(for: session).requestSection = .body
        interface.synchronizeSelection(model: model)
        let (window, host, _) = mount(ContentView(model: model, interface: interface, loadsWorkspace: false), height: 580)
        defer { window.close() }
        spin(); flush(host)
        // Enable the local accessibility tree without requiring permission to
        // inspect another process. These flags exist only in the test executable.
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface"))
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXManualAccessibility"))
        for orientation in [ResponseOrientation.right, .bottom] {
            interface.responseOrientation = orientation
            for width in [1248.0, 1000.0, 900.0, 720.0] {
                window.setContentSize(NSSize(width: width, height: 580))
                spin(); flush(host)
                let tree = elements(host)
                for name in ["Send Request", "Params", "Headers", "Body", "Auth", "Note", "Format Body"] {
                    precondition(tree.contains { $0.role == "AXButton" && $0.name == name }, "Missing accessible button: \(name)")
                }
                // Edit Long URL and History live in the composer's overflow menu at every width.
                precondition(tree.contains { ["AXButton", "AXMenuButton", "AXPopUpButton"].contains($0.role) && $0.name == "Request Actions" },
                    "Missing accessible menu: Request Actions")
                let context = "\(orientation) at \(Int(width)) pt"
                checkSectionBar(tree, selected: .body, context: context, clipped: "Clipped request control")
                // Every section stays reachable: selecting it scrolls its tab fully into view,
                // clear of the pinned section actions.
                for section in RequestPanelSection.allCases {
                    interface.presentation(for: session).requestSection = section
                    spin(0.3); flush(host)
                    checkSectionBar(elements(host), selected: section, context: "\(context), \(section.rawValue) selected",
                        clipped: "Clipped request control")
                }
                interface.presentation(for: session).requestSection = .body
                spin(0.3); flush(host)
            }
        }
        interface.responseOrientation = .right
        window.setContentSize(NSSize(width: 1248, height: 580))
        interface.responseLayout(for: model.sessions.activeGroupID).requestWidth = 1000
        spin(); flush(host)
        let before = elements(host).first { $0.name == "Content Type" }!.frame
        let run = RunID(); session.beginRun(run)
        var done = false
        Task { @MainActor in
            await session.consume(.head(ResponseHead(status: 200, version: "HTTP/1.1", headers: [], timeToHeadersNS: 1)), runID: run)
            await session.consume(.complete(RunCompletion(bytesReceived: 0, totalTimeNS: 1)), runID: run)
            done = true
        }
        while !done { spin(0.002) }
        spin(); flush(host)
        precondition(elements(host).first { $0.name == "Content Type" }!.frame == before, "Receiving headers must not shift request controls")
        _ = model.sessions.split(tabID: session.id)
        interface.synchronizeSelection(model: model)
        if let copy = model.sessions.activeSession { interface.presentation(for: copy).requestSection = .body }
        spin(0.3); flush(host)
        // Both groups' section bars: their visible parts stay inside their own pane.
        checkSectionBar(elements(host), selected: .body, context: "split", clipped: "Clipped split control")
        print("interface_regressions=passed")

        /// The section tabs scroll inside a strip and the current section's actions are
        /// pinned after it. Only the visible (strip-clipped) part of a tab may be on
        /// screen; it must never overlap a pinned action, the selected tab must be fully
        /// visible, and the strip and actions must fit inside the request pane.
        func checkSectionBar(_ tree: [Element], selected: RequestPanelSection, context: String, clipped: String) {
            // Report the failing layout, since optimized builds drop precondition messages.
            func require(_ condition: Bool, _ message: @autoclosure () -> String) {
                if !condition { fail(message()) }
            }
            func fail(_ message: String) -> Never {
                FileHandle.standardError.write(Data("interface check failed: \(message)\n".utf8))
                exit(1)
            }
            let panes = textViews(host).compactMap { $0.enclosingScrollView }.map { window.convertToScreen($0.convert($0.bounds, to: nil)) }
            let sectionNames = Set(RequestPanelSection.allCases.map(\.rawValue))
            let tabs = tree.filter { $0.role == "AXButton" && sectionNames.contains($0.name) && !$0.frame.isEmpty }
            // Response tabs share some names; only request strips hold Params, Auth, Note or Settings.
            let strips = tree.filter { $0.role == "AXScrollArea" && $0.frame.height < 60 && !$0.frame.isEmpty }.map(\.frame).filter { strip in
                tabs.contains { ["Params", "Auth", "Note", "Settings"].contains($0.name) && $0.frame.intersects(strip) }
            }
            require(!strips.isEmpty, "Section tabs must scroll in their own strip (\(context))")
            for strip in strips {
                // The body editor spans the visible request pane; other sections are checked
                // against their bar, whose width is the pane's when nothing is clipped.
                let bar = tree.first { $0.name == "Request sections" && $0.frame.minY <= strip.midY && $0.frame.maxY >= strip.midY
                    && $0.frame.minX <= strip.minX + 1 && $0.frame.maxX >= strip.maxX - 1 }?.frame
                guard let pane = selected == .body
                    ? panes.first(where: { $0.minX <= strip.minX + 1 && $0.maxX >= strip.maxX - 1 })
                    : bar else {
                    fail("\(clipped): section strip (\(context))")
                }
                if let bar { require(bar.minX >= pane.minX - 1 && bar.maxX <= pane.maxX + 1, "\(clipped): section bar (\(context))") }
                let row = tree.filter { $0.frame.midY >= strip.minY && $0.frame.midY <= strip.maxY
                    && $0.frame.maxX > pane.minX && $0.frame.minX < pane.maxX }
                let rowTabs = row.filter { $0.role == "AXButton" && sectionNames.contains($0.name) }
                guard let tab = rowTabs.first(where: { $0.name == selected.rawValue }) else {
                    fail("Missing section tab \(selected.rawValue) (\(context))")
                }
                require(strip.insetBy(dx: -1, dy: -1).contains(tab.frame), "Selected section \(selected.rawValue) must be fully visible (\(context))")
                let actions = row.filter { ["Content Type", "Auth Type", "Format Body", "Add Part", "Body Actions", "New Entry",
                    "Section Actions", "More Sections"].contains($0.name) && $0.role != "AXScrollArea" && !$0.frame.isEmpty }
                for action in actions {
                    require(pane.minX <= action.frame.minX + 1 && pane.maxX >= action.frame.maxX - 1, "\(clipped): \(action.name) (\(context))")
                    require(!strip.insetBy(dx: 0.5, dy: 0).intersects(action.frame), "\(action.name) must not overlap the section tabs (\(context))")
                    for other in rowTabs {
                        let visible = other.frame.intersection(strip)
                        guard !visible.isNull, visible.width > 0.5 else { continue }
                        require(!visible.insetBy(dx: 0.5, dy: 0).intersects(action.frame),
                            "Visible part of \(other.name) overlaps \(action.name) (\(context))")
                    }
                }
            }
        }
    }

    static func proxyScreenshots(root: URL) throws {
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")), cookieJar: CookieJar())
        model.workspace.name = "Demo workspace"
        let manual = ProxyDocument.manual(routes: [ProxyRouteDocument(destination: .all, endpoint: "http://127.0.0.1:18766")])
        try model.proxyPreferences.save(manual)
        model.settingsTab = "network"
        let output = root.deletingLastPathComponent()
        func capture<V: View>(_ view: V, name: String, width: CGFloat, height: CGFloat) throws {
            let (window, host, _) = mount(view, width: width, height: height)
            defer { window.close() }
            spin(0.15); flush(host)
            guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { preconditionFailure("Could not capture native view") }
            host.cacheDisplay(in: host.bounds, to: bitmap)
            try bitmap.representation(using: .png, properties: [:])!.write(to: output.appending(path: name + ".png"))
        }
        try capture(WireboltSettingsView(model: model), name: "01-app-default", width: 1248, height: 900)
        try capture(WorkspaceNetworkSettings(model: model), name: "02-workspace-inherited", width: 590, height: 580)
        model.workspace.proxy = manual
        try capture(WorkspaceNetworkSettings(model: model), name: "03-workspace-manual", width: 590, height: 680)
        let session = model.sessions.open(draft: RequestDraft(name: "Local API", url: "http://127.0.0.1:18765/json"))
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        interface.responseOrientation = .right
        interface.synchronizeSelection(model: model)
        interface.presentation(for: session).requestSection = .settings
        try capture(ContentView(model: model, interface: interface, loadsWorkspace: false), name: "04-request-inherited", width: 1248, height: 760)
        session.draft.proxy = .direct
        try capture(ContentView(model: model, interface: interface, loadsWorkspace: false), name: "05-request-direct", width: 1248, height: 760)
        let credentials = ProxyCredentialsDocument(username: "demo.proxy.user", password: "demo.proxy.password")
        session.draft.proxy = .manual(.manual(routes: [ProxyRouteDocument(destination: .all, endpoint: "socks5h://localhost:1080", credentials: credentials)]))
        try capture(NetworkSettingsPage(model: model, scope: .request, session: session), name: "06-request-authentication", width: 590, height: 680)
        print("native_proxy_screenshots=written")
    }

    static func settingsUI(root: URL) throws {
        let preferences = ProxyPreferences(defaults: fixtureDefaults)
        let original = ProxyDocument.manual(routes: [ProxyRouteDocument(destination: .all, endpoint: "http://localhost:8080")])
        try preferences.save(original)
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")),
            cookieJar: CookieJar(), proxyPreferences: preferences)
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        _ = model.sessions.open(draft: RequestDraft(url: "https://example.invalid"))
        interface.synchronizeSelection(model: model)
        let (window, host, _) = mount(ContentView(model: model, interface: interface, loadsWorkspace: false), width: 1248, height: 900)
        defer { window.close() }
        window.makeKey()
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface"))
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXManualAccessibility"))
        spin(); flush(host)
        func press(_ title: String, in targetWindow: NSWindow? = nil) {
            let targetWindow = targetWindow ?? window
            let targetHost = targetWindow.contentView!
            guard let button = elements(targetHost).first(where: { $0.role == "AXButton" && $0.name == title }) else {
                preconditionFailure("Missing settings action: \(title)")
            }
            precondition(button.frame.height >= 28, "Settings actions must keep their minimum hit height")
            let frame = targetWindow.convertFromScreen(button.frame)
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                let event = NSEvent.mouseEvent(with: type, location: NSPoint(x: frame.midX, y: frame.midY), modifierFlags: [],
                    timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: targetWindow.windowNumber,
                    context: nil, eventNumber: 1, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0)!
                targetWindow.sendEvent(event)
            }
            spin(); flush(host)
        }
        func fields(_ view: NSView) -> [NSTextField] {
            (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap { fields($0) }
        }
        guard let urlField = fields(host).first(where: { $0.accessibilityLabel() == "Request URL" }) else {
            preconditionFailure("The workspace URL must remain reachable before opening Settings")
        }
        window.makeFirstResponder(urlField)
        (urlField.currentEditor() as? NSTextView)?.setSelectedRange(NSRange(location: 2, length: 4))
        presentAppSettings(model)
        spin(0.1); flush(host)
        precondition(model.settingsTab == "appearance")
        precondition(!elements(host).contains { $0.name == "Request URL" }, "The settings modal must hide background controls from accessibility")
        guard let searchEditor = window.firstResponder as? NSTextView, let searchField = searchEditor.delegate as? NSTextField,
              searchField.placeholderString == "Search" else { preconditionFailure("Opening Settings must focus its search field") }
        let navigation = ["General", "Appearance", "Network", "Keyboard", "Privacy"]
        let buttons = elements(host).filter { $0.role == "AXButton" && navigation.contains($0.name) }
        precondition(buttons.count == navigation.count && buttons.allSatisfy { $0.frame.width >= 180 && $0.frame.height >= 28 },
            "Settings must use the approved sidebar, with every section reachable")
        press("Network")
        precondition(model.settingsTab == "network")
        guard let field = fields(host).first(where: { $0.isEditable && $0.stringValue == "localhost" }) else {
            preconditionFailure("Network settings must preserve the native editable proxy form")
        }
        field.selectText(nil)
        guard let editor = field.currentEditor() as? NSTextView else { preconditionFailure("Proxy host must accept keyboard input") }
        editor.insertText("edited.proxy.invalid", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        window.makeFirstResponder(nil)
        spin(); flush(host)
        press("Appearance")
        precondition(model.settingsTab == "appearance")
        precondition(!elements(host).contains { $0.name == "Proxy host" }, "Hidden settings pages must leave the accessibility tree")
        press("General")
        precondition(elements(host).contains { $0.name == "Word wrap" })
        press("Keyboard")
        precondition(elements(host).contains { $0.name == "Send request or message" })
        press("Privacy")
        press("Privacy Policy")
        for _ in 0..<100 where window.attachedSheet == nil { spin(0.002) }
        guard let sheet = window.attachedSheet else { preconditionFailure("Privacy Policy must open its content") }
        press("Done", in: sheet)
        for _ in 0..<100 where window.attachedSheet != nil { spin(0.002) }
        precondition(window.attachedSheet == nil)
        press("Network")
        precondition(fields(host).contains { $0.isEditable && $0.stringValue == "edited.proxy.invalid" },
            "Switching settings sections must preserve unapplied proxy edits")
        precondition(preferences.configuration == original, "Navigation must not save a staged proxy edit")
        press("Back to workspace")
        precondition(window.isVisible && !model.isShowingAppSettings, "Closing Settings must leave the workspace window open")
        precondition(urlField.currentEditor() === window.firstResponder && (window.firstResponder as? NSTextView)?.selectedRange() == NSRange(location: 2, length: 4),
            "Closing Settings must restore the previous URL focus and selection")
        precondition(elements(host).contains { $0.name == "Request URL" }, "Closing the modal must restore access to the workspace")
        presentAppSettings(model)
        spin(0.1); flush(host)
        precondition(fields(host).contains { $0.isEditable && $0.stringValue == "edited.proxy.invalid" },
            "Closing and reopening Settings must preserve unapplied proxy edits")
        press("Reset")
        precondition(fields(host).contains { $0.isEditable && $0.stringValue == "localhost" })
        window.makeFirstResponder(searchField)
        guard let editor = searchField.currentEditor() as? NSTextView else { preconditionFailure("Search must remain editable") }
        editor.insertText("theme", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        spin(); flush(host)
        precondition(model.settingsTab == "appearance" && !elements(host).contains { $0.role == "AXButton" && $0.name == "Network" },
            "Searching settings must filter and select matching sections")
        press("Clear settings search")
        precondition(elements(host).filter { $0.role == "AXButton" && navigation.contains($0.name) }.count == navigation.count)
        press("Close Settings")
        precondition(window.isVisible && !model.isShowingAppSettings)
        model.settingsTab = "network"
        presentAppSettings(model)
        spin(0.1); flush(host)
        precondition(model.settingsTab == "network" && elements(host).filter { $0.role == "AXButton" && navigation.contains($0.name) }.count == navigation.count,
            "Reopening a settings destination must clear the old search filter")
        press("Close Settings")
        print("settings_modal_search_navigation_staged_proxy_and_privacy=passed")
    }

    static func composerFocus(root: URL) {
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")), cookieJar: CookieJar())
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        let session = model.sessions.open(draft: RequestDraft(url: "https://example.invalid/products"))
        interface.synchronizeSelection(model: model)
        let (window, host, _) = mount(ContentView(model: model, interface: interface, loadsWorkspace: false))
        defer { window.close() }
        window.makeKey()
        spin(); flush(host)
        func fields(_ view: NSView) -> [NSTextField] {
            (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap { fields($0) }
        }
        guard let field = fields(host).first(where: { $0.accessibilityLabel() == "Request URL" }) else {
            preconditionFailure("The composer must expose its URL field")
        }
        window.makeFirstResponder(field)
        spin(); flush(host)
        guard let editor = field.currentEditor() as? NSTextView else { preconditionFailure("URL input must accept native editing focus") }
        guard field.focusRingType == .none, !field.isBordered, !field.isBezeled else {
            fputs("composer_focus: unexpected rectangular field chrome, ring=\(field.focusRingType.rawValue) border=\(field.isBordered) bezel=\(field.isBezeled)\n", stderr)
            exit(1)
        }
        editor.setSelectedRange(NSRange(location: 8, length: 7))
        editor.insertText("wirebolt", replacementRange: editor.selectedRange())
        spin(); flush(host)
        precondition(session.draft.url == "https://wirebolt.invalid/products", "Focus styling must preserve native URL editing")
        window.makeFirstResponder(nil)
        spin(); flush(host)
        interface.focusURLTrigger += 1
        spin(); flush(host)
        precondition(field.currentEditor() === window.firstResponder, "Edit URL must retain its keyboard focus route")
        print("composer_focus_native_editing_and_keyboard=passed")
    }

    static func proxySettings(root: URL) throws {
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")), cookieJar: CookieJar())
        try model.proxyPreferences.save(.manual(routes: [ProxyRouteDocument(destination: .all, endpoint: "http://localhost:8080")]))
        let session = model.sessions.open(draft: RequestDraft(name: "Proxy fixture", url: "https://example.invalid"))
        let (window, host, _) = mount(NetworkSettingsPage(model: model, scope: .request, session: session), width: 590, height: 850)
        defer { window.close() }
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface"))
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXManualAccessibility"))
        spin(); flush(host)
        precondition(elements(host).contains { $0.name.contains("Source: App default") }, "Request must explain inherited app policy")
        precondition(session.draft.proxy == .inherit && model.workspace.proxy == nil)
        session.draft.proxy = .manual(.manual(routes: [ProxyRouteDocument(destination: .all, endpoint: "http://localhost:8080")]))
        spin(); flush(host)
        for width in [590.0, 390.0] {
            window.setContentSize(NSSize(width: width, height: 850))
            spin(); flush(host)
            let frame = window.convertToScreen(host.convert(host.bounds, to: nil))
            let tree = elements(host)
            for name in ["Proxy host", "Proxy port", "Proxy protocol"] {
                guard let field = tree.first(where: { $0.name == name }) else { preconditionFailure("Missing proxy field: \(name)") }
                precondition(field.frame.minX >= frame.minX && field.frame.maxX <= frame.maxX, "Proxy field clipped at \(width): \(name)")
            }
        }
        window.setContentSize(NSSize(width: 590, height: 850))
        spin(); flush(host)
        func fields(_ view: NSView) -> [NSTextField] {
            (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap { fields($0) }
        }
        guard let hostField = fields(host).first(where: { $0.isEditable && $0.stringValue == "localhost" }) else {
            preconditionFailure("Proxy host must be a native editable field")
        }
        window.makeKey()
        hostField.selectText(nil)
        guard let editor = hostField.currentEditor() as? NSTextView else { preconditionFailure("Proxy host must accept text input") }
        editor.insertText("127.0.0.1", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        window.makeFirstResponder(nil)
        spin(); flush(host)
        guard let portField = fields(host).first(where: { $0.isEditable && $0.stringValue == "8080" }) else { preconditionFailure("Missing proxy port") }
        portField.selectText(nil)
        guard let portEditor = portField.currentEditor() as? NSTextView else { preconditionFailure("Port must accept input") }
        portEditor.insertText("12345", replacementRange: NSRange(location: 0, length: portEditor.string.utf16.count))
        window.makeFirstResponder(nil)
        spin(); flush(host)
        precondition(elements(host).contains { $0.name == "HTTP · 127.0.0.1:12345" }, "The accessible connection preview must track host and port edits")
        guard let apply = elements(host).first(where: { $0.name == "Apply to request" }) else { preconditionFailure("Missing apply action") }
        let rect = window.convertFromScreen(apply.frame)
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            let event = NSEvent.mouseEvent(with: type, location: NSPoint(x: rect.midX, y: rect.midY), modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, eventNumber: 1, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0)!
            window.sendEvent(event)
        }
        spin(0.2); flush(host)
        precondition(session.draft.proxy == .manual(.manual(routes: [ProxyRouteDocument(destination: .all, endpoint: "http://127.0.0.1:12345")])), "Native text editing and Apply must update the request")
        precondition(model.workspace.proxy == nil && model.proxyPreferences.configuration != .direct, "Request UI must not mutate parents")
        print("proxy_settings_interface=passed")
    }

    /// Sidebar reordering through SwiftUI's AppKit drop destinations: a row being dragged
    /// drops before or after another row, and text dragged from elsewhere is refused.
    static func sidebarReorder(root: URL) throws {
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")), cookieJar: CookieJar())
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        let requests = (0..<6).map { RequestLocation(collectionID: "drag", order: $0, request: RequestDraft(id: "r\($0)", name: "Drag Request \($0)", url: "https://example.invalid/\($0)")) }
        model.workspace.collections = [CollectionDraft(id: "drag", name: "Drag Fixture", requests: requests)]
        let (window, host, _) = mount(ContentView(model: model, interface: interface, loadsWorkspace: false), height: 720)
        defer { window.close() }
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface"))
        spin(0.4); flush(host)
        func rowFrame(_ index: Int) -> NSRect {
            guard let row = elements(window).first(where: { $0.name == "GET request, Drag Request \(index)" }) else {
                preconditionFailure("Sidebar row is missing from the accessibility tree")
            }
            return window.convertFromScreen(row.frame)
        }
        /// The AppKit views SwiftUI registers for drops, deepest last.
        func destinations(at point: NSPoint) -> [NSView] {
            func walk(_ view: NSView) -> [NSView] {
                let own = !view.registeredDraggedTypes.isEmpty && view.convert(view.bounds, to: nil).contains(point) ? [view] : []
                return own + view.subviews.flatMap(walk)
            }
            return window.contentView.map(walk) ?? []
        }
        func drag(_ payload: String, to point: NSPoint) -> (NSDragOperation, Bool) {
            let pasteboard = NSPasteboard(name: NSPasteboard.Name("wirebolt-drag-\(UUID().uuidString)"))
            defer { pasteboard.releaseGlobally() }
            pasteboard.clearContents()
            pasteboard.writeObjects([payload as NSString])
            let info = FixtureDraggingInfo(window: window, location: point, pasteboard: pasteboard)
            guard let target = destinations(at: point).last else { return ([], false) }
            _ = target.draggingEntered(info)
            let operation = target.draggingUpdated(info)
            spin(0.05); flush(host)
            guard operation != [] else { target.draggingExited(info); spin(0.05); return (operation, false) }
            let performed = target.prepareForDragOperation(info) && target.performDragOperation(info)
            target.concludeDragOperation(info)
            spin(0.1); flush(host)
            return (operation, performed)
        }
        let row = rowFrame(1)
        // Text dragged from elsewhere is refused.
        let foreign = drag("request|drag|r4", to: NSPoint(x: row.midX, y: row.maxY - 4))
        precondition(foreign.0 == [] && !foreign.1, "Text dragged from elsewhere must not reorder the sidebar")
        // A row dragged within the sidebar (onDrag records it) drops before another row...
        interface.sidebarDragIdentifier = "request|drag|r4"
        spin(0.05); flush(host)
        let before = drag("request|drag|r4", to: NSPoint(x: row.midX, y: row.maxY - 4))
        precondition(before.0 == .move && before.1, "Dropping on a row's upper half must be accepted")
        precondition(interface.sidebarDragIdentifier == nil, "A completed drop must end the sidebar drag")
        // ...or after it.
        interface.sidebarDragIdentifier = "request|drag|r5"
        spin(0.05); flush(host)
        let after = drag("request|drag|r5", to: NSPoint(x: row.midX, y: row.minY + 4))
        precondition(after.0 == .move && after.1, "Dropping on a row's lower half must be accepted")
        precondition(interface.sidebarDragIdentifier == nil, "A completed drop must end the sidebar drag")
        print("sidebar_reorder_drop=passed")
    }

    static func requestClicks(root: URL, tabs: Bool = false, tabCount: Int = 2, noteBytes: Int = 0) throws {
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")), cookieJar: CookieJar())
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        let note = String(repeating: "# Hidden note\n\n", count: noteBytes / 15)
        let requests = (0..<(tabs ? max(2, tabCount) : 20)).map { RequestLocation(collectionID: "clicks", order: $0, request: RequestDraft(id: "r\($0)", name: "Click Request \($0)", url: "https://example.invalid/\($0)", note: note)) }
        model.workspace.collections = [CollectionDraft(id: "clicks", name: "Click Audit", requests: requests)]
        if tabs {
            for request in requests { interface.activateSavedRequest(request, model: model, preview: false) }
        }
        interface.activateSavedRequest(requests[0], model: model)
        let (window, host, _) = mount(ContentView(model: model, interface: interface, loadsWorkspace: false), height: 720)
        defer { window.close() }
        window.makeKey()
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface"))
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXManualAccessibility"))
        spin(0.4); flush(host)
        func namePoint(_ index: Int) -> NSPoint {
            guard let row = elements(window).first(where: { $0.name == (tabs ? "Click Request \(index)" : "GET request, Click Request \(index)") }) else {
                preconditionFailure("Request row is missing from the accessibility tree")
            }
            let rect = window.convertFromScreen(row.frame)
            return NSPoint(x: tabs ? rect.midX : rect.minX + min(100, rect.width * 0.7), y: rect.midY)
        }
        func click(_ point: NSPoint, count: Int = 1) {
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                    timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                    context: nil, eventNumber: count, clickCount: count, pressure: type == .leftMouseDown ? 1 : 0)!
                window.sendEvent(event)
            }
        }
        var times: [Double] = []
        for index in (tabs && tabCount < 5 ? [1, 0, 1, 0, 1, 0] : [1, 2, 3, 1, 4, 2]) {
            let point = namePoint(index)
            let start = ContinuousClock.now
            click(point)
            while model.sessions.activeSession?.requestID != "r\(index)" && ms(start) < 2000 { spin(0.001) }
            flush(host)
            times.append(ms(start))
            precondition(model.sessions.activeSession?.requestID == "r\(index)", "Name click must open its request")
            spin(0.15)
        }
        record(tabs ? "request_tab_click" : "request_name_click", times, budget: 150, details: ["path": tabs ? "NSWindow mouse down/up on document tab through selection and layout" : "NSWindow mouse down/up on sidebar name through selection and layout", "double_click_interval_ms": NSEvent.doubleClickInterval * 1000])
        spin(NSEvent.doubleClickInterval)
        let renameIndex = tabs ? 0 : 2
        let point = namePoint(renameIndex)
        click(point); spin(0.05); click(point, count: 2); spin(0.15); flush(host)
        func fields(_ view: NSView) -> [NSTextField] {
            (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap { fields($0) }
        }
        precondition(fields(host).contains { $0.isEditable && $0.stringValue == "Click Request \(renameIndex)" }, "Double-click must still enter rename")
        print("request_click_regressions=passed")
    }

    static func verifyNotes() async throws {
        let fixture = TextFixture("# Original\n\n**Saved** note")
        let (window, host, _) = mount(NotesFixture(fixture: fixture), width: 430, height: 420)
        defer { window.close() }
        try await Task.sleep(for: .milliseconds(50)); flush(host)
        let before = await MarkdownPreviewCache.shared.parseCount
        precondition(before == 0, "Edit must never start a preview parse")
        fixture.preview = true
        for _ in 0..<100 {
            try await Task.sleep(for: .milliseconds(10)); flush(host)
            if textViews(host).contains(where: { !$0.isEditable && $0.string == "Original\nSaved note" }) { break }
        }
        guard let preview = textViews(host).first(where: { !$0.isEditable }) else { preconditionFailure("Preview missing") }
        precondition(preview.string == "Original\nSaved note")
        guard let scroll = preview.enclosingScrollView else { preconditionFailure("Preview must scroll") }
        precondition(scroll.frame.height > 350, "The mode toolbar must not consume the preview height")
        fixture.text = String(repeating: "# Obsolete\n\nold text\n\n", count: 40_000)
        flush(host)
        try await Task.sleep(for: .milliseconds(5))
        fixture.text = "# Latest\n\nCorrect request"
        for _ in 0..<300 {
            try await Task.sleep(for: .milliseconds(10)); flush(host)
            if textViews(host).contains(where: { !$0.isEditable && $0.string == "Latest\nCorrect request" }) { break }
        }
        precondition(textViews(host).contains(where: { !$0.isEditable && $0.string == "Latest\nCorrect request" }), "An obsolete render must not replace current content")
        fixture.preview = false
        try await Task.sleep(for: .milliseconds(50)); flush(host)
        precondition(textViews(host).contains(where: { $0.isEditable && $0.string == fixture.text }), "Edit must retain original Markdown")
        print("notes_modes_cancellation_layout=passed")
    }

    static func markdownPreview(_ bytes: Int) async throws {
        let paragraph = "# Endpoint\n\nUse **authorization** and *JSON*.\n\n- First item\n- Second item\n\n```json\n{\"ok\":true}\n```\n\n"
        let source = String(repeating: paragraph, count: max(1, bytes / paragraph.utf8.count))
        let cache = MarkdownPreviewCache()
        let initialParses = await cache.parseCount
        precondition(initialParses == 0, "Creating a cache must not parse hidden notes")
        let start = ContinuousClock.now
        let render = Task { try await cache.render(source) }
        // A main-actor hop must remain available while the background parser runs.
        let pulse = ContinuousClock.now
        try await Task.sleep(for: .milliseconds(5))
        record("markdown_main_actor_pulse", [ms(pulse)], budget: 100)
        let document = try await render.value
        record("markdown_parse", [ms(start)], budget: 10_000, details: ["bytes": source.utf8.count])
        if document.cost <= 8 * 1_024 * 1_024 {
            let hit = ContinuousClock.now
            let cached = try await cache.render(source)
            precondition(cached === document)
            record("markdown_cache_hit", [ms(hit)], budget: 50)
        } else {
            let bytes = await cache.retainedBytes
            precondition(bytes == 0, "Oversized previews must not occupy the cache")
        }
        let (window, host, mounted) = mount(MarkdownTextView(rendered: document), width: 600, height: 500)
        defer { window.close() }
        record("markdown_native_first_view", [mounted], budget: 150)
        guard let text = textViews(host).first else { preconditionFailure("Missing native preview") }
        precondition(text.string.contains("Endpoint") && !text.isEditable && text.isSelectable)
        let scrollStart = ContinuousClock.now
        text.scrollToEndOfDocument(nil); flush(host)
        record("markdown_scroll_to_end", [ms(scrollStart)], budget: 150)
        let finalParses = await cache.parseCount
        precondition(finalParses == 1, "Cached previews must not reparse")
        print("markdown_native=passed")
    }

    static func closeFixture(_ count: Int, root: URL) -> (WireboltModel, WorkspaceUIState) {
        let model = WireboltModel(
            runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")),
            cookieJar: CookieJar(storageURL: root.appending(path: "cookies.json")),
            proxyPreferences: ProxyPreferences(defaults: fixtureDefaults))
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        let raw = String(repeating: "x", count: 1_048_576)
        for i in 0..<count {
            _ = model.sessions.open(draft: RequestDraft(id: "body\(i)", name: "Body \(i)", body: .json(value: raw)))
        }
        interface.synchronizeSelection(model: model)
        return (model, interface)
    }

    static func closeTabs(_ count: Int, root: URL) {
        var values: [Double] = []
        for _ in 0..<max(5, samples) {
            let (model, interface) = closeFixture(count, root: root)
            let id = model.sessions.activeSession!.id
            let start = ContinuousClock.now
            interface.closeTab(id: id, model: model)
            values.append(ms(start))
            precondition(model.sessions.sessions.count == count - 1)
        }
        // Run the traffic guard separately: retaining the removed session to read
        // counters must not keep document destruction out of the timed command.
        let (model, interface) = closeFixture(count, root: root)
        let sessions = Array(model.sessions.sessions.values)
        let trafficBefore = sessions.map { $0.rust.bridgeTraffic() }
        interface.closeTab(id: model.sessions.activeSession!.id, model: model)
        for (session, before) in zip(sessions, trafficBefore) {
            let after = session.rust.bridgeTraffic()
            precondition(after.documentReads == before.documentReads && after.draftWrites == before.draftWrites)
            precondition(session.draftSynchronizationCount == 0)
        }
        precondition(model.sessions.lastCloseRestoredDraftCount == 0, "Clean close must carry no draft payloads")
        record("tab_close_command", values, budget: tabCloseBudget, details: ["tabs": count, "body_bytes_each": 1_048_576,
            "draft_serializations": 0, "restored_drafts": 0,
            "bridge_document_reads": 0, "bridge_draft_writes": 0, "bridge_body_bytes": 0])
    }

    static func verifyLastTabReopen(root: URL) {
        let model = WireboltModel(runner: OfflineRunner(),
            history: HistoryRepository(root: root.appending(path: "history")),
            cookieJar: CookieJar(storageURL: root.appending(path: "cookies.json")),
            proxyPreferences: ProxyPreferences(defaults: fixtureDefaults))
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        let session = model.sessions.open(draft: RequestDraft(id: "last", name: "Saved"))
        interface.synchronizeSelection(model: model)
        session.draft.note = "Unsaved"
        interface.closeTab(id: session.id, model: model)
        precondition(interface.dirtyCloseRequest?.documentIDs == [session.id])
        interface.confirmDirtyClose(model: model)
        precondition(model.sessions.sessions.isEmpty)
        interface.reopenLastDocument(model: model)
        precondition(model.sessions.activeSession === session)
        precondition(session.draft.note.isEmpty && !session.isDirty)
        session.draft.note = "After reopen"
        session.synchronizeDraft()
        precondition(session.rust.document().draft.note == "After reopen")
        interface.closeTab(id: session.id, model: model)
        interface.isShowingDirtyClose = false
        precondition(model.sessions.activeSession === session)
    }

    static func main() async throws {
        let budgets = try JSONDecoder().decode(Budgets.self, from: Data(contentsOf: URL(fileURLWithPath: "performance/budgets.json")))
        windowFirstContentBudget = budgets.nativeColdWindowFirstContentMilliseconds
        rendererColdSwitchBudget = budgets.nativeResponseRendererColdSwitchMilliseconds
        rendererSwitchBudget = budgets.nativeResponseRendererSwitchP95Milliseconds
        tabCloseBudget = budgets.nativeTabCloseCommandP95Milliseconds
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        defer { fixtureDefaults.removePersistentDomain(forName: defaultsName) }
        fixtureDefaults.set(12.0, forKey: "editor.fontSize")
        fixtureDefaults.set(ProcessInfo.processInfo.environment["WIREBOLT_BENCH_WRAP"] != "off", forKey: "editor.wordWrap")
        fixtureDefaults.set(true, forKey: "editor.showInvisibles")
        let args=CommandLine.arguments
        precondition(args.count>=4,"mode size output [samples]")
        let mode=args[1], size=Int(args[2])!
        if args.count>4 { samples=Int(args[4])! }
        let root=URL(fileURLWithPath:args[3]).deletingLastPathComponent().appending(path:"fixture-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at:root,withIntermediateDirectories:true)
        defer { try? FileManager.default.removeItem(at:root) }
        let watchdog=DispatchWorkItem { @Sendable in exit(86) }
        DispatchQueue.global().asyncAfter(deadline:.now()+120,execute:watchdog)
        defer { watchdog.cancel() }
        if mode=="notes-ui" { try await verifyNotes() }
        else if mode=="markdown" { try await markdownPreview(size) }
        else if mode=="editor" { try editor(size) }
        else if mode=="workspace" { try workspace(size,root:root) }
        else if mode=="tab-close" { closeTabs(size, root: root); verifyLastTabReopen(root: root) }
        else if mode=="response" { try response(size) }
        else if mode=="interface" { try verifyInterface(root: root); try await verifyRedesignFlows(root: root); verifyTreeTheme() }
        else if mode=="request-click-notes" || mode=="tab-click-notes" {
            try requestClicks(root: root, tabs: mode=="tab-click-notes", tabCount: 5, noteBytes: size)
            let count = await MarkdownPreviewCache.shared.parseCount
            precondition(count == 0, "Hidden notes must never be parsed when switching requests")
        }
        else if mode=="request-click" { try requestClicks(root: root) }
        else if mode=="sidebar-reorder" { try sidebarReorder(root: root) }
        else if mode=="proxy-screenshots" { try proxyScreenshots(root: root) }
        else if mode=="proxy-settings" { try proxySettings(root: root) }
        else if mode=="settings-ui" { try settingsUI(root: root) }
        else if mode=="composer-focus" { composerFocus(root: root) }
        else if mode=="tab-click" { try requestClicks(root: root, tabs: true, tabCount: size) }
        else { preconditionFailure("unknown mode") }
        var usage=rusage(); getrusage(RUSAGE_SELF,&usage)
        let output:[String:Any]=["mode":mode,"size":size,"measurements":measurements,
            "peak_rss_bytes":usage.ru_maxrss,"thermal_state":ProcessInfo.processInfo.thermalState.rawValue,
            "physical_frames_measured":false,"optimized":true]
        try JSONSerialization.data(withJSONObject:output,options:[.prettyPrinted,.sortedKeys]).write(to:URL(fileURLWithPath:args[3]))
        if measurements.contains(where: { $0["over_budget"] as? Bool == true }) { exit(1) }
    }
}

/// A dragging session for the sidebar drop workload; SwiftUI reads the location and pasteboard.
@MainActor
final class FixtureDraggingInfo: NSObject, @MainActor NSDraggingInfo {
    let window: NSWindow
    let location: NSPoint
    let pasteboard: NSPasteboard
    init(window: NSWindow, location: NSPoint, pasteboard: NSPasteboard) {
        self.window = window
        self.location = location
        self.pasteboard = pasteboard
    }
    var draggingDestinationWindow: NSWindow? { window }
    var draggingSourceOperationMask: NSDragOperation { [.move, .copy, .generic] }
    var draggingLocation: NSPoint { location }
    var draggedImageLocation: NSPoint { location }
    var draggedImage: NSImage? { nil }
    var draggingPasteboard: NSPasteboard { pasteboard }
    var draggingSource: Any? { nil }
    var draggingSequenceNumber: Int { 1 }
    func slideDraggedImage(to screenPoint: NSPoint) {}
    var draggingFormation: NSDraggingFormation = .default
    var animatesToDestination = false
    var numberOfValidItemsForDrop = 1
    func enumerateDraggingItems(options enumOpts: NSDraggingItemEnumerationOptions = [], for view: NSView?,
                                classes classArray: [AnyClass], searchOptions: [NSPasteboard.ReadingOptionKey: Any] = [:],
                                using block: @escaping (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void) {}
    var springLoadingHighlight: NSSpringLoadingHighlight { .none }
    func resetSpringLoading() {}
    /// Read by SwiftUI; AppKit's own dragging info provides it.
    private var lastOperation: UInt = 0
    @objc func _lastDragDestinationOperation() -> UInt { lastOperation }
    @objc func _setLastDragDestinationOperation(_ operation: UInt) { lastOperation = operation }
}
