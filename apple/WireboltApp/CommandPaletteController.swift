import AppKit
import Observation
import SwiftUI

/// AppKit responder identity and UTF-16 selection; never retains document content.
@MainActor final class WorkspaceFocusCapture {
    weak var window: NSWindow?
    weak var previous: NSResponder?
    var selection: NSRange?
    init(window: NSWindow) {
        self.window = window
        let responder = window.firstResponder
        if let editor = responder as? NSTextView {
            previous = editor.isFieldEditor ? editor.delegate as? NSResponder : editor
            selection = editor.selectedRange()
        } else { previous = responder }
    }
    func restore() -> Bool {
        guard let window, window.attachedSheet == nil, let previous,
              let view = previous as? NSView, view.window === window else { return false }
        window.contentView?.layoutSubtreeIfNeeded()
        guard window.makeFirstResponder(previous) else { return false }
        if let selection, let editor = window.firstResponder as? NSTextView {
            let length = (editor.string as NSString).length
            let start = min(selection.location, length)
            editor.setSelectedRange(NSRange(location: start, length: min(selection.length, length - start)))
        }
        return true
    }
}

@MainActor @Observable
final class CommandPaletteController {
    enum Presentation { case closed, opening, presented, executing }
    private(set) var presentation = Presentation.closed
    var isOpen: Bool { presentation != .closed }
    var blocksWorkspace: Bool { presentation == .presented || presentation == .executing }
    let projection = CommandPaletteProjection()
    private(set) var failure: String?
    @ObservationIgnored private weak var model: WireboltModel?
    @ObservationIgnored private weak var interface: WorkspaceUIState?
    @ObservationIgnored private weak var window: NSWindow?
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let themes: ThemeStore
    @ObservationIgnored private var origin: WorkspaceFocusCapture?
    @ObservationIgnored private var epoch: UInt64 = 0
    @ObservationIgnored private var originTabID: String?
    @ObservationIgnored private var originGroupID: String?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var monitor: Any?
    @ObservationIgnored private var observers: [NSObjectProtocol] = []

    init(model: WireboltModel, interface: WorkspaceUIState, defaults: UserDefaults, themes: ThemeStore = .shared) {
        self.model = model; self.interface = interface; self.defaults = defaults; self.themes = themes
    }

    func canOpen(in window: NSWindow?) -> Bool {
        guard let model, let interface, let window,
              window === WorkspaceWindowRegistry.primary, interface.isWorkspaceWindowActive(window),
              model.hasLoadedWorkspace, !interface.workspaceInteractionBlocked(model: model),
              !interface.isRenamingInSidebar, window.attachedSheet == nil, NSApp.modalWindow == nil else { return false }
        return true
    }

    func toggle(in window: NSWindow?) {
        if isOpen { dismiss(restoringFocus: true); return }
        guard canOpen(in: window), let model, let window else { return }
        // IME owns marked text; do not end composition to open a modal.
        guard (window.firstResponder as? NSTextView)?.hasMarkedText() != true else { return }
        let captured = WorkspaceFocusCapture(window: window)
        guard window.makeFirstResponder(nil) else { return }
        self.window = window; origin = captured
        epoch = model.workspaceEpoch
        originTabID = model.sessions.activeSession?.id
        originGroupID = model.sessions.activeGroupID
        presentation = .opening
        generation += 1
        let token = generation
        Task {
            guard await model.environmentEditing.flush() else {
                guard generation == token else { return }
                dismiss(restoringFocus: false)
                interface?.focusEnvironmentInspector()
                return
            }
            guard generation == token, presentation == .opening,
                  model.workspaceEpoch == epoch, canOpen(in: window) else {
                if generation == token { dismiss(restoringFocus: true) }
                return
            }
            updateMetadata()
            projection.open()
            failure = nil
            presentation = .presented
            installIsolation(in: window)
        }
    }

    private func installIsolation(in window: NSWindow) {
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            MainActor.assumeIsolated { self?.handleKey(event) == true } ? nil : event
        }
        for name in [NSWindow.willCloseNotification, NSWindow.willBeginSheetNotification] {
            observers.append(NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.dismiss(restoringFocus: false) }
            })
        }
    }

    /// Also called by the application tab monitor. No synthetic global input is needed.
    func handleKey(_ event: NSEvent) -> Bool {
        guard isOpen, event.window === window else { return false }
        let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
        if modifiers == .control || modifiers == [.control, .shift], event.keyCode == 48 { return true }
        if modifiers == .command, event.charactersIgnoringModifiers?.lowercased() == "w" {
            dismiss(restoringFocus: true); return true
        }
        // ⌘K is registered solely in Navigate. Let the menu dispatch it.
        if modifiers.contains(.command) || modifiers.contains(.control) || modifiers.contains(.option) { return false }
        if (window?.firstResponder as? NSTextView)?.hasMarkedText() == true { return false }
        switch event.keyCode {
        case 125: projection.move(1); announceSelection(); return true
        case 126: projection.move(-1); announceSelection(); return true
        case 36, 76: confirm(); return true
        case 53: backOrDismiss(); return true
        case 51 where projection.snapshot?.query.isEmpty == true && projection.snapshot?.page != .root:
            backOrDismiss(); return true
        default: return false
        }
    }

    func updateMetadata() {
        guard let model, let interface else { return }
        var context = model.workspaceCommandContext
        context.sidebarHidden = interface.columnVisibility == .detailOnly
        let choices = [PaletteChoice(id: ThemeSelection.system.rawValue, name: "Follow System")]
            + ThemePreset.allCases.map { PaletteChoice(id: ThemeSelection.builtIn(preset: $0).rawValue, name: $0.name) }
            + themes.customThemes.map { PaletteChoice(id: ThemeSelection.custom(id: $0.id).rawValue, name: $0.name) }
        projection.update(model: model, context: context, themes: choices,
            appearance: ThemeSelection(rawValue: defaults.string(forKey: "interfaceAppearance") ?? "").rawValue)
    }

    func query(_ query: String, filter: PaletteFilter? = nil) {
        guard presentation == .presented else { return }
        failure = nil
        projection.query(query, filter: filter ?? projection.snapshot?.filter ?? .all)
        announceSelection()
    }
    func highlight(_ id: String) { projection.select(id); announceSelection() }
    func backOrDismiss() {
        failure = nil
        if !projection.back() { dismiss(restoringFocus: true) }
        else { announceSelection() }
    }
    func confirm(_ id: String? = nil) {
        guard presentation == .presented, let model, let interface,
              model.workspaceEpoch == epoch, let window, canOpen(in: window),
              let id = id ?? projection.snapshot?.selectedId else { return }
        updateMetadata()
        guard let result = projection.rust.resolve(id: id) else {
            failure = "This destination is no longer available."; projection.refresh(); return
        }
        if let reason = result.unavailableReason { failure = reason; projection.refresh(); return }
        switch result.destination {
        case .command(let command) where command == .switchEnvironment || command == .changeTheme:
            projection.enter(command == .switchEnvironment ? .environments : .themes)
            announceSelection()
        case .environment(let environmentID):
            presentation = .executing
            let token = generation
            Task {
                guard await model.environmentEditing.flush() else {
                    guard generation == token else { return }
                    dismiss(restoringFocus: false)
                    interface.focusEnvironmentInspector()
                    return
                }
                guard generation == token, model.workspaceEpoch == epoch,
                      environmentID == nil || model.workspace.selectableEnvironments.contains(where: { $0.id == environmentID }) else {
                    if generation == token { presentation = .presented; failure = "This environment is no longer available." }
                    return
                }
                model.selectedEnvironmentID = environmentID
                dismiss(restoringFocus: true)
            }
        case .theme(let id):
            presentation = .executing
            defaults.set(id, forKey: "interfaceAppearance")
            dismiss(restoringFocus: true)
        case .tab(let groupID, let tabID):
            guard model.sessions.groups.contains(where: { $0.id == groupID && $0.tabIDs.contains(tabID) }) else {
                failure = "This tab is no longer open."; return
            }
            dismiss(restoringFocus: false)
            interface.activateTab(id: tabID, groupID: groupID, model: model)
            interface.focusURLTrigger += 1
        case .request(let collectionID, let requestID):
            guard let location = model.workspace.collections.first(where: { $0.id == collectionID })?.requests.first(where: { $0.request.id == requestID }) else {
                failure = "This request is no longer available."; return
            }
            let target = projection.rust.requestTab(collection: collectionID, request: requestID)
            dismiss(restoringFocus: false)
            if case .tab(let groupID, let tabID) = target {
                interface.activateTab(id: tabID, groupID: groupID, model: model)
                model.sessions.pin(tabID: tabID)
            } else { interface.activateSavedRequest(location, model: model, preview: false) }
            interface.focusURLTrigger += 1
        case .command(let command) where command == .newHttp || command == .newWebSocket:
            presentation = .executing
            let token = generation
            Task {
                let session = await interface.prepareNewRequest(model: model, kind: command == .newHttp ? .http : .webSocket)
                guard generation == token, model.workspaceEpoch == epoch else { return }
                if let session {
                    dismiss(restoringFocus: false)
                    interface.finishNewRequest(session)
                } else {
                    model.operationFailure = nil
                    failure = "The request could not be created. Check the workspace and try again."
                    presentation = .presented
                }
            }
        case .command(.save):
            presentation = .executing
            let token = generation
            Task {
                let saved = await interface.saveCommand(model: model)
                guard generation == token, model.workspaceEpoch == epoch else { return }
                if saved { dismiss(restoringFocus: true) }
                else {
                    model.operationFailure = nil
                    failure = "The request could not be saved. Check the workspace and try again."
                    presentation = .presented
                }
            }
        case .command(let command):
            let captured = origin
            dismiss(restoringFocus: false)
            if command == .settings { presentAppSettings(model, focus: captured) }
            else { interface.dispatch(command, model: model) }
        }
    }

    func dismiss(restoringFocus: Bool) {
        guard isOpen else { return }
        let captured = origin
        let expectedEpoch = epoch, tab = originTabID, group = originGroupID
        generation += 1
        let token = generation
        presentation = .closed
        projection.clear(); failure = nil; origin = nil
        if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
        observers.forEach { NotificationCenter.default.removeObserver($0) }; observers.removeAll()
        guard restoringFocus else { return }
        DispatchQueue.main.async { [weak self] in
            guard let self, generation == token, !isOpen, let model, let interface,
                  model.workspaceEpoch == expectedEpoch, !interface.workspaceInteractionBlocked(model: model) else { return }
            let sameDocument = model.sessions.activeSession?.id == tab && model.sessions.activeGroupID == group
            if sameDocument, captured?.restore() == true { return }
            if model.sessions.activeSession != nil { interface.focusURLTrigger += 1 }
            else { interface.focusSidebar() }
        }
    }
    func announceSelection() {
        guard let window, let snapshot = projection.snapshot else { return }
        let result = snapshot.results.first { $0.id == snapshot.selectedId }
        let message = result.map { [$0.title, $0.subtitle, $0.current ? "Current" : "", $0.unavailableReason ?? ""].filter { !$0.isEmpty }.joined(separator: ", ") } ?? "No results"
        NSAccessibility.post(element: window, notification: .announcementRequested,
            userInfo: [.announcement: message, .priority: NSAccessibilityPriorityLevel.medium.rawValue])
    }
}
