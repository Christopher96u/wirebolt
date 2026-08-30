import Foundation

enum PerformanceProbe {
    static func markReady() {
        guard let path = ProcessInfo.processInfo.environment["WIREBOLT_READY_FILE"] else {
            return
        }

        _ = FileManager.default.createFile(atPath: path, contents: Data([1]))
    }
}
