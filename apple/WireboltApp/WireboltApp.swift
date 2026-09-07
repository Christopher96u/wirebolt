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
            socketConnector: RustWebSocketRunner()
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
            CommandGroup(replacing: .newItem) {
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

private struct WireboltSettingsView: View {
    @Bindable var model: WireboltModel
    @AppStorage("interfaceAppearance") private var interfaceAppearance = "system"
    @AppStorage("editor.fontSize") private var fontSize = 12.0
    @State private var showingPrivacyPolicy = false

    var body: some View {
        TabView {
            Tab("General", systemImage: "gearshape") {
                Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 13) {
                    GridRow(alignment: .top) {
                        Text("General:").gridColumnAlignment(.trailing)
                        VStack(alignment: .leading, spacing: 7) {
                            Toggle("Validate SSL Certificate", isOn: $model.workspace.transport.validateTLS)
                            Text("Validate the SSL Certificate (Common Name, Expiry date, etc)")
                                .font(.system(size: 11)).foregroundStyle(.secondary)
                            Toggle("Automatically Follow Redirect", isOn: $model.workspace.transport.followRedirects)
                            Text("Always follow the Redirection of the Response. Max 10 cycles.")
                                .font(.system(size: 11)).foregroundStyle(.secondary)
                        }
                    }
                    GridRow(alignment: .top) {
                        Text("Request Timeout:")
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                TextField("Seconds", value: timeout, format: .number)
                                    .multilineTextAlignment(.trailing).frame(width: 100)
                                Text("seconds")
                            }
                            Text("Set a Second that the Request will timeout. Enter 0 to disable timeouts.")
                                .font(.system(size: 11)).foregroundStyle(.secondary)
                        }
                    }
                    Divider().gridCellColumns(2).padding(.vertical, 4)
                    GridRow(alignment: .top) {
                        Text("Font Size:")
                        VStack(alignment: .leading, spacing: 6) {
                            Picker("Font Size", selection: $fontSize) {
                                ForEach(Array(10...20) + [24, 28], id: \.self) { size in
                                    Text(String(size)).tag(Double(size))
                                }
                            }.labelsHidden().frame(width: 60)
                            Text("Only Apply to the Body Tab.").font(.system(size: 11)).foregroundStyle(.secondary)
                        }
                    }
                    GridRow {
                        Text("App Theme:")
                        Picker("App Theme", selection: $interfaceAppearance) {
                            Text("System").tag("system")
                            Text("Light").tag("light")
                            Text("Dark").tag("dark")
                        }.labelsHidden().frame(width: 80)
                    }
                }
                .font(.system(size: 13)).controlSize(.regular)
                .padding(.leading, 44).padding(.trailing, 20).padding(.vertical, 20)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            }
            Tab("Privacy", systemImage: "person.fill") {
                Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 18) {
                    GridRow(alignment: .top) {
                        Text("Analytics:").frame(width: 104, alignment: .trailing)
                        VStack(alignment: .leading, spacing: 6) {
                            Toggle("Share analytics with Wirebolt", isOn: .constant(false)).disabled(true)
                                .frame(height: 18, alignment: .top).padding(.leading, 2)
                            Text("Wirebolt does not collect usage analytics or send identifiers automatically. Your collections, requests and environments stay in your workspace.")
                                .font(.system(size: 11)).foregroundStyle(.secondary)
                                .frame(width: 384, height: 44, alignment: .topLeading).padding(.leading, 2)
                        }
                    }
                    GridRow(alignment: .top) {
                        Text("Crash Report:").frame(width: 104, alignment: .trailing)
                        VStack(alignment: .leading, spacing: 6) {
                            Toggle("Share crash reports with Wirebolt", isOn: .constant(false)).disabled(true)
                                .frame(height: 18, alignment: .top).padding(.leading, 2)
                            Text("Wirebolt does not upload crash reports or diagnostics automatically. You decide what information to include when reporting a problem.")
                                .font(.system(size: 11)).foregroundStyle(.secondary)
                                .frame(width: 384, height: 44, alignment: .topLeading).padding(.leading, 2)
                            Text("Your data stays under your control.")
                                .font(.system(size: 13)).foregroundStyle(.secondary).padding(.leading, 2)
                            Button { showingPrivacyPolicy = true } label: {
                                Text("Privacy Policy").frame(width: 86, height: 18)
                            }
                                .padding(.top, 12).padding(.leading, 2)
                        }
                    }
                }
                .font(.system(size: 13)).controlSize(.regular)
                .padding(.leading, 44).padding(.trailing, 18).padding(.top, 20)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                .sheet(isPresented: $showingPrivacyPolicy) {
                    VStack(alignment: .leading, spacing: 16) {
                        Text("Privacy Policy").font(.title2.weight(.semibold))
                        Text("Wirebolt stores your workspace on this Mac. It does not automatically send analytics, identifiers, crash reports or diagnostics.")
                        Text("Requests connect to the addresses you choose and may use your configured proxy. Sign-in and Git sync connect to their services when you use those features.")
                        Text("You control which workspace files or diagnostic details you share with others.")
                        HStack { Spacer(); Button("Done") { showingPrivacyPolicy = false }.keyboardShortcut(.defaultAction) }
                    }.padding(24).frame(width: 480)
                }
            }
        }
        .frame(width: 562, height: 309)
        .task(id: model.workspace.transport) {
            do { try await Task.sleep(for: .milliseconds(250)) } catch { return }
            await model.saveWorkspaceTransport()
        }
    }

    private var timeout: Binding<Double> {
        Binding(
            get: { Double(model.workspace.transport.totalTimeoutMS) / 1000 },
            set: { seconds in
                guard seconds.isFinite, seconds >= 0, seconds < Double(UInt64.max) / 1000 else { return }
                model.workspace.transport.totalTimeoutMS = UInt64(seconds * 1000)
            }
        )
    }
}
