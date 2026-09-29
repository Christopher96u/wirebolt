import AppKit
import SwiftUI

@main
struct WireboltApp: App {
    @NSApplicationDelegateAdaptor(WireboltAppDelegate.self) private var appDelegate

    init() {
        PerformanceProbe.beginLaunch()
    }

    var body: some Scene {
        // One workspace window: two windows on the same workspace folder would each hold
        // their own copy of the tree and overwrite each other's saves. File ▸ New Window is
        // replaced by New Request, and restoration cannot bring back extra windows.
        WindowGroup("Wirebolt", id: "workspace") {
            ContentView(
                model: appDelegate.model,
                interface: appDelegate.interface
            )
            .focusedSceneValue(\.workspaceCommands, appDelegate.interface)
            .handlesExternalEvents(preferring: ["*"], allowing: ["*"])
        }
        .defaultSize(width: 1_248, height: 580)
        .windowResizability(.contentMinSize)
        .windowToolbarStyle(.unified)
        .restorationBehavior(.disabled)
        .commands {
            WireboltCommands(
                model: appDelegate.model,
                interface: appDelegate.interface,
                state: appDelegate.commandState
            )
        }

        Window("Wirebolt Help", id: WireboltCommands.helpWindowID) {
            WireboltHelpView()
        }
        .defaultSize(width: 650, height: 590)
        .windowResizability(.contentMinSize)

        Settings {
            WireboltSettingsView(model: appDelegate.model)
        }
    }
}

/// Owns the app-wide model so the delegate can guard quit and restore state on launch.
@MainActor
final class WireboltAppDelegate: NSObject, NSApplicationDelegate {
    let model = WireboltModel(
        runner: RustRequestRunner(),
        socketConnector: RustWebSocketRunner(),
        proxyPreferences: ProxyPreferences(defaults: .standard)
    )
    let interface = WorkspaceUIState()
    lazy var commandState = WorkspaceCommandState(model: model)
    private var appearanceObservation: NSKeyValueObservation?

    func applicationWillFinishLaunching(_: Notification) {
        // In-app tabs own ⌘T; native window tabs would stack a second tab bar on top.
        NSWindow.allowsAutomaticWindowTabbing = false
        Self.applyInterfaceAppearance()
        appearanceObservation = UserDefaults.standard.observe(\.interfaceAppearance) { _, _ in
            Task { @MainActor in WireboltAppDelegate.applyInterfaceAppearance() }
        }
    }

    func application(_: NSApplication, open urls: [URL]) {
        ExternalFileQueue.shared.enqueue(urls)
    }

    func applicationDidResignActive(_: Notification) {
        interface.persistSessionLayout(model: model)
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard model.hasUnsavedRequestChanges else {
            interface.persistSessionLayout(model: model)
            return .terminateNow
        }
        let window = WorkspaceWindowRegistry.primary.flatMap { $0.isVisible ? $0 : nil } ?? NSApp.mainWindow
        Task {
            let proceed = await confirmUnsavedChanges(model: model, in: window, quitting: true)
            if proceed { interface.persistSessionLayout(model: model) }
            sender.reply(toApplicationShouldTerminate: proceed)
        }
        return .terminateLater
    }

    func applicationWillTerminate(_: Notification) {
        interface.persistSessionLayout(model: model)
    }

    /// Applied app-wide so every window, including Settings, follows the choice, and
    /// System (nil) fully reverts to the macOS appearance.
    static func applyInterfaceAppearance() {
        NSApp.appearance = switch UserDefaults.standard.string(forKey: "interfaceAppearance") {
        case "light": NSAppearance(named: .aqua)
        case "dark": NSAppearance(named: .darkAqua)
        default: nil
        }
    }
}

private extension UserDefaults {
    /// KVO-observable mirror of the `interfaceAppearance` preference.
    @objc dynamic var interfaceAppearance: String? { string(forKey: "interfaceAppearance") }
}

struct WireboltCommands: Commands {
    static let helpWindowID = "help"

    @FocusedValue(\.workspaceCommands) private var focusedWorkspace
    @Environment(\.openWindow) private var openWindow
    let model: WireboltModel
    @Bindable var interface: WorkspaceUIState
    let state: WorkspaceCommandState
    private var recents: RecentWorkspaces { .shared }

    private var workspaceActionsBusy: Bool { model.isLoadingWorkspace || model.isGitBusy || model.isOAuthBusy }
    private var noWorkspace: Bool { focusedWorkspace == nil }

    var body: some Commands {
        TextEditingCommands()
        CommandGroup(replacing: .help) {
            Button("Wirebolt Help") { openWindow(id: Self.helpWindowID) }
                .keyboardShortcut("?", modifiers: .command)
        }
        CommandGroup(replacing: .newItem) {
            Button("New Request") { interface.makeNewRequest(model: model) }
                .keyboardShortcut("n", modifiers: .command)
                .disabled(noWorkspace)
            Button("New Tab") { interface.makeNewRequest(model: model, rename: false) }
                .keyboardShortcut("t", modifiers: .command)
                .disabled(noWorkspace)
            Divider()
            Button("New Workspace…") { chooseWorkspace(model: model, interface: interface, create: true) }
                .disabled(workspaceActionsBusy)
            Button("Open Workspace…") { chooseWorkspace(model: model, interface: interface, create: false) }
                .keyboardShortcut("o", modifiers: .command)
                .disabled(workspaceActionsBusy)
            Menu("Open Recent") {
                ForEach(recents.urls, id: \.self) { url in
                    Button(url.lastPathComponent) { openRecentWorkspace(url, model: model, interface: interface) }
                        .help(url.path)
                }
                Divider()
                Button("Clear Menu") { recents.clear() }
                    .disabled(recents.urls.isEmpty)
            }
            .disabled(workspaceActionsBusy)
            Divider()
            Button("New Collection…") { interface.promptForNewCollection() }
                .disabled(noWorkspace)
        }
        CommandGroup(replacing: .saveItem) {
            Button("Close Tab") {
                if let focusedWorkspace { focusedWorkspace.closeActiveTab(model: model) }
                else { NSApp.keyWindow?.performClose(nil) }
            }.keyboardShortcut("w", modifiers: .command)
            Button("Close Window") {
                guard let window = NSApp.keyWindow else { return }
                if focusedWorkspace != nil { requestWorkspaceWindowClose(window, model: model, interface: interface) }
                else { window.performClose(nil) }
            }.keyboardShortcut("w", modifiers: [.command, .shift])
            Divider()
            Button("Save") {
                guard let collectionID = model.workspace.collections.first?.id else { return }
                Task { await model.saveCurrentRequest(collectionID: collectionID) }
            }
            .keyboardShortcut("s", modifiers: .command)
            .disabled(noWorkspace || !state.hasActiveSession || !state.hasName || !state.hasCollections)
        }
        CommandGroup(replacing: .toolbar) {
            Button(NSApp.keyWindow?.toolbar?.isVisible == false ? "Show Toolbar" : "Hide Toolbar") {
                NSApp.keyWindow?.toggleToolbarShown(nil)
            }.keyboardShortcut("t", modifiers: [.command, .option]).disabled(noWorkspace)
            Button("Customize Toolbar…") { NSApp.keyWindow?.runToolbarCustomizationPalette(nil) }
                .disabled(noWorkspace)
            Divider()
            Button("Filter Requests") { interface.focusSearchTrigger += 1 }
                .keyboardShortcut("f", modifiers: [.command, .shift]).disabled(noWorkspace)
            Divider()
            Button(interface.columnVisibility == .detailOnly ? "Show Sidebar" : "Hide Sidebar") {
                interface.columnVisibility = interface.columnVisibility == .detailOnly ? .all : .detailOnly
            }.keyboardShortcut("s", modifiers: [.command, .control]).disabled(noWorkspace)
        }
        CommandMenu("Request") {
            Button("Send") {
                guard let session = model.sessions.activeSession else { return }
                NSApp.keyWindow?.makeFirstResponder(nil)
                if session.kind == .webSocket { Task { await session.socket.send(body: session.draft.body) } }
                else { Task { await model.send(session) } }
            }.keyboardShortcut(.return, modifiers: .command)
                .disabled(noWorkspace || !state.hasURL || state.isRunning
                    || (state.activeKind == .webSocket && state.socketStatus != .connected))
            Button("Cancel Request") { model.cancel() }
                .keyboardShortcut(".", modifiers: .command)
                .disabled(noWorkspace || !state.isRunning)
            Button(state.socketStatus == .connected ? "Disconnect" : "Connect") {
                guard let session = model.sessions.activeSession, session.kind == .webSocket else { return }
                if session.socket.status == .disconnected { Task { await model.connectWebSocket(session) } }
                else { session.socket.disconnect() }
            }.keyboardShortcut(.return, modifiers: [.command, .control])
                .disabled(noWorkspace || state.activeKind != .webSocket || !state.hasURL)
            Divider()
            Menu("New Request") {
                Button("HTTP") { interface.makeNewRequest(model: model) }
                    .keyboardShortcut("n", modifiers: [.command, .shift])
                Button("WebSocket") { interface.makeNewRequest(model: model, kind: .webSocket) }
            }.disabled(noWorkspace)
            Button("New Folder") { interface.makeNewFolder(model: model) }
                .keyboardShortcut("n", modifiers: [.command, .option]).disabled(noWorkspace)
            Divider()
            Button("Copy cURL") { copyRequestAsCurl(model.draft, model: model) }
                .keyboardShortcut("c", modifiers: [.command, .shift])
                .disabled(noWorkspace || state.activeKind != .http)
            Divider()
            Button("Edit URL") { interface.focusURLTrigger += 1 }
                .keyboardShortcut("l", modifiers: .command)
                .disabled(noWorkspace || !state.hasActiveSession)
            Button("Add Key") { interface.addKey() }
                .keyboardShortcut("k", modifiers: [.command, .shift])
                .disabled(noWorkspace || !interface.canEditFields)
            Divider()
            Toggle("Bulk Edit", isOn: $interface.isBulkEditing)
                .keyboardShortcut("b", modifiers: .command)
                .disabled(noWorkspace || !interface.canEditFields)
        }
        CommandMenu("Navigate") {
            Button("Go Back") { model.sessions.goBack(); interface.synchronizeSelection(model: model) }
                .keyboardShortcut(.leftArrow, modifiers: [.command, .control])
                .disabled(noWorkspace || !state.canGoBack)
            Button("Go Forward") { model.sessions.goForward(); interface.synchronizeSelection(model: model) }
                .keyboardShortcut(.rightArrow, modifiers: [.command, .control])
                .disabled(noWorkspace || !state.canGoForward)
            Divider()
            Button("Split Right") {
                if let tab = model.sessions.activeSession { interface.openInNewSplit(tabID: tab.id, model: model) }
            }.keyboardShortcut("d", modifiers: [.command, .shift])
                .disabled(noWorkspace || !state.hasActiveSession)
            Divider()
            Button("Select Next Tab") { interface.selectTab(offset: 1, model: model) }
                .keyboardShortcut(.tab, modifiers: .control).disabled(noWorkspace)
            Button("Select Previous Tab") { interface.selectTab(offset: -1, model: model) }
                .keyboardShortcut(.tab, modifiers: [.control, .shift]).disabled(noWorkspace)
            Menu("Select Tab at Index") {
                ForEach(1...9, id: \.self) { index in
                    Button("Tab \(index)") { interface.selectTab(index: index - 1, model: model) }
                        .keyboardShortcut(KeyEquivalent(Character(String(index))), modifiers: .command)
                        .disabled(noWorkspace || state.tabCount < index)
                }
            }
        }
        CommandMenu("Workspace") {
            Button("Workspace Settings…") { model.isShowingWorkspaceSettings = true }
                .disabled(noWorkspace)
            Button("Git Collaboration…") { model.isShowingGitCollaboration = true }
                .keyboardShortcut("g", modifiers: [.command, .shift])
                .disabled(noWorkspace || model.isLoadingWorkspace)
        }
    }
}
