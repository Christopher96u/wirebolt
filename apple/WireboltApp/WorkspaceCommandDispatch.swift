import AppKit
import SwiftUI

extension WorkspaceUIState {
    var isCommandPaletteOpen: Bool { commandPalette?.isOpen == true }
    func workspaceInteractionBlocked(model: WireboltModel) -> Bool {
        model.isLoadingWorkspace || model.isImporting || model.isGitBusy || model.isOAuthBusy
            || model.isShowingAppSettings || model.isShowingWorkspaceSettings || model.isShowingGitCollaboration
            || model.importSummary?.needsReview == true || model.importFailureMessage != nil || model.operationFailure != nil
            || model.operationFailureMessage != nil || isShowingImporter || isShowingCurlImporter
            || workspaceNamePrompt != nil || workspaceDeleteRequest != nil || isShowingDirtyClose
    }
    func toggleCommandPalette(model: WireboltModel, in window: NSWindow? = NSApp.keyWindow) {
        if commandPalette == nil {
            commandPalette = CommandPaletteController(model: model, interface: self, defaults: defaults)
        }
        commandPalette?.toggle(in: window)
    }
    func commandDescriptor(_ command: WorkspaceCommand, context: CommandContext) -> CommandDescriptor {
        var context = context
        context.sidebarHidden = columnVisibility == .detailOnly
        return workspaceCommandDescriptor(command: command, context: context)
    }
    /// All menu and palette effects pass this guard at execution time.
    @discardableResult func dispatch(_ command: WorkspaceCommand, model: WireboltModel) -> Bool {
        guard !isCommandPaletteOpen else { return false }
        if command == .settings || command == .help {
            guard NSApp.modalWindow == nil, NSApp.keyWindow?.attachedSheet == nil else { return false }
            if command == .settings {
                presentAppSettings(model)
                if let window = WorkspaceWindowRegistry.primary { window.makeKeyAndOrderFront(nil) }
                else { openAuxiliaryWindow?("workspace") }
            } else { openAuxiliaryWindow?("help") }
            return true
        }
        guard !workspaceInteractionBlocked(model: model),
              let window = WorkspaceWindowRegistry.primary, isWorkspaceWindowActive(window),
              window.attachedSheet == nil, NSApp.modalWindow == nil,
              workspaceCommandDescriptor(command: command, context: model.workspaceCommandContext).unavailableReason == nil else { return false }
        switch command {
        case .newHttp: makeNewRequest(model: model)
        case .newWebSocket: makeNewRequest(model: model, kind: .webSocket)
        case .save:
            Task { _ = await saveCommand(model: model) }
        case .filterRequests: focusRequestSearch()
        case .toggleSidebar: columnVisibility = columnVisibility == .detailOnly ? .all : .detailOnly
        case .editUrl: focusURLTrigger += 1
        case .focusResponse: focusResponseTrigger += 1
        case .focusSidebar: focusSidebar()
        case .back: model.sessions.goBack(); synchronizeSelection(model: model)
        case .forward: model.sessions.goForward(); synchronizeSelection(model: model)
        case .nextTab: selectTab(offset: 1, model: model)
        case .previousTab: selectTab(offset: -1, model: model)
        case .splitRight:
            guard let tab = model.sessions.activeSession else { return false }
            openInNewSplit(tabID: tab.id, model: model)
        case .requestParams: requestSection = .params
        case .requestHeaders: requestSection = .headers
        case .requestBody: requestSection = .body
        case .requestAuth: requestSection = .auth
        case .requestNote: requestSection = .note
        case .requestSettings: requestSection = .settings
        case .responseHeaders: responseSection = .headers
        case .responseBody: responseSection = .body
        case .responseCookies: responseSection = .cookies
        case .responseRaw: responseSection = .raw
        case .responseRequest: responseSection = .request
        case .settings: presentAppSettings(model)
        case .help: openAuxiliaryWindow?("help")
        case .workspaceSettings: model.isShowingWorkspaceSettings = true
        case .environmentInspector: focusEnvironmentInspector()
        case .switchEnvironment, .changeTheme: toggleCommandPalette(model: model)
        }
        return true
    }
    func saveCommand(model: WireboltModel) async -> Bool {
        guard let session = model.sessions.activeSession,
              let collectionID = model.workspace.collections.first?.id else { return false }
        return await model.save(session, fallbackCollectionID: collectionID)
    }
    func focusEnvironmentInspector() {
        isShowingEnvironmentInspector = true
        environmentFocusTrigger += 1
    }
    /// ⌘W is modal while the palette owns the workspace.
    @discardableResult func dismissCommandPaletteForClose() -> Bool {
        guard isCommandPaletteOpen else { return false }
        commandPalette?.dismiss(restoringFocus: true)
        return true
    }
}

struct WorkspaceCommandButton: View {
    let command: WorkspaceCommand
    let model: WireboltModel
    let interface: WorkspaceUIState
    let state: WorkspaceCommandState
    var disabled = false
    var body: some View {
        let descriptor = interface.commandDescriptor(command, context: state.paletteContext)
        Group {
            if let key = descriptor.key.first {
                button(descriptor).keyboardShortcut(KeyEquivalent(key), modifiers: modifiers(descriptor.modifiers))
            } else { button(descriptor) }
        }
        .disabled(disabled || interface.isCommandPaletteOpen || interface.workspaceInteractionBlocked(model: model) || descriptor.unavailableReason != nil)
    }
    private func button(_ descriptor: CommandDescriptor) -> some View {
        Button(descriptor.title) { interface.dispatch(command, model: model) }
            .help(descriptor.unavailableReason ?? descriptor.title)
    }
    private func modifiers(_ value: UInt8) -> EventModifiers {
        var result: EventModifiers = []
        if value & 1 != 0 { result.insert(.command) }
        if value & 2 != 0 { result.insert(.shift) }
        if value & 4 != 0 { result.insert(.option) }
        if value & 8 != 0 { result.insert(.control) }
        return result
    }
}
