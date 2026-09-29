import Foundation
import os

enum PerformanceProbe {
    private static let log = OSLog(
        subsystem: "io.github.christopher96u.wirebolt",
        category: .pointsOfInterest
    )

    static func beginLaunch() {
        os_signpost(.begin, log: log, name: "Launch")
    }

    static func markReady() {
        os_signpost(.end, log: log, name: "Launch")
        guard let path = ProcessInfo.processInfo.environment["WIREBOLT_READY_FILE"] else {
            return
        }

        _ = FileManager.default.createFile(atPath: path, contents: Data([1]))
    }

    static func beginWorkspaceLoad() {
        os_signpost(.begin, log: log, name: "Workspace Load")
    }

    static func endWorkspaceLoad() {
        os_signpost(.end, log: log, name: "Workspace Load")
    }

    static func tabSwitched() {
        os_signpost(.event, log: log, name: "Tab Switch")
    }

    static func beginPrepare(runID: String) {
        os_signpost(.begin, log: log, name: "Prepare", "%{public}s", runID)
    }

    static func endPrepare(runID: String) {
        os_signpost(.end, log: log, name: "Prepare", "%{public}s", runID)
    }

    static func dispatched(runID: String) {
        os_signpost(.event, log: log, name: "Dispatch", "%{public}s", runID)
    }

    static func firstHead(runID: String) {
        os_signpost(.event, log: log, name: "First Head", "%{public}s", runID)
    }

    static func firstViewport(runID: String) {
        os_signpost(.event, log: log, name: "First Viewport", "%{public}s", runID)
    }

    static func rendererReady(_ renderer: String) {
        os_signpost(.event, log: log, name: "Renderer Ready", "%{public}s", renderer)
    }
}
