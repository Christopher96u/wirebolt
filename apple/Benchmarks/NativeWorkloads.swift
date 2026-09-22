import AppKit
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
    init(_ text: String) { self.text = text }
}
private struct EditorFixture: View {
    @Bindable var fixture: TextFixture
    var body: some View { NativeCodeEditor(text: $fixture.text, language: .json) }
}
final class ProbeWindow: NSWindow { override var canBecomeKey: Bool { true } }

@main @MainActor struct NativeWorkloads {
    static let log = OSLog(subsystem: "com.wirebolt.profiling", category: .pointsOfInterest)
    static var measurements: [[String: Any]] = []
    private struct Budgets: Decodable {
        let nativeColdWindowFirstContentMilliseconds: Double
    }
    static var windowFirstContentBudget = 0.0
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
        if let scroll = editor.enclosingScrollView {
            for i in 0..<samples {
                let start = ContinuousClock.now
                let y = max(0, editor.bounds.height - scroll.contentView.bounds.height) * CGFloat(i % 10) / 9
                scroll.contentView.scroll(to: NSPoint(x: 0, y: y))
                scroll.reflectScrolledClipView(scroll.contentView)
                flush(host)
                editor.display()
                scrolling.append(ms(start))
                spin(0.01)
            }
        }
        if !scrolling.isEmpty { record("editor_scroll", scrolling, budget: 16, details: ["rows": rows]) }
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
        record("response_renderer_first_switch",[switches[0]],budget:50,details:["rows":rows,"raw_bytes":raw.utf8.count])
        record("response_renderer_switch",Array(switches.dropFirst(2)),budget:50,details:["rows":rows,"raw_bytes":raw.utf8.count,"alternating":"Raw,JSON","readiness":"expected text or indexed first viewport drawn; not GPU presentation", "cold_switches_ms":Array(switches.prefix(2))])
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
                for name in ["Send request", "Edit Long URL", "Params", "Headers", "Body", "Auth", "Note", "Format Body"] {
                    precondition(tree.contains { $0.role == "AXButton" && $0.name == name }, "Missing accessible button: \(name)")
                }
                guard let note = tree.first(where: { $0.name == "Note" }),
                      let type = tree.first(where: { $0.name == "Content Type" }) else {
                    preconditionFailure("Section labels must be accessible")
                }
                precondition(!note.frame.isEmpty && !type.frame.isEmpty)
                precondition(!note.frame.intersects(type.frame), "Note and Content Type must not overlap")
                let panes = textViews(host).compactMap { $0.enclosingScrollView }.map { window.convertToScreen($0.convert($0.bounds, to: nil)) }
                for control in tree where ["Params", "Headers", "Body", "Auth", "Note", "Content Type", "Format Body"].contains(control.name) {
                    precondition(panes.contains { $0.minX <= control.frame.minX && $0.maxX >= control.frame.maxX }, "Clipped request control: \(control.name)")
                }
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
        spin(); flush(host)
        let panes = textViews(host).compactMap { $0.enclosingScrollView }.map { window.convertToScreen($0.convert($0.bounds, to: nil)) }
        for control in elements(host) where ["Params", "Auth", "Note", "Content Type", "Format Body"].contains(control.name) {
            precondition(panes.contains { $0.minX <= control.frame.minX && $0.maxX >= control.frame.maxX }, "Clipped split control: \(control.name)")
        }
        print("interface_regressions=passed")
    }

    static func proxyScreenshots(root: URL) throws {
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")), cookieJar: CookieJar())
        model.workspace.name = "Demo workspace"
        let manual = ProxyDocument.manual(routes: [ProxyRouteDocument(destination: "all", endpoint: "http://127.0.0.1:18766")])
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
        try capture(WireboltSettingsView(model: model), name: "01-app-default", width: 620, height: 570)
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
        session.draft.proxy = .manual(.manual(routes: [ProxyRouteDocument(destination: "all", endpoint: "socks5h://localhost:1080", credentials: credentials)]))
        try capture(NetworkSettingsPage(model: model, scope: .request, session: session), name: "06-request-authentication", width: 590, height: 680)
        print("native_proxy_screenshots=written")
    }

    static func proxySettings(root: URL) throws {
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")), cookieJar: CookieJar())
        try model.proxyPreferences.save(.manual(routes: [ProxyRouteDocument(destination: "all", endpoint: "http://localhost:8080")]))
        let session = model.sessions.open(draft: RequestDraft(name: "Proxy fixture", url: "https://example.invalid"))
        let (window, host, _) = mount(NetworkSettingsPage(model: model, scope: .request, session: session), width: 590, height: 850)
        defer { window.close() }
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXEnhancedUserInterface"))
        NSApp.accessibilitySetValue(true, forAttribute: NSAccessibility.Attribute(rawValue: "AXManualAccessibility"))
        spin(); flush(host)
        precondition(elements(host).contains { $0.name.contains("Source: App default") }, "Request must explain inherited app policy")
        precondition(session.draft.proxy == .inherit && model.workspace.proxy == nil)
        session.draft.proxy = .manual(.manual(routes: [ProxyRouteDocument(destination: "all", endpoint: "http://localhost:8080")]))
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
        precondition(session.draft.proxy == .manual(.manual(routes: [ProxyRouteDocument(destination: "all", endpoint: "http://127.0.0.1:12345")])), "Native text editing and Apply must update the request")
        precondition(model.workspace.proxy == nil && model.proxyPreferences.configuration != .direct, "Request UI must not mutate parents")
        print("proxy_settings_interface=passed")
    }

    static func requestClicks(root: URL, tabs: Bool = false, tabCount: Int = 2) throws {
        let model = WireboltModel(runner: OfflineRunner(), history: HistoryRepository(root: root.appending(path: "history")), cookieJar: CookieJar())
        let interface = WorkspaceUIState(defaults: fixtureDefaults)
        let requests = (0..<(tabs ? max(2, tabCount) : 20)).map { RequestLocation(collectionID: "clicks", request: RequestDraft(id: "r\($0)", name: "Click Request \($0)", url: "https://example.invalid/\($0)")) }
        model.workspace.collections = [CollectionDraft(id: "clicks", name: "Click Audit", requests: requests)]
        if tabs {
            for request in requests { interface.activateSavedRequest(request, model: model) }
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

    static func main() throws {
        windowFirstContentBudget = try JSONDecoder().decode(Budgets.self,
            from: Data(contentsOf: URL(fileURLWithPath: "performance/budgets.json"))).nativeColdWindowFirstContentMilliseconds
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
        let watchdog=DispatchWorkItem { exit(86) }
        DispatchQueue.global().asyncAfter(deadline:.now()+120,execute:watchdog)
        defer { watchdog.cancel() }
        if mode=="editor" { try editor(size) }
        else if mode=="workspace" { try workspace(size,root:root) }
        else if mode=="response" { try response(size) }
        else if mode=="interface" { try verifyInterface(root: root) }
        else if mode=="request-click" { try requestClicks(root: root) }
        else if mode=="proxy-screenshots" { try proxyScreenshots(root: root) }
        else if mode=="proxy-settings" { try proxySettings(root: root) }
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
