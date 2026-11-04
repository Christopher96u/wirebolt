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
    func canOpenCommandPalette(model: WireboltModel, in window: NSWindow?) -> Bool {
        guard let window else { return false }
        return window === WorkspaceWindowRegistry.primary && isWorkspaceWindowActive(window)
            && model.hasLoadedWorkspace && !workspaceInteractionBlocked(model: model)
            && !isRenamingInSidebar && window.attachedSheet == nil && NSApp.modalWindow == nil
    }
    func commandContext(model: WireboltModel) -> CommandContext {
        model.workspaceCommandContext(sidebarHidden: columnVisibility == .detailOnly,
            canOpenPalette: isCommandPaletteOpen || canOpenCommandPalette(model: model, in: WorkspaceWindowRegistry.primary))
    }
    func commandDescriptor(_ command: WorkspaceCommand, model: WireboltModel) -> CommandDescriptor {
        domainWorkspaceCommandDescriptor(command: command, context: commandContext(model: model))
    }
    func commandIsAvailable(_ command: WorkspaceCommand, model: WireboltModel) -> Bool {
        if command == .help || command == .settings { return true }
        if command == .commandPalette { return commandDescriptor(command, model: model).unavailableReason == nil }
        return !isCommandPaletteOpen && !workspaceInteractionBlocked(model: model)
            && commandDescriptor(command, model: model).unavailableReason == nil
    }
    /// One gate for document menu commands, focused actions and the tab key monitor.
    @discardableResult func dispatchWorkspaceAction(_ action: () -> Void) -> Bool {
        guard !isCommandPaletteOpen else { return false }
        action()
        return true
    }
    @discardableResult func dispatch(_ command: WorkspaceCommand, model: WireboltModel) -> Bool {
        if command == .settings {
            let focus = commandPalette?.capturedFocus
            commandPalette?.dismiss(restoringFocus: false)
            presentAppSettings(model, focus: focus)
            if let window = WorkspaceWindowRegistry.primary { window.makeKeyAndOrderFront(nil) }
            else { openAuxiliaryWindow?("workspace") }
            return true
        }
        if command == .help {
            commandPalette?.dismiss(restoringFocus: true)
            openAuxiliaryWindow?(helpWindowID)
            return true
        }
        if command == .commandPalette {
            guard commandIsAvailable(command, model: model) else { return false }
            toggleCommandPalette(model: model, in: WorkspaceWindowRegistry.primary)
            return true
        }
        guard commandIsAvailable(command, model: model),
              let window = WorkspaceWindowRegistry.primary, isWorkspaceWindowActive(window),
              window.attachedSheet == nil, NSApp.modalWindow == nil else { return false }
        return dispatchWorkspaceAction {
            switch command {
            case .newHttp: makeNewRequest(model: model)
            case .newWebSocket: makeNewRequest(model: model, kind: .webSocket)
            case .save: Task { _ = await saveCommand(model: model) }
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
                if let tab = model.sessions.activeSession { openInNewSplit(tabID: tab.id, model: model) }
            case .requestParams: requestSection = .params; focusRequestContentTrigger += 1
            case .requestHeaders: requestSection = .headers; focusRequestContentTrigger += 1
            case .requestBody: requestSection = .body; focusRequestContentTrigger += 1
            case .requestAuth: requestSection = .auth; focusRequestContentTrigger += 1
            case .requestNote: requestSection = .note; focusRequestContentTrigger += 1
            case .requestSettings: requestSection = .settings; focusRequestContentTrigger += 1
            case .responseHeaders: responseSection = .headers; focusResponseTrigger += 1
            case .responseBody: responseSection = .body; focusResponseTrigger += 1
            case .responseCookies: responseSection = .cookies; focusResponseTrigger += 1
            case .responseRaw: responseSection = .raw; focusResponseTrigger += 1
            case .responseRequest: responseSection = .request; focusResponseTrigger += 1
            case .workspaceSettings: model.isShowingWorkspaceSettings = true
            case .environmentInspector: focusEnvironmentInspector()
            case .switchEnvironment, .changeTheme: toggleCommandPalette(model: model)
            default: break
            }
        }
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
    @discardableResult func dismissCommandPaletteForClose(in window: NSWindow? = NSApp.keyWindow) -> Bool {
        guard let commandPalette, commandPalette.owns(window) else { return false }
        commandPalette.dismiss(restoringFocus: true)
        return true
    }
}

struct WorkspaceCommandButton: View {
    let command: WorkspaceCommand
    let model: WireboltModel
    let interface: WorkspaceUIState
    var disabled = false
    var body: some View {
        let descriptor = interface.commandDescriptor(command, model: model)
        Group {
            if let shortcut = descriptor.shortcut {
                button(descriptor).keyboardShortcut(key(shortcut.key), modifiers: modifiers(shortcut.modifiers))
            } else { button(descriptor) }
        }
        .disabled(disabled || !interface.commandIsAvailable(command, model: model))
    }
    private func button(_ descriptor: CommandDescriptor) -> some View {
        Button(descriptor.title) { interface.dispatch(command, model: model) }
            .help(descriptor.unavailableReason ?? descriptor.title)
    }
    private func key(_ key: ShortcutKey) -> KeyEquivalent {
        switch key {
        case .character(let value): KeyEquivalent(value.first!)
        case .leftArrow: .leftArrow
        case .rightArrow: .rightArrow
        }
    }
    private func modifiers(_ values: [ShortcutModifier]) -> EventModifiers {
        values.reduce(into: EventModifiers()) { result, value in
            switch value {
            case .command: result.insert(.command)
            case .shift: result.insert(.shift)
            case .option: result.insert(.option)
            case .control: result.insert(.control)
            }
        }
    }
}
