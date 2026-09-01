import Foundation
import Observation

public struct RunID: Codable, CustomStringConvertible, Hashable, Sendable {
    public let rawValue: UUID

    public init(rawValue: UUID = UUID()) {
        self.rawValue = rawValue
    }

    public var description: String { rawValue.uuidString.lowercased() }
}

public enum DocumentKind: String, Codable, Equatable, Sendable {
    case http
    case webSocket = "web_socket"
}

@MainActor
@Observable
public final class DocumentSession: Identifiable {
    public static let previewByteLimit = ResponseBodyStore.viewportByteCount

    public let id: String
    public let kind: DocumentKind
    public let collectionID: String?
    public let requestID: String
    public var draft: RequestDraft
    public var note: String
    public private(set) var savedDraft: RequestDraft?
    public private(set) var responseHead: ResponseHead?
    public private(set) var preparedRun: PreparedRunSnapshot?
    public private(set) var responseText = ""
    public private(set) var responsePreviewData = Data()
    public private(set) var responseBytes: UInt64 = 0
    public private(set) var responseWasTruncated = false
    public private(set) var completion: RunCompletion?
    public private(set) var failure: RunFailure?
    public private(set) var activeRunID: RunID?
    public private(set) var bodyStore: ResponseBodyStore?
    public private(set) var responseCookies: [CookieSnapshot] = []

    @ObservationIgnored private var presentedBodyBytes = 0

    public init(
        id: String = UUID().uuidString.lowercased(),
        kind: DocumentKind = .http,
        collectionID: String? = nil,
        requestID: String? = nil,
        draft: RequestDraft,
        note: String = "",
        savedDraft: RequestDraft? = nil
    ) {
        self.id = id
        self.kind = kind
        self.collectionID = collectionID
        self.requestID = requestID ?? draft.id
        self.draft = draft
        self.note = note
        self.savedDraft = savedDraft
    }

    public var title: String { draft.name }
    public var isDirty: Bool { savedDraft != draft }
    public var isRunning: Bool { activeRunID != nil }

    public func beginRun(_ runID: RunID) {
        resetResponse()
        bodyStore = try? ResponseBodyStore(runID: runID)
        activeRunID = runID
    }

    public func consume(_ event: RunEvent, runID: RunID) async {
        guard activeRunID == runID else { return }
        switch event {
        case let .prepared(snapshot):
            preparedRun = snapshot
        case let .head(head):
            responseHead = head
        case let .chunk(data):
            try? await bodyStore?.append(data)
            responseBytes += UInt64(data.count)
            let remaining = max(Self.previewByteLimit - presentedBodyBytes, 0)
            guard remaining > 0 else {
                responseWasTruncated = true
                return
            }
            let prefix = data.prefix(remaining)
            responsePreviewData.append(prefix)
            responseText.append(String(decoding: prefix, as: UTF8.self))
            presentedBodyBytes += prefix.count
            responseWasTruncated = prefix.count < data.count
        case let .complete(value):
            try? await bodyStore?.finish()
            completion = value
            responseBytes = value.bytesReceived
            activeRunID = nil
        }
    }

    public func finish(runID: RunID, failure: RunFailure?) async {
        guard activeRunID == runID else { return }
        try? await bodyStore?.finish()
        self.failure = failure
        activeRunID = nil
    }

    public func cancel(runID: RunID) {
        guard activeRunID == runID else { return }
        activeRunID = nil
    }

    public func markSaved(_ draft: RequestDraft) {
        self.draft = draft
        savedDraft = draft
    }

    public func resetResponse() {
        if let bodyStore { Task { await bodyStore.remove() } }
        responseHead = nil
        preparedRun = nil
        responseText = ""
        responsePreviewData = Data()
        responseBytes = 0
        responseWasTruncated = false
        completion = nil
        failure = nil
        bodyStore = nil
        responseCookies = []
        presentedBodyBytes = 0
    }

    public func setResponseCookies(_ cookies: [CookieSnapshot]) {
        responseCookies = cookies
    }

    public func restore(_ entry: RunHistoryEntry, viewport: Data) {
        preparedRun = entry.prepared
        responseHead = entry.responseHead
        completion = entry.completion
        responseText = String(decoding: viewport, as: UTF8.self)
        responsePreviewData = viewport
        responseBytes = entry.completion?.bytesReceived ?? UInt64(viewport.count)
        responseWasTruncated = responseBytes > UInt64(viewport.count)
        activeRunID = nil
        failure = entry.failure
        bodyStore = try? ResponseBodyStore(existingURL: URL(fileURLWithPath: entry.bodyPath))
    }
}

public struct EditorGroup: Identifiable, Codable, Equatable, Sendable {
    public let id: String
    public var tabIDs: [String]
    public var selectedTabID: String?
    public var backwardTabIDs: [String]
    public var forwardTabIDs: [String]

    public init(
        id: String = UUID().uuidString.lowercased(),
        tabIDs: [String] = [],
        selectedTabID: String? = nil,
        backwardTabIDs: [String] = [],
        forwardTabIDs: [String] = []
    ) {
        self.id = id
        self.tabIDs = tabIDs
        self.selectedTabID = selectedTabID
        self.backwardTabIDs = backwardTabIDs
        self.forwardTabIDs = forwardTabIDs
    }
}

public enum TabCloseScope: Equatable, Sendable {
    case one(String)
    case others(String)
    case rightOf(String)
    case all
}

@MainActor
@Observable
public final class DocumentSessionStore {
    public private(set) var sessions: [String: DocumentSession]
    public private(set) var groups: [EditorGroup]
    public var activeGroupID: String

    public init() {
        let group = EditorGroup()
        sessions = [:]
        groups = [group]
        activeGroupID = group.id
    }

    public var activeGroup: EditorGroup? {
        groups.first(where: { $0.id == activeGroupID })
    }

    public var activeSession: DocumentSession? {
        guard let selected = activeGroup?.selectedTabID else { return nil }
        return sessions[selected]
    }

    public func session(id: String) -> DocumentSession? {
        sessions[id]
    }

    @discardableResult
    public func open(
        draft: RequestDraft,
        collectionID: String? = nil,
        kind: DocumentKind = .http,
        in groupID: String? = nil,
        forceNewSession: Bool = false,
        isSaved: Bool = true
    ) -> DocumentSession {
        let targetGroupID = groupID ?? activeGroupID
        if !forceNewSession,
           let existing = sessions.values.first(where: {
               $0.requestID == draft.id && $0.collectionID == collectionID && $0.kind == kind
           })
        {
            select(tabID: existing.id, in: targetGroupID)
            return existing
        }

        let session = DocumentSession(
            kind: kind,
            collectionID: collectionID,
            requestID: draft.id,
            draft: draft,
            savedDraft: isSaved ? draft : nil
        )
        sessions[session.id] = session
        mutateGroup(id: targetGroupID) { group in
            group.tabIDs.append(session.id)
            select(session.id, in: &group)
        }
        activeGroupID = targetGroupID
        return session
    }

    @discardableResult
    public func openTemporary(
        kind: DocumentKind = .http,
        collectionID: String? = nil
    ) -> DocumentSession {
        let method: HTTPMethod = .get
        let title = kind == .http ? "Untitled Request" : "Untitled WebSocket"
        return open(
            draft: RequestDraft(
                id: UUID().uuidString.lowercased(),
                name: title,
                method: method
            ),
            collectionID: collectionID,
            kind: kind,
            forceNewSession: true,
            isSaved: false
        )
    }

    public func select(tabID: String, in groupID: String? = nil) {
        let targetGroupID = groupID ?? activeGroupID
        guard sessions[tabID] != nil else { return }
        mutateGroup(id: targetGroupID) { group in
            guard group.tabIDs.contains(tabID) else { return }
            select(tabID, in: &group)
        }
        activeGroupID = targetGroupID
    }

    public func goBack() {
        mutateGroup(id: activeGroupID) { group in
            guard let destination = group.backwardTabIDs.popLast() else { return }
            if let selected = group.selectedTabID { group.forwardTabIDs.append(selected) }
            group.selectedTabID = destination
        }
    }

    public func goForward() {
        mutateGroup(id: activeGroupID) { group in
            guard let destination = group.forwardTabIDs.popLast() else { return }
            if let selected = group.selectedTabID { group.backwardTabIDs.append(selected) }
            group.selectedTabID = destination
        }
    }

    @discardableResult
    public func split(tabID: String) -> String? {
        guard let source = sessions[tabID] else { return nil }
        let group = EditorGroup()
        groups.append(group)
        activeGroupID = group.id
        let copy = open(
            draft: source.draft,
            collectionID: source.collectionID,
            kind: source.kind,
            in: group.id,
            forceNewSession: true
        )
        copy.note = source.note
        return group.id
    }

    @discardableResult
    public func close(
        _ scope: TabCloseScope,
        in groupID: String? = nil,
        allowDirty: Bool = false
    ) -> [DocumentSession] {
        let targetGroupID = groupID ?? activeGroupID
        guard let group = groups.first(where: { $0.id == targetGroupID }) else { return [] }
        let targetIDs: [String] = switch scope {
        case let .one(id): [id]
        case let .others(id): group.tabIDs.filter { $0 != id }
        case let .rightOf(id):
            if let index = group.tabIDs.firstIndex(of: id) {
                Array(group.tabIDs.dropFirst(index + 1))
            } else {
                []
            }
        case .all: group.tabIDs
        }
        let blocked = targetIDs.compactMap { sessions[$0] }.filter(\.isDirty)
        guard allowDirty || blocked.isEmpty else { return blocked }

        mutateGroup(id: targetGroupID) { mutable in
            let priorSelection = mutable.selectedTabID
            mutable.tabIDs.removeAll { targetIDs.contains($0) }
            mutable.backwardTabIDs.removeAll { targetIDs.contains($0) }
            mutable.forwardTabIDs.removeAll { targetIDs.contains($0) }
            if let priorSelection, targetIDs.contains(priorSelection) {
                mutable.selectedTabID = mutable.tabIDs.last
            }
        }
        removeUnreferencedSessions(targetIDs)
        removeEmptySecondaryGroups()
        return []
    }

    public func removeAll() {
        sessions.removeAll(keepingCapacity: true)
        let group = EditorGroup()
        groups = [group]
        activeGroupID = group.id
    }

    private func select(_ tabID: String, in group: inout EditorGroup) {
        guard group.selectedTabID != tabID else { return }
        if let current = group.selectedTabID { group.backwardTabIDs.append(current) }
        group.selectedTabID = tabID
        group.forwardTabIDs.removeAll(keepingCapacity: true)
    }

    private func mutateGroup(id: String, _ mutation: (inout EditorGroup) -> Void) {
        guard let index = groups.firstIndex(where: { $0.id == id }) else { return }
        mutation(&groups[index])
    }

    private func removeUnreferencedSessions(_ candidateIDs: [String]) {
        let referenced = Set(groups.flatMap(\.tabIDs))
        for id in candidateIDs where !referenced.contains(id) {
            sessions[id] = nil
        }
    }

    private func removeEmptySecondaryGroups() {
        groups.removeAll { $0.id != activeGroupID && $0.tabIDs.isEmpty }
        if groups.first(where: { $0.id == activeGroupID })?.tabIDs.isEmpty == true,
           groups.count > 1
        {
            groups.removeAll { $0.id == activeGroupID }
            activeGroupID = groups[0].id
        }
    }
}
