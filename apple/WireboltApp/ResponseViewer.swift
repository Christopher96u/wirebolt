import AppKit
import SwiftUI

struct ResponseViewer: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        Group {
            if let failure = model.failure {
                LightweightPlaceholder(
                    title: "Request Failed",
                    systemImage: "exclamationmark.triangle",
                    description: failureDescription(failure)
                )
            } else if hasResponse {
                VStack(spacing: 0) {
                    ResponseSectionBar(model: model, interface: interface)
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
                text: displayedResponseText,
                wasTruncated: model.responseWasTruncated
            )
        case .headers:
            ResponseHeadersTable(headers: displayedHeaders)
        case .cookies:
            ResponseCookiesTable()
        case .raw:
            SyntaxTextView(text: displayedResponseText, language: .plain)
        case .request:
            SentRequestViewer(request: model.draft)
        case .timeline:
            ResponseTimelineView(model: model)
        }
    }

    private var displayedResponseText: String {
        model.responseText.isEmpty
            ? interface.demoResponseText(for: interface.activeTabID)
            : model.responseText
    }

    private var displayedHeaders: [ResponseHeader] {
        guard let headers = model.responseHead?.headers, !headers.isEmpty else {
            return DemoResponseData.headers
        }
        return headers
    }

    private var hasResponse: Bool {
        model.responseHead != nil
            || !model.responseText.isEmpty
            || interface.activeTab?.status != nil
    }

    private func failureDescription(_ failure: RunFailure) -> String {
        if let issue = failure.issues.first {
            return "\(issue.path): \(issue.kind.replacingOccurrences(of: "_", with: " "))"
        }
        return failure.kind.replacingOccurrences(of: "_", with: " ").capitalized
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
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

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
                            badge: badge(for: section),
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
            ResponseTransferMetrics(model: model, interface: interface)
                .padding(.trailing, 10)
        }
        .frame(height: 34)
        .background(WireboltTheme.barBackground)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Response sections")
    }

    private func badge(for section: ResponsePanelSection) -> Int? {
        switch section {
        case .headers:
            let count = model.responseHead?.headers.count ?? DemoResponseData.headers.count
            return count
        case .cookies: return nil
        case .body, .raw, .request, .timeline: return nil
        }
    }
}

private struct ResponseTransferMetrics: View {
    @Bindable var model: WireboltModel
    @Bindable var interface: WorkspaceUIState

    var body: some View {
        HStack(spacing: 10) {
            if model.isRunning {
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
        guard let completion = model.completion else {
            return switch interface.activeTabID {
            case "get-request": "1 s 90 ms"
            case "my-first-api": "421 ms"
            default: "271 ms"
            }
        }
        let milliseconds = Double(completion.totalTimeNS) / 1_000_000
        return milliseconds < 1
            ? String(format: "%.0f µs", Double(completion.totalTimeNS) / 1_000)
            : String(format: "%.0f ms", milliseconds)
    }

    private var sizeLabel: String {
        let bytes = model.responseText.isEmpty
            ? interface.demoResponseText(for: interface.activeTabID).utf8.count
            : model.responseText.utf8.count
        return String(format: "%.3f KB", Double(bytes) / 1_000)
    }
}

private struct ResponseBodyViewer: View {
    @Bindable var interface: WorkspaceUIState
    let text: String
    let wasTruncated: Bool

    @State private var search = ""
    @State private var isSearching = false

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
                    Text("\(matchCount) matches")
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
                    Button("Save Response…") {}
                        .disabled(true)
                    Button("Copy as cURL") {}
                        .disabled(true)
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
                Text("Preview limited to 5 MB; the full response was still streamed.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
                    .background(.bar)
            }
        }
    }

    @ViewBuilder
    private var rendererContent: some View {
        switch interface.responseRenderer {
        case .json:
            SyntaxTextView(text: text, language: .json)
        case .tree:
            JSONTreeView(text: text)
        case .image:
            LightweightPlaceholder(
                title: "Image Preview",
                systemImage: "photo",
                description: "This response is not an image."
            )
        case .xml:
            SyntaxTextView(text: text, language: .xml)
        case .html:
            SyntaxTextView(text: text, language: .html)
        case .raw:
            SyntaxTextView(text: text, language: .plain)
        }
    }

    private var matchCount: Int {
        guard !search.isEmpty else { return 0 }
        return text.components(separatedBy: search).count - 1
    }
}

private struct ResponseHeadersTable: View {
    let headers: [ResponseHeader]

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Header List")
                    .foregroundStyle(.secondary)
                Spacer()
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
                    ForEach(headers) { header in
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
}

private struct ResponseCookiesTable: View {
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
                Button("Copy Cookies", systemImage: "doc.on.doc") {}
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .disabled(true)
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
                TableColumn("Expires", value: \.expires)
                TableColumn("Secure") { cookie in
                    Image(systemName: cookie.isSecure ? "checkmark.circle.fill" : "circle")
                        .foregroundStyle(cookie.isSecure ? .green : .secondary)
                        .accessibilityLabel(cookie.isSecure ? "Secure" : "Not secure")
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

    private var visibleCookies: [DemoCookie] {
        guard !search.isEmpty else { return DemoResponseData.cookies }
        return DemoResponseData.cookies.filter {
            $0.name.localizedCaseInsensitiveContains(search)
                || $0.domain.localizedCaseInsensitiveContains(search)
        }
    }
}

private struct SentRequestViewer: View {
    let request: RequestDraft

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
        let url = URL(string: request.url)
        let path = url?.path.isEmpty == false ? url?.path ?? "/" : "/"
        let host = url?.host ?? "api.example.com"
        var lines = [
            "\(request.method.rawValue) \(path) HTTP/1.1",
            "Host: \(host)",
        ]
        lines.append(contentsOf: request.headers.filter(\.enabled).map {
            "\($0.name): \($0.value.editableValue)"
        })
        if request.authentication != .none {
            lines.append("Authorization: Bearer ••••••••••••")
        }
        if let bodyText {
            lines.append("Content-Length: \(bodyText.utf8.count)")
            lines.append("")
            lines.append(bodyText)
        }
        return lines.joined(separator: "\n")
    }

    private var headerText: String {
        sentRequestText
            .components(separatedBy: "\n\n")
            .first ?? sentRequestText
    }

    private var bodyText: String? {
        switch request.body {
        case let .json(value), let .text(_, value): value
        case .empty: nil
        case let .formURLEncoded(fields):
            fields.filter(\.enabled).map {
                "\($0.name)=\($0.value.editableValue)"
            }.joined(separator: "&")
        }
    }
}

private enum SentRequestMode: String, CaseIterable, Identifiable {
    case raw = "Raw"
    case headers = "Headers"

    var id: Self { self }
}

private struct ResponseTimelineView: View {
    @Bindable var model: WireboltModel

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Network Timeline")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Spacer()
                Text(model.responseHead?.version ?? "HTTP/2")
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 12)
            .frame(height: 43)
            .background(WireboltTheme.barBackground)
            Divider()

            VStack(spacing: 0) {
                ForEach(DemoResponseData.timeline) { event in
                    HStack(spacing: 10) {
                        Image(systemName: event.symbol)
                            .foregroundStyle(event.color)
                            .frame(width: 18)
                        Text(event.name)
                        Spacer()
                        Text(event.duration)
                            .font(.body.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 14)
                    .frame(height: 38)
                    Divider()
                }
            }
            Spacer()
        }
    }
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
            return DemoResponseData.jsonTree
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

private struct DemoCookie: Identifiable {
    let id: String
    let name: String
    let value: String
    let domain: String
    let path: String
    let expires: String
    let isSecure: Bool
    let sameSite: String
}

private struct TimelineEvent: Identifiable {
    let id: String
    let name: String
    let duration: String
    let symbol: String
    let color: Color
}

private enum DemoResponseData {
    static let headers = [
        ResponseHeader(name: "date", value: "Thu, 23 May 2024 02:20:04 GMT"),
        ResponseHeader(name: "content-type", value: "application/json; charset=utf-8"),
        ResponseHeader(name: "content-length", value: "302"),
        ResponseHeader(name: "etag", value: "W/\"12e-nFb0V670Pbf0r8Iu48b1L2w\""),
        ResponseHeader(
            name: "set-cookie",
            value: "sails.sid=s%3Ap-o2XfjNkkN1C1e0QGopn1h9X7.Y0gc15YhUBRrPDOMisdXrB4Crzw; Path=/; HttpOnly"
        ),
    ]

    static let cookies = [
        DemoCookie(
            id: "session_id",
            name: "session_id",
            value: "eyJhbGciOiJIUzI1NiJ9",
            domain: ".api.example.com",
            path: "/",
            expires: "Session",
            isSecure: true,
            sameSite: "Lax"
        ),
        DemoCookie(
            id: "csrf_token",
            name: "csrf_token",
            value: "csrf_8f31a29c",
            domain: "api.example.com",
            path: "/v1",
            expires: "01 Oct 2026",
            isSecure: true,
            sameSite: "Strict"
        ),
    ]

    static let timeline = [
        TimelineEvent(id: "dns", name: "DNS Lookup", duration: "8 ms", symbol: "network", color: .blue),
        TimelineEvent(id: "connect", name: "Connection", duration: "21 ms", symbol: "cable.connector", color: .purple),
        TimelineEvent(id: "tls", name: "TLS Handshake", duration: "34 ms", symbol: "lock", color: .green),
        TimelineEvent(id: "upload", name: "Request Upload", duration: "3 ms", symbol: "arrow.up", color: .orange),
        TimelineEvent(id: "ttfb", name: "Time to First Byte", duration: "102 ms", symbol: "hourglass", color: .blue),
        TimelineEvent(id: "download", name: "Response Download", duration: "16 ms", symbol: "arrow.down", color: .green),
    ]

    static let jsonTree = [
        JSONNode(
            id: "root",
            key: "Root",
            type: "Object",
            value: "3 items",
            children: [
                JSONNode(id: "root.id", key: "id", type: "String", value: "usr_8f31", children: nil),
                JSONNode(id: "root.active", key: "active", type: "Boolean", value: "true", children: nil),
                JSONNode(id: "root.total", key: "total", type: "Number", value: "42", children: nil),
            ]
        ),
    ]
}
