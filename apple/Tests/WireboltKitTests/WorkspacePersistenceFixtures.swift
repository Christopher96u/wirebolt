import Foundation
@testable import WireboltKit

extension WorkspaceDraft {
    var loadResult: WorkspaceLoadResult {
        WorkspaceLoadResult(workspace: domain, diagnostics: WorkspaceLoadDiagnostics(problems: [], omitted: 0, notice: nil))
    }
}

extension MoveEligibility {
    var isAllowed: Bool {
        if case .allowed = self { return true }
        return false
    }
}
