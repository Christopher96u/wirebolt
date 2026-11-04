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
        guard let window, window.attachedSheet == nil else { return false }
        window.contentView?.layoutSubtreeIfNeeded()
        guard let previous, let view = previous as? NSView, view.window === window,
              !view.isHiddenOrHasHiddenAncestor, !view.visibleRect.isEmpty else { return false }
        guard window.makeFirstResponder(previous) else { return false }
        if let selection, let editor = window.firstResponder as? NSTextView {
            let length = (editor.string as NSString).length
            let start = min(selection.location, length)
            editor.setSelectedRange(NSRange(location: start, length: min(selection.length, length - start)))
        }
        return true
    }
}

private enum PaletteOriginSurface { case native, sidebar }

@MainActor private struct PaletteOrigin {
    let focus: WorkspaceFocusCapture
    let epoch: UInt64
    let tabID: String?
    let groupID: String
    let surface: PaletteOriginSurface
}

private enum PaletteKeyCode {
    static let down: UInt16 = 125, up: UInt16 = 126, returnKey: UInt16 = 36, enter: UInt16 = 76
    static let escape: UInt16 = 53, delete: UInt16 = 51, tab: UInt16 = 48
}

@MainActor @Observable
final class CommandPaletteController {
    enum Presentation { case closed, opening, presented, executing }
    private(set) var presentation = Presentation.closed
    var isOpen: Bool { presentation != .closed }
    var blocksWorkspace: Bool { presentation == .presented || presentation == .executing }
    let projection: CommandPaletteProjection
    private(set) var failureReason: PaletteFailure?
    var failure: String? { failureReason.map { domainPaletteFailureMessage(reason: $0) } }
    @ObservationIgnored private weak var model: WireboltModel?
    @ObservationIgnored private weak var interface: WorkspaceUIState?
    @ObservationIgnored private weak var window: NSWindow?
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let themes: ThemeStore
    @ObservationIgnored private var origin: PaletteOrigin?
    var capturedFocus: WorkspaceFocusCapture? { origin?.focus }
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var monitor: Any?
    @ObservationIgnored private var observers: [NSObjectProtocol] = []

    init(model: WireboltModel, interface: WorkspaceUIState, defaults: UserDefaults, themes: ThemeStore = .shared) {
        self.model = model; self.interface = interface; self.defaults = defaults; self.themes = themes
        projection = model.commandPaletteProjection()
    }

    func owns(_ window: NSWindow?) -> Bool { isOpen && self.window === window }

    func canOpen(in window: NSWindow?) -> Bool {
        guard let model, let interface else { return false }
        return interface.canOpenCommandPalette(model: model, in: window)
    }

    private func isCurrent(_ token: Int) -> Bool {
        generation == token && model?.workspaceEpoch == origin?.epoch
    }

    func toggle(in window: NSWindow?) {
        if isOpen { dismiss(restoringFocus: true); return }
        guard canOpen(in: window), let model, let window else { return }
        // IME owns marked text; do not end composition to open a modal.
        guard (window.firstResponder as? NSTextView)?.hasMarkedText() != true else { return }
        let captured = WorkspaceFocusCapture(window: window)
        let surface: PaletteOriginSurface = interface?.sidebarHasFocus == true ? .sidebar : .native
        guard window.makeFirstResponder(nil) else { return }
        self.window = window
        origin = PaletteOrigin(focus: captured, epoch: model.workspaceEpoch, tabID: model.sessions.activeSession?.id, groupID: model.sessions.activeGroupID, surface: surface)
        presentation = .opening
        generation += 1
        let token = generation
        Task {
            guard await model.environmentEditing.flush() else {
                guard isCurrent(token) else { return }
                dismiss(restoringFocus: false)
                interface?.focusEnvironmentInspector()
                return
            }
            guard isCurrent(token), presentation == .opening,
                  canOpen(in: window) else {
                if isCurrent(token) { dismiss(restoringFocus: true) }
                return
            }
            updateMetadata()
            projection.open()
            failureReason = nil
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
        if modifiers == .control || modifiers == [.control, .shift], event.keyCode == PaletteKeyCode.tab { return true }
        if modifiers == .command, event.charactersIgnoringModifiers?.lowercased() == "w" {
            dismiss(restoringFocus: true); return true
        }
        if modifiers == [.command, .shift], event.charactersIgnoringModifiers?.lowercased() == "w",
           let model, let interface, let window {
            requestWorkspaceWindowClose(window, model: model, interface: interface)
            return true
        }
        // Option composition and Control editing belong to the native search field.
        if !modifiers.intersection([.control, .option]).isEmpty && !modifiers.contains(.command) { return false }
        // Palette, window, app and native search-editing shortcuts retain their owners.
        if modifiers.contains(.command) {
            let key = event.charactersIgnoringModifiers?.lowercased() ?? ""
            return !(modifiers == .command && ["k", "q", ",", "?", "a", "c", "v", "x", "z"].contains(key)
                || modifiers == [.command, .shift] && ["?", "z"].contains(key))
        }
        if (window?.firstResponder as? NSTextView)?.hasMarkedText() == true { return false }
        guard presentation == .presented else { return true }
        switch event.keyCode {
        case PaletteKeyCode.down: projection.move(1); announceSelection(); return true
        case PaletteKeyCode.up: projection.move(-1); announceSelection(); return true
        case PaletteKeyCode.returnKey, PaletteKeyCode.enter: confirm(); return true
        case PaletteKeyCode.escape: backOrDismiss(); return true
        case PaletteKeyCode.delete where projection.snapshot?.query.isEmpty == true && projection.snapshot?.page != .root:
            backOrDismiss(); return true
        default: return false
        }
    }

    func updateMetadata() {
        guard let model, let interface else { return }
        let context = interface.commandContext(model: model)
        let choices = [PaletteTheme(id: ThemeSelection.system.rawValue, name: "Follow System", kind: .builtIn)]
            + ThemePreset.allCases.map { PaletteTheme(id: ThemeSelection.builtIn(preset: $0).rawValue, name: $0.name, kind: .builtIn) }
            + themes.customThemes.map { PaletteTheme(id: ThemeSelection.custom(id: $0.id).rawValue, name: $0.name, kind: .custom) }
        projection.update(model: model, context: context, themes: choices,
            appearance: ThemeSelection(rawValue: defaults.string(forKey: "interfaceAppearance") ?? "").rawValue)
    }

    func query(_ query: String, filter: PaletteFilter? = nil) {
        guard presentation == .presented else { return }
        failureReason = nil
        projection.query(query, filter: filter ?? projection.snapshot?.filter ?? .all)
        announceSelection()
    }
    func highlight(_ id: String) { guard presentation == .presented else { return }; projection.select(id); announceSelection() }
    func backOrDismiss() {
        guard presentation == .presented else { return }
        failureReason = nil
        if !projection.back() { dismiss(restoringFocus: true) }
        else { announceSelection() }
    }
    func confirm(_ id: String? = nil) {
        guard presentation == .presented, let model, let interface,
              model.workspaceEpoch == origin?.epoch, let window, canOpen(in: window),
              let id = id ?? projection.snapshot?.selectedId else { return }
        updateMetadata()
        let result: PaletteResult
        switch projection.rust.resolve(id: id) {
        case .ready(let resolved): result = resolved
        case .failed(let reason): failureReason = reason; projection.refresh(); return
        }
        switch result.destination {
        case .command(let command) where command == .switchEnvironment || command == .changeTheme:
            projection.enter(command == .switchEnvironment ? .environments : .themes)
            announceSelection()
        case .environment:
            presentation = .executing
            let token = generation
            Task {
                guard await model.environmentEditing.flush() else {
                    guard isCurrent(token) else { return }
                    dismiss(restoringFocus: false)
                    interface.focusEnvironmentInspector()
                    return
                }
                guard isCurrent(token) else { return }
                updateMetadata()
                switch projection.rust.resolve(id: id) {
                case .ready(let current):
                    if case .environment(let selection) = current.destination {
                        switch selection {
                        case .globals: model.selectedEnvironmentID = nil
                        case .environment(let environmentID): model.selectedEnvironmentID = environmentID
                        }
                    }
                case .failed(let reason): presentation = .presented; failureReason = reason; projection.refresh(); return
                }
                dismiss(restoringFocus: true)
            }
        case .theme(let id):
            presentation = .executing
            defaults.set(id, forKey: "interfaceAppearance")
            dismiss(restoringFocus: true)
        case .tab(let groupID, let tabID):
            dismiss(restoringFocus: false)
            interface.activateTab(id: tabID, groupID: groupID, model: model)
            interface.focusURLTrigger += 1
        case .request(let collectionID, let requestID):
            // Rust resolved against the synchronous projection of this exact workspace.
            let location = model.workspace.location(collectionID: collectionID, requestID: requestID)!
            dismiss(restoringFocus: false)
            if let target = result.openTab {
                interface.activateTab(id: target.tabId, groupID: target.groupId, model: model)
                model.sessions.pin(tabID: target.tabId)
            } else { interface.activateSavedRequest(location, model: model, preview: false) }
            interface.focusURLTrigger += 1
        case .command(let command) where command == .newHttp || command == .newWebSocket:
            presentation = .executing
            let token = generation
            Task {
                let session = await interface.prepareNewRequest(model: model, kind: command == .newHttp ? .http : .webSocket)
                guard isCurrent(token) else { return }
                if let session {
                    dismiss(restoringFocus: false)
                    interface.finishNewRequest(session)
                } else {
                    model.operationFailure = nil
                    failureReason = .createFailed
                    presentation = .presented
                }
            }
        case .command(.save):
            presentation = .executing
            let token = generation
            Task {
                let saved = await interface.saveCommand(model: model)
                guard isCurrent(token) else { return }
                if saved { dismiss(restoringFocus: true) }
                else {
                    model.operationFailure = nil
                    failureReason = .saveFailed
                    presentation = .presented
                }
            }
        case .command(let command):
            let captured = origin
            let restoresOrigin = [.toggleSidebar, .back, .forward, .nextTab, .previousTab].contains(command)
            dismiss(restoringFocus: restoresOrigin)
            if command == .settings { presentAppSettings(model, focus: captured?.focus) }
            else { interface.dispatch(command, model: model) }
        }
    }

    func dismiss(restoringFocus: Bool) {
        guard isOpen else { return }
        let captured = origin
        let expectedEpoch = captured?.epoch, tab = captured?.tabID, group = captured?.groupID
        generation += 1
        let token = generation
        presentation = .closed
        projection.clear(); failureReason = nil; origin = nil
        if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
        observers.forEach { NotificationCenter.default.removeObserver($0) }; observers.removeAll()
        guard restoringFocus else { return }
        DispatchQueue.main.async { [weak self] in
            guard let self, generation == token, !isOpen, let model, let interface,
                  model.workspaceEpoch == expectedEpoch, !interface.workspaceInteractionBlocked(model: model) else { return }
            let sameDocument = model.sessions.activeSession?.id == tab && model.sessions.activeGroupID == group
            if sameDocument {
                if captured?.surface == .sidebar {
                    if interface.columnVisibility != .detailOnly { interface.focusSidebar(); return }
                } else if captured?.focus.restore() == true { return }
            }
            if model.sessions.activeSession != nil { interface.focusURLTrigger += 1 }
            else { interface.focusSidebar() }
        }
    }
    func announceSelection() {
        guard let window, let snapshot = projection.snapshot else { return }
        let result = snapshot.results.first { $0.id == snapshot.selectedId }
        let message = result?.accessibilityLabel ?? "No results"
        NSAccessibility.post(element: window, notification: .announcementRequested,
            userInfo: [.announcement: message, .priority: NSAccessibilityPriorityLevel.medium.rawValue])
    }
}
