import AppKit
import SwiftUI

@main
struct WireboltApp: App {
    @NSApplicationDelegateAdaptor(WireboltAppDelegate.self) private var appDelegate

    init() {
        PerformanceProbe.beginLaunch()
        UserDefaults.standard.register(defaults: [
            // `--workspace <folder>` must not become an open-document event: it would be
            // treated as an import and suppress the workspace window.
            "NSTreatUnknownArgumentsAsOpen": "NO",
            // Wirebolt restores its own tabs and layout. Stale AppKit window-restoration state
            // could otherwise relaunch the app without its workspace window.
            "ApplePersistenceIgnoreState": "YES",
        ])
    }

    var body: some Scene {
        // One workspace window: two windows on the same workspace folder would each hold
        // their own copy of the tree and overwrite each other's saves. File ▸ New Window is
        // replaced by New Request. AppKit state restoration is ignored (see init), so only the
        // window frame is remembered.
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
    private var tabSwitchMonitor: Any?

    func applicationWillFinishLaunching(_: Notification) {
        // In-app tabs own ⌘T; native window tabs would stack a second tab bar on top.
        NSWindow.allowsAutomaticWindowTabbing = false
        Self.applyInterfaceAppearance()
        appearanceObservation = UserDefaults.standard.observe(\.interfaceAppearance) { _, _ in
            Task { @MainActor in WireboltAppDelegate.applyInterfaceAppearance() }
        }
        tabSwitchMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            MainActor.assumeIsolated { self?.handleTabSwitchKey(event) == true } ? nil : event
        }
    }

    /// ⌃⇥ / ⌃⇧⇥ switch tabs in the workspace window, alongside the menu's ⌘} / ⌘{. A menu
    /// item holds only one shortcut, so the second pair is handled here.
    private func handleTabSwitchKey(_ event: NSEvent) -> Bool {
        let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
        guard event.keyCode == 48, modifiers == .control || modifiers == [.control, .shift],
              let window = event.window, window === WorkspaceWindowRegistry.primary, window.attachedSheet == nil
        else { return false }
        interface.selectTab(offset: modifiers.contains(.shift) ? -1 : 1, model: model)
        return true
    }

    func application(_: NSApplication, open urls: [URL]) {
        // Only regular files are imports; folders (such as a workspace dropped on the Dock
        // icon) are ignored.
        let files = urls.filter { url in
            var isDirectory: ObjCBool = false
            return FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory) && isDirectory.boolValue == false
        }
        ExternalFileQueue.shared.enqueue(files)
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
    @FocusedValue(\.sidebarMove) private var sidebarMove
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
        // Creation commands live here once; toolbar and context menus reuse them without
        // declaring their own shortcuts.
        CommandGroup(replacing: .newItem) {
            Button("New Request") { interface.makeNewRequest(model: model) }
                .keyboardShortcut("n", modifiers: .command)
                .disabled(noWorkspace)
            Button("New WebSocket Request") { interface.makeNewRequest(model: model, kind: .webSocket) }
                .disabled(noWorkspace)
            Button("New Tab") { interface.makeNewRequest(model: model, rename: false) }
                .keyboardShortcut("t", modifiers: .command)
                .disabled(noWorkspace)
            Button("New Folder") { interface.makeNewFolder(model: model) }
                .keyboardShortcut("n", modifiers: [.command, .option])
                .disabled(noWorkspace)
            Button("New Collection…") { interface.promptForNewCollection() }
                .keyboardShortcut("n", modifiers: [.command, .shift])
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
        // The system toolbar items keep "Show/Hide Toolbar" in sync with the window.
        ToolbarCommands()
        CommandGroup(after: .toolbar) {
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
                commitPendingEdits(in: NSApp.keyWindow)
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
            // Reorders the request or folder selected in the focused sidebar.
            Button("Move Up") {
                if let sidebarMove { interface.moveSidebarItem(sidebarMove.identifier, by: -1, model: model) }
            }.keyboardShortcut(.upArrow, modifiers: [.command, .option])
                .disabled(sidebarMove?.canMoveUp != true)
            Button("Move Down") {
                if let sidebarMove { interface.moveSidebarItem(sidebarMove.identifier, by: 1, model: model) }
            }.keyboardShortcut(.downArrow, modifiers: [.command, .option])
                .disabled(sidebarMove?.canMoveDown != true)
            Divider()
            Button("Copy cURL") { copyRequestAsCurl(model.draft, model: model) }
                .keyboardShortcut("c", modifiers: [.command, .shift])
                .disabled(noWorkspace || state.activeKind != .http)
            Divider()
            Button("Edit URL") { interface.focusURLTrigger += 1 }
                .keyboardShortcut("l", modifiers: .command)
                .disabled(noWorkspace || !state.hasActiveSession)
            // Switches to Params first when the visible section has no key-value table.
            Button("Add Key") { interface.addKey() }
                .keyboardShortcut("k", modifiers: [.command, .shift])
                .disabled(noWorkspace || !state.hasActiveSession)
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
            Button("Focus Sidebar") { interface.focusSidebar() }
                .keyboardShortcut("0", modifiers: .command)
                .disabled(noWorkspace)
            Button("Focus Response") { interface.focusResponseTrigger += 1 }
                .keyboardShortcut("0", modifiers: [.command, .option])
                .disabled(noWorkspace || !state.hasActiveSession)
            Menu("Request Section") {
                ForEach(Array(RequestPanelSection.allCases.enumerated()), id: \.element) { index, section in
                    Button(section == .body && state.activeKind == .webSocket ? "Message" : section.rawValue) {
                        interface.requestSection = section
                    }
                    .keyboardShortcut(KeyEquivalent(Character(String(index + 1))), modifiers: [.command, .option])
                }
            }.disabled(noWorkspace || !state.hasActiveSession)
            // Shift is avoided with digits: ⇧1 types "!" and would not match on every layout.
            Menu("Response Section") {
                ForEach(Array(ResponsePanelSection.allCases.enumerated()), id: \.element) { index, section in
                    Button(section.rawValue) { interface.responseSection = section }
                        .keyboardShortcut(KeyEquivalent(Character(String(index + 1))), modifiers: [.command, .control])
                }
            }.disabled(noWorkspace || state.activeKind != .http)
            Divider()
            // ⌘} and ⌘{ (typed as ⇧⌘] and ⇧⌘[ on U.S. keyboards). ⌃⇥ and ⌃⇧⇥ also switch
            // tabs; see `WireboltAppDelegate.handleTabSwitchKey(_:)`.
            Button("Show Next Tab") { interface.selectTab(offset: 1, model: model) }
                .keyboardShortcut("}", modifiers: .command).disabled(noWorkspace || state.tabCount < 2)
            Button("Show Previous Tab") { interface.selectTab(offset: -1, model: model) }
                .keyboardShortcut("{", modifiers: .command).disabled(noWorkspace || state.tabCount < 2)
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
            Button("Rename Workspace…") { promptToRenameWorkspace(model: model) }
                .disabled(noWorkspace || workspaceActionsBusy)
            Button("Git Collaboration…") { model.isShowingGitCollaboration = true }
                .keyboardShortcut("g", modifiers: [.command, .shift])
                .disabled(noWorkspace || model.isLoadingWorkspace)
        }
    }
}
