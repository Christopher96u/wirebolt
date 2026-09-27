import AppKit
import SwiftUI

@main
struct WireboltApp: App {
    @FocusedValue(\.workspaceCommands) private var focusedWorkspace
    @State private var model: WireboltModel
    @State private var interface: WorkspaceUIState

    init() {
        PerformanceProbe.beginLaunch()
        _model = State(initialValue: WireboltModel(
            runner: RustRequestRunner(),
            socketConnector: RustWebSocketRunner(),
            proxyPreferences: ProxyPreferences(defaults: .standard)
        ))
        _interface = State(initialValue: WorkspaceUIState())
    }

    var body: some Scene {
        WindowGroup("Wirebolt", id: "workspace") {
            ContentView(
                model: model,
                interface: interface
            )
            .focusedSceneValue(\.workspaceCommands, interface)
        }
        .defaultSize(width: 1_248, height: 580)
        .windowResizability(.contentMinSize)
        .windowToolbarStyle(.unified)
        .commands {
            TextEditingCommands()
            CommandGroup(replacing: .help) {
                Button("Wirebolt Help") { model.isShowingHelp = true }
                    .keyboardShortcut("?", modifiers: .command)
            }
            CommandGroup(after: .newItem) {
                Button("New Workspace…") { chooseWorkspace(model: model, interface: interface, create: true) }
                    .disabled(model.isLoadingWorkspace || model.isGitBusy || model.isOAuthBusy)
                Button("Open Workspace…") { chooseWorkspace(model: model, interface: interface, create: false) }
                    .disabled(model.isLoadingWorkspace || model.isGitBusy || model.isOAuthBusy)
                    .keyboardShortcut("o", modifiers: .command)
                Button("New Collection…") { interface.promptForNewCollection() }
            }
            CommandMenu("Workspace") {
                Button("Workspace Settings…") { model.isShowingWorkspaceSettings = true }
                Button("Git Collaboration…") { model.isShowingGitCollaboration = true }
                    .keyboardShortcut("g", modifiers: [.command, .shift])
                    .disabled(model.isLoadingWorkspace)
            }
            CommandGroup(after: .newItem) {
                Button("New Tab") { interface.makeNewRequest(model: model, rename: false) }
                    .keyboardShortcut("t", modifiers: .command)
                    .disabled(focusedWorkspace == nil)
            }
            CommandGroup(replacing: .saveItem) {
                Button("Close Tab") {
                    if let focusedWorkspace { focusedWorkspace.closeActiveTab(model: model) }
                    else { NSApp.keyWindow?.performClose(nil) }
                }.keyboardShortcut("w", modifiers: .command)
                Button("Save…") {
                    guard let collectionID = model.workspace.collections.first?.id else { return }
                    Task { await model.saveCurrentRequest(collectionID: collectionID) }
                }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(
                    focusedWorkspace == nil || model.sessions.activeSession == nil
                        || model.draft.name.isEmpty
                        || model.workspace.collections.isEmpty
                )
            }
            CommandGroup(replacing: .toolbar) {
                Button(NSApp.keyWindow?.toolbar?.isVisible == false ? "Show Toolbar" : "Hide Toolbar") {
                    NSApp.keyWindow?.toggleToolbarShown(nil)
                }.keyboardShortcut("t", modifiers: [.command, .option]).disabled(focusedWorkspace == nil)
                Button("Customize Toolbar…") { NSApp.keyWindow?.runToolbarCustomizationPalette(nil) }
                    .disabled(focusedWorkspace == nil)
                Divider()
                Button("Filter Requests") { interface.focusSearchTrigger += 1 }
                    .keyboardShortcut("f", modifiers: [.command, .shift]).disabled(focusedWorkspace == nil)
                Divider()
                Button(interface.columnVisibility == .detailOnly ? "Show Sidebar" : "Hide Sidebar") {
                    interface.columnVisibility = interface.columnVisibility == .detailOnly ? .all : .detailOnly
                }.keyboardShortcut("s", modifiers: [.command, .control]).disabled(focusedWorkspace == nil)
            }
            CommandMenu("Request") {
                Button("Send") {
                    guard let session = model.sessions.activeSession else { return }
                    NSApp.keyWindow?.makeFirstResponder(nil)
                    if session.kind == .webSocket { Task { await session.socket.send(body: session.draft.body) } }
                    else { Task { await model.send(session) } }
                }.keyboardShortcut(.return, modifiers: .command)
                    .disabled(focusedWorkspace == nil || model.draft.url.isEmpty || model.isRunning
                        || (model.sessions.activeSession?.kind == .webSocket && model.sessions.activeSession?.socket.status != .connected))
                Button(model.sessions.activeSession?.socket.status == .connected ? "Disconnect" : "Connect") {
                    guard let session = model.sessions.activeSession, session.kind == .webSocket else { return }
                    if session.socket.status == .disconnected { Task { await model.connectWebSocket(session) } }
                    else { session.socket.disconnect() }
                }.keyboardShortcut(.return, modifiers: [.command, .control])
                    .disabled(focusedWorkspace == nil || model.sessions.activeSession?.kind != .webSocket || model.draft.url.isEmpty)
                Divider()
                Menu("New Request") {
                    Button("HTTP") { interface.makeNewRequest(model: model) }
                        .keyboardShortcut("n", modifiers: [.command, .shift])
                    Button("WebSocket") { interface.makeNewRequest(model: model, kind: .webSocket) }
                }.disabled(focusedWorkspace == nil)
                Button("New Folder") { interface.makeNewFolder(model: model) }
                    .keyboardShortcut("n", modifiers: [.command, .option]).disabled(focusedWorkspace == nil)
                Divider()
                Button("Copy cURL") { copyRequestAsCurl(model.draft, model: model) }
                    .keyboardShortcut("c", modifiers: [.command, .shift])
                    .disabled(focusedWorkspace == nil || model.sessions.activeSession?.kind != .http)
                Divider()
                Button("Edit URL") { interface.focusURLTrigger += 1 }
                    .keyboardShortcut("l", modifiers: .command)
                    .disabled(focusedWorkspace == nil || model.sessions.activeSession == nil)
                Button("Add Key") { interface.addKey() }
                    .keyboardShortcut("k", modifiers: [.command, .shift])
                    .disabled(focusedWorkspace == nil || !interface.canEditFields)
                Divider()
                Toggle("Bulk Edit", isOn: $interface.isBulkEditing)
                    .keyboardShortcut("b", modifiers: .command)
                    .disabled(focusedWorkspace == nil || !interface.canEditFields)
            }
            CommandMenu("Navigate") {
                Button("Go Back") { model.sessions.goBack(); interface.synchronizeSelection(model: model) }
                    .keyboardShortcut(.leftArrow, modifiers: [.command, .control])
                    .disabled(focusedWorkspace == nil || model.sessions.activeGroup?.backwardTabIDs.isEmpty != false)
                Button("Go Forward") { model.sessions.goForward(); interface.synchronizeSelection(model: model) }
                    .keyboardShortcut(.rightArrow, modifiers: [.command, .control])
                    .disabled(focusedWorkspace == nil || model.sessions.activeGroup?.forwardTabIDs.isEmpty != false)
                Divider()
                Button("Split Right") {
                    if let tab = model.sessions.activeSession { interface.openInNewSplit(tabID: tab.id, model: model) }
                }.keyboardShortcut("d", modifiers: [.command, .shift])
                    .disabled(focusedWorkspace == nil || model.sessions.activeSession == nil)
                Divider()
                Button("Select Next Tab") { interface.selectTab(offset: 1, model: model) }
                    .keyboardShortcut(.tab, modifiers: .control).disabled(focusedWorkspace == nil)
                Button("Select Previous Tab") { interface.selectTab(offset: -1, model: model) }
                    .keyboardShortcut(.tab, modifiers: [.control, .shift]).disabled(focusedWorkspace == nil)
                Menu("Select Tab at Index") {
                    ForEach(1...9, id: \.self) { index in
                        Button("Tab \(index)") { interface.selectTab(index: index - 1, model: model) }
                            .keyboardShortcut(KeyEquivalent(Character(String(index))), modifiers: .command)
                            .disabled(focusedWorkspace == nil || (model.sessions.activeGroup?.tabIDs.count ?? 0) < index)
                    }
                }
            }
        }

        Settings {
            WireboltSettingsView(model: model)
        }
    }
}
