import Foundation
import os

enum PerformanceProbe {
    // The launch harness probes this before starting its timer, and the timed
    // process checks again before constructing the App or reading preferences.
    static func validateLaunchIsolation() {
        let environment = ProcessInfo.processInfo.environment
        guard let root = environment["WIREBOLT_LAUNCH_ISOLATION_ROOT"] else { return }
        let home = URL(fileURLWithPath: root).appendingPathComponent("home").standardizedFileURL
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let domain = environment["WIREBOLT_LAUNCH_DEFAULTS_DOMAIN"] ?? ""
        precondition(environment["HOME"] == home.path && environment["CFFIXED_USER_HOME"] == home.path)
        precondition(URL(fileURLWithPath: NSHomeDirectory()).standardizedFileURL == home)
        precondition(FileManager.default.homeDirectoryForCurrentUser.standardizedFileURL == home)
        precondition(support.standardizedFileURL.path.hasPrefix(home.path + "/"))
        precondition(domain.hasPrefix("io.github.christopher96u.wirebolt.performance."))
        precondition(Bundle.main.bundleIdentifier == domain)
        if let report = environment["WIREBOLT_ISOLATION_PROBE_FILE"] {
            let payload = ["home": home.path, "applicationSupport": support.path, "defaultsDomain": domain]
            do {
                try JSONSerialization.data(withJSONObject: payload).write(to: URL(fileURLWithPath: report))
                exit(0)
            } catch { exit(1) }
        }
    }

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
