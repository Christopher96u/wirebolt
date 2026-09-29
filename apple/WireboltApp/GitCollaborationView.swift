import SwiftUI

struct GitCollaborationView: View {
    @Bindable var model: WireboltModel
    @Environment(\.dismiss) private var dismiss
    @State private var commitMessage = ""

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            changes
            Divider()
            actions
        }
        .frame(minWidth: 620, idealWidth: 680, minHeight: 460, idealHeight: 540)
        .task {
            await model.refreshGitStatus()
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Git Collaboration")
                        .font(.title2.bold())
                    Text("Explicit local-first collaboration. Wirebolt never syncs in the background.")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Close") { dismiss() }.keyboardShortcut(.cancelAction)
                if model.isGitBusy {
                    ProgressView()
                        .controlSize(.small)
                        .accessibilityLabel("Git operation in progress")
                }
            }

            if let status = model.gitStatus {
                HStack(spacing: 14) {
                    Label(status.branch ?? "Detached HEAD", systemImage: "arrow.triangle.branch")
                    if let upstream = status.upstream {
                        Text(upstream).foregroundStyle(.secondary)
                    } else {
                        Text("No upstream configured").foregroundStyle(.secondary)
                            .help("Publish pushes this branch to origin and tracks it as the upstream.")
                    }
                    if status.ahead > 0 {
                        Text("↑ \(status.ahead)").help("Commits ahead of upstream")
                    }
                    if status.behind > 0 {
                        Text("↓ \(status.behind)").help("Commits behind upstream")
                    }
                }
                .font(.callout.monospaced())
                .accessibilityElement(children: .combine)
            }

            if let failure = model.gitFailure {
                Label(failure.reason, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(WireboltTheme.danger)
                    .textSelection(.enabled)
                    .accessibilityLabel("Git error: \(failure.reason)")
            } else if let operation = model.gitOperation {
                Label(operationLabel(operation.outcome), systemImage: operationIcon(operation.outcome))
                    .foregroundStyle(operation.outcome == .conflicted ? WireboltTheme.danger : .secondary)
            }

            if model.hasUnsavedRequestChanges {
                Label("Save or discard request edits before pulling.", systemImage: "pencil.circle")
                    .foregroundStyle(WireboltTheme.warning)
            }
        }
        .padding(18)
    }

    @ViewBuilder
    private var changes: some View {
        if let status = model.gitStatus, !status.changes.isEmpty {
            List(status.changes) { change in
                HStack(spacing: WireboltTheme.Spacing.medium) {
                    Label(change.kind.rawValue, systemImage: symbol(for: change.kind))
                        .font(.caption.weight(.medium))
                        .foregroundStyle(color(for: change.kind))
                        .frame(width: 104, alignment: .leading)
                        .help(porcelainHelp(change))
                    VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xxSmall) {
                        Text(change.path).lineLimit(1)
                        if let previousPath = change.previousPath {
                            Text("from \(previousPath)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }
                    Spacer()
                }
                .accessibilityElement(children: .combine)
            }
        } else if model.gitStatus != nil {
            ContentUnavailableView(
                "Working Tree Clean",
                systemImage: "checkmark.circle",
                description: Text("There are no local Git changes.")
            )
        } else {
            ContentUnavailableView(
                "Git Status Unavailable",
                systemImage: "arrow.triangle.branch",
                description: Text("Refresh to inspect this workspace.")
            )
        }
    }

    private var actions: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Only saved Wirebolt workspace documents are included in commits.")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack(spacing: 8) {
                TextField("Commit message", text: $commitMessage)
                    .textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Git commit message")
                    .onSubmit(commit)

                Button("Commit", action: commit)
                    .disabled(model.isGitBusy || model.gitStatus == nil || trimmedCommitMessage.isEmpty)

                Button("Pull") {
                    Task { await model.pullGit() }
                }
                .disabled(model.isGitBusy || model.gitStatus?.upstream == nil || model.hasUnsavedRequestChanges)
                .help("Pull the configured upstream. Conflicts are never resolved automatically.")

                pushButton

                Button("Refresh", systemImage: "arrow.clockwise") {
                    Task { await model.refreshGitStatus() }
                }
                .labelStyle(.iconOnly)
                .disabled(model.isGitBusy)
                .help("Refresh Git status without fetching")
            }
        }
        .padding(18)
    }

    @ViewBuilder
    private var pushButton: some View {
        let availability = model.gitStatus?.pushAvailability ?? .unavailable(reason: "Refresh Git status first.")
        let title = if case .publish = availability { "Publish" } else { "Push" }
        Button(title) {
            Task { await model.pushGit() }
        }
        .disabled(model.isGitBusy || { if case .unavailable = availability { true } else { false } }())
        .help(pushHelp(availability))
    }

    private func pushHelp(_ availability: GitPushAvailability) -> String {
        switch availability {
        case .push: "Push committed workspace documents to the upstream."
        case let .publish(branch): "No upstream configured. Push \(branch) to origin and track it as the upstream."
        case let .unavailable(reason): reason
        }
    }

    private var trimmedCommitMessage: String {
        commitMessage.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func commit() {
        let message = trimmedCommitMessage
        guard !message.isEmpty else { return }
        Task {
            await model.commitGit(message: message)
            if model.gitOperation?.outcome == .committed {
                commitMessage = ""
            }
        }
    }

    private func symbol(for kind: GitChangeKind) -> String {
        switch kind {
        case .new: "plus.circle.fill"
        case .modified: "pencil.circle.fill"
        case .deleted: "minus.circle.fill"
        case .renamed: "arrow.right.circle.fill"
        case .copied: "doc.on.doc.fill"
        case .typeChanged: "arrow.triangle.2.circlepath.circle.fill"
        case .conflicted: "exclamationmark.triangle.fill"
        }
    }

    private func color(for kind: GitChangeKind) -> Color {
        switch kind {
        case .new: WireboltTheme.success
        case .deleted, .conflicted: WireboltTheme.danger
        case .modified, .renamed, .copied, .typeChanged: .secondary
        }
    }

    /// The Git porcelain code stays available for people who know it.
    private func porcelainHelp(_ change: GitChangeSnapshot) -> String {
        let code = change.conflicted ? "UU" : shortLabel(change.staged) + shortLabel(change.unstaged)
        return "Git status: \(code.replacingOccurrences(of: " ", with: "·"))"
    }

    private func shortLabel(_ delta: GitDeltaSnapshot) -> String {
        switch delta {
        case .none: " "
        case .added: "A"
        case .modified: "M"
        case .deleted: "D"
        case .renamed: "R"
        case .copied: "C"
        case .typeChanged: "T"
        case .untracked: "?"
        case .unmerged: "U"
        }
    }

    private func operationLabel(_ outcome: GitOperationOutcome) -> String {
        switch outcome {
        case .nothingToCommit: "Nothing to commit"
        case .committed: "Workspace documents committed"
        case .updated: "Workspace updated from upstream"
        case .upToDate: "Already up to date"
        case .pushed: "Current branch pushed"
        case .conflicted: "Pull stopped with conflicts"
        }
    }

    private func operationIcon(_ outcome: GitOperationOutcome) -> String {
        outcome == .conflicted ? "exclamationmark.triangle.fill" : "checkmark.circle"
    }
}
