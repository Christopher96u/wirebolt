import AppKit
import Foundation
import SwiftUI

private final class DocumentationWindow: NSWindow {
    override var canBecomeKey: Bool { true }
}

/// This adapter supplies display-only Git data. It never invokes Git or a remote.
private struct DocumentationGit: GitCollaboration {
    func status() async throws -> GitStatusSnapshot {
        GitStatusSnapshot(branch: "main", upstream: "origin/main", ahead: 0, behind: 0, changes: [
            GitChangeSnapshot(path: "collections/catalog/requests/create-order.toml", staged: .none, unstaged: .modified, conflicted: false),
            GitChangeSnapshot(path: "environments/staging.toml", staged: .added, unstaged: .none, conflicted: false)
        ])
    }
    func pull() async throws -> GitOperationSnapshot { throw WorkspaceMutationError.unsupported }
    func commit(message: String) async throws -> GitOperationSnapshot { throw WorkspaceMutationError.unsupported }
    func push() async throws -> GitOperationSnapshot { throw WorkspaceMutationError.unsupported }
}

/// Production native views, disposable workspace/preferences/storage, literal demo values.
/// HTTP uses the production Rust transport against the documentation loopback fixture.
/// WebSocket events and Git status are injected; no remote service or Keychain is used.
@main @MainActor struct DocumentationScreenshots {
    static let output = URL(fileURLWithPath: CommandLine.arguments[1])
    static let root = URL(fileURLWithPath: CommandLine.arguments[2])
    static let baseURL = CommandLine.arguments[3]
    static let domain = "wirebolt.docs-screenshots.\(UUID().uuidString)"
    static let defaults = UserDefaults(suiteName: domain)!
    static var files: [String] = []

    static func capture(_ host: NSView, name: String) async throws {
        await NativeTestSupport.settle(host)
        try NativeTestSupport.capture(host, directory: output, name: name)
        files.append(name + ".png")
        print("Captured \(name).png")
    }

    static func mount<V: View>(_ view: V, width: CGFloat = 1440, height: CGFloat = 900) async -> (NSWindow, NSView) {
        let window = DocumentationWindow(contentRect: NSRect(x: 40, y: 40, width: width, height: height),
            styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.appearance = NSAppearance(named: .darkAqua)
        let host = NSHostingView(rootView: view
            .defaultAppStorage(defaults)
            .environment(\.customThemeStore, ThemeStore.shared)
            .modifier(ThemeScope())
            .preferredColorScheme(.dark)
            .frame(width: width, height: height))
        window.contentView = host
        window.orderFront(nil)
        window.makeKey()
        await NativeTestSupport.settle(host)
        return (window, host)
    }

    static func save<V: View>(_ view: V, name: String, width: CGFloat = 1440, height: CGFloat = 900) async throws {
        let (window, host) = await mount(view, width: width, height: height)
        defer { window.close() }
        try await capture(host, name: name)
    }

    static func choose(_ command: WorkspaceCommand, controller: CommandPaletteController, model: WireboltModel, interface: WorkspaceUIState) {
        controller.query(">" + interface.commandDescriptor(command, model: model).title)
        guard let result = controller.projection.snapshot?.results.first(where: { $0.destination == .command(command: command) }) else {
            preconditionFailure("Documentation command is missing")
        }
        controller.confirm(result.id)
    }

    static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        NSApp.appearance = NSAppearance(named: .darkAqua)
        let restoreAX = NativeTestSupport.enableAccessibility()
        defer { restoreAX(); defaults.removePersistentDomain(forName: domain) }
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
        defaults.set("black", forKey: "interfaceAppearance")
        defaults.set("comfortable", forKey: "interfaceDensity")
        defaults.set(260.0, forKey: "workspace.sidebarWidth")
        defaults.set(13.0, forKey: "editor.fontSize")
        defaults.set(true, forKey: "editor.wordWrap")
        ThemeStore.shared.loadLibrary(at: root.appending(path: "themes.json"))
        let harbor = domainDeriveTheme(background: PaletteColor(hex: 0x101923), accent: PaletteColor(hex: 0x64cec7))
        _ = await ThemeStore.shared.save(name: "Harbor", preview: harbor)
        let list = RequestDraft(id: "list", name: "List products", url: baseURL + "/products", proxy: .direct)
        let order = RequestDraft(id: "order", name: "Create order", method: .post, url: baseURL + "/echo",
            note: """
            # Create order

            Sends an order payload to the local demo API.

            - Use **Staging** for local development.
            - A successful response returns **200 OK**.
            - Send with **⌘Return**; save with **⌘S**.

            > Demo data only. No credentials required.

            ```json
            {"name": "Desk lamp", "quantity": 2}
            ```
            """,
            headers: [RequestField(name: "Accept", value: .literal("application/json"))],
            body: .json(value: "{\n  \"name\": \"Desk lamp\",\n  \"quantity\": 2\n}"), proxy: .direct)
        let socketDraft = RequestDraft(id: "events", name: "Catalog events", url: "ws://127.0.0.1:18991/events", webSocket: true,
            body: .json(value: "{\n  \"type\": \"subscribe\",\n  \"channel\": \"catalog\"\n}"), proxy: .direct)
        let workspace = WorkspaceDraft(name: "Catalog API", collections: [
            CollectionDraft(id: "catalog", name: "Catalog API", groups: [GroupDraft(id: "products", name: "Products"), GroupDraft(id: "orders", name: "Orders")], requests: [
                RequestLocation(collectionID: "catalog", groupID: "products", request: list),
                RequestLocation(collectionID: "catalog", groupID: "orders", request: order)
            ]),
            CollectionDraft(id: "realtime", name: "Realtime", requests: [RequestLocation(collectionID: "realtime", request: socketDraft)])
        ], environments: [
            EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Globals", variables: [
                EnvironmentVariableDraft(id: "base-global", key: "base_url", value: .literal(baseURL))]),
            EnvironmentDraft(id: "staging", name: "Staging", variables: [
                EnvironmentVariableDraft(id: "base-staging", key: "base_url", value: .literal(baseURL)),
                EnvironmentVariableDraft(id: "region", key: "region", value: .literal("ap-southeast-2")),
                EnvironmentVariableDraft(id: "debug", key: "debug", value: .literal("true"), enabled: false)]),
            EnvironmentDraft(id: "production", name: "Production", variables: [
                EnvironmentVariableDraft(id: "base-prod", key: "base_url", value: .literal("https://api.example.test"))])
        ])
        let preferences = ProxyPreferences(defaults: defaults)
        try preferences.save(.direct)
        let cookies = CookieJar(storageURL: root.appending(path: "cookies.json"))
        _ = await cookies.store(headers: [ResponseHeader(name: "Set-Cookie", value: "session=demo-only; Path=/; HttpOnly; SameSite=Lax"),
            ResponseHeader(name: "Set-Cookie", value: "region=au; Path=/; Max-Age=86400")], requestURL: URL(string: baseURL)!)
        let model = WireboltModel(runner: RustRequestRunner(), persistence: PaletteFixtureStore(workspace), gitCollaboration: DocumentationGit(),
            history: HistoryRepository(root: root.appending(path: "history")), cookieJar: cookies, cookieDirectory: nil, proxyPreferences: preferences)
        await model.loadWorkspace()
        model.selectedEnvironmentID = "staging"
        let interface = WorkspaceUIState(defaults: defaults)
        interface.expandedSidebarGroups = ["catalog:products", "catalog:orders"]
        interface.responseOrientation = .right
        interface.activateSavedRequest(workspace.location(collectionID: "catalog", requestID: "list")!, model: model, preview: false)
        let listSession = model.sessions.activeSession!
        await model.send(listSession)
        precondition(listSession.responseHead?.status == 200 && listSession.responseText.contains("products"), "The documentation HTTP fixture must answer GET")
        interface.activateSavedRequest(workspace.location(collectionID: "catalog", requestID: "order")!, model: model, preview: false)
        let orderSession = model.sessions.activeSession!
        await model.send(orderSession)
        precondition(orderSession.responseHead?.status == 200 && orderSession.responseText.contains("Desk lamp"), "The documentation HTTP fixture must echo JSON")
        interface.requestSection = .body
        let content = ContentView(model: model, interface: interface, loadsWorkspace: false)
        let (window, host) = await mount(content)
        interface.isWorkspaceWindowActive = { [weak window] in $0 === window }
        WorkspaceWindowRegistry.primary = window
        defer { interface.commandPalette?.dismiss(restoringFocus: false); window.close(); WorkspaceWindowRegistry.primary = nil }
        try await capture(host, name: "overview")
        interface.requestSection = .note
        interface.presentation(for: orderSession).previewsNotes = true
        try await capture(host, name: "notes")
        interface.requestSection = .body
        interface.isShowingEnvironmentInspector = true
        try await capture(host, name: "environments")
        interface.isShowingEnvironmentInspector = false

        interface.toggleCommandPalette(model: model, in: window)
        await NativeTestSupport.wait(host) { interface.commandPalette?.presentation == .presented }
        let palette = interface.commandPalette!
        palette.query("products")
        try await capture(host, name: "command-palette")
        palette.query("")
        choose(.switchEnvironment, controller: palette, model: model, interface: interface)
        try await capture(host, name: "command-palette-environments")
        palette.backOrDismiss()
        choose(.changeTheme, controller: palette, model: model, interface: interface)
        try await capture(host, name: "command-palette-themes")
        palette.dismiss(restoringFocus: false)

        interface.activateSavedRequest(workspace.location(collectionID: "realtime", requestID: "events")!, model: model, preview: false)
        let socketSession = model.sessions.activeSession!
        let socket = FixtureSocket()
        socketSession.socket.connect(input: RunInput(draft: socketSession.draft, variables: [:]), connector: FixtureConnector(socket: socket))
        socket.continuation.yield(.connected([ResponseHeader(name: "Upgrade", value: "websocket"), ResponseHeader(name: "Connection", value: "Upgrade")]))
        socket.continuation.yield(.message(Data("{\"type\":\"subscribe\",\"channel\":\"catalog\"}".utf8), binary: false, outgoing: true))
        socket.continuation.yield(.message(Data("{\"event\":\"product.updated\",\"id\":\"prod_001\",\"in_stock\":true}".utf8), binary: false, outgoing: false))
        interface.requestSection = .body
        await NativeTestSupport.settle(host)
        if let table = tables(host).last {
            table.selectRowIndexes(IndexSet(integer: 2), byExtendingSelection: false)
            table.delegate?.tableViewSelectionDidChange?(Notification(name: NSTableView.selectionDidChangeNotification, object: table))
        }
        try await capture(host, name: "websocket")
        socketSession.socket.disconnect()
        interface.activateTab(id: orderSession.id, model: model)
        interface.requestSection = .auth
        orderSession.draft.authentication = .oauth2(configuration: OAuth2Configuration(authorizationURL: "https://auth.example.test/authorize", tokenURL: "https://auth.example.test/token", clientID: "wirebolt-demo", scopes: "catalog:read orders:write"))
        try await capture(host, name: "authentication")
        orderSession.draft.authentication = .none
        model.workspace.proxy = .manual(routes: [ProxyRouteDocument(destination: .all, endpoint: "http://127.0.0.1:8080")])
        try await save(WorkspaceNetworkSettings(model: model), name: "proxy", width: 590, height: 580)
        try await save(GitCollaborationView(model: model), name: "git", width: 760, height: 540)
        try await save(CookiesView(model: model), name: "cookies", width: 1100, height: 440)
        model.settingsTab = "appearance"
        model.isShowingAppSettings = true
        try await save(WireboltSettingsView(model: model), name: "appearance", width: 1200, height: 940)
        let manifest: [String: Any] = ["files": files, "httpFixture": baseURL,
            "capture": "Production native views; live loopback HTTP; injected WebSocket events and Git status; disposable preferences and storage."]
        try JSONSerialization.data(withJSONObject: manifest, options: [.prettyPrinted, .sortedKeys]).write(to: output.appending(path: "manifest.json"))
        precondition(files.count == 12)
    }

    static func tables(_ view: NSView) -> [NSTableView] {
        (view as? NSTableView).map { [$0] } ?? view.subviews.flatMap(tables)
    }
}
