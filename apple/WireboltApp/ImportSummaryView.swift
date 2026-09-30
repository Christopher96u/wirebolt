import SwiftUI

/// Lists what an import created and everything it could not carry over exactly.
struct ImportSummaryView: View {
    let summary: ImportSummary
    let done: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(
                summary.warnings.isEmpty ? "Import Complete" : "Imported with Warnings",
                systemImage: summary.warnings.isEmpty ? "checkmark.circle.fill" : "exclamationmark.triangle.fill"
            )
            .font(.headline)
            .symbolRenderingMode(.multicolor)
            Text(summary.headline)
            if let environmentLine = summary.environmentLine {
                Text(environmentLine)
                    .foregroundStyle(.secondary)
            }
            if !summary.warnings.isEmpty {
                Text(summary.warnings.count == 1 ? "1 item needs attention:" : "\(summary.warnings.count) items need attention:")
                    .font(.subheadline.weight(.semibold))
                    .padding(.top, 4)
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(Array(summary.warnings.enumerated()), id: \.offset) { _, warning in
                            HStack(alignment: .firstTextBaseline, spacing: 6) {
                                Text("•").accessibilityHidden(true)
                                Text(warning)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
                    .padding(10)
                }
                .frame(minHeight: 60, maxHeight: 260)
                .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 6))
            }
            HStack {
                Spacer()
                Button("Done", action: done)
                    .keyboardShortcut(.defaultAction)
            }
        }
    }
}

/// A transient confirmation for imports that need no review.
struct ImportCompleteBanner: View {
    let summary: ImportSummary
    let dismiss: () -> Void

    var body: some View {
        HStack(spacing: 7) {
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.green)
                .accessibilityHidden(true)
            Text(summary.headline)
                .lineLimit(1)
            Spacer(minLength: 4)
            Button("Dismiss", systemImage: "xmark", action: dismiss)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
                .help("Dismiss")
        }
        .font(.callout)
        .padding(.horizontal, 12)
        .frame(height: 34)
        .fixedSize(horizontal: true, vertical: false)
        .background(.regularMaterial, in: .capsule)
        .overlay { Capsule().stroke(WireboltTheme.separator, lineWidth: 0.5) }
        .shadow(color: .black.opacity(0.16), radius: 8, y: 3)
        .accessibilityElement(children: .combine)
        .task(id: summary.id) {
            // Long enough to read, short enough not to cover content for long.
            try? await Task.sleep(for: .seconds(5))
            if !Task.isCancelled { dismiss() }
        }
    }
}
