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
                    } else { Text("No upstream configured").foregroundStyle(.secondary) }
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
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
                    .accessibilityLabel("Git error: \(failure.reason)")
            } else if let operation = model.gitOperation {
                Label(operationLabel(operation.outcome), systemImage: operationIcon(operation.outcome))
                    .foregroundStyle(operation.outcome == .conflicted ? .red : .secondary)
            }

            if model.hasUnsavedRequestChanges {
                Label("Save or discard request edits before pulling.", systemImage: "pencil.circle")
                    .foregroundStyle(.orange)
            }
        }
        .padding(18)
    }

    @ViewBuilder
    private var changes: some View {
        if let status = model.gitStatus, !status.changes.isEmpty {
            List(status.changes) { change in
                HStack(spacing: 10) {
                    Text(changeLabel(change))
                        .font(.caption.monospaced().bold())
                        .foregroundStyle(change.conflicted ? .red : .secondary)
                        .frame(width: 28, alignment: .leading)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(change.path).lineLimit(1)
                        if let previousPath = change.previousPath {
                            Text("from \(previousPath)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }
                    Spacer()
                    if change.conflicted {
                        Text("Conflict")
                            .font(.caption.bold())
                            .foregroundStyle(.red)
                    }
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

                Button("Push") {
                    Task { await model.pushGit() }
                }
                .disabled(model.isGitBusy || model.gitStatus == nil)

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

    private func changeLabel(_ change: GitChangeSnapshot) -> String {
        if change.conflicted { return "UU" }
        return shortLabel(change.staged) + shortLabel(change.unstaged)
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
