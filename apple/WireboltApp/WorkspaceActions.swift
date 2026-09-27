import AppKit
import SwiftUI

@MainActor
func chooseWorkspace(model: WireboltModel, interface: WorkspaceUIState, create: Bool) {
    guard !model.isLoadingWorkspace, !model.isGitBusy, !model.isOAuthBusy else { return }
    if model.hasUnsavedRequestChanges {
        let alert = NSAlert()
        alert.messageText = "Discard unsaved request changes?"
        alert.informativeText = "Opening another workspace closes its tabs and cancels active requests. Save your edits first to keep them."
        alert.addButton(withTitle: "Cancel")
        alert.addButton(withTitle: "Discard and Open")
        guard alert.runModal() == .alertSecondButtonReturn else { return }
    }
    let url: URL?
    if create {
        let panel = NSSavePanel()
        panel.title = "New Workspace"
        panel.prompt = "Create"
        panel.nameFieldStringValue = "New Workspace"
        panel.message = "Create a folder for your requests, collections and environments."
        url = panel.runModal() == .OK ? panel.url : nil
    } else {
        let panel = NSOpenPanel()
        panel.title = "Open Workspace"
        panel.prompt = "Open"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose the folder containing wirebolt.toml."
        url = panel.runModal() == .OK ? panel.url : nil
    }
    guard let url else { return }
    Task {
        do {
            let persistence = try await Task.detached(priority: .userInitiated) {
                try RustWorkspacePersistence(path: url, mode: create ? .create : .open)
            }.value
            if await model.openWorkspace(using: persistence, gitCollaboration: persistence) {
                UserDefaults.standard.set(url.path, forKey: "workspace.lastOpenedPath")
                interface.resetForWorkspace(model: model)
            }
        } catch {
            let alert = NSAlert()
            alert.messageText = create ? "Workspace could not be created" : "Workspace could not be opened"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }
}

struct WireboltHelpView: View {
    @Environment(\.dismiss) private var dismiss
    private let topics: [(String, String)] = [
        ("Workspaces & collections", "Use File → New Workspace or Open Workspace to choose where requests and environments are stored. New Collection creates a top-level container. The + menu creates requests and folders. Drag items to reorder siblings or move requests between folders and collections. Save edits with ⌘S."),
        ("Requests & tabs", "Choose an HTTP method and URL, then Send (⌘Return). Params, Headers, Body and Auth configure the request. Cancel stops an active send. Use Navigate → Split Right to compare requests. Closing a dirty tab asks before discarding changes."),
        ("Variables & secrets", "Open the environment menu → Configure Environments. Global variables apply to every request; the selected environment overrides matching names. Use {{name}} in URLs and fields. Disable a row to omit it. Secret references resolve from this Mac’s Keychain; sharing a collection does not share those credentials."),
        ("Proxy & transport", "Settings → Network sets the app default. Workspace Settings overrides it for a workspace; a request’s Settings tab can override that again. Inherit follows the parent, Direct bypasses proxies, System uses macOS settings, Manual uses your routes. Save or Apply proxy edits. Test connection sends a HEAD request without request headers, body or cookies. Transport edits save automatically at workspace scope; request edits require ⌘S. Reconnect WebSockets after changing settings."),
        ("Import & export", "The + menu imports cURL, HAR, Postman v2 and Wirebolt / Legacy Collection v1 JSON. Export writes saved requests, so save edits first. API Key and OAuth use Wirebolt-specific authentication metadata: reimport with Wirebolt to preserve it. Other clients may not support these extensions. Referenced credentials must be configured on the destination Mac; response history is not exported."),
        ("WebSocket", "Create a WebSocket request, enter ws:// or wss:// and Connect. Choose Text, JSON, Binary (Hex/Base64) or File for messages. Send transmits the selected representation. Disconnect ends the connection; reconnect applies updated settings."),
        ("Notes", "Write Markdown in Note → Edit. Preview renders headings, lists, emphasis, links and code locally when opened. Save with ⌘S. Images show alternative text; previews do not fetch remote content."),
        ("Git collaboration", "Workspace → Git Collaboration opens status, commit, pull and push. The workspace must already be in a Git repository. Configure its remote/upstream and authentication with Git first. Only saved workspace documents are committed. Save or discard edits before pulling; conflicts require explicit resolution. Wirebolt does not sync in the background."),
        ("Keyboard shortcuts", "⌘T new tab · ⌘S save · ⌘W close tab · ⌘Return send · ⌃⌘Return connect/disconnect · ⌘L edit URL · ⌘⇧F filter requests · ⌘⇧D split right · ⌃Tab next tab · ⌘1–9 select tab."),
    ]
    var body: some View {
        VStack(spacing: 0) {
            HStack { Text("Wirebolt Help").font(.title2.bold()); Spacer(); Button("Done") { dismiss() }.keyboardShortcut(.cancelAction) }.padding(20)
            Divider()
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
        }.frame(width: 650, height: 590)
    }
}
