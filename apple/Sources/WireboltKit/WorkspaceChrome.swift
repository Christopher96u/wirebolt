import Foundation

/// Names for a workspace window. The toolbar shows only the workspace, because the active
/// tab already names the request; the window title keeps both for the Window menu, Mission
/// Control and VoiceOver.
public enum WorkspaceWindowName {
    /// The workspace name the toolbar shows. Never empty, so the window always has a name.
    public static func workspace(_ name: String) -> String {
        domainWorkspaceName(name: name)
    }

    /// "<Workspace> — <Request>", or the workspace alone when no request is open.
    public static func title(workspace: String, request: String?) -> String {
        domainWindowTitle(workspace: workspace, request: request)
    }
}

public enum DocumentTabDescription {
    /// What VoiceOver reads after a tab's title: its method, then unsaved edits and preview.
    public static func accessibilityValue(method: HTTPMethod, webSocket: Bool, isDirty: Bool, isPreview: Bool) -> String {
        domainTabDescription(method: method.rawValue, webSocket: webSocket, dirty: isDirty, preview: isPreview)
    }
}

public extension AuthenticationKind {
    /// The request composer's short label for the authentication.
    var composerTitle: String {
        domainAuthTitle(kind: self)
    }
}

public extension RequestAuthentication {
    /// Choosing the current kind again keeps its credentials; another kind starts empty.
    func switching(to kind: AuthenticationKind) -> RequestAuthentication {
        RequestAuthentication(domain: domainSwitchAuth(authentication: domain, kind: kind))
    }
}

public extension TransportSettings {
    /// The request composer's label for the total timeout, where 0 means no limit.
    func totalTimeoutTitle(locale: Locale = .autoupdatingCurrent) -> String {
        let label = domainTimeoutPresentation(milliseconds: totalTimeoutMS)
        if let text = label.text { return text }
        return (label.seconds ?? 0).formatted(.number.precision(.fractionLength(0...1)).locale(locale)) + label.suffix
    }
}

public extension EffectiveProxy {
    /// Whether the request composer shows the connection: a workspace or request override,
    /// or an invalid configuration. The app default applies everywhere and needs no reminder.
    var needsIndicator: Bool {
        domainProxyNeedsIndicator(configuration: configuration.domain, source: source)
    }
}

public extension Array where Element == RequestField {
    /// The rows a section tab counts: enabled and named. A row still being typed counts
    /// once it has a name; a disabled or nameless row does not.
    var activeCount: Int {
        Int(domainActiveCount(fields: map(\.domain)))
    }
}

public extension WireboltModel {
    /// The transport a run of `draft` uses: the workspace's, unless the request overrides it.
    func effectiveTransport(for draft: RequestDraft) -> TransportSettings {
        TransportSettings(domain: domainEffectiveTransport(request: draft.transport.domain, workspace: workspace.transport.domain, inherits: draft.inheritsWorkspaceTransport))
    }
}
