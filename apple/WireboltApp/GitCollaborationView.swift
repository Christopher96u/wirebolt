import AppKit
import SwiftUI

struct GitCollaborationView: View {
    @Bindable var model: WireboltModel
    @Environment(\.dismiss) private var dismiss
    @State private var commitMessage = ""
    @State private var isConfirmingAbort = false

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
        .confirmationDialog("Abort the merge?", isPresented: $isConfirmingAbort) {
            Button("Abort Merge", role: .destructive) { Task { await model.abortGitMerge() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Workspace files return to your last commit and the pulled changes are set aside. You can pull again later.")
        }
    }

    /// The merge a conflicted pull left behind, if any.
    private var isMerging: Bool {
        guard let status = model.gitStatus else { return false }
        return status.merging || !status.conflictedChanges.isEmpty
    }

    private var repositoryProblem: GitRepositoryProblem? {
        guard model.gitStatus == nil, let kind = model.gitFailure?.kind else { return nil }
        return GitRepositoryProblem(kind: kind)
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

            if repositoryProblem != nil {
                EmptyView()
            } else if let failure = model.gitFailure {
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
        if let problem = repositoryProblem {
            repositoryProblemView(problem)
        } else if let status = model.gitStatus, !status.changes.isEmpty {
            VStack(spacing: 0) {
                if isMerging { mergeBanner(status) }
                changeList(status)
            }
        } else if let status = model.gitStatus, status.merging {
            VStack(spacing: 0) {
                mergeBanner(status)
                Spacer()
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

    private func mergeBanner(_ status: GitStatusSnapshot) -> some View {
        let conflicts = status.conflictedChanges
        return VStack(alignment: .leading, spacing: 8) {
            Label(
                conflicts.isEmpty
                    ? "A merge is in progress."
                    : "Pull stopped with conflicts in \(conflicts.count == 1 ? "1 file" : "\(conflicts.count) files").",
                systemImage: "exclamationmark.triangle.fill"
            )
            .font(.headline)
            .foregroundStyle(WireboltTheme.danger)
            Text("Resolve the conflicts in a text editor or Git tool and finish the merge there, or abort the merge to return to your last commit. Commit, Pull and Push are unavailable until then.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                Button("Abort Merge…") { isConfirmingAbort = true }
                    .disabled(model.isGitBusy)
                    .help("Run git merge --abort and restore the workspace files.")
                Button("Show in Finder") { revealInFinder(conflicts) }
                    .disabled(model.workspaceLocation == nil)
                    .help(conflicts.isEmpty ? "Show the workspace folder in Finder." : "Select the conflicted files in Finder.")
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(WireboltTheme.danger.opacity(0.08))
    }

    private func revealInFinder(_ changes: [GitChangeSnapshot]) {
        guard let root = model.workspaceLocation else { return }
        let files = changes.map { root.appending(path: $0.path) }
            .filter { FileManager.default.fileExists(atPath: $0.path) }
        NSWorkspace.shared.activateFileViewerSelecting(files.isEmpty ? [root] : files)
    }

    @ViewBuilder
    private func repositoryProblemView(_ problem: GitRepositoryProblem) -> some View {
        switch problem {
        case .notRepository:
            ContentUnavailableView {
                Label("Not a Git Repository", systemImage: "arrow.triangle.branch")
            } description: {
                Text("This workspace folder isn’t a Git repository yet. Initialize one here, then add a remote with Git to share it.")
            } actions: {
                Button("Initialize Git Repository") { Task { await model.initializeGitRepository() } }
                    .disabled(model.isGitBusy)
                    .help("Run git init in the workspace folder.")
            }
        case .notRepositoryRoot:
            ContentUnavailableView {
                Label("Workspace Isn’t the Repository Root", systemImage: "folder.badge.questionmark")
            } description: {
                Text("This workspace is inside a Git repository but isn’t its top folder. Wirebolt works with Git only when the folder containing wirebolt.toml is the repository root. Open the repository root as the workspace, or move the workspace into its own repository.")
            } actions: {
                Button("Show in Finder") {
                    if let root = model.workspaceLocation { NSWorkspace.shared.activateFileViewerSelecting([root]) }
                }
                .disabled(model.workspaceLocation == nil)
            }
        case .gitUnavailable:
            ContentUnavailableView(
                "Git Is Unavailable",
                systemImage: "exclamationmark.triangle",
                description: Text("Install the Xcode Command Line Tools (xcode-select --install) or Git, then refresh.")
            )
        }
    }

    private func changeList(_ status: GitStatusSnapshot) -> some View {
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
                    .disabled(model.isGitBusy || model.gitStatus == nil || trimmedCommitMessage.isEmpty || isMerging)
                    .help(isMerging ? "Finish or abort the merge first." : "Commit saved workspace documents.")

                Button("Pull") {
                    Task { await model.pullGit() }
                }
                .disabled(model.isGitBusy || model.gitStatus?.upstream == nil || model.hasUnsavedRequestChanges || isMerging)
                .help(isMerging ? "Finish or abort the merge first."
                    : "Pull the configured upstream. Conflicts are never resolved automatically.")

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
        .disabled(model.isGitBusy || isMerging || { if case .unavailable = availability { true } else { false } }())
        .help(isMerging ? "Finish or abort the merge first." : pushHelp(availability))
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
        case .mergeAborted: "Merge aborted; workspace restored to the last commit"
        }
    }

    private func operationIcon(_ outcome: GitOperationOutcome) -> String {
        outcome == .conflicted ? "exclamationmark.triangle.fill" : "checkmark.circle"
    }
}

