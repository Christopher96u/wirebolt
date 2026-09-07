import AppKit
import SwiftUI
import WebKit

struct ResponseViewer: View {
    @Bindable var interface: DocumentPresentationState
    @Bindable var session: DocumentSession

    var body: some View {
        Group {
            if session.isRunning {
                VStack(spacing: 9) {
                    ProgressView().controlSize(.small)
                    Text("Sending…").font(.system(size: 12)).foregroundStyle(.secondary)
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let failure = session.failure {
                LightweightPlaceholder(
                    title: failure.kind == "cancelled" ? "Cancelled" : "",
                    systemImage: "exclamationmark.circle",
                    description: failureDescription(failure)
                )
            } else if hasResponse {
                VStack(spacing: 0) {
                    ResponseSectionBar(interface: interface, session: session)
                    responseContent
                }
            } else {
                NoResponsePlaceholder()
            }
        }
        .background(WireboltTheme.paneBackground)
        .onChange(of: session.responseHead) {
            guard interface.usesAutomaticRenderer, let head = session.responseHead else { return }
            let mime = head.headers.first { $0.name.lowercased() == "content-type" }?.value.lowercased() ?? ""
            if mime.contains("json") { interface.responseRenderer = .json }
            else if mime.contains("html") { interface.responseRenderer = .html }
            else if mime.contains("xml") { interface.responseRenderer = .xml }
            else if mime.hasPrefix("image/") { interface.responseRenderer = .image }
            else { interface.responseRenderer = .raw }
        }
    }

    @ViewBuilder
    private var responseContent: some View {
        switch interface.responseSection {
        case .body:
            ResponseBodyViewer(
                interface: interface,
                text: session.responseText,
                previewData: session.responsePreviewData,
                receivedBytes: session.responseBytes,
                wasTruncated: session.responseWasTruncated,
                store: session.bodyStore,
                snapshot: session.preparedRun
            )
        case .headers:
            ResponseHeadersTable(headers: session.responseHead?.headers ?? [])
        case .cookies:
            ResponseCookiesTable(cookies: session.responseCookies)
        case .raw:
            ResponseSourceView(title: "Raw Response", text: rawResponseText, bodyStore: session.bodyStore,
                byteCount: session.responseBytes, prefix: rawResponseHeaders)
        case .request:
            SentRequestViewer(snapshot: session.preparedRun)
        }
    }

    private var hasResponse: Bool {
        session.isRunning
            || session.responseHead != nil
            || session.responseText.isEmpty == false
            || session.completion != nil
    }

    private var rawResponseText: String {
        rawResponseHeaders + session.responseText
    }

    private var rawResponseHeaders: String {
        guard let head = session.responseHead else { return "" }
        let statusLine = "\(head.version) \(head.status)"
        let headers = head.headers.map { "\($0.name): \($0.value)" }.joined(separator: "\n")
        return [statusLine, headers, "", ""].joined(separator: "\n")
    }

    private func failureDescription(_ failure: RunFailure) -> String {
        if let issue = failure.issues.first {
            return "\(issue.path): \(issue.kind.replacingOccurrences(of: "_", with: " "))"
        }
        if failure.kind == "connection" {
            return "An error occurred while connecting to the server, please re-check your URL or connection and try again."
        }
        return failure.kind.replacingOccurrences(of: "_", with: " ").capitalized
    }
}

private struct ResponseSourceView: View {
    let title: String
    let text: String
    var bodyStore: ResponseBodyStore?
    var byteCount: UInt64 = 0
    var prefix = ""
    @State private var find = EditorFindState()
    @State private var loadedText: String?
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text(title).foregroundStyle(.secondary)
                Spacer(minLength: 4)
                Button("Find", systemImage: "magnifyingglass") { find.isVisible = true }
                    .labelStyle(.iconOnly).buttonStyle(.borderless)
                Menu("Actions", systemImage: "ellipsis.circle") {
                    Button("Copy") {
                        Task {
                            let value: String
                            if let bodyStore {
                                let data = try? await bodyStore.viewport(length: Int(byteCount))
                                value = prefix + String(decoding: data ?? Data(), as: UTF8.self)
                            } else { value = text }
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(value, forType: .string)
                        }
                    }
                    Divider()
                    EditorPreferencesMenu()
                }.menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().labelStyle(.iconOnly)
            }.font(.system(size: 13)).padding(.horizontal, 12).frame(height: 27)
                .background(WireboltTheme.barBackground)
            Divider()
            if let bodyStore, byteCount > 1024 * 1024 {
                IndexedResponseEditor(url: bodyStore.url, preview: text, language: .http, search: "", prefix: prefix, find: find)
            } else {
                NativeCodeEditor(text: .constant(loadedText ?? text), editable: false, language: .http, label: title, find: find)
                    .frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
                    .editorFindOverlay(find)
            }
        }
        .task(id: bodyStore?.url) {
            guard let bodyStore, byteCount <= 1024 * 1024 else { return }
            let data = try? await bodyStore.viewport(length: Int(byteCount))
            guard !Task.isCancelled else { return }
            loadedText = prefix + String(decoding: data ?? Data(), as: UTF8.self)
        }
    }
}

struct EditorPreferencesMenu: View {
    @AppStorage("editor.wordWrap") private var wordWrap = true
    @AppStorage("editor.showInvisibles") private var invisibles = true
    @AppStorage("editor.scrollBeyondLastLine") private var scrollBeyond = true
    var body: some View {
        Menu("UI Settings", systemImage: "slider.vertical.3") {
            Toggle("Word Wrap", systemImage: "text.word.spacing", isOn: $wordWrap)
            Divider()
            Toggle("Show Invisibles Chars", systemImage: "a", isOn: $invisibles)
            Toggle("Scroll beyond Last Line", systemImage: "arrow.down.to.line", isOn: $scrollBeyond)
        }
    }
}

private struct NoResponsePlaceholder: View {
    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: "paperplane")
                .font(.system(size: 49, weight: .regular))
            Text("No Response")
                .font(.system(size: 20, weight: .regular))
        }
        .foregroundStyle(.tertiary)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
    }
}

private struct ResponseSectionBar: View {
    @Bindable var interface: DocumentPresentationState
    @Bindable var session: DocumentSession

    var body: some View {
        GeometryReader { geometry in
        HStack(spacing: 0) {
                HStack(spacing: 10) {
                    ForEach([
                        ResponsePanelSection.headers,
                        .body,
                        .cookies,
                        .raw,
                        .request,
                    ]) { section in
                        PanelTabButton(
                            title: section.rawValue,
                            badge: section == .headers ? session.responseHead?.headers.count : nil,
                            isSelected: interface.responseSection == section,
                            action: { interface.responseSection = section }
                        )
                        .offset(y: -0.5)
                        if section == .raw { Divider().frame(height: 14).padding(.horizontal, 1) }
                    }
                }
                .padding(.leading, 11)
                .padding(.trailing, 10)
                .fixedSize(horizontal: true, vertical: false)

            Spacer(minLength: 4)
            ResponseTransferMetrics(session: session)
                .padding(.trailing, 10)
        }
        .frame(minWidth: geometry.size.width, maxHeight: .infinity, alignment: .leading)
        }
        .frame(height: 34)
        .clipped()
        .background(WireboltTheme.barBackground)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Response sections")
    }
}

private struct ResponseTransferMetrics: View {
    @Bindable var session: DocumentSession

    var body: some View {
        HStack(spacing: 10) {
            if session.isRunning {
                ProgressView()
                    .controlSize(.small)
                Text("Sending…")
            } else {
                Label(durationLabel, systemImage: "clock.fill")
                Label(sizeLabel, systemImage: "arrow.down.circle.fill")
            }
        }
        .font(.system(size: 15))
        .foregroundStyle(.secondary)
        .fixedSize()
        .accessibilityElement(children: .combine)
    }

    private var durationLabel: String {
        guard let completion = session.completion else { return "—" }
        let milliseconds = completion.totalTimeNS / 1_000_000
        if milliseconds < 1000 { return "\(milliseconds) ms" }
        return "\(milliseconds / 1000) s \(milliseconds % 1000) ms"
    }

    private var sizeLabel: String {
        let kilobytes = Double(session.responseBytes) / 1024
        if kilobytes < 1024 { return kilobytes.formatted(.number.grouping(.never).precision(.significantDigits(1...3))) + " KB" }
        let megabytes = kilobytes / 1024
        if megabytes < 1024 { return megabytes.formatted(.number.grouping(.never).precision(.significantDigits(1...3))) + " MB" }
        return (megabytes / 1024).formatted(.number.grouping(.never).precision(.significantDigits(1...3))) + " GB"
    }
}

private struct ResponseBodyViewer: View {
    @Bindable var interface: DocumentPresentationState
    let text: String
    let previewData: Data
    let receivedBytes: UInt64
    let wasTruncated: Bool
    let store: ResponseBodyStore?
    let snapshot: PreparedRunSnapshot?

    @State private var find = EditorFindState()
    @State private var actionError: String?
    @State private var loadedViewportData: Data?
    @State private var storeSize: UInt64 = 0

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text("Body")
                    .foregroundStyle(.secondary)

                NativeRendererPicker(selection: Binding(
                    get: { interface.responseRenderer },
                    set: { interface.responseRenderer = $0; interface.usesAutomaticRenderer = false }
                ))
                    .controlSize(.small)
                    .frame(width: 122, height: 22)
                    .offset(y: -1)
                .help("Response Renderer")

                Spacer(minLength: 8)

                HStack(spacing: 13) {
                Button("Find", systemImage: "magnifyingglass") {
                    if [.json, .xml, .html, .raw].contains(interface.responseRenderer) { find.isVisible = true }
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .frame(width: 20)
                .help("Find in Response (⌘F)")

                Menu("Response Actions", systemImage: "ellipsis.circle") {
                    Button("Copy Body", systemImage: "doc.on.doc") {
                        Task {
                            guard let store, let data = try? await store.viewport(offset: 0, length: Int(clamping: receivedBytes)) else { return }
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(String(decoding: data, as: UTF8.self), forType: .string)
                        }
                    }
                    Divider()
                    Button("Export Body…", systemImage: "square.and.arrow.up", action: saveResponse).disabled(store == nil)
                    Divider()
                    EditorPreferencesMenu()
                    Divider()
                    Menu("Open With", systemImage: "square.and.arrow.up") {
                        if let store {
                            ForEach(NSWorkspace.shared.urlsForApplications(toOpen: store.url), id: \.self) { application in
                                Button(application.deletingPathExtension().lastPathComponent) {
                                    NSWorkspace.shared.open([store.url], withApplicationAt: application,
                                        configuration: NSWorkspace.OpenConfiguration())
                                }
                            }
                        }
                        Button("Default Application", action: openResponse).disabled(store == nil)
                    }
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .labelStyle(.iconOnly)
                .foregroundStyle(.secondary)
                .fixedSize()
                .frame(width: 20)
                }
                .offset(y: -1)
            }
            .font(.system(size: 13))
            .padding(.leading, 11)
            .padding(.trailing, 11)
            .frame(height: 27)
            .background(WireboltTheme.barBackground)
            Divider()

            if receivedBytes == 0 {
                Text("No Body").font(.system(size: 16, weight: .semibold)).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else { rendererContent }

        }
        .task(id: store?.url) {
            storeSize = await store?.size() ?? UInt64(previewData.count)
            loadedViewportData = nil
            if let store, storeSize <= 1024 * 1024 {
                loadedViewportData = try? await store.viewport(offset: 0, length: Int(storeSize))
            }
        }
        .onChange(of: receivedBytes) { _, value in storeSize = max(storeSize, value) }
        .alert("Response Action Failed", isPresented: Binding(
            get: { actionError != nil },
            set: { if !$0 { actionError = nil } }
        )) {
            Button("OK", role: .cancel) { actionError = nil }
        } message: {
            Text(actionError ?? "The operation could not be completed.")
        }
    }

    private var renderedData: Data { loadedViewportData ?? previewData }
    private var renderedText: String { String(decoding: renderedData, as: UTF8.self) }

    private func saveResponse() {
        guard let store else { return }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "response.body"
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, let destination = panel.url else { return }
        Task {
            do { try await store.export(to: destination) }
            catch { actionError = error.localizedDescription }
        }
    }

    private func openResponse() {
        guard let store else { return }
        NSWorkspace.shared.open(store.url)
    }

    private func showInFinder() {
        guard let store else { return }
        NSWorkspace.shared.activateFileViewerSelecting([store.url])
    }

    @ViewBuilder
    private var rendererContent: some View {
        switch interface.responseRenderer {
        case .json:
            textRenderer(.json)
        case .tree:
            JSONResponseTree(url: store?.url, preview: renderedData).clipped()
        case .image:
            ResponseImageView(url: store?.url, data: renderedData)
        case .xml:
            textRenderer(.xml)
        case .html:
            textRenderer(.html)
        case .webView:
            ResponseWebPreview(url: store?.url, preview: renderedText)
        case .raw:
            textRenderer(.plain)
        case .hex:
            ResponseHexView(store: store, data: renderedData, byteCount: receivedBytes)
        }
    }

    @ViewBuilder
    private func textRenderer(_ language: SyntaxLanguage) -> some View {
        if wasTruncated, storeSize > 1024 * 1024, let store {
            IndexedResponseEditor(url: store.url, preview: renderedText, language: language, search: "", find: find)
        } else { SyntaxTextView(text: renderedText, language: language, search: "", find: find) }
    }

}

private extension UInt64 {
    func saturatingSubtract(_ amount: UInt64) -> UInt64 {
        self > amount ? self - amount : 0
    }
}

private struct ResponseImageView: View {
    let url: URL?
    let data: Data

    var body: some View {
        if let image = url.flatMap({ NSImage(contentsOf: $0) }) ?? NSImage(data: data) {
            ScrollView([.horizontal, .vertical]) {
                Image(nsImage: image)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
                    .padding(20)
            }
        } else {
            LightweightPlaceholder(
                title: "Image Preview",
                systemImage: "photo",
                description: "This response is not a supported image."
            )
        }
    }
}

private struct ResponseWebPreview: View {
    let url: URL?
    let preview: String
    @State private var html: String?
    var body: some View {
        IsolatedWebPreview(html: html ?? preview)
            .task(id: url) {
                guard let url else { return }
                let reader = Task.detached(priority: .userInitiated) { try String(contentsOf: url, encoding: .utf8) }
                let result = try? await withTaskCancellationHandler { try await reader.value } onCancel: { reader.cancel() }
                guard !Task.isCancelled else { return }
                html = result
            }
    }
}

private struct IsolatedWebPreview: NSViewRepresentable {
    let html: String

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context _: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = false
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.underPageBackgroundColor = .clear
        return view
    }

    func updateNSView(_ view: WKWebView, context: Context) {
        let fingerprint = html.hashValue
        guard context.coordinator.fingerprint != fingerprint else { return }
        context.coordinator.fingerprint = fingerprint
        view.loadHTMLString(html, baseURL: nil)
    }

    final class Coordinator {
        var fingerprint: Int?
    }
}

struct ResponseHeadersTable: View {
    let headers: [ResponseHeader]
    var body: some View {
        ResponseKeyValueTable(title: "Header List", rows: headers.map { ($0.name, $0.value, false) })
    }
}

private struct ResponseCookiesTable: View {
    let cookies: [CookieSnapshot]
    var body: some View {
        ResponseKeyValueTable(title: "Cookies", rows: cookies.flatMap { cookie in
            var rows = [(cookie.name, cookie.value, true), ("    Path", cookie.path, false)]
            if cookie.httpOnly { rows.append(("    HttpOnly", "True", false)) }
            if cookie.secure { rows.append(("    Secure", "True", false)) }
            if let expires = cookie.expiresAt { rows.append(("    Expires", expires.formatted(), false)) }
            if !cookie.sameSite.isEmpty { rows.append(("    SameSite", cookie.sameSite, false)) }
            return rows
        })
    }
}

private struct ResponseKeyValueTable: View {
    let title: String
    let rows: [(String, String, Bool)]
    @State private var search = ""
    @State private var isSearching = false
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text(title).foregroundStyle(.secondary)
                Spacer(minLength: 4)
                if isSearching { TextField("Find", text: $search).frame(width: 140) }
                Button("Find", systemImage: "magnifyingglass") { isSearching.toggle() }
                    .labelStyle(.iconOnly).buttonStyle(.borderless)
                Button("Copy", systemImage: "doc.on.doc") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(rows.map { "\($0.0): \($0.1)" }.joined(separator: "\n"), forType: .string)
                }.labelStyle(.iconOnly).buttonStyle(.borderless)
            }.font(.system(size: 13)).padding(.horizontal, 12).frame(height: 27)
                .background(WireboltTheme.barBackground)
            Divider()
            HStack(spacing: 0) {
                Text("Key").frame(width: 178, alignment: .leading).padding(.leading, 10)
                Divider().frame(height: 16)
                Text("Value").padding(.leading, 6)
                Spacer()
            }.font(.system(size: 11)).frame(height: 27)
            Divider()
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                        if search.isEmpty || row.0.localizedCaseInsensitiveContains(search) || row.1.localizedCaseInsensitiveContains(search) {
                            HStack(alignment: .top, spacing: 0) {
                                Text(row.0).frame(width: 178, alignment: .leading).padding(.leading, 10)
                                Text(row.1).frame(maxWidth: .infinity, alignment: .leading).padding(.leading, 6)
                            }.font(.system(size: 12, weight: row.2 ? .bold : .regular, design: .monospaced))
                                .textSelection(.enabled).padding(.vertical, 6).frame(minHeight: 28)
                        }
                    }
                }
            }
        }
    }
}

private struct SentRequestViewer: View {
    let snapshot: PreparedRunSnapshot?

    var body: some View { ResponseSourceView(title: "Raw Request", text: sentRequestText) }

    private var sentRequestText: String {
        guard let snapshot else { return "No request has been sent from this tab." }
        let url = URL(string: snapshot.url)
        let components = url.flatMap { URLComponents(url: $0, resolvingAgainstBaseURL: false) }
        let path = (components?.percentEncodedPath.isEmpty == false ? components?.percentEncodedPath ?? "/" : "/")
            + (components?.percentEncodedQuery.map { "?" + $0 } ?? "")
        let host = (url?.host ?? "[redacted]") + (url?.port.map { ":" + String($0) } ?? "")
        var lines = [
            "\(snapshot.method) \(path) HTTP/1.1",
            "Host: \(host)",
        ]
        lines.append(contentsOf: snapshot.headers.map {
            "\($0.name): \($0.value)"
        })
        if let bodyText = snapshot.body.textPreview {
            lines.append("Content-Length: \(snapshot.body.byteCount)")
            lines.append("")
            lines.append(bodyText)
        } else if snapshot.body.byteCount > 0 {
            lines.append("Content-Length: \(snapshot.body.byteCount)")
            lines.append("")
            lines.append(snapshot.body.redacted ? "••••••••" : "[binary body]")
        }
        return lines.joined(separator: "\n")
    }

    private var headerText: String {
        sentRequestText
            .components(separatedBy: "\n\n")
            .first ?? sentRequestText
    }

}

private enum SentRequestMode: String, CaseIterable, Identifiable {
    case raw = "Raw"
    case headers = "Headers"

    var id: Self { self }
}

private struct JSONResponseTree: View {
    let url: URL?
    let preview: Data
    @State private var nodes: [JSONNode] = []
    @State private var revision = 0
    var body: some View {
        JSONTreeView(nodes: nodes, revision: revision)
            .task(id: url) {
                let url = url, preview = preview
                let parse = Task.detached(priority: .userInitiated) {
                    let data = try url.map { try Data(contentsOf: $0, options: .mappedIfSafe) } ?? preview
                    try Task.checkCancellation()
                    return JSONNode.makeRoot(from: data)
                }
                let result = try? await withTaskCancellationHandler { try await parse.value } onCancel: { parse.cancel() }
                guard !Task.isCancelled else { return }
                nodes = result ?? []
                revision += 1
            }
    }
}

private struct JSONTreeView: NSViewRepresentable {
    let nodes: [JSONNode]
    let revision: Int

    func makeCoordinator() -> Coordinator { Coordinator() }
    func sizeThatFits(_ proposal: ProposedViewSize, nsView: NSScrollView, context: Context) -> CGSize? {
        CGSize(width: proposal.width ?? 300, height: proposal.height ?? 200)
    }
    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        let outline = NSOutlineView()
        outline.style = .plain
        outline.rowSizeStyle = .custom
        outline.rowHeight = 19
        outline.intercellSpacing = .zero
        outline.indentationPerLevel = 16
        outline.backgroundColor = .textBackgroundColor
        outline.headerView = NSTableHeaderView()
        let key = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("key"))
        key.title = "Key"
        key.width = 207
        key.minWidth = 80
        let value = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("value"))
        value.title = "Value"
        value.minWidth = 80
        outline.addTableColumn(key)
        outline.addTableColumn(value)
        outline.outlineTableColumn = key
        outline.columnAutoresizingStyle = .lastColumnOnlyAutoresizingStyle
        outline.dataSource = context.coordinator
        outline.delegate = context.coordinator
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = .textBackgroundColor
        updateNSView(scroll, context: context)
        return scroll
    }
    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let outline = scroll.documentView as? NSOutlineView, context.coordinator.revision != revision else { return }
        context.coordinator.revision = revision
        context.coordinator.roots = nodes.map(Node.init)
        outline.reloadData()
        for root in context.coordinator.roots { outline.expandItem(root) }
    }

    fileprivate final class Node: NSObject {
        let value: JSONNode
        lazy var children: [Node] = (value.children ?? []).map(Node.init)
        init(_ value: JSONNode) {
            self.value = value
        }
    }

    @MainActor
    final class Coordinator: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate {
        fileprivate var roots: [Node] = []
        var revision = -1
        func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
            (item as? Node)?.children.count ?? roots.count
        }
        func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
            ((item as? Node)?.children ?? roots)[index]
        }
        func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
            (item as? Node)?.children.isEmpty == false
        }
        func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
            guard let node = item as? Node, let column = tableColumn else { return nil }
            let field = (outlineView.makeView(withIdentifier: column.identifier, owner: nil) as? NSTextField)
                ?? NSTextField(labelWithString: "")
            field.identifier = column.identifier
            field.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
            field.lineBreakMode = .byTruncatingTail
            field.maximumNumberOfLines = 1
            field.stringValue = column.identifier.rawValue == "key" ? node.value.key : node.value.value
            if node.value.key == "Root" { field.textColor = .secondaryLabelColor }
            else if column.identifier.rawValue == "key" { field.textColor = WireboltTheme.nsJSONKey }
            else {
                field.textColor = switch node.value.type {
                case "String": WireboltTheme.nsJSONString
                case "Number": WireboltTheme.nsJSONNumber
                case "Boolean": WireboltTheme.nsJSONBoolean
                case "Null": WireboltTheme.nsJSONNull
                default: .secondaryLabelColor
                }
            }
            return field
        }
    }
}

@MainActor
private final class FixedRendererPopupButton: NSPopUpButton {
    override var intrinsicContentSize: NSSize {
        NSSize(width: 122, height: 22)
    }
}

private struct NativeRendererPicker: NSViewRepresentable {
    @Binding var selection: ResponseRenderer

    func makeCoordinator() -> Coordinator {
        Coordinator(selection: $selection)
    }

    func makeNSView(context: Context) -> FixedRendererPopupButton {
        let button = FixedRendererPopupButton(frame: .zero, pullsDown: false)
        button.controlSize = .small
        button.font = .systemFont(ofSize: 11)
        button.alignment = .center
        button.addItems(withTitles: ResponseRenderer.allCases.map(\.rawValue))
        button.target = context.coordinator
        button.action = #selector(Coordinator.changed(_:))
        button.selectItem(withTitle: selection.rawValue)
        return button
    }

    func updateNSView(_ button: FixedRendererPopupButton, context: Context) {
        context.coordinator.selection = $selection
        if button.titleOfSelectedItem != selection.rawValue {
            button.selectItem(withTitle: selection.rawValue)
        }
    }

    @MainActor
    final class Coordinator: NSObject {
        var selection: Binding<ResponseRenderer>

        init(selection: Binding<ResponseRenderer>) {
            self.selection = selection
        }

        @objc func changed(_ sender: NSPopUpButton) {
            guard let title = sender.titleOfSelectedItem,
                  let renderer = ResponseRenderer(rawValue: title)
            else { return }
            selection.wrappedValue = renderer
        }
    }
}
