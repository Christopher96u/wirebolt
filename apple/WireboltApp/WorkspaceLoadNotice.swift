import AppKit
import SwiftUI

struct WorkspaceLoadNotice: View {
    let message: String
    let dismiss: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Label(message, systemImage: "exclamationmark.triangle")
                .font(.callout)
                .lineLimit(4)
                .help(message)
            Spacer(minLength: 0)
            Button("Dismiss", systemImage: "xmark", action: dismiss)
                .labelStyle(.iconOnly)
                .buttonStyle(.plain)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.regularMaterial)
    }
}

struct EnvironmentEditNotice: View {
    let message: String
    let dismiss: () -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(message).font(.callout).textSelection(.enabled).lineLimit(4).help(message)
            HStack {
                Button("Copy Discarded Edits") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(message, forType: .string)
                }
                Spacer()
                Button("Dismiss", action: dismiss)
            }
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading).background(.regularMaterial)
    }
}
