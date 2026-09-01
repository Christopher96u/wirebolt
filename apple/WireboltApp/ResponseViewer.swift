import AppKit
import SwiftUI
import WebKit

struct ResponseViewer: View {
    @Bindable var interface: WorkspaceUIState
    @Bindable var session: DocumentSession

    var body: some View {
        Group {
            if let failure = session.failure {
                LightweightPlaceholder(
                    title: "Request Failed",
                    systemImage: "exclamationmark.triangle",
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
            RawResponseViewer(text: rawResponseText, bodyStore: session.bodyStore)
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
        guard let head = session.responseHead else { return session.responseText }
        let statusLine = "\(head.version) \(head.status)"
        let headers = head.headers.map { "\($0.name): \($0.value)" }.joined(separator: "\n")
        return [statusLine, headers, "", session.responseText].joined(separator: "\n")
    }

    private func failureDescription(_ failure: RunFailure) -> String {
        if let issue = failure.issues.first {
            return "\(issue.path): \(issue.kind.replacingOccurrences(of: "_", with: " "))"
        }
        return failure.kind.replacingOccurrences(of: "_", with: " ").capitalized
    }
}

private struct RawResponseViewer: View {
    let text: String
    let bodyStore: ResponseBodyStore?

    @State private var search = ""

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("Find in Raw Response", text: $search)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 220)
                if search.isEmpty == false {
                    Text("\(text.components(separatedBy: search).count - 1) matches")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Copy Raw Response", systemImage: "doc.on.doc", action: copy)
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                Button("Show Body in Finder", systemImage: "folder", action: showInFinder)
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .disabled(bodyStore == nil)
            }
            .padding(.horizontal, 12)
            .frame(height: 43)
            .background(WireboltTheme.barBackground)
            Divider()
            SyntaxTextView(text: text, language: .http)
        }
    }

    private func copy() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    private func showInFinder() {
        guard let bodyStore else { return }
        NSWorkspace.shared.activateFileViewerSelecting([bodyStore.url])
    }
}

private struct NoResponsePlaceholder: View {
    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: "paperplane")
                .font(.system(size: 49, weight: .light))
            Text("No Response")
                .font(.title2.weight(.semibold))
        }
        .foregroundStyle(.tertiary)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
    }
}

private struct ResponseSectionBar: View {
    @Bindable var interface: WorkspaceUIState
    @Bindable var session: DocumentSession

    var body: some View {
        HStack(spacing: 0) {
            ScrollView(.horizontal) {
                HStack(spacing: 22) {
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
                        .offset(y: -1)
                    }
                }
                .padding(.leading, 11)
                .padding(.trailing, 10)
            }
            .scrollIndicators(.hidden)

            Spacer(minLength: 12)
            ResponseTransferMetrics(session: session)
                .padding(.trailing, 10)
        }
        .frame(height: 34)
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
        .font(.callout.monospacedDigit())
        .foregroundStyle(.secondary)
        .fixedSize()
        .accessibilityElement(children: .combine)
    }

    private var durationLabel: String {
        guard let completion = session.completion else { return "—" }
        let milliseconds = Double(completion.totalTimeNS) / 1_000_000
        return milliseconds < 1
            ? String(format: "%.0f µs", Double(completion.totalTimeNS) / 1_000)
            : String(format: "%.0f ms", milliseconds)
    }

    private var sizeLabel: String {
        String(format: "%.3f KB", Double(session.responseBytes) / 1_000)
    }
}

private struct ResponseBodyViewer: View {
    @Bindable var interface: WorkspaceUIState
    let text: String
    let previewData: Data
    let receivedBytes: UInt64
    let wasTruncated: Bool
    let store: ResponseBodyStore?
    let snapshot: PreparedRunSnapshot?

    @State private var search = ""
    @State private var isSearching = false
    @State private var fullMatchCount = 0
    @State private var searchTask: Task<Void, Never>?
    @State private var actionError: String?
    @State private var loadedViewportData: Data?
    @State private var viewportOffset: UInt64 = 0
    @State private var storeSize: UInt64 = 0

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text("Body")
                    .foregroundStyle(.secondary)

                NativeRendererPicker(selection: $interface.responseRenderer)
                    .frame(width: 122, height: 22)
                .help("Response Renderer")

                Spacer(minLength: 8)

                if isSearching {
                    TextField("Find", text: $search)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 150)
                    Text("\(fullMatchCount) matches")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Button("Find", systemImage: "magnifyingglass") {
                    isSearching.toggle()
                    if !isSearching { search = "" }
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .help("Find in Response (⌘F)")

                Menu("Response Actions", systemImage: "ellipsis.circle") {
                    Button("Save Response…", action: saveResponse)
                        .disabled(store == nil)
                    Button("Open Response", action: openResponse)
                        .disabled(store == nil)
                    Button("Show in Finder", action: showInFinder)
                        .disabled(store == nil)
                    Divider()
                    Button("Copy as cURL", action: copyAsCurl)
                        .disabled(snapshot == nil)
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .labelStyle(.iconOnly)
                .foregroundStyle(.secondary)
                .fixedSize()
            }
            .padding(.leading, 11)
            .padding(.trailing, 12)
            .frame(height: 27)
            .background(WireboltTheme.barBackground)
            Divider()

            rendererContent

            if wasTruncated {
                HStack {
                    Button("Previous Viewport", systemImage: "chevron.left") {
                        loadViewport(offset: viewportOffset.saturatingSubtract(UInt64(ResponseBodyStore.viewportByteCount)))
                    }
                    .labelStyle(.iconOnly)
                    .disabled(viewportOffset == 0)
                    Text("Bytes \(viewportOffset.formatted())–\(min(viewportOffset + UInt64(renderedData.count), storeSize).formatted()) of \(storeSize.formatted())")
                    Button("Next Viewport", systemImage: "chevron.right") {
                        loadViewport(offset: viewportOffset + UInt64(ResponseBodyStore.viewportByteCount))
                    }
                    .labelStyle(.iconOnly)
                    .disabled(viewportOffset + UInt64(renderedData.count) >= storeSize)
                    Spacer()
                    Text("32 KB viewport · full response remains file-backed")
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(8)
                .background(.bar)
            }
        }
        .onChange(of: search) { _, query in
            searchTask?.cancel()
            guard query.isEmpty == false, let store else {
                fullMatchCount = 0
                return
            }
            searchTask = Task {
                let count = (try? await store.countOccurrences(of: query)) ?? 0
                guard Task.isCancelled == false else { return }
                fullMatchCount = count
            }
        }
        .onDisappear { searchTask?.cancel() }
        .task(id: store?.url) {
            storeSize = await store?.size() ?? UInt64(previewData.count)
            loadedViewportData = nil
            viewportOffset = 0
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

    private func loadViewport(offset: UInt64) {
        guard let store else { return }
        Task {
            let size = await store.size()
            let bounded = min(offset, size.saturatingSubtract(1))
            guard let data = try? await store.viewport(offset: bounded) else { return }
            storeSize = size
            viewportOffset = bounded
            loadedViewportData = data
        }
    }

    private func copyAsCurl() {
        guard let snapshot else { return }
        var parts = ["curl", "-X", shellQuote(snapshot.method), shellQuote(snapshot.url)]
        for header in snapshot.headers {
            parts += ["-H", shellQuote("\(header.name): \(header.value)")]
        }
        if let body = snapshot.body.textPreview {
            parts += ["--data-raw", shellQuote(body)]
        } else if snapshot.body.byteCount > 0 {
            parts += ["--data-binary", snapshot.body.redacted ? "'[redacted]'" : "'@response-body'" ]
        }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(parts.joined(separator: " "), forType: .string)
    }

    private func shellQuote(_ value: String) -> String {
        "'\(value.replacingOccurrences(of: "'", with: "'\\''"))'"
    }

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
            SyntaxTextView(text: renderedText, language: .json)
        case .tree:
            JSONTreeView(text: renderedText)
        case .image:
            ResponseImageView(data: renderedData)
        case .xml:
            SyntaxTextView(text: renderedText, language: .xml)
        case .html:
            SyntaxTextView(text: renderedText, language: .html)
        case .webView:
            IsolatedWebPreview(html: renderedText)
        case .raw:
            SyntaxTextView(text: renderedText, language: .plain)
        case .hex:
            SyntaxTextView(text: hexText, language: .plain)
        }
    }

    private var hexText: String {
        renderedData.enumerated().reduce(into: "") { output, item in
            let (offset, byte) = item
            if offset.isMultiple(of: 16) {
                if offset > 0 { output.append("\n") }
                output.append(String(format: "%08x  ", offset))
            }
            output.append(String(format: "%02x ", byte))
        }
    }
}

private extension UInt64 {
    func saturatingSubtract(_ amount: UInt64) -> UInt64 {
        self > amount ? self - amount : 0
    }
}

private struct ResponseImageView: View {
    let data: Data

    var body: some View {
        if let image = NSImage(data: data) {
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
                description: "The first response viewport is not a supported image."
            )
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

private struct ResponseHeadersTable: View {
    let headers: [ResponseHeader]

    @State private var search = ""

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("Search Headers", text: $search)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 220)
                Spacer()
                Button("Copy Headers", systemImage: "doc.on.doc", action: copyHeaders)
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .disabled(headers.isEmpty)
                Button("Export Headers…", systemImage: "square.and.arrow.down", action: exportHeaders)
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .disabled(headers.isEmpty)
            }
            .padding(.horizontal, 12)
            .frame(height: 43)
            .background(WireboltTheme.barBackground)
            Divider()

            HStack(spacing: 0) {
                Text("Key")
                    .frame(minWidth: 150, maxWidth: .infinity, alignment: .leading)
                    .padding(.leading, 12)
                Divider()
                Text("Value")
                    .frame(minWidth: 180, maxWidth: .infinity, alignment: .leading)
                    .padding(.leading, 12)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .frame(height: 30)
            Divider()

            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(filteredHeaders) { header in
                        HStack(alignment: .top, spacing: 0) {
                            Text(header.name)
                                .frame(minWidth: 150, maxWidth: .infinity, alignment: .leading)
                                .padding(.leading, 12)
                                .textSelection(.enabled)
                            Text(header.value)
                                .frame(minWidth: 180, maxWidth: .infinity, alignment: .leading)
                                .padding(.leading, 12)
                                .fixedSize(horizontal: false, vertical: true)
                                .textSelection(.enabled)
                        }
                        .font(.body.monospaced())
                        .padding(.vertical, 7)
                        .frame(minHeight: 32, alignment: .top)
                    }
                }
            }
        }
    }

    private var filteredHeaders: [ResponseHeader] {
        guard search.isEmpty == false else { return headers }
        return headers.filter {
            $0.name.localizedCaseInsensitiveContains(search)
                || $0.value.localizedCaseInsensitiveContains(search)
        }
    }

    private var headerText: String {
        headers.map { "\($0.name): \($0.value)" }.joined(separator: "\n")
    }

    private func copyHeaders() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(headerText, forType: .string)
    }

    private func exportHeaders() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "response-headers.txt"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do { try Data(headerText.utf8).write(to: url, options: .atomic) }
        catch { NSSound.beep() }
    }
}

private struct ResponseCookiesTable: View {
    let cookies: [CookieSnapshot]

    @State private var revealValues = false
    @State private var search = ""

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                TextField("Search Cookies", text: $search)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 220)
                Spacer()
                Toggle("Reveal Values", systemImage: revealValues ? "eye.slash" : "eye", isOn: $revealValues)
                    .toggleStyle(.button)
                    .labelStyle(.iconOnly)
                    .help(revealValues ? "Hide Sensitive Values" : "Reveal Sensitive Values")
                Button("Copy Cookies", systemImage: "doc.on.doc", action: copyCookies)
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .disabled(cookies.isEmpty)
            }
            .padding(.horizontal, 12)
            .frame(height: 43)
            .background(WireboltTheme.barBackground)
            Divider()

            Table(visibleCookies) {
                TableColumn("Name", value: \.name)
                    .width(min: 100, ideal: 125)
                TableColumn("Value") { cookie in
                    Text(revealValues ? cookie.value : String(repeating: "•", count: 12))
                        .font(.body.monospaced())
                }
                TableColumn("Domain", value: \.domain)
                TableColumn("Path", value: \.path)
                    .width(50)
                TableColumn("Expires") { cookie in
                    Text(cookie.expiresAt?.formatted() ?? "Session")
                }
                TableColumn("Secure") { cookie in
                    Image(systemName: cookie.secure ? "checkmark.circle.fill" : "circle")
                        .foregroundStyle(cookie.secure ? .green : .secondary)
                        .accessibilityLabel(cookie.secure ? "Secure" : "Not secure")
                }
                .width(58)
                TableColumn("SameSite", value: \.sameSite)
            }
            .tableStyle(.bordered(alternatesRowBackgrounds: true))

            HStack(spacing: 6) {
                Image(systemName: "eye.slash")
                Text(revealValues ? "Sensitive values visible" : "Sensitive values hidden")
                Spacer()
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .frame(height: 27)
            .background(WireboltTheme.barBackground)
            .overlay(alignment: .top) { Divider() }
        }
    }

    private var visibleCookies: [CookieSnapshot] {
        guard !search.isEmpty else { return cookies }
        return cookies.filter {
            $0.name.localizedCaseInsensitiveContains(search)
                || $0.domain.localizedCaseInsensitiveContains(search)
        }
    }

    private func copyCookies() {
        let value = cookies.map { "\($0.name)=\($0.value)" }.joined(separator: "; ")
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(value, forType: .string)
    }
}

private struct SentRequestViewer: View {
    let snapshot: PreparedRunSnapshot?

    @State private var mode = SentRequestMode.raw

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Sent Request")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Spacer()
                Picker("Request View", selection: $mode) {
                    ForEach(SentRequestMode.allCases) { mode in
                        Text(mode.rawValue).tag(mode)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 135)
                Button("Copy Request", systemImage: "doc.on.doc") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(sentRequestText, forType: .string)
                }
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 12)
            .frame(height: 43)
            .background(WireboltTheme.barBackground)
            Divider()

            SyntaxTextView(
                text: mode == .raw ? sentRequestText : headerText,
                language: mode == .raw ? .http : .plain
            )

            HStack(spacing: 6) {
                Image(systemName: "lock")
                Text("Secrets redacted")
                Spacer()
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .frame(height: 27)
            .background(WireboltTheme.barBackground)
            .overlay(alignment: .top) { Divider() }
        }
    }

    private var sentRequestText: String {
        guard let snapshot else { return "No request has been sent from this tab." }
        let url = URL(string: snapshot.url)
        let path = url?.path.isEmpty == false ? url?.path ?? "/" : "/"
        let host = url?.host ?? "[redacted]"
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

private struct JSONTreeView: View {
    let text: String

    private let nodes: [JSONNode]

    init(text: String) {
        self.text = text
        nodes = JSONNode.makeRoot(from: text)
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text("Key")
                    .frame(minWidth: 145, maxWidth: .infinity, alignment: .leading)
                Divider()
                Text("Value")
                    .frame(minWidth: 180, maxWidth: .infinity, alignment: .leading)
                    .padding(.leading, 16)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10)
            .frame(height: 30)
            .background(WireboltTheme.barBackground)
            Divider()

            List {
                ForEach(nodes) { node in
                    JSONNodeTreeRow(node: node)
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(WireboltTheme.paneBackground)
        }
    }
}

private struct JSONNodeTreeRow: View {
    let node: JSONNode

    @State private var isExpanded: Bool

    init(node: JSONNode) {
        self.node = node
        _isExpanded = State(initialValue: node.key != "args")
    }

    var body: some View {
        if let children = node.children, !children.isEmpty {
            DisclosureGroup(isExpanded: $isExpanded) {
                ForEach(children) { child in
                    JSONNodeTreeRow(node: child)
                }
            } label: {
                JSONNodeRow(node: node)
            }
        } else {
            JSONNodeRow(node: node)
        }
    }
}

private struct JSONNodeRow: View {
    let node: JSONNode

    var body: some View {
        HStack(spacing: 0) {
            Text(node.key)
                .font(.body.monospaced())
                .foregroundStyle(node.key == "Root" ? Color.secondary : WireboltTheme.treeKey)
                .frame(minWidth: 145, maxWidth: .infinity, alignment: .leading)
            Text(node.value)
                .font(.body.monospaced())
                .foregroundStyle(valueColor)
                .lineLimit(1)
                .frame(minWidth: 110, maxWidth: .infinity, alignment: .leading)
        }
        .frame(height: 28)
        .accessibilityElement(children: .combine)
    }

    private var valueColor: Color {
        switch node.type {
        case "String", "Number": WireboltTheme.treeValue
        case "Boolean": WireboltTheme.jsonBoolean
        case "Null": WireboltTheme.jsonNull
        default: .secondary
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
        button.font = .systemFont(ofSize: NSFont.systemFontSize)
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

private struct JSONNode: Identifiable {
    let id: String
    let key: String
    let type: String
    let value: String
    let children: [JSONNode]?

    static func makeRoot(from text: String) -> [JSONNode] {
        guard let data = text.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data)
        else {
            return []
        }
        return [make(key: "Root", value: object, path: "root")]
    }

    private static func make(key: String, value: Any, path: String) -> JSONNode {
        if let dictionary = value as? [String: Any] {
            let children = dictionary.keys.sorted(by: preferredJSONKeyOrder).map {
                make(key: $0, value: dictionary[$0]!, path: "\(path).\($0)")
            }
            return JSONNode(
                id: path,
                key: key,
                type: "Object",
                value: "Object (\(children.count) items)",
                children: children
            )
        }
        if let array = value as? [Any] {
            let children = array.enumerated().map {
                make(key: "\($0.offset)", value: $0.element, path: "\(path)[\($0.offset)]")
            }
            return JSONNode(
                id: path,
                key: key,
                type: "Array",
                value: "Array (\(children.count) items)",
                children: children
            )
        }
        if value is NSNull {
            return JSONNode(id: path, key: key, type: "Null", value: "null", children: nil)
        }
        if let boolean = value as? Bool {
            return JSONNode(id: path, key: key, type: "Boolean", value: boolean ? "true" : "false", children: nil)
        }
        if let number = value as? NSNumber {
            return JSONNode(id: path, key: key, type: "Number", value: number.stringValue, children: nil)
        }
        return JSONNode(id: path, key: key, type: "String", value: String(describing: value), children: nil)
    }

    private static func preferredJSONKeyOrder(_ lhs: String, _ rhs: String) -> Bool {
        let preferred = [
            "headers",
            "args",
            "url",
            "x-amzn-trace-id",
            "x-forwarded-port",
            "x-forwarded-proto",
            "host",
        ]
        let lhsIndex = preferred.firstIndex(of: lhs) ?? preferred.endIndex
        let rhsIndex = preferred.firstIndex(of: rhs) ?? preferred.endIndex
        return lhsIndex == rhsIndex ? lhs < rhs : lhsIndex < rhsIndex
    }
}

enum SyntaxLanguage: Equatable {
    case json
    case xml
    case html
    case http
    case plain
}

struct SyntaxTextView: View {
    let text: String
    let language: SyntaxLanguage

    var body: some View {
        ScrollView([.horizontal, .vertical]) {
            HStack(alignment: .top, spacing: 0) {
                Text(lineNumbers)
                    .font(.system(.body, design: .monospaced))
                    .monospacedDigit()
                    .foregroundStyle(.tertiary)
                    .multilineTextAlignment(.trailing)
                    .lineSpacing(3)
                    .frame(width: 38, alignment: .trailing)
                    .padding(.trailing, 8)
                    .accessibilityHidden(true)

                Divider()

                if language == .json {
                    Text(foldingMarkers)
                        .font(.system(.body, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .lineSpacing(3)
                        .frame(width: 10, alignment: .trailing)
                        .accessibilityHidden(true)
                }

                Text(highlightedText)
                    .font(.system(.body, design: .monospaced))
                    .foregroundStyle(.primary)
                    .lineSpacing(3)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: true, vertical: false)
                    .padding(.leading, 10)
                    .background(alignment: .topLeading) {
                        if language == .json {
                            JSONIndentGuides(text: text)
                                .allowsHitTesting(false)
                        }
                    }
                    .accessibilityLabel(accessibilityLabel)
            }
            .padding(.vertical, 0)
            .padding(.trailing, 12)
        }
        .background(WireboltTheme.paneBackground)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .defaultScrollAnchor(.topLeading)
    }

    private var lineNumbers: String {
        let count = max(text.components(separatedBy: .newlines).count, 1)
        return (1 ... count).map(String.init).joined(separator: "\n")
    }

    private var foldingMarkers: String {
        text.components(separatedBy: .newlines).map { line in
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            return (trimmed.hasSuffix("{") || trimmed.hasSuffix("[")) ? "⌄" : ""
        }.joined(separator: "\n")
    }

    private var accessibilityLabel: String {
        switch language {
        case .json: "JSON response"
        case .xml: "XML response"
        case .html: "HTML response"
        case .http: "HTTP request"
        case .plain: "Raw response"
        }
    }

    private var highlightedText: AttributedString {
        let highlighted = SyntaxHighlighter.attributedString(text: text, language: language)
        return (try? AttributedString(highlighted, including: \.appKit)) ?? AttributedString(text)
    }
}

private struct JSONIndentGuides: View {
    let text: String

    var body: some View {
        Canvas { context, size in
            let lines = text.components(separatedBy: .newlines)
            let lineHeight = size.height / CGFloat(max(lines.count, 1))

            for (lineIndex, line) in lines.enumerated() {
                let leadingSpaces = line.prefix { $0 == " " }.count
                let indentation = leadingSpaces / 2
                guard indentation > 0 else { continue }

                for level in 0 ..< indentation {
                    let x = CGFloat(level) * 15.65 + 4
                    var path = Path()
                    path.move(to: CGPoint(x: x, y: CGFloat(lineIndex) * lineHeight))
                    path.addLine(to: CGPoint(x: x, y: CGFloat(lineIndex + 1) * lineHeight))
                    context.stroke(
                        path,
                        with: .color(WireboltTheme.separator.opacity(0.7)),
                        lineWidth: 0.5
                    )
                }
            }
        }
    }
}

@MainActor
private enum SyntaxHighlighter {
    static func attributedString(text: String, language: SyntaxLanguage) -> NSAttributedString {
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = 3
        let result = NSMutableAttributedString(
            string: text,
            attributes: [
                .font: NSFont.monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular),
                .foregroundColor: NSColor.labelColor,
                .paragraphStyle: paragraph,
            ]
        )

        switch language {
        case .json:
            apply(#"\"(?:\\.|[^\"\\])*\""#, color: WireboltTheme.nsJSONString, to: result)
            apply(#"\"(?:\\.|[^\"\\])*\"(?=\s*:)"#, color: WireboltTheme.nsJSONKey, to: result)
            apply(#"\b(true|false)\b"#, color: WireboltTheme.nsJSONBoolean, to: result)
            apply(#"\bnull\b"#, color: WireboltTheme.nsJSONNull, to: result)
            apply(#"-?\b\d+(?:\.\d+)?\b"#, color: WireboltTheme.nsJSONNumber, to: result)
            applyURL(to: result)
        case .xml, .html:
            apply(#"</?[A-Za-z][^>]*>"#, color: .systemTeal, to: result)
            apply(#"\"[^\"]*\""#, color: .systemOrange, to: result)
        case .http:
            apply(#"(?m)^[A-Z]+\s+\S+\s+HTTP/\d(?:\.\d)?$"#, color: .systemGreen, to: result)
            apply(#"(?m)^[A-Za-z0-9-]+(?=:)"#, color: .systemTeal, to: result)
            apply(#"•+"#, color: .systemOrange, to: result)
        case .plain:
            break
        }
        return result
    }

    private static func apply(
        _ pattern: String,
        color: NSColor,
        to text: NSMutableAttributedString
    ) {
        guard let expression = try? NSRegularExpression(pattern: pattern) else { return }
        let range = NSRange(location: 0, length: text.length)
        expression.enumerateMatches(in: text.string, range: range) { match, _, _ in
            guard let match else { return }
            text.addAttribute(.foregroundColor, value: color, range: match.range)
        }
    }

    private static func applyURL(to text: NSMutableAttributedString) {
        guard let expression = try? NSRegularExpression(pattern: #"https?://[^\"\s]+"#) else { return }
        let range = NSRange(location: 0, length: text.length)
        expression.enumerateMatches(in: text.string, range: range) { match, _, _ in
            guard let match else { return }
            text.addAttributes([
                .underlineStyle: NSUnderlineStyle.single.rawValue,
                .link: text.attributedSubstring(from: match.range).string,
            ], range: match.range)
        }
    }
}
