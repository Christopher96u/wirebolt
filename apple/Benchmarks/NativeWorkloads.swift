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
        record("editor_mount",[cold],budget:50,details:["bytes":json.utf8.count,"rows":rows])
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
        record("workspace_mount",[cold],budget:50,details:["requests":count,"tabs":tabs.count,"excludes_deferred_content":false])
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
        record("response_pretty_ready",[ms(start)],budget:50,details:["rows":rows,"raw_bytes":raw.utf8.count,"poll_interval_ms":2,"includes_framework_and_window_construction":true])
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
        let received = try Data(contentsOf: session.bodyStore!.url)
        precondition(received == Data(raw.utf8), "presentation must not change received bytes")
    }
    static func main() throws {
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
        else { preconditionFailure("unknown mode") }
        var usage=rusage(); getrusage(RUSAGE_SELF,&usage)
        let output:[String:Any]=["mode":mode,"size":size,"measurements":measurements,
            "peak_rss_bytes":usage.ru_maxrss,"thermal_state":ProcessInfo.processInfo.thermalState.rawValue,
            "physical_frames_measured":false,"optimized":true]
        try JSONSerialization.data(withJSONObject:output,options:[.prettyPrinted,.sortedKeys]).write(to:URL(fileURLWithPath:args[3]))
        let gated: Set<String> = ["editor_insert", "tab_switch", "sidebar_filter", "response_renderer_switch"]
        if measurements.contains(where: { gated.contains($0["name"] as? String ?? "") && $0["over_budget"] as? Bool == true }) { exit(1) }
    }
}
