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
    @Environment(\.wireboltReduceMotion) private var reduceMotion

    // The viewer stays mounted across tab switches so the common empty state is updated
    // rather than rebuilt; everything that holds per-document state (the response views and
    // the run progress) is identified by the document instead.
    var body: some View {
        Group {
            if hasPresentation {
                VStack(spacing: 0) {
                    ResponseSectionBar(interface: interface, session: session, cancel: cancel)
                    sectionContent
                        // Fill the pane so short content (failure, empty states) keeps the section
                        // bar pinned to the top instead of centering the whole stack.
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        // Keep the previous response (scroll, find, folding) until the new head arrives.
                        .opacity(isAwaitingNewResponse ? 0.35 : 1)
                        .allowsHitTesting(!isAwaitingNewResponse)
                        .accessibilityHidden(isAwaitingNewResponse)
                        // An overlay rather than a ZStack: the response content alone decides the
                        // size, so laying out a large body is not measured twice.
                        .overlay {
                            if isAwaitingNewResponse, let startedAt = session.runStartedAt {
                                RunProgressView(startedAt: startedAt, cancel: cancel)
                                    .padding(.horizontal, 22).padding(.vertical, 16)
                                    .background(.regularMaterial, in: .rect(cornerRadius: WireboltTheme.Radius.group))
                                    .overlay { RoundedRectangle(cornerRadius: WireboltTheme.Radius.group).stroke(WireboltTheme.separator, lineWidth: 0.5) }
                                    .transition(.opacity)
                            }
                        }
                }
                .id(session.id)
            } else if session.isRunning, let startedAt = session.runStartedAt {
                RunProgressView(startedAt: startedAt, cancel: cancel)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .id(session.id)
                    .transition(.opacity)
            } else {
                NoResponseState(session: session, canSend: send != nil) { send?() }
            }
        }
        // Phase changes of one document animate; switching documents does not.
        .transaction(value: session.id) { $0.animation = nil }
        .animation(reduceMotion ? nil : .easeOut(duration: 0.18), value: phase)
        .background(WireboltTheme.paneBackground)
        .background { ResponseRunObserver(interface: interface, session: session).id(session.id) }
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
        if let failure = session.failure, interface.responseSection != .request, failure.kind != "cancelled" || session.responseBytes == 0 {
            ResponseFailureView(
                message: RunFailureMessage(failure, host: RunFailureMessage.host(from: session.preparedRun?.url ?? session.draft.url)),
                retry: send,
                showNetworkSettings: { interface.requestSection = .settings },
                viewRequest: session.preparedRun == nil ? nil : { interface.responseSection = .request }
            )
        } else {
            VStack(spacing: 0) {
                if session.failure?.kind == "cancelled" {
                    Text("Cancelled · Partial response").font(WireboltTheme.Typography.detail)
                        .foregroundStyle(WireboltTheme.warning).padding(WireboltTheme.Spacing.medium)
                }
                responseContent
            }
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

}

/// Reacts to one document's run: picks the renderer for a new response and announces the
/// outcome. Identified by the document, so switching tabs never looks like a change.
private struct ResponseRunObserver: View {
    let interface: DocumentPresentationState
    let session: DocumentSession

    var body: some View {
        Color.clear
            .accessibilityHidden(true)
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
    var text = ""
    @State private var pausedText: String?
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: WireboltTheme.Spacing.medium) {
                ProgressView().controlSize(.mini)
                Text("Partial preview · first \(ResponseFormatting.byteCount(UInt64(DocumentSession.previewByteLimit))) maximum")
                Spacer(minLength: 0)
                Text(ResponseFormatting.byteCount(byteCount)).monospacedDigit()
                Button(pausedText == nil ? "Pause" : "Resume") {
                    if pausedText == nil { pausedText = text } else { pausedText = nil }
                }
            }
            .font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
            .padding(WireboltTheme.Spacing.medium)
            NativeCodeEditor(text: .constant(pausedText ?? text), editable: false, language: .plain, label: "Partial streaming response")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
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
                ReceivingBodyView(byteCount: byteCount, text: text)
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
    @Environment(\.wireboltTheme) private var theme
    @State private var showsDetails = false
    @State private var actionError: String?
    @AppStorage("response.decodesUnicodeEscapes") private var decodesUnicodeEscapes = true

    var body: some View {
        VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xSmall) {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: WireboltTheme.Spacing.large) {
                    summary
                    Spacer(minLength: 0)
                    tabs
                    actions
                }
                VStack(alignment: .leading, spacing: WireboltTheme.Spacing.small) {
                    summary
                    HStack(spacing: WireboltTheme.Spacing.small) { tabs; Spacer(minLength: 0); actions }
                }
            }
            if showsDetails {
                VStack(alignment: .leading, spacing: WireboltTheme.Spacing.small) {
                    Text(session.responseHead?.version ?? "Waiting for response")
                    if let snapshot = session.preparedRun {
                        Text(snapshot.url).lineLimit(2).textSelection(.enabled)
                    }
                    Text("\(session.responseHead?.headers.count ?? 0) headers · \(session.responseCookies.count) cookies")
                }
                .font(WireboltTheme.Typography.detail).foregroundStyle(WireboltTheme.muted)
                .padding(.vertical, WireboltTheme.Spacing.medium)
            }
        }
        .padding(.horizontal, WireboltTheme.Spacing.large)
        .padding(.vertical, WireboltTheme.Spacing.small)
        .background(WireboltTheme.paneBackground)
        .overlay(alignment: .top) { theme.hairline.color.frame(height: 1) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Response sections")
        .alert("Response Action Failed", isPresented: Binding(get: { actionError != nil }, set: { if !$0 { actionError = nil } })) {
            Button("OK", role: .cancel) {}
        } message: { Text(actionError ?? "The operation could not be completed.") }
    }

    private var summary: some View {
        HStack(spacing: WireboltTheme.Spacing.medium) {
            Button { showsDetails.toggle() } label: {
                HStack(spacing: WireboltTheme.Spacing.medium) {
                    if let head = session.responseHead {
                        Text(ResponseFormatting.statusLine(head.status)).foregroundStyle(WireboltTheme.statusColor(head.status))
                    }
                    Text("·").foregroundStyle(WireboltTheme.dim)
                    if session.isRunning, let start = session.runStartedAt { ElapsedTimeText(startedAt: start) }
                    else { Text(session.completion.map { ResponseFormatting.duration(nanoseconds: $0.totalTimeNS) } ?? "—") }
                    Text("·").foregroundStyle(WireboltTheme.dim)
                    Text(ResponseFormatting.byteCount(session.responseBytes))
                    Image(systemName: showsDetails ? "chevron.down" : "chevron.right")
                }
                .font(WireboltTheme.Typography.tab).foregroundStyle(WireboltTheme.muted)
                .frame(minHeight: theme.density.controlHeight)
                .fixedSize()
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Response summary")
            .accessibilityValue(ResponseFormatting.completionSummary(status: session.responseHead?.status,
                totalTimeNS: session.completion?.totalTimeNS, bytes: session.responseBytes))
            .accessibilityHint(showsDetails ? "Collapse details" : "Expand details")
            if session.isRunning, let cancel {
                Button("Cancel Request", systemImage: "xmark", action: cancel)
                    .labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
            }
        }
    }

    private var tabs: some View {
        ScrollView(.horizontal) {
        HStack(spacing: WireboltTheme.Spacing.xSmall) {
            ForEach([ResponsePanelSection.body, .headers, .cookies, .raw, .request]) { section in
                PanelTabButton(title: section.rawValue,
                    badge: section == .headers ? session.responseHead?.headers.count : nil,
                    isSelected: interface.responseSection == section, height: theme.density.controlHeight,
                    action: { interface.responseSection = section })
            }
        }
        }.scrollIndicators(.never).frame(height: theme.density.controlHeight)
    }

    private var rendererSelection: Binding<ResponseRenderer> {
        Binding(get: { interface.responseRenderer }, set: { interface.responseRenderer = $0; interface.usesAutomaticRenderer = false })
    }

    private var actions: some View {
        HStack(spacing: WireboltTheme.Spacing.xSmall) {
            // Choosing JSON, Tree or Text is a primary action, so the renderer stays
            // visible like the request body's format menu rather than in the overflow menu.
            Menu {
                Picker("Renderer", selection: rendererSelection) {
                    ForEach(ResponseRenderer.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.inline).labelsHidden()
            } label: {
                Text(interface.responseRenderer.rawValue)
                    .font(WireboltTheme.Typography.tab).foregroundStyle(WireboltTheme.muted)
            }
            .menuStyle(.borderlessButton).fixedSize()
            .frame(minHeight: 28).help("Response Renderer")
            .accessibilityLabel("Response renderer")
            .accessibilityValue(interface.responseRenderer.rawValue)
            Button("Find in Body", systemImage: "magnifyingglass", action: findBody)
                .labelStyle(.iconOnly).buttonStyle(GhostIconStyle()).disabled(session.isRunning)
            Button("Copy Body", systemImage: "doc.on.doc", action: copyBody)
                .labelStyle(.iconOnly).buttonStyle(GhostIconStyle()).disabled(session.bodyStore == nil || session.isRunning)
        Menu("Response Actions", systemImage: "ellipsis") {
            Button("Find in Body", systemImage: "magnifyingglass", action: findBody).disabled(session.isRunning)
            Button("Copy Body", systemImage: "doc.on.doc", action: copyBody).disabled(session.bodyStore == nil || session.isRunning)
            Button("Export Body…", systemImage: "square.and.arrow.up", action: saveResponse)
                .disabled(session.bodyStore == nil || session.isRunning)
            Toggle("Decode Unicode Escapes", isOn: $decodesUnicodeEscapes).disabled(interface.responseRenderer != .json)
            EditorPreferencesMenu()
            Menu("Open With") { OpenWithMenuItems(store: session.bodyStore) }.disabled(session.isRunning)
        }
        .menuStyle(.borderlessButton).menuIndicator(.hidden).labelStyle(.iconOnly)
        .frame(minWidth: 28, minHeight: 28).help("Response Actions")
        }
    }

    private func findBody() {
        if ![ResponseRenderer.json, .xml, .html, .raw].contains(interface.responseRenderer) { interface.responseRenderer = .raw }
        interface.responseSection = .body
        interface.responseFindTrigger += 1
    }

    private func copyBody() {
        guard let store = session.bodyStore else { return }
        let length = Int(clamping: session.responseBytes)
        Task {
            do {
                let data = try await store.viewport(length: length)
                let text = await Task.detached(priority: .userInitiated) { String(decoding: data, as: UTF8.self) }.value
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(text, forType: .string)
            } catch { actionError = error.localizedDescription }
        }
    }

    private func saveResponse() {
        guard let store = session.bodyStore else { return }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "response.body"
        panel.canCreateDirectories = true
        Task {
            guard await present(panel, in: NSApp.keyWindow) == .OK, let destination = panel.url else { return }
            do { try await store.export(to: destination) } catch { actionError = error.localizedDescription }
        }
    }
}

/// The empty response pane. Its own view: it reads the URL, so typing re-renders only this
/// rather than the whole response viewer.
private struct NoResponseState: View {
    let session: DocumentSession
    let canSend: Bool
    let send: () -> Void

    var body: some View {
        let hasURL = !session.draft.url.isEmpty
        ContentUnavailableView {
            Label("No Response", systemImage: "paperplane")
        } description: {
            Text(hasURL ? "Send the request (⌘↩) to see the response here."
                : "Enter a URL, then send the request (⌘↩) to see the response here.")
        } actions: {
            if canSend {
                Button("Send Request", action: send)
                    .disabled(!hasURL)
                    .help("Send Request (⌘↩)")
            }
        }
    }
}

/// The applications that open the body. Their own view, so the Launch Services lookup runs
/// when the body file changes rather than every time the response bar updates (for
/// example on each renderer switch).
private struct OpenWithMenuItems: View {
    let store: ResponseBodyStore?

    var body: some View {
        if let store {
            ForEach(NSWorkspace.shared.urlsForApplications(toOpen: store.url), id: \.self) { application in
                Button(application.deletingPathExtension().lastPathComponent) {
                    NSWorkspace.shared.open([store.url], withApplicationAt: application,
                        configuration: NSWorkspace.OpenConfiguration())
                }
            }
        }
        Button("Default Application") {
            if let store { NSWorkspace.shared.open(store.url) }
        }
        .disabled(store == nil)
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
    @State private var loadedViewportData: Data?
    @State private var storeSize: UInt64 = 0
    @State private var jsonDocument: JSONResponseDocument?
    /// The body file and escape mode `jsonDocument` was formatted for.
    @State private var formattedSource: String?
    @AppStorage("response.decodesUnicodeEscapes") private var decodesUnicodeEscapes = true
    private var formattingKey: String? { store.map { "\($0.url.path)#\(decodesUnicodeEscapes)" } }

    var body: some View {
        VStack(spacing: 0) {
            if isReceiving {
                ReceivingBodyView(byteCount: receivedBytes, text: text)
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
        // Consume the request so remounting this viewer (tab or section switch) does not
        // reopen Find on its own.
        .onChange(of: interface.responseFindTrigger, initial: true) {
            guard interface.responseFindTrigger > 0 else { return }
            find.isVisible = true
            interface.responseFindTrigger = 0
        }

    }

    private var renderedData: Data { loadedViewportData ?? previewData }
    private var renderedText: String { String(decoding: renderedData, as: UTF8.self) }

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
                // Decoded once per update rather than for each use.
                let text = renderedText
                Group {
                    if let store, ResponseTextPresentation.usesIndex(byteCount: max(storeSize, receivedBytes), preview: text) {
                        IndexedResponseEditor(url: store.url, preview: text, language: language, search: "", find: displayedOtherFind)
                    } else {
                        SyntaxTextView(text: text, language: language, find: displayedOtherFind, storageKey: "Response body \(language)")
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

struct JSONTreeView: NSViewRepresentable {
    @Environment(\.wireboltTheme) private var theme
    @AppStorage("editor.fontSize") private var fontSize = 12.0
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
        outline.intercellSpacing = .zero
        outline.indentationPerLevel = 16
        outline.backgroundColor = theme.definition.window.nsColor
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
        scroll.backgroundColor = theme.definition.window.nsColor
        updateNSView(scroll, context: context)
        return scroll
    }
    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let outline = scroll.documentView as? NSOutlineView else { return }
        let coordinator = context.coordinator
        let contentChanged = coordinator.revision != revision
        let styleChanged = coordinator.themeRevision != theme.revision || coordinator.fontSize != fontSize
        guard contentChanged || styleChanged else { return }
        coordinator.theme = theme
        coordinator.themeRevision = theme.revision
        coordinator.fontSize = fontSize
        outline.backgroundColor = theme.definition.window.nsColor
        scroll.backgroundColor = theme.definition.window.nsColor
        outline.rowHeight = max(theme.density.controlHeight, ceil(fontSize * 1.5) + 1)
        if contentChanged {
            coordinator.revision = revision
            coordinator.roots = nodes.map(Node.init)
            outline.reloadData()
            for root in coordinator.roots { outline.expandItem(root) }
        } else {
            // Refresh cell styles without rebuilding nodes or losing expansion/selection.
            outline.reloadData(forRowIndexes: IndexSet(integersIn: 0..<outline.numberOfRows),
                columnIndexes: IndexSet(integersIn: 0..<outline.numberOfColumns))
        }
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
        var theme = ResolvedTheme(preset: .black)
        var themeRevision: UInt64?
        var fontSize = 12.0
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
            field.font = .monospacedSystemFont(ofSize: fontSize, weight: .regular)
            field.lineBreakMode = .byTruncatingTail
            field.maximumNumberOfLines = 1
            field.stringValue = column.identifier.rawValue == "key" ? node.value.key : node.value.value
            if node.value.key == "Root" { field.textColor = theme.muted.nsColor }
            else if column.identifier.rawValue == "key" { field.textColor = theme.definition.syntaxKey.nsColor }
            else {
                field.textColor = switch node.value.type {
                case "String": theme.definition.syntaxString.nsColor
                case "Number": theme.definition.syntaxNumber.nsColor
                case "Boolean": theme.definition.syntaxKeyword.nsColor
                case "Null": theme.definition.syntaxKeyword.nsColor
                default: theme.muted.nsColor
                }
            }
            return field
        }
    }
}
