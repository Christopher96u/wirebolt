import AppKit
import ImageIO
import SwiftUI
import WebKit

struct ResponseViewer: View {
    @Bindable var interface: DocumentPresentationState
    @Bindable var session: DocumentSession
    /// Re-sends this document's request. Actions that need it are hidden when nil.
    var send: (() -> Void)?
    /// Cancels this document's active run. The in-pane Cancel button is hidden when nil.
    var cancel: (() -> Void)?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Group {
            if hasPresentation {
                VStack(spacing: 0) {
                    ResponseSectionBar(interface: interface, session: session, cancel: cancel)
                    ZStack {
                        sectionContent
                            // Keep the previous response (scroll, find, folding) until the new head arrives.
                            .opacity(isAwaitingNewResponse ? 0.35 : 1)
                            .allowsHitTesting(!isAwaitingNewResponse)
                            .accessibilityHidden(isAwaitingNewResponse)
                        if isAwaitingNewResponse, let startedAt = session.runStartedAt {
                            RunProgressView(startedAt: startedAt, cancel: cancel)
                                .padding(.horizontal, 22).padding(.vertical, 16)
                                .background(.regularMaterial, in: .rect(cornerRadius: 10))
                                .overlay { RoundedRectangle(cornerRadius: 10).stroke(Color(nsColor: .separatorColor), lineWidth: 0.5) }
                                .transition(.opacity)
                        }
                    }
                }
            } else if session.isRunning, let startedAt = session.runStartedAt {
                RunProgressView(startedAt: startedAt, cancel: cancel)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .transition(.opacity)
            } else {
                ContentUnavailableView {
                    Label("No Response", systemImage: "paperplane")
                } description: {
                    Text(session.draft.url.isEmpty ? "Enter a URL, then send the request (⌘↩) to see the response here."
                        : "Send the request (⌘↩) to see the response here.")
                } actions: {
                    if let send {
                        Button("Send Request", action: send)
                            .disabled(session.draft.url.isEmpty)
                            .help("Send Request (⌘↩)")
                    }
                }
            }
        }
        .animation(reduceMotion ? nil : .easeOut(duration: 0.18), value: phase)
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
        .onChange(of: session.isRunning) { wasRunning, isRunning in
            if wasRunning, !isRunning { announceOutcome() }
        }
    }

    private enum Phase: Equatable {
        case empty, sending, awaiting, response, failure
    }

    private var phase: Phase {
        if isAwaitingNewResponse { return .awaiting }
        if session.failure != nil { return .failure }
        if hasPresentation { return .response }
        return session.isRunning ? .sending : .empty
    }

    /// A response (possibly the previous one) or a failure is on screen.
    private var hasPresentation: Bool {
        session.failure != nil
            || session.responseHead != nil
            || session.responseText.isEmpty == false
            || session.completion != nil
    }

    private var isAwaitingNewResponse: Bool {
        session.isRunning && session.isAwaitingResponseHead
    }

    /// The presented body file is still being written. Renderers read the file,
    /// so they mount (and load) only once it is complete.
    private var isReceivingBody: Bool {
        session.isRunning && !session.isAwaitingResponseHead
    }

    @ViewBuilder
    private var sectionContent: some View {
        if let failure = session.failure, interface.responseSection != .request {
            ResponseFailureView(
                message: RunFailureMessage(failure, host: RunFailureMessage.host(from: session.preparedRun?.url ?? session.draft.url)),
                retry: send,
                showNetworkSettings: { interface.requestSection = .settings },
                viewRequest: session.preparedRun == nil ? nil : { interface.responseSection = .request }
            )
        } else {
            responseContent
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
                isReceiving: isReceivingBody,
                store: session.bodyStore,
                snapshot: session.preparedRun
            )
        case .headers:
            ResponseHeadersTable(headers: session.responseHead?.headers ?? [])
        case .cookies:
            ResponseCookiesTable(cookies: session.responseCookies)
        case .raw:
            ResponseSourceView(title: "Raw Response", text: rawResponseText, bodyStore: session.bodyStore,
                byteCount: session.responseBytes, prefix: rawResponseHeaders, isReceiving: isReceivingBody)
        case .request:
            SentRequestViewer(snapshot: session.preparedRun)
        }
    }

    private var rawResponseText: String {
        rawResponseHeaders + session.responseText
    }

    private var rawResponseHeaders: String {
        guard let head = session.responseHead else { return "" }
        // HTTP/2 and later carry no reason phrase on the wire.
        let statusLine = head.version.hasPrefix("HTTP/1") ? "\(head.version) \(ResponseFormatting.statusLine(head.status))" : "\(head.version) \(head.status)"
        let headers = head.headers.map { "\($0.name): \($0.value)" }.joined(separator: "\n")
        return [statusLine, headers, "", ""].joined(separator: "\n")
    }

    private func announceOutcome() {
        let text: String
        if let failure = session.failure {
            text = failure.kind == "cancelled" ? "Request cancelled" : "Request failed. " + RunFailureMessage(failure).title
        } else if session.completion != nil || session.responseHead != nil {
            text = ResponseFormatting.completionSummary(status: session.responseHead?.status,
                totalTimeNS: session.completion?.totalTimeNS, bytes: session.responseBytes)
        } else { return }
        AccessibilityNotification.Announcement(text).post()
    }
}

/// Indeterminate progress with a live elapsed time and an optional Cancel button.
private struct RunProgressView: View {
    let startedAt: Date
    let cancel: (() -> Void)?

    var body: some View {
        VStack(spacing: 10) {
            ProgressView().controlSize(.small)
            Text("Sending Request…").font(.system(size: 13)).foregroundStyle(.secondary)
            ElapsedTimeText(startedAt: startedAt)
                .font(.system(size: 12).monospacedDigit())
                .foregroundStyle(.tertiary)
            if let cancel {
                Button("Cancel", action: cancel)
                    .controlSize(.small)
                    .help("Cancel Request (⌘.)")
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Sending request")
    }
}

private struct ElapsedTimeText: View {
    let startedAt: Date

    var body: some View {
        TimelineView(.periodic(from: startedAt, by: 0.1)) { context in
            Text(ResponseFormatting.elapsed(seconds: context.date.timeIntervalSince(startedAt)))
        }
        .accessibilityLabel("Elapsed time")
    }
}

private struct ReceivingBodyView: View {
    let byteCount: UInt64

    var body: some View {
        VStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text("Receiving Body…").font(.system(size: 13)).foregroundStyle(.secondary)
            Text(ResponseFormatting.byteCount(byteCount)).font(.system(size: 12).monospacedDigit()).foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
    }
}

private struct ResponseFailureView: View {
    let message: RunFailureMessage
    let retry: (() -> Void)?
    let showNetworkSettings: () -> Void
    let viewRequest: (() -> Void)?

    var body: some View {
        ContentUnavailableView {
            Label(message.title, systemImage: message.systemImage)
        } description: {
            Text(message.message)
        } actions: {
            HStack(spacing: 8) {
                if let retry {
                    Button(message.category == .cancelled ? "Send Again" : "Retry", action: retry)
                        .buttonStyle(.borderedProminent)
                        .help("Send the request again (⌘↩)")
                }
                if message.suggestsNetworkSettings {
                    Button("Network Settings", action: showNetworkSettings)
                        .help("Show this request’s proxy, TLS and timeout settings")
                }
                if let viewRequest {
                    Button("View Request", action: viewRequest)
                        .help("Show the request exactly as it was sent")
                }
            }
        }
    }
}

private struct ResponseSourceView: View {
    let title: String
    let text: String
    var bodyStore: ResponseBodyStore?
    var byteCount: UInt64 = 0
    var prefix = ""
    var isReceiving = false
    @State private var find = EditorFindState()
    @State private var loadedText: String?
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text(title).foregroundStyle(.secondary)
                Spacer(minLength: 4)
                Button("Find", systemImage: "magnifyingglass") { find.isVisible = true }
                    .labelStyle(.iconOnly).buttonStyle(.borderless)
                    .help("Find (⌘F)")
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
                .help("Actions")
            }.font(.system(size: 13)).padding(.horizontal, 12).frame(height: 27)
                .background(WireboltTheme.barBackground)
            Divider()
            if isReceiving {
                ReceivingBodyView(byteCount: byteCount)
            } else if let bodyStore, ResponseTextPresentation.usesIndex(byteCount: byteCount, preview: loadedText ?? text) {
                IndexedResponseEditor(url: bodyStore.url, preview: text, language: .http, search: "", prefix: prefix, find: find)
            } else {
                NativeCodeEditor(text: .constant(loadedText ?? text), editable: false, language: .http, label: title, find: find)
                    .frame(maxWidth: .infinity, maxHeight: .infinity).clipped()
                    .editorFindOverlay(find)
            }
        }
        .task(id: "\(bodyStore?.url.path ?? ""):\(isReceiving)") {
            loadedText = nil
            guard let bodyStore, !isReceiving, byteCount <= 1024 * 1024 else { return }
            let data = try? await bodyStore.viewport(length: Int(byteCount))
            guard !Task.isCancelled else { return }
            loadedText = prefix + String(decoding: data ?? Data(), as: UTF8.self)
        }
    }
}

struct EditorPreferencesMenu: View {
    @AppStorage("editor.wordWrap") private var wordWrap = true
    @AppStorage("editor.showInvisibles") private var invisibles = false
    @AppStorage("editor.scrollBeyondLastLine") private var scrollBeyond = true
    var body: some View {
        Menu("UI Settings", systemImage: "slider.vertical.3") {
            Toggle("Word Wrap", systemImage: "text.word.spacing", isOn: $wordWrap)
            Divider()
            Toggle("Show Invisible Characters", systemImage: "a", isOn: $invisibles)
            Toggle("Scroll beyond Last Line", systemImage: "arrow.down.to.line", isOn: $scrollBeyond)
        }
    }
}

private struct ResponseSectionBar: View {
    @Bindable var interface: DocumentPresentationState
    @Bindable var session: DocumentSession
    let cancel: (() -> Void)?

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 4) {
                tabs
                Spacer(minLength: 0)
                ResponseTransferMetrics(session: session, cancel: cancel).padding(.trailing, 10)
            }.frame(height: 34)
            VStack(spacing: 0) {
                tabs.frame(maxWidth: .infinity, alignment: .leading).frame(height: 34)
                ResponseTransferMetrics(session: session, cancel: cancel)
                    .padding(.horizontal, 10)
                    .frame(maxWidth: .infinity, alignment: .trailing).frame(height: 26)
            }
        }
        .background(WireboltTheme.barBackground)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Response sections")
    }

    private var tabs: some View {
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
    }
}

private struct ResponseTransferMetrics: View {
    @Bindable var session: DocumentSession
    let cancel: (() -> Void)?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 10) {
            Group {
                if session.isRunning, let startedAt = session.runStartedAt {
                    Label {
                        ElapsedTimeText(startedAt: startedAt).monospacedDigit()
                    } icon: {
                        ProgressView().controlSize(.mini)
                    }
                    if !session.isAwaitingResponseHead {
                        Label(ResponseFormatting.byteCount(session.responseBytes), systemImage: "arrow.down.circle.fill")
                            .monospacedDigit()
                    }
                } else {
                    Label(durationLabel, systemImage: "clock.fill")
                        .contentTransition(reduceMotion ? .identity : .numericText())
                        .accessibilityLabel("Time \(durationLabel)")
                    Label(ResponseFormatting.byteCount(session.responseBytes), systemImage: "arrow.down.circle.fill")
                        .contentTransition(reduceMotion ? .identity : .numericText())
                        .accessibilityLabel("Size \(ResponseFormatting.byteCount(session.responseBytes))")
                }
            }
            .accessibilityElement(children: .combine)
            if session.isRunning, !session.isAwaitingResponseHead, let cancel {
                Button("Cancel Request", systemImage: "xmark.circle.fill", action: cancel)
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
                    .help("Cancel Request (⌘.)")
            }
        }
        .font(.system(size: 15))
        .foregroundStyle(.secondary)
        .fixedSize()
        .animation(reduceMotion ? nil : .snappy(duration: 0.2), value: session.completion)
    }

    private var durationLabel: String {
        guard let completion = session.completion else { return "—" }
        return ResponseFormatting.duration(nanoseconds: completion.totalTimeNS)
    }
}

private struct ResponseBodyViewer: View {
    @Bindable var interface: DocumentPresentationState
    let text: String
    let previewData: Data
    let receivedBytes: UInt64
    let wasTruncated: Bool
    let isReceiving: Bool
    let store: ResponseBodyStore?
    let snapshot: PreparedRunSnapshot?

    @State private var jsonFind = EditorFindState()
    @State private var otherFind = EditorFindState()
    @State private var hasShownOtherText = false
    @State private var otherLanguage = SyntaxLanguage.plain
    private var find: EditorFindState { interface.responseRenderer == .json ? jsonFind : otherFind }
    @State private var actionError: String?
    @State private var loadedViewportData: Data?
    @State private var storeSize: UInt64 = 0
    @State private var jsonDocument: JSONResponseDocument?
    /// The body file and escape mode `jsonDocument` was formatted for.
    @State private var formattedSource: String?
    @AppStorage("response.decodesUnicodeEscapes") private var decodesUnicodeEscapes = true
    private var formattingKey: String? { store.map { "\($0.url.path)#\(decodesUnicodeEscapes)" } }

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
                    Toggle("Decode Unicode Escapes", systemImage: "textformat.characters", isOn: $decodesUnicodeEscapes)
                        .disabled(interface.responseRenderer != .json)
                        .help("Show \\uXXXX escapes as characters in the JSON view. Raw keeps the exact bytes.")
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
                .help("Response Actions")
                }
                .offset(y: -1)
            }
            .font(.system(size: 13))
            .padding(.leading, 11)
            .padding(.trailing, 11)
            .frame(height: 27)
            .background(WireboltTheme.barBackground)
            Divider()

            if isReceiving {
                ReceivingBodyView(byteCount: receivedBytes)
            } else if receivedBytes == 0 {
                ContentUnavailableView {
                    Label("No Body", systemImage: "doc")
                } description: {
                    Text("The server sent an empty body. Status and headers describe the response.")
                } actions: {
                    Button("View Headers") { interface.responseSection = .headers }
                }
            } else { rendererContent }

        }
        // Keyed on completion too: a viewport read while the body streams would be stale.
        .task(id: "\(store?.url.path ?? ""):\(isReceiving)") {
            loadedViewportData = nil
            guard !isReceiving else { return }
            let size = await store?.size() ?? UInt64(previewData.count)
            guard !Task.isCancelled else { return }
            storeSize = size
            if let store, size <= 64 * 1024 {
                let data = try? await store.viewport(offset: 0, length: Int(size))
                guard !Task.isCancelled else { return }
                loadedViewportData = data
            }
        }
        .onChange(of: interface.responseRenderer) { _, renderer in
            if renderer != .json {
                hasShownOtherText = true
                if renderer == .xml { otherLanguage = .xml }
                else if renderer == .html { otherLanguage = .html }
                else { otherLanguage = .plain }
            }
        }
        .onChange(of: receivedBytes) { _, value in storeSize = max(storeSize, value) }
        // An open body store cannot be formatted yet (it reports nil), so wait for completion.
        .task(id: interface.responseRenderer == .json && !isReceiving ? formattingKey : nil) {
            guard interface.responseRenderer == .json, !isReceiving, let store, let key = formattingKey else {
                return
            }
            guard formattedSource != key else { return }
            do {
                let document = try await store.formattedJSON(decodingUnicodeEscapes: decodesUnicodeEscapes)
                try Task.checkCancellation()
                jsonDocument = document
                formattedSource = key
            } catch is CancellationError {
            } catch {
                guard !Task.isCancelled else { return }
                jsonDocument = nil
                formattedSource = key
            }
        }
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
        Task {
            guard await present(panel, in: NSApp.keyWindow) == .OK, let destination = panel.url else { return }
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
        case .json, .xml, .html, .raw:
            textRenderer
        case .tree:
            JSONResponseTree(url: store?.url, preview: renderedData).clipped()
        case .image:
            ResponseImageView(url: store?.url, data: renderedData)
        case .webView:
            ResponseWebPreview(url: store?.url, preview: renderedText)
        case .hex:
            ResponseHexView(store: store, data: renderedData, byteCount: receivedBytes)
        }
    }

    @ViewBuilder
    private var textRenderer: some View {
        let isJSON = interface.responseRenderer == .json
        // While only the escape mode changes, keep showing the previous presentation of the same body.
        let sameBody = store.map { formattedSource?.hasPrefix($0.url.path + "#") == true } ?? false
        let formatted = sameBody ? jsonDocument : nil
        let pending = isJSON && store != nil && !sameBody
        let showsOther = !isJSON || (!pending && formatted == nil)
        let language: SyntaxLanguage = switch interface.responseRenderer {
        case .json: showsOther ? .json : otherLanguage
        case .xml: .xml
        case .html: .html
        default: .plain
        }
        let displayedOtherFind = showsOther && isJSON ? jsonFind : otherFind
        ZStack {
            // Both are final presentations. Keeping them mounted preserves native
            // folding, selection and scroll without rebuilding on every switch.
            if let formatted {
                Group {
                    if formatted.byteCount > 1024 * 1024 {
                        IndexedResponseEditor(url: formatted.url, preview: formatted.preview, language: .json, search: "", find: jsonFind)
                    } else {
                        SyntaxTextView(text: formatted.preview, language: .json, find: jsonFind, storageKey: "Response body json")
                    }
                }
                .environment(\.editorIsActive, isJSON)
                .opacity(isJSON ? 1 : 0)
                .allowsHitTesting(isJSON)
                .accessibilityHidden(!isJSON)
            }
            if showsOther || hasShownOtherText {
                Group {
                    if let store, ResponseTextPresentation.usesIndex(byteCount: max(storeSize, receivedBytes), preview: renderedText) {
                        IndexedResponseEditor(url: store.url, preview: renderedText, language: language, search: "", find: displayedOtherFind)
                    } else {
                        SyntaxTextView(text: renderedText, language: language, find: displayedOtherFind, storageKey: "Response body \(language)")
                            .id(language)
                    }
                }
                .environment(\.editorIsActive, showsOther)
                .opacity(showsOther ? 1 : 0)
                .allowsHitTesting(showsOther)
                .accessibilityHidden(!showsOther)
            }
            if pending { ProgressView("Formatting JSON…") }
        }
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
    @Environment(\.displayScale) private var displayScale
    @State private var image: DecodedImage?
    @State private var isLoading = true

    /// Bounds decoded memory for huge images; responses are previews, not editors.
    private nonisolated static let maximumPixelSize = 4096

    fileprivate struct DecodedImage: @unchecked Sendable {
        let image: CGImage
    }

    var body: some View {
        Group {
            if let image {
                ScrollView([.horizontal, .vertical]) {
                    Image(decorative: image.image, scale: displayScale)
                        .resizable()
                        .interpolation(.high)
                        .scaledToFit()
                        // Never upscale small images beyond their natural size.
                        .frame(maxWidth: Double(image.image.width) / displayScale, maxHeight: Double(image.image.height) / displayScale)
                        .padding(20)
                }
            } else if isLoading {
                ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ContentUnavailableView("Can’t Preview Image", systemImage: "photo",
                    description: Text("This response isn’t a supported image. Choose Raw or Hex to inspect it."))
            }
        }
        .task(id: url?.path ?? String(data.count)) {
            isLoading = true
            let url = url, data = data
            let decoder = Task.detached(priority: .userInitiated) { Self.decode(url: url, data: data) }
            let result = await withTaskCancellationHandler { await decoder.value } onCancel: { decoder.cancel() }
            guard !Task.isCancelled else { return }
            image = result
            isLoading = false
        }
    }

    private nonisolated static func decode(url: URL?, data: Data) -> DecodedImage? {
        let source = url.flatMap { CGImageSourceCreateWithURL($0 as CFURL, nil) } ?? CGImageSourceCreateWithData(data as CFData, nil)
        guard let source, CGImageSourceGetCount(source) > 0 else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: maximumPixelSize,
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary).map(DecodedImage.init)
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
                    .help(isSearching ? "Hide Find" : "Find")
                Button("Copy All", systemImage: "doc.on.doc") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(rows.map { "\($0.0): \($0.1)" }.joined(separator: "\n"), forType: .string)
                }.labelStyle(.iconOnly).buttonStyle(.borderless)
                .help("Copy All")
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

    var body: some View {
        if snapshot == nil {
            ContentUnavailableView("No Request Sent", systemImage: "paperplane",
                description: Text("Send the request (⌘↩) to see exactly what was transmitted."))
        } else {
            sentRequest
        }
    }

    private var sentRequest: some View {
        VStack(spacing: 0) {
            if let proxy = snapshot?.proxy {
                HStack {
                    Label(proxy.summary(for: snapshot?.url), systemImage: "network")
                    Spacer()
                    Text("Source: \(proxy.source.title)").foregroundStyle(.secondary)
                }.font(.caption).padding(10).background(WireboltTheme.barBackground)
                Divider()
            }
            ResponseSourceView(title: "Raw Request", text: sentRequestText)
        }
    }

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
    @State private var isParsing = true
    var body: some View {
        JSONTreeView(nodes: nodes, revision: revision)
            .opacity(nodes.isEmpty ? 0 : 1)
            .overlay {
                if isParsing {
                    ProgressView().controlSize(.small)
                } else if nodes.isEmpty {
                    ContentUnavailableView("Not JSON", systemImage: "curlybraces",
                        description: Text("The response body isn’t valid JSON. Choose Raw to read it as text."))
                }
            }
            .task(id: url) {
                isParsing = true
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
                isParsing = false
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
        CodeScroller.configure(scroll)
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
        button.setAccessibilityLabel("Response renderer")
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
