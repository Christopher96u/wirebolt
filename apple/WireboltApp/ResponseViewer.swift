import AppKit
import SwiftUI

struct ResponseViewer: View {
    @Bindable var model: WireboltModel
    @State private var tab = ResponseTab.body
    @State private var search = ""

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                if let head = model.responseHead {
                    Text("\(head.status)")
                        .font(.headline.monospacedDigit())
                        .foregroundStyle(statusColor(head.status))
                    Text(head.version).foregroundStyle(.secondary)
                    Text(formatDuration(head.timeToHeadersNS) + " TTFB")
                        .foregroundStyle(.secondary)
                    if let completion = model.completion {
                        Text(formatDuration(completion.totalTimeNS) + " total")
                            .foregroundStyle(.secondary)
                    }
                    Text(ByteCountFormatter.string(fromByteCount: Int64(model.responseBytes), countStyle: .file))
                        .foregroundStyle(.secondary)
                } else {
                    Text(model.isRunning ? "Waiting for response…" : "Response")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if tab == .body, !model.responseText.isEmpty {
                    TextField("Find", text: $search)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 180)
                        .accessibilityLabel("Find in response")
                }
                Picker("Response section", selection: $tab) {
                    ForEach(ResponseTab.allCases) { tab in Text(tab.title).tag(tab) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 170)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

            Divider()

            if let failure = model.failure {
                LightweightPlaceholder(
                    title: "Request Failed",
                    systemImage: "exclamationmark.triangle",
                    description: failureDescription(failure)
                )
            } else if model.responseHead == nil, model.responseText.isEmpty {
                LightweightPlaceholder(
                    title: "No Response Yet",
                    systemImage: "bolt.horizontal",
                    description: "Send a request to inspect its response."
                )
            } else {
                switch tab {
                case .body:
                    VStack(spacing: 0) {
                        if !search.isEmpty {
                            HStack {
                                Text("\(matchCount) matches")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                Spacer()
                            }
                            .padding(.horizontal, 12)
                            .padding(.vertical, 4)
                        }
                        ReadOnlyTextView(text: model.responseText)
                        if model.responseWasTruncated {
                            Text("Preview limited to 5 MB; the full response was still streamed.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(8)
                                .background(.bar)
                        }
                    }
                case .headers:
                    Table(model.responseHead?.headers ?? []) {
                        TableColumn("Header", value: \.name)
                        TableColumn("Value", value: \.value)
                    }
                }
            }
        }
    }

    private var matchCount: Int {
        guard !search.isEmpty else { return 0 }
        return model.responseText.components(separatedBy: search).count - 1
    }

    private func statusColor(_ status: UInt16) -> Color {
        switch status {
        case 200 ..< 300: .green
        case 300 ..< 400: .orange
        default: .red
        }
    }

    private func formatDuration(_ nanoseconds: UInt64) -> String {
        let milliseconds = Double(nanoseconds) / 1_000_000
        return milliseconds < 1
            ? String(format: "%.0f µs", Double(nanoseconds) / 1_000)
            : String(format: "%.1f ms", milliseconds)
    }

    private func failureDescription(_ failure: RunFailure) -> String {
        if let issue = failure.issues.first {
            return "\(issue.path): \(issue.kind.replacingOccurrences(of: "_", with: " "))"
        }
        return failure.kind.replacingOccurrences(of: "_", with: " ").capitalized
    }
}

private enum ResponseTab: CaseIterable, Identifiable {
    case body, headers
    var id: Self { self }
    var title: String { self == .body ? "Body" : "Headers" }
}

private struct ReadOnlyTextView: NSViewRepresentable {
    let text: String

    func makeNSView(context _: Context) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder

        let textView = NSTextView()
        textView.isEditable = false
        textView.isSelectable = true
        textView.isRichText = false
        textView.drawsBackground = false
        textView.font = .monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        textView.textContainerInset = NSSize(width: 10, height: 10)
        textView.isHorizontallyResizable = true
        textView.autoresizingMask = [.width]
        textView.textContainer?.widthTracksTextView = false
        textView.textContainer?.containerSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        scrollView.documentView = textView
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context _: Context) {
        guard let textView = scrollView.documentView as? NSTextView,
              textView.string != text
        else { return }
        textView.string = text
    }
}
