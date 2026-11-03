import Foundation
import Observation

/// Serializes native end-edit callbacks; Rust owns buffers, reconciliation and commits.
@MainActor @Observable
public final class EnvironmentInspectorEditing {
    public let inspector = AppEnvironmentInspector(active: nil)
    public private(set) var drafts: [EnvironmentFieldDraft] = []
    public var problems: [EnvironmentField: String] = [:]
    public var commandProblem: String?
    private var work: Task<Bool, Never>?
    private var pendingCommits = 0
    private unowned let model: WireboltModel
    public var hasEdits: Bool { !drafts.isEmpty || pendingCommits > 0 }
    init(model: WireboltModel) { self.model = model }
    public func text(for field: EnvironmentField) -> String? { drafts.first { $0.field == field }?.text }
    public func edit(_ field: EnvironmentField, text: String) {
        inspector.edit(field: field, text: text)
        drafts = inspector.drafts()
    }
    public func reconcile() {
        if let notice = inspector.reconcile(environments: model.workspace.environments.map(\.domain), active: model.selectedEnvironmentID) {
            model.recordEnvironmentEditingNotice(notice)
        }
        drafts = inspector.drafts()
        problems = problems.filter { field, _ in drafts.contains { $0.field == field } }
    }
    public func commit(_ field: EnvironmentField) {
        guard let text = text(for: field) else { return }
        let previous = work
        pendingCommits += 1
        work = Task { @MainActor in
            _ = await previous?.value
            defer { pendingCommits -= 1 }
            reconcile()
            guard self.text(for: field) != nil else { return true }
            let input = environmentFieldCommit(field: field, text: text)
            let problem = await model.commitEnvironment(id: input.environmentId, commit: input.commit)
            problems[field] = problem
            if problem == nil { inspector.committed(field: field, text: text) }
            reconcile()
            return problem == nil
        }
    }
    public func perform(id: String, commit action: EnvironmentCommit, completion: @escaping () -> Void) {
        reconcile()
        for draft in drafts { commit(draft.field) }
        let previous = work
        pendingCommits += 1
        work = Task { @MainActor in
            _ = await previous?.value
            defer { pendingCommits -= 1 }
            reconcile()
            guard drafts.isEmpty else { return false }
            commandProblem = await model.commitEnvironment(id: id, commit: action)
            if commandProblem == nil { completion() }
            return commandProblem == nil
        }
    }
    public func flush() async -> Bool {
        reconcile()
        _ = await work?.value
        for draft in drafts { commit(draft.field); _ = await work?.value }
        reconcile()
        return drafts.isEmpty
    }
}
