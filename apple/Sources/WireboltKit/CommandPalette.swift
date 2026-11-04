import Foundation
import Observation

/// Fine-grained native observation over Rust's ephemeral query, selection and back stack.
@MainActor @Observable
public final class CommandPaletteProjection {
    public private(set) var snapshot: PaletteSnapshot?
    @ObservationIgnored public let rust = AppCommandPalette()
    @ObservationIgnored private var workspaceRevision: (epoch: UInt64, collections: UInt64)?

    public init() {}

    public func update(model: WireboltModel, context: CommandContext, themes: [PaletteTheme], appearance: String) {
        if workspaceRevision?.epoch != model.workspaceEpoch || workspaceRevision?.collections != model.workspace.collectionsRevision {
            rust.replaceWorkspace(collections: model.workspace.collections.map { collection in
                PaletteCollection(id: collection.id, name: collection.name, groups: collection.groups.map(\.domain),
                    requests: collection.requests.map { location in
                        PaletteRequest(id: location.request.id, name: location.request.name,
                            method: location.request.method.rawValue, webSocket: location.request.webSocket, groupId: location.groupID)
                    })
            })
            workspaceRevision = (model.workspaceEpoch, model.workspace.collectionsRevision)
        }
        rust.updateTabs(tabs: model.sessions.groups.enumerated().flatMap { index, group in
            group.tabIDs.compactMap { id -> PaletteTab? in
                guard let session = model.sessions.session(id: id) else { return nil }
                return PaletteTab(groupId: group.id, tabId: id, collectionId: session.collectionID,
                    requestId: session.requestID, title: session.title, method: session.draft.method.rawValue,
                    webSocket: session.kind == .webSocket, groupIndex: UInt64(index))
            }
        }, activeGroup: model.sessions.activeGroupID, groupCount: UInt64(model.sessions.groups.count))
        rust.updateChoices(environments: model.workspace.selectableEnvironments.map { PaletteChoice(id: $0.id, name: $0.name) },
            themes: themes, selectedEnvironment: model.selectedEnvironmentID.map { .environment(id: $0) } ?? .globals, selectedTheme: appearance)
        rust.setContext(context: context)
    }
    public func open() { snapshot = rust.open() }
    public func refresh() { snapshot = rust.refresh() }
    public func query(_ query: String, filter: PaletteFilter) { snapshot = rust.query(query: query, filter: filter) }
    public func move(_ delta: Int) { snapshot = rust.moveSelection(delta: Int64(delta)) }
    public func select(_ id: String) { snapshot = rust.select(id: id) }
    public func enter(_ page: PalettePage) { snapshot = rust.enter(page: page) }
    @discardableResult public func back() -> Bool {
        guard let parent = rust.back() else { return false }
        snapshot = parent
        return true
    }
    public func clear() { snapshot = nil }
}

extension WireboltModel {
    /// The single bounded context used by menus, the palette and confirmation dispatch.
    public func workspaceCommandContext(sidebarHidden: Bool, canOpenPalette: Bool) -> CommandContext {
        let session = sessions.activeSession
        return CommandContext(hasSession: session != nil, webSocket: session?.kind == .webSocket,
            hasName: session.map { !$0.draft.name.isEmpty } ?? false, hasCollections: !workspace.collections.isEmpty,
            canGoBack: sessions.activeGroup.map { !$0.backwardTabIDs.isEmpty } ?? false,
            canGoForward: sessions.activeGroup.map { !$0.forwardTabIDs.isEmpty } ?? false,
            tabCount: UInt64(sessions.activeGroup?.tabIDs.count ?? 0), sidebarHidden: sidebarHidden, canOpenPalette: canOpenPalette)
    }
}
