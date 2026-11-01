import Foundation
import Testing
@testable import WireboltKit

struct GitPresentationTests {
    private func change(_ staged: GitDeltaSnapshot, _ unstaged: GitDeltaSnapshot, conflicted: Bool = false) -> GitChangeSnapshot {
        GitChangeSnapshot(path: "requests/a.toml", staged: staged, unstaged: unstaged, conflicted: conflicted)
    }

    @Test("File statuses read as words instead of porcelain codes")
    func changeKinds() {
        #expect(change(.none, .untracked).kind == .new)
        #expect(change(.added, .modified).kind == .new)
        #expect(change(.modified, .none).kind == .modified)
        #expect(change(.none, .deleted).kind == .deleted)
        #expect(change(.renamed, .none).kind == .renamed)
        #expect(change(.renamed, .modified).kind == .renamed)
        #expect(change(.unmerged, .unmerged).kind == .conflicted)
        #expect(change(.modified, .modified, conflicted: true).kind == .conflicted)
        #expect(GitChangeKind.new.rawValue == "New")
    }

    @Test("Push is disabled with a reason when nothing can be pushed")
    func pushAvailability() {
        func status(branch: String?, upstream: String?, ahead: UInt64) -> GitStatusSnapshot {
            GitStatusSnapshot(branch: branch, upstream: upstream, ahead: ahead, behind: 0, changes: [])
        }
        #expect(status(branch: "main", upstream: "origin/main", ahead: 2).pushAvailability == .push)
        #expect(status(branch: "main", upstream: nil, ahead: 0).pushAvailability == .publish(branch: "main"))
        guard case .unavailable = status(branch: "main", upstream: "origin/main", ahead: 0).pushAvailability else {
            Issue.record("Up-to-date branch must not offer Push"); return
        }
        guard case .unavailable = status(branch: nil, upstream: nil, ahead: 0).pushAvailability else {
            Issue.record("Detached HEAD must not offer Push"); return
        }
    }
}

struct GitMergeRecoveryTests {
    @Test("Typed status and operation records preserve merge state")
    func projectsMergeState() {
        let merging = GitStatusSnapshot(WorkspaceGitStatus(
            branch: "main", upstream: "origin/main", ahead: 1, behind: 1,
            changes: [WorkspaceGitChange(path: "wirebolt.toml", previousPath: nil, staged: .unmerged, unstaged: .unmerged, conflicted: true),
                WorkspaceGitChange(path: "environments/dev.toml", previousPath: nil, staged: .modified, unstaged: .none, conflicted: false)], merging: true))
        #expect(merging.merging)
        #expect(merging.conflictedChanges.map(\.path) == ["wirebolt.toml"])
        let clean = WorkspaceGitStatus(branch: "main", upstream: nil, ahead: 0, behind: 0, changes: [], merging: false)
        #expect(GitStatusSnapshot(clean).merging == false)
        let aborted = GitOperationSnapshot(WorkspaceGitOperation(outcome: .mergeAborted, revision: "abc", status: clean))
        #expect(aborted.outcome == .mergeAborted)
    }

    @Test("Repository failures that need guidance are recognised by kind")
    func repositoryProblems() {
        #expect(GitRepositoryProblem(kind: .notRepository) == .notRepository)
        #expect(GitRepositoryProblem(kind: .workspaceNotRepositoryRoot) == .notRepositoryRoot)
        #expect(GitRepositoryProblem(kind: .gitUnavailable) == .gitUnavailable)
        #expect(GitRepositoryProblem(kind: .authenticationRequired) == nil)
    }

    @Test("Abort Merge reloads the restored workspace; Initialize shows the new repository")
    @MainActor
    func abortAndInitialize() async {
        let collaboration = MergeRecorder()
        let persistence = RenamingPersistence()
        let model = WireboltModel(runner: SilentRunner(), persistence: persistence, gitCollaboration: collaboration)
        await model.loadWorkspace()

        await model.abortGitMerge()
        #expect(model.gitOperation?.outcome == .mergeAborted)
        #expect(model.gitStatus?.merging == false)
        #expect(model.workspace.name == "Reloaded")

        await model.initializeGitRepository()
        #expect(model.gitOperation == nil)
        #expect(model.gitStatus?.upstream == nil)
        #expect(model.gitFailure == nil)
        #expect(await collaboration.calls == ["abort", "initialize"])
    }

    @Test("Persistence without Git recovery reports the action as unsupported")
    @MainActor
    func unsupportedRecovery() async {
        let model = WireboltModel(runner: SilentRunner(), gitCollaboration: StatusOnlyGit())
        await model.abortGitMerge()
        #expect(model.gitFailure?.kind == .unsupported)
    }
}

private actor MergeRecorder: GitCollaboration {
    private(set) var calls: [String] = []
    private let clean = GitStatusSnapshot(branch: "main", upstream: "origin/main", ahead: 1, behind: 0, changes: [])

    func status() async throws -> GitStatusSnapshot { clean }
    func pull() async throws -> GitOperationSnapshot { GitOperationSnapshot(outcome: .upToDate, revision: nil, status: clean) }
    func commit(message _: String) async throws -> GitOperationSnapshot { GitOperationSnapshot(outcome: .committed, revision: nil, status: clean) }
    func push() async throws -> GitOperationSnapshot { GitOperationSnapshot(outcome: .pushed, revision: nil, status: clean) }
    func abortMerge() async throws -> GitOperationSnapshot {
        calls.append("abort")
        return GitOperationSnapshot(outcome: .mergeAborted, revision: "abc", status: clean)
    }
    func initializeRepository() async throws -> GitStatusSnapshot {
        calls.append("initialize")
        return GitStatusSnapshot(branch: "main", upstream: nil, ahead: 0, behind: 0, changes: [])
    }
}

private struct StatusOnlyGit: GitCollaboration {
    private var clean: GitStatusSnapshot { GitStatusSnapshot(branch: "main", upstream: nil, ahead: 0, behind: 0, changes: []) }
    func status() async throws -> GitStatusSnapshot { clean }
    func pull() async throws -> GitOperationSnapshot { GitOperationSnapshot(outcome: .upToDate, revision: nil, status: clean) }
    func commit(message _: String) async throws -> GitOperationSnapshot { GitOperationSnapshot(outcome: .committed, revision: nil, status: clean) }
    func push() async throws -> GitOperationSnapshot { GitOperationSnapshot(outcome: .pushed, revision: nil, status: clean) }
}

private actor RenamingPersistence: WorkspacePersistence {
    private var loads = 0
    func load() async throws -> WorkspaceDraft {
        loads += 1
        return WorkspaceDraft(name: loads == 1 ? "Conflicted" : "Reloaded")
    }
    func save(request _: RequestDraft, in _: String) async throws {}
    func save(environment _: EnvironmentDraft) async throws {}
    func saveSecret(name _: String, value _: String) async throws {}
}
