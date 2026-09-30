import AppKit
import SwiftUI

@MainActor
func chooseWorkspace(model: WireboltModel, interface: WorkspaceUIState, create: Bool) {
    guard !model.isLoadingWorkspace, !model.isGitBusy, !model.isOAuthBusy else { return }
    let window = NSApp.keyWindow
    Task {
        guard await confirmDiscardingForWorkspaceSwitch(model: model, in: window) else { return }
        let panel: NSSavePanel
        if create {
            panel = NSSavePanel()
            panel.title = "New Workspace"
            panel.prompt = "Create"
            panel.nameFieldStringValue = "New Workspace"
            panel.message = "Create a folder for your requests, collections and environments."
        } else {
            let open = NSOpenPanel()
            open.title = "Open Workspace"
            open.prompt = "Open"
            open.canChooseDirectories = true
            open.canChooseFiles = false
            open.allowsMultipleSelection = false
            open.message = "Choose the folder containing wirebolt.toml."
            panel = open
        }
        guard await present(panel, in: window) == .OK, let url = panel.url else { return }
        await openWorkspace(at: url, create: create, model: model, interface: interface, window: window)
    }
}

/// Opens a workspace from File ▸ Open Recent.
@MainActor
func openRecentWorkspace(_ url: URL, model: WireboltModel, interface: WorkspaceUIState) {
    guard !model.isLoadingWorkspace, !model.isGitBusy, !model.isOAuthBusy else { return }
    let window = NSApp.keyWindow
    Task {
        guard await confirmDiscardingForWorkspaceSwitch(model: model, in: window) else { return }
        await openWorkspace(at: url, create: false, model: model, interface: interface, window: window)
    }
}

@MainActor
private func confirmDiscardingForWorkspaceSwitch(model: WireboltModel, in window: NSWindow?) async -> Bool {
    guard model.hasUnsavedRequestChanges else { return true }
    let alert = NSAlert()
    alert.messageText = "Discard unsaved request changes?"
    alert.informativeText = "Opening another workspace closes its tabs and cancels active requests. Save your edits first to keep them."
    alert.addButton(withTitle: "Cancel")
    alert.addButton(withTitle: "Discard and Open")
    return await present(alert, in: window) == .alertSecondButtonReturn
}

@MainActor
private func openWorkspace(at url: URL, create: Bool, model: WireboltModel, interface: WorkspaceUIState, window: NSWindow?) async {
    do {
        let persistence = try await Task.detached(priority: .userInitiated) {
            try RustWorkspacePersistence(path: url, mode: create ? .create : .open)
        }.value
        interface.persistSessionLayout(model: model)
        if await model.openWorkspace(using: persistence, gitCollaboration: persistence,
                                     restoring: interface.savedSessionLayout(for: url)) {
            UserDefaults.standard.set(url.path, forKey: "workspace.lastOpenedPath")
            RecentWorkspaces.shared.note(url)
            interface.workspaceURL = url
            interface.resetForWorkspace(model: model)
        }
    } catch {
        if !create, (error as? WorkspaceSelectionError) == .workspaceNotFound { RecentWorkspaces.shared.remove(url) }
        let alert = NSAlert()
        alert.messageText = create ? "Workspace could not be created" : "Workspace could not be opened"
        alert.informativeText = error.localizedDescription
        _ = await present(alert, in: window)
    }
}

/// Workspace ▸ Rename Workspace…: the name lives in wirebolt.toml; the folder keeps its name.
@MainActor
func promptToRenameWorkspace(model: WireboltModel) {
    let window = NSApp.keyWindow
    Task {
        let alert = NSAlert()
        alert.messageText = "Rename Workspace"
        alert.informativeText = "The name is saved in the workspace and shown in the window title. The folder keeps its name."
        let field = NSTextField(string: model.workspace.name)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        field.setAccessibilityLabel("Workspace name")
        alert.accessoryView = field
        alert.addButton(withTitle: "Rename")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        guard await present(alert, in: window) == .alertFirstButtonReturn else { return }
        let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { NSSound.beep(); return }
        await model.renameWorkspace(name)
    }
}

/// Asks before unsaved request edits are lost. Returns true when the caller may
/// continue: edits were saved, or the person chose Don't Save (edits are reverted).
@MainActor
func confirmUnsavedChanges(model: WireboltModel, in window: NSWindow?, quitting: Bool) async -> Bool {
    let dirty = model.dirtySessions
    guard !dirty.isEmpty else { return true }
    let alert = NSAlert()
    alert.messageText = dirty.count == 1
        ? "Do you want to save the changes you made to “\(dirty[0].title)”?"
        : "You have unsaved changes in \(dirty.count) requests. Do you want to save them?"
    alert.informativeText = quitting
        ? "Your changes will be lost if you quit without saving."
        : "Your changes will be lost if you don’t save them."
    alert.addButton(withTitle: "Save")
    alert.addButton(withTitle: "Cancel")
    let discard = alert.addButton(withTitle: "Don’t Save")
    discard.keyEquivalent = "d"
    discard.keyEquivalentModifierMask = .command
    switch await present(alert, in: window) {
    case .alertFirstButtonReturn:
        return await model.saveAllDirtySessions()
    case .alertThirdButtonReturn:
        model.discardUnsavedChanges()
        return true
    default:
        return false
    }
}

/// The window close button and File ▸ Close Window route here so unsaved edits are confirmed.
@MainActor
func requestWorkspaceWindowClose(_ window: NSWindow, model: WireboltModel, interface: WorkspaceUIState) {
    Task {
        guard await confirmUnsavedChanges(model: model, in: window, quitting: false) else { return }
        interface.persistSessionLayout(model: model)
        window.close()
    }
}

/// The single workspace window, used to enforce one instance and to anchor quit alerts.
@MainActor
enum WorkspaceWindowRegistry {
    static weak var primary: NSWindow?
}

/// Presents as a sheet on a visible window so the rest of the app stays usable.
@MainActor
func present(_ alert: NSAlert, in window: NSWindow?) async -> NSApplication.ModalResponse {
    guard let window, window.isVisible, window.attachedSheet == nil else { return alert.runModal() }
    return await withCheckedContinuation { continuation in
        alert.beginSheetModal(for: window) { continuation.resume(returning: $0) }
    }
}

@MainActor
func present(_ panel: NSSavePanel, in window: NSWindow?) async -> NSApplication.ModalResponse {
    guard let window, window.isVisible, window.attachedSheet == nil else { return panel.runModal() }
    return await withCheckedContinuation { continuation in
        panel.beginSheetModal(for: window) { continuation.resume(returning: $0) }
    }
}

/// Writes an export chosen in a save sheet; failures explain why instead of beeping.
@MainActor
func saveExportedDocument(named name: String, content: String) {
    let window = NSApp.keyWindow
    Task {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(name).json"
        panel.allowedContentTypes = [.json]
        panel.canCreateDirectories = true
        guard await present(panel, in: window) == .OK, let destination = panel.url else { return }
        do {
            try Data(content.utf8).write(to: destination, options: .atomic)
        } catch {
            let alert = NSAlert(error: error)
            alert.messageText = "The export could not be saved."
            alert.informativeText = error.localizedDescription
            _ = await present(alert, in: window)
        }
    }
}

/// Workspace folders for File ▸ Open Recent, most recent first.
@MainActor
@Observable
final class RecentWorkspaces {
    static let shared = RecentWorkspaces()
    private static let key = "workspace.recentPaths"
    private static let limit = 10
    private(set) var urls: [URL]

    private init() {
        urls = (UserDefaults.standard.stringArray(forKey: Self.key) ?? []).map { URL(fileURLWithPath: $0, isDirectory: true) }
    }

    func note(_ url: URL) {
        let url = url.standardizedFileURL
        let updated = [url] + urls.filter { $0.path != url.path && FileManager.default.fileExists(atPath: $0.path) }
        store(Array(updated.prefix(Self.limit)))
    }

    func remove(_ url: URL) { store(urls.filter { $0.path != url.standardizedFileURL.path }) }

    func clear() { store([]) }

    private func store(_ updated: [URL]) {
        guard updated != urls else { return }
        urls = updated
        UserDefaults.standard.set(updated.map(\.path), forKey: Self.key)
    }
}

/// Files opened from Finder or dropped on the window, waiting for the workspace to load.
@MainActor
@Observable
final class ExternalFileQueue {
    static let shared = ExternalFileQueue()
    private(set) var pending: [URL] = []

    func enqueue(_ urls: [URL]) { pending.append(contentsOf: urls.filter(\.isFileURL)) }

    func drain() -> [URL] {
        defer { pending = [] }
        return pending
    }
}

/// Imports a file with the importer that matches its contents.
@MainActor
func importExternalFile(_ url: URL, model: WireboltModel) async {
    let source = await Task.detached(priority: .userInitiated) {
        try? String(contentsOf: url, encoding: .utf8)
    }.value
    guard let source, let format = ImportFormat.detect(fileExtension: url.pathExtension, contents: source) else {
        model.importFailureMessage = "“\(url.lastPathComponent)” isn’t a cURL command, HAR file, Postman Collection v2 or Wirebolt collection."
        return
    }
    if format == .curl {
        await model.importDocument(source: source, format: .curl)
    } else {
        await model.importDocument(url: url, format: format)
    }
}

/// Shown in its own window so help can stay open beside the workspace.
struct WireboltHelpView: View {
    private let topics: [(String, String)] = [
        ("Workspaces & collections", "Use File → New Workspace or Open Workspace to choose where requests and environments are stored. New Collection creates a top-level container. The + menu creates requests and folders. Drag items to reorder siblings or move requests between folders and collections. Save edits with ⌘S. Workspace → Rename Workspace changes the name in the window title. Workspace → Cookies lists this workspace’s cookies; session cookies last until Wirebolt quits."),
        ("Requests & tabs", "Choose an HTTP method and URL, then Send (⌘Return). Params, Headers, Body and Auth configure the request; Auth supports Basic, Bearer Token, API Key and OAuth 2.0, with credentials stored in Keychain per request. Cancel stops an active send. Use Navigate → Split Right to compare requests. Closing a dirty tab asks before discarding changes."),
        ("Variables & secrets", "Choose Configure Environments… from the environment menu. Global variables apply to every request; the selected environment overrides matching names. Use {{name}} in URLs and fields. Disable a row to omit it. Click a variable’s lock button to make it secret: Save stores the value in this Mac’s Keychain and the workspace keeps only a reference, so sharing or exporting does not share the value."),
        ("Proxy & transport", "Settings → Network sets the app default. Workspace Settings overrides it for a workspace; a request’s Settings tab can override that again. Inherit follows the parent, Direct bypasses proxies, System uses macOS settings, Manual uses your routes. Test connection sends a HEAD request without request headers, body or cookies. In Workspace Settings, Save keeps proxy and transport edits and Cancel discards them. In a request’s Settings tab, click Apply to request for proxy edits (transport edits apply to the draft directly), then save the request with ⌘S. Timeouts, redirects & TLS also sets a client certificate for mutual TLS, stored in Keychain, and a custom CA file. Reconnect WebSockets after changing settings."),
        ("Import & export", "The + menu imports cURL, HAR, Postman v2 and Wirebolt / Legacy Collection v1 JSON. Export writes saved requests, so save edits first. API Key and OAuth use Wirebolt-specific authentication metadata: reimport with Wirebolt to preserve it. Other clients may not support these extensions. Referenced credentials must be configured on the destination Mac; response history is not exported."),
        ("WebSocket", "Create a WebSocket request, enter ws:// or wss:// and Connect. Choose Text, JSON, Binary (Hex/Base64) or File for messages. Send transmits the selected representation. Disconnect ends the connection; reconnect applies updated settings."),
        ("Notes", "Write Markdown in Note → Edit. Preview renders headings, lists, emphasis, links and code locally when opened. Save with ⌘S. Images show alternative text; previews do not fetch remote content."),
        ("Git collaboration", "Workspace → Git Collaboration opens status, commit, pull and push. The workspace folder must be the root of its Git repository; if it isn’t a repository yet, choose Initialize Git Repository. Configure its remote/upstream and authentication with Git. Only saved workspace documents are committed. Save or discard edits before pulling. After a conflicted pull, resolve the listed files with Git or choose Abort Merge to return to your last commit. Wirebolt does not sync in the background."),
        ("Keyboard shortcuts", "⌘N new request · ⌘T new tab · ⌥⌘N new folder · ⇧⌘N new collection · ⌘S save · ⌘W close tab · ⇧⌘W close window · ⌘Return send · ⌘. cancel request · ⌃⌘Return connect/disconnect · ⌘L edit URL · ⇧⌘K add key · ⌘B bulk edit · ⌥⇧⌘C copy cURL · ⇧⌘F filter requests · ⌘0 focus sidebar · ⌥⌘0 focus response · ⌥⌘1–6 request sections · ⌃⌘1–5 response sections · ⇧⌘D split right · ⇧⌘] / ⇧⌘[ or ⌃Tab / ⌃⇧Tab next / previous tab · ⌘1–9 select tab · ⌃⌘G Git collaboration. The menus list every shortcut."),
    ]
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                ForEach(topics, id: \.0) { title, text in
                    VStack(alignment: .leading, spacing: 8) {
                        Text(title).font(.headline)
                        Text(text).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    }
                }
            }.padding(24).frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(minWidth: 420, idealWidth: 650, minHeight: 320, idealHeight: 590)
    }
}
