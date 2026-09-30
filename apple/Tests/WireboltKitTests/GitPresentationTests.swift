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
