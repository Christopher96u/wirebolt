import Foundation
import Testing
@testable import WireboltKit

@Suite("Workspace chrome labels")
struct WorkspaceChromeTests {
    @Test("The toolbar names only the workspace; the window title adds the active request")
    func windowNames() {
        #expect(WorkspaceWindowName.workspace("Payments") == "Payments")
        #expect(WorkspaceWindowName.workspace("") == "Wirebolt")
        #expect(WorkspaceWindowName.workspace("  ") == "Wirebolt")
        #expect(WorkspaceWindowName.title(workspace: "Payments", request: "Capture payment") == "Payments — Capture payment")
        #expect(WorkspaceWindowName.title(workspace: "Payments", request: nil) == "Payments")
        #expect(WorkspaceWindowName.title(workspace: "Payments", request: "") == "Payments")
        #expect(WorkspaceWindowName.title(workspace: "", request: "Refund") == "Wirebolt — Refund")
    }

    @Test("A tab's VoiceOver value names its method before edits and preview")
    func tabAccessibility() {
        #expect(DocumentTabDescription.accessibilityValue(method: .put, webSocket: false, isDirty: false, isPreview: false) == "PUT")
        #expect(DocumentTabDescription.accessibilityValue(method: .post, webSocket: false, isDirty: true, isPreview: true) == "POST, Edited, Preview")
        #expect(DocumentTabDescription.accessibilityValue(method: .get, webSocket: true, isDirty: false, isPreview: true) == "WebSocket, Preview")
    }

    @Test("Choosing the current authentication keeps its credentials; another kind starts empty")
    func switchingAuthentication() {
        let bearer = RequestAuthentication.bearer(token: .secret("request.token"))
        #expect(bearer.switching(to: .bearer) == bearer)
        let basic = bearer.switching(to: .basic)
        #expect(basic.kind == .basic)
        guard case let .basic(username, _) = basic else { Issue.record("Expected Basic"); return }
        #expect(username == .literal(""))
        #expect(bearer.switching(to: .none) == .none)
        #expect(RequestAuthentication.none.switching(to: .apiKey).kind == .apiKey)
    }

    @Test("Every authentication kind has a short composer label")
    func authenticationTitles() {
        #expect(AuthenticationKind.allCases.map(\.composerTitle) == ["No auth", "Basic", "Bearer", "API key", "OAuth 2"])
    }

    @Test("The timeout label reads 0 as no limit and keeps sub-second and fractional values")
    func timeoutTitles() {
        let english = Locale(identifier: "en_US")
        func title(_ milliseconds: UInt64) -> String {
            TransportSettings(totalTimeoutMS: milliseconds).totalTimeoutTitle(locale: english)
        }
        #expect(title(30_000) == "30 s")
        #expect(title(0) == "No limit")
        #expect(title(500) == "500 ms")
        #expect(title(1_500) == "1.5 s")
        #expect(title(90_000) == "90 s")
        #expect(title(120_000) == "2 min")
        #expect(TransportSettings(totalTimeoutMS: 1_500).totalTimeoutTitle(locale: Locale(identifier: "es_ES")) == "1,5 s")
    }

    @Test("The composer shows the workspace timeout unless the request overrides it")
    @MainActor func effectiveTransport() {
        let model = WireboltModel(runner: SilentRunner())
        model.workspace.transport = TransportSettings(totalTimeoutMS: 5_000)
        let inherited = RequestDraft(transport: TransportSettings(totalTimeoutMS: 30_000), inheritsWorkspaceTransport: true)
        #expect(model.effectiveTransport(for: inherited).totalTimeoutMS == 5_000)
        let overridden = RequestDraft(transport: TransportSettings(totalTimeoutMS: 60_000), inheritsWorkspaceTransport: false)
        #expect(model.effectiveTransport(for: overridden).totalTimeoutMS == 60_000)
    }

    @Test("The composer flags workspace and request proxy overrides, and any invalid proxy")
    func proxyIndicator() {
        let manual = ProxyDocument.manual(routes: [ProxyRouteDocument(destination: "all", endpoint: "http://localhost:8080")])
        for app in [ProxyDocument.system, .direct, manual] {
            #expect(!EffectiveProxy.resolve(request: nil, workspace: nil, app: app).needsIndicator)
            #expect(EffectiveProxy.resolve(request: nil, workspace: .system, app: app).needsIndicator)
            #expect(EffectiveProxy.resolve(request: .direct, workspace: nil, app: app).needsIndicator)
        }
        // An invalid configuration always shows, so the error is visible before sending.
        #expect(EffectiveProxy.resolve(request: nil, workspace: nil, app: .manual(routes: [])).needsIndicator)
    }
}

