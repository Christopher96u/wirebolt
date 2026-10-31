import Foundation

/// Names for a workspace window. The toolbar shows only the workspace, because the active
/// tab already names the request; the window title keeps both for the Window menu, Mission
/// Control and VoiceOver.
public enum WorkspaceWindowName {
    /// The workspace name the toolbar shows. Never empty, so the window always has a name.
    public static func workspace(_ name: String) -> String {
        name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Wirebolt" : name
    }

    /// "<Workspace> — <Request>", or the workspace alone when no request is open.
    public static func title(workspace: String, request: String?) -> String {
        let workspace = self.workspace(workspace)
        guard let request, !request.isEmpty else { return workspace }
        return "\(workspace) — \(request)"
    }
}

public enum DocumentTabDescription {
    /// What VoiceOver reads after a tab's title: its method, then unsaved edits and preview.
    public static func accessibilityValue(method: HTTPMethod, webSocket: Bool, isDirty: Bool, isPreview: Bool) -> String {
        let parts: [String?] = [webSocket ? "WebSocket" : method.rawValue, isDirty ? "Edited" : nil, isPreview ? "Preview" : nil]
        return parts.compactMap(\.self).joined(separator: ", ")
    }
}

public extension AuthenticationKind {
    /// The request composer's short label for the authentication.
    var composerTitle: String {
        switch self {
        case .none: "No auth"
        case .basic: "Basic"
        case .bearer: "Bearer"
        case .apiKey: "API key"
        case .oauth2: "OAuth 2"
        }
    }
}

public extension RequestAuthentication {
    /// Choosing the current kind again keeps its credentials; another kind starts empty.
    func switching(to kind: AuthenticationKind) -> RequestAuthentication {
        kind == self.kind ? self : .new(kind)
    }
}

public extension TransportSettings {
    /// The request composer's label for the total timeout, where 0 means no limit.
    func totalTimeoutTitle(locale: Locale = .autoupdatingCurrent) -> String {
        let milliseconds = totalTimeoutMS
        if milliseconds == 0 { return "No limit" }
        if milliseconds < 1000 { return "\(milliseconds) ms" }
        if milliseconds.isMultiple(of: 60_000) { return "\(milliseconds / 60_000) min" }
        let seconds = Double(milliseconds) / 1000
        return seconds.formatted(.number.precision(.fractionLength(0...1)).locale(locale)) + " s"
    }
}

public extension EffectiveProxy {
    /// Whether the request composer shows the connection: a workspace or request override,
    /// or an invalid configuration. The app default applies everywhere and needs no reminder.
    var needsIndicator: Bool {
        guard (try? configuration.validate()) != nil else { return true }
        return switch source {
        case .request, .workspace: true
        case .appDefault, .systemDefault: false
        }
    }
}

public extension WireboltModel {
    /// The transport a run of `draft` uses: the workspace's, unless the request overrides it.
    func effectiveTransport(for draft: RequestDraft) -> TransportSettings {
        draft.inheritsWorkspaceTransport ? workspace.transport : draft.transport
    }
}
