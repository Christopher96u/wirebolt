import AppKit
import Darwin
import Foundation
import WireboltStreamFFI

private let performanceHeadCallback: @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, UInt
) -> Void = { _, _, _ in }
private let performanceChunkCallback: @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, UInt
) -> UInt8 = { _, _, _ in 1 }
private let performanceTerminalCallback: @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, UInt
) -> Void = { _, _, _ in }

private struct PerformanceBudgets: Decodable {
    let referenceMachine: String
    let processLaunchP95Milliseconds: Double
    let idleRssP95Mebibytes: Double
    let bridgeHandshakeP95Microseconds: Double
    let requestSetupP95Microseconds: Double
    let requestDispatchP95Microseconds: Double
    let responseColdFirstViewportMilliseconds: Double
    let responseWarmFirstViewportP95Milliseconds: Double
    let responseWarmMaximumMainThreadSliceMilliseconds: Double
    let workspaceOpenFirstContentMilliseconds: Double
    let workspaceExpandCollapseP95Milliseconds: Double
    let workspaceFilterP95Milliseconds: Double
    let tabSwitchP95Milliseconds: Double
    let tabCloseRestoreP95Milliseconds: Double
    let typingFeedbackP95Milliseconds: Double
    let largeResponseColdFirstViewportMilliseconds: Double
    let largeResponseSearchP95Milliseconds: Double
    let largeResponseRendererSwitchP95Milliseconds: Double
    let largeResponseResidentDeltaMebibytes: Double
    let historyInsertP95Milliseconds: Double
    let historyRestoreP95Milliseconds: Double
    let historyEvictionP95Milliseconds: Double
    let largeImportParseP95Milliseconds: Double
}

private struct PerformanceResults: Encodable {
    let referenceMachine: String
    let measuredMachine: String
    let operatingSystem: String
    let responseBodyBytes: Int
    let responseViewportBytes: Int
    let processLaunchP50Milliseconds: Double
    let processLaunchP95Milliseconds: Double
    let idleRssP50Mebibytes: Double
    let idleRssP95Mebibytes: Double
    let bridgeHandshakeP50Microseconds: Double
    let bridgeHandshakeP95Microseconds: Double
    let requestSetupP50Microseconds: Double
    let requestSetupP95Microseconds: Double
    let requestDispatchP50Microseconds: Double
    let requestDispatchP95Microseconds: Double
    let responseColdFirstViewportMilliseconds: Double
    let responseWarmFirstViewportP50Milliseconds: Double
    let responseWarmFirstViewportP95Milliseconds: Double
    let responseWarmMaximumMainThreadSliceMilliseconds: Double
    let workspaceOpenFirstContentMilliseconds: Double
    let workspaceExpandCollapseP95Milliseconds: Double
    let workspaceFilterP95Milliseconds: Double
    let tabSwitchP95Milliseconds: Double
    let tabCloseRestoreP95Milliseconds: Double
    let typingFeedbackP95Milliseconds: Double
    let largeResponseColdFirstViewportMilliseconds: Double
    let largeResponseSearchP95Milliseconds: Double
    let largeResponseRendererSwitchP95Milliseconds: Double
    let largeResponseResidentDeltaMebibytes: Double
    let historyInsertP95Milliseconds: Double
    let historyRestoreP95Milliseconds: Double
    let historyEvictionP95Milliseconds: Double
    let largeImportParseP95Milliseconds: Double
    let raw: RawSamples
    let violations: [String]

    struct RawSamples: Encodable {
        let processLaunchMilliseconds: [Double]
        let idleRssMebibytes: [Double]
        let bridgeHandshakeMicroseconds: [Double]
        let requestSetupMicroseconds: [Double]
        let requestDispatchMicroseconds: [Double]
        let responseFirstViewportMilliseconds: [Double]
        let workspaceExpandCollapseMilliseconds: [Double]
        let workspaceFilterMilliseconds: [Double]
        let tabSwitchMilliseconds: [Double]
        let tabCloseRestoreMilliseconds: [Double]
        let typingFeedbackMilliseconds: [Double]
        let largeResponseSearchMilliseconds: [Double]
        let largeResponseRendererSwitchMilliseconds: [Double]
        let historyInsertMilliseconds: [Double]
        let historyRestoreMilliseconds: [Double]
        let historyEvictionMilliseconds: [Double]
        let largeImportParseMilliseconds: [Double]
    }
}

private struct Arguments {
    let smokeOnly: Bool
    let appExecutable: URL?
    let budgets: URL?

    static func parse() throws -> Arguments {
        var values = Array(CommandLine.arguments.dropFirst())
        if values == ["--smoke"] {
            return Arguments(smokeOnly: true, appExecutable: nil, budgets: nil)
        }

        guard values.count == 2 || values.count == 4,
              values.removeFirst() == "--app"
        else {
            throw HarnessError.usage
        }

        let appExecutable = URL(fileURLWithPath: values.removeFirst())
        var budgets: URL?
        if !values.isEmpty {
            guard values.removeFirst() == "--budgets" else {
                throw HarnessError.usage
            }
            budgets = URL(fileURLWithPath: values.removeFirst())
        }

        return Arguments(smokeOnly: false, appExecutable: appExecutable, budgets: budgets)
    }
}

private enum HarnessError: Error, CustomStringConvertible {
    case appExited(Int32)
    case appReadinessTimedOut
    case invalidBudgetFile(String)
    case invalidMeasurement(String)
    case usage

    var description: String {
        switch self {
        case let .appExited(status):
            "Wirebolt exited before becoming ready (status \(status))"
        case .appReadinessTimedOut:
            "Wirebolt did not become ready within five seconds"
        case let .invalidBudgetFile(reason):
            "invalid performance budget file: \(reason)"
        case let .invalidMeasurement(reason):
            "invalid performance measurement: \(reason)"
        case .usage:
            "usage: performance-contract --smoke | --app <executable> [--budgets <json>]"
        }
    }
}

@main
private enum PerformanceContract {
    private static let launchSamples = 20
    private static let metricSamples = 25
    private static let bridgeIterations = 10_000
    private static let requestIterations = 2_000
    private static let responseBodyBytes = 8 * 1024 * 1024

    static func main() {
        do {
            let arguments = try Arguments.parse()
            if arguments.smokeOnly {
                try smokeTest()
                print("performance_smoke=passed")
                return
            }

            guard let appExecutable = arguments.appExecutable else {
                throw HarnessError.usage
            }

            let budgets = try arguments.budgets.map(loadBudgets)
            let results = try measure(appExecutable: appExecutable, budgets: budgets)
            try printJSON(results)

            if !results.violations.isEmpty {
                exit(EXIT_FAILURE)
            }
        } catch {
            FileHandle.standardError.write(Data("error: \(error)\n".utf8))
            exit(EXIT_FAILURE)
        }
    }

    private static func smokeTest() throws {
        let handshake = coreHandshake()
        guard handshake.product == "Wirebolt" else {
            throw HarnessError.invalidMeasurement("bridge handshake failed")
        }

        let summary = try prepareRequest(draft: representativeRequest())
        guard summary.method == "POST",
              summary.headerCount == 8,
              summary.bodyBytes == 1024
        else {
            throw HarnessError.invalidMeasurement("request preparation lost data")
        }

        let body = representativeResponseBody()
        let viewport = ResponseViewport.make(body: body)
        guard viewport.totalBytes == responseBodyBytes,
              viewport.presentedBytes == ResponseViewport.defaultByteLimit,
              viewport.text.utf8.count == ResponseViewport.defaultByteLimit
        else {
            throw HarnessError.invalidMeasurement("large response was not bounded to one viewport")
        }
    }

    private static func measure(
        appExecutable: URL,
        budgets: PerformanceBudgets?
    ) throws -> PerformanceResults {
        let launch = try measureLaunch(executable: appExecutable, count: launchSamples)
        let bridge = try measureBridge()
        let request = try measureRequestSetup()
        let dispatch = try measureRequestDispatch()
        let response = try measureResponseFirstViewport()
        let features = try FeatureWorkloads.measure()

        let warmResponse = Array(response.dropFirst())
        let values = CalculatedValues(
            launchP50: percentile(launch.milliseconds, 0.50),
            launchP95: percentile(launch.milliseconds, 0.95),
            idleRssP50: percentile(launch.rssMebibytes, 0.50),
            idleRssP95: percentile(launch.rssMebibytes, 0.95),
            bridgeP50: percentile(bridge, 0.50),
            bridgeP95: percentile(bridge, 0.95),
            requestP50: percentile(request, 0.50),
            requestP95: percentile(request, 0.95),
            dispatchP50: percentile(dispatch, 0.50),
            dispatchP95: percentile(dispatch, 0.95),
            responseCold: response[0],
            responseWarmP50: percentile(warmResponse, 0.50),
            responseWarmP95: percentile(warmResponse, 0.95),
            responseWarmMaximum: warmResponse.max() ?? .infinity,
            workspaceOpenFirstContent: features.workspaceOpenFirstContentMilliseconds,
            workspaceExpandCollapseP95: percentile(features.workspaceExpandCollapseMilliseconds, 0.95),
            workspaceFilterP95: percentile(features.workspaceFilterMilliseconds, 0.95),
            tabSwitchP95: percentile(features.tabSwitchMilliseconds, 0.95),
            tabCloseRestoreP95: percentile(features.tabCloseRestoreMilliseconds, 0.95),
            typingFeedbackP95: percentile(features.typingFeedbackMilliseconds, 0.95),
            largeResponseColdFirstViewport: features.responseColdFirstViewportMilliseconds,
            largeResponseSearchP95: percentile(features.responseSearchMilliseconds, 0.95),
            largeResponseRendererSwitchP95: percentile(features.responseRendererSwitchMilliseconds, 0.95),
            largeResponseResidentDelta: features.responseResidentDeltaMebibytes,
            historyInsertP95: percentile(features.historyInsertMilliseconds, 0.95),
            historyRestoreP95: percentile(features.historyRestoreMilliseconds, 0.95),
            historyEvictionP95: percentile(features.historyEvictionMilliseconds, 0.95),
            largeImportParseP95: percentile(features.importParseMilliseconds, 0.95)
        )

        return PerformanceResults(
            referenceMachine: budgets?.referenceMachine ?? "unbudgeted",
            measuredMachine: commandOutput("/usr/sbin/sysctl", ["-n", "machdep.cpu.brand_string"]),
            operatingSystem: ProcessInfo.processInfo.operatingSystemVersionString,
            responseBodyBytes: responseBodyBytes,
            responseViewportBytes: ResponseViewport.defaultByteLimit,
            processLaunchP50Milliseconds: values.launchP50,
            processLaunchP95Milliseconds: values.launchP95,
            idleRssP50Mebibytes: values.idleRssP50,
            idleRssP95Mebibytes: values.idleRssP95,
            bridgeHandshakeP50Microseconds: values.bridgeP50,
            bridgeHandshakeP95Microseconds: values.bridgeP95,
            requestSetupP50Microseconds: values.requestP50,
            requestSetupP95Microseconds: values.requestP95,
            requestDispatchP50Microseconds: values.dispatchP50,
            requestDispatchP95Microseconds: values.dispatchP95,
            responseColdFirstViewportMilliseconds: values.responseCold,
            responseWarmFirstViewportP50Milliseconds: values.responseWarmP50,
            responseWarmFirstViewportP95Milliseconds: values.responseWarmP95,
            responseWarmMaximumMainThreadSliceMilliseconds: values.responseWarmMaximum,
            workspaceOpenFirstContentMilliseconds: values.workspaceOpenFirstContent,
            workspaceExpandCollapseP95Milliseconds: values.workspaceExpandCollapseP95,
            workspaceFilterP95Milliseconds: values.workspaceFilterP95,
            tabSwitchP95Milliseconds: values.tabSwitchP95,
            tabCloseRestoreP95Milliseconds: values.tabCloseRestoreP95,
            typingFeedbackP95Milliseconds: values.typingFeedbackP95,
            largeResponseColdFirstViewportMilliseconds: values.largeResponseColdFirstViewport,
            largeResponseSearchP95Milliseconds: values.largeResponseSearchP95,
            largeResponseRendererSwitchP95Milliseconds: values.largeResponseRendererSwitchP95,
            largeResponseResidentDeltaMebibytes: values.largeResponseResidentDelta,
            historyInsertP95Milliseconds: values.historyInsertP95,
            historyRestoreP95Milliseconds: values.historyRestoreP95,
            historyEvictionP95Milliseconds: values.historyEvictionP95,
            largeImportParseP95Milliseconds: values.largeImportParseP95,
            raw: PerformanceResults.RawSamples(
                processLaunchMilliseconds: launch.milliseconds,
                idleRssMebibytes: launch.rssMebibytes,
                bridgeHandshakeMicroseconds: bridge,
                requestSetupMicroseconds: request,
                requestDispatchMicroseconds: dispatch,
                responseFirstViewportMilliseconds: response,
                workspaceExpandCollapseMilliseconds: features.workspaceExpandCollapseMilliseconds,
                workspaceFilterMilliseconds: features.workspaceFilterMilliseconds,
                tabSwitchMilliseconds: features.tabSwitchMilliseconds,
                tabCloseRestoreMilliseconds: features.tabCloseRestoreMilliseconds,
                typingFeedbackMilliseconds: features.typingFeedbackMilliseconds,
                largeResponseSearchMilliseconds: features.responseSearchMilliseconds,
                largeResponseRendererSwitchMilliseconds: features.responseRendererSwitchMilliseconds,
                historyInsertMilliseconds: features.historyInsertMilliseconds,
                historyRestoreMilliseconds: features.historyRestoreMilliseconds,
                historyEvictionMilliseconds: features.historyEvictionMilliseconds,
                largeImportParseMilliseconds: features.importParseMilliseconds
            ),
            violations: violations(values: values, budgets: budgets)
        )
    }

    private struct CalculatedValues {
        let launchP50: Double
        let launchP95: Double
        let idleRssP50: Double
        let idleRssP95: Double
        let bridgeP50: Double
        let bridgeP95: Double
        let requestP50: Double
        let requestP95: Double
        let dispatchP50: Double
        let dispatchP95: Double
        let responseCold: Double
        let responseWarmP50: Double
        let responseWarmP95: Double
        let responseWarmMaximum: Double
        let workspaceOpenFirstContent: Double
        let workspaceExpandCollapseP95: Double
        let workspaceFilterP95: Double
        let tabSwitchP95: Double
        let tabCloseRestoreP95: Double
        let typingFeedbackP95: Double
        let largeResponseColdFirstViewport: Double
        let largeResponseSearchP95: Double
        let largeResponseRendererSwitchP95: Double
        let largeResponseResidentDelta: Double
        let historyInsertP95: Double
        let historyRestoreP95: Double
        let historyEvictionP95: Double
        let largeImportParseP95: Double
    }

    private static func violations(
        values: CalculatedValues,
        budgets: PerformanceBudgets?
    ) -> [String] {
        guard let budgets else {
            return []
        }

        let checks: [(String, Double, Double)] = [
            ("process launch p95 (ms)", values.launchP95, budgets.processLaunchP95Milliseconds),
            ("idle RSS p95 (MiB)", values.idleRssP95, budgets.idleRssP95Mebibytes),
            ("bridge handshake p95 (us)", values.bridgeP95, budgets.bridgeHandshakeP95Microseconds),
            ("request setup p95 (us)", values.requestP95, budgets.requestSetupP95Microseconds),
            ("request dispatch p95 (us)", values.dispatchP95, budgets.requestDispatchP95Microseconds),
            ("response cold first viewport (ms)", values.responseCold, budgets.responseColdFirstViewportMilliseconds),
            ("response warm first viewport p95 (ms)", values.responseWarmP95, budgets.responseWarmFirstViewportP95Milliseconds),
            ("response warm maximum main-thread slice (ms)", values.responseWarmMaximum, budgets.responseWarmMaximumMainThreadSliceMilliseconds),
            ("workspace open first content (ms)", values.workspaceOpenFirstContent, budgets.workspaceOpenFirstContentMilliseconds),
            ("workspace expand/collapse p95 (ms)", values.workspaceExpandCollapseP95, budgets.workspaceExpandCollapseP95Milliseconds),
            ("workspace filter p95 (ms)", values.workspaceFilterP95, budgets.workspaceFilterP95Milliseconds),
            ("tab switch p95 (ms)", values.tabSwitchP95, budgets.tabSwitchP95Milliseconds),
            ("tab close/restore p95 (ms)", values.tabCloseRestoreP95, budgets.tabCloseRestoreP95Milliseconds),
            ("typing feedback p95 (ms)", values.typingFeedbackP95, budgets.typingFeedbackP95Milliseconds),
            ("100 MiB response first viewport (ms)", values.largeResponseColdFirstViewport, budgets.largeResponseColdFirstViewportMilliseconds),
            ("100 MiB response search p95 (ms)", values.largeResponseSearchP95, budgets.largeResponseSearchP95Milliseconds),
            ("response renderer switch p95 (ms)", values.largeResponseRendererSwitchP95, budgets.largeResponseRendererSwitchP95Milliseconds),
            ("100 MiB response resident delta (MiB)", values.largeResponseResidentDelta, budgets.largeResponseResidentDeltaMebibytes),
            ("history insert p95 (ms)", values.historyInsertP95, budgets.historyInsertP95Milliseconds),
            ("history restore p95 (ms)", values.historyRestoreP95, budgets.historyRestoreP95Milliseconds),
            ("history eviction p95 (ms)", values.historyEvictionP95, budgets.historyEvictionP95Milliseconds),
            ("large import parse p95 (ms)", values.largeImportParseP95, budgets.largeImportParseP95Milliseconds),
        ]

        return checks.compactMap { name, measured, budget in
            measured > budget
                ? "\(name): measured \(format(measured)), budget \(format(budget))"
                : nil
        }
    }

    private static func measureBridge() throws -> [Double] {
        var checksum = 0
        for _ in 0 ..< 1_000 {
            checksum &+= coreHandshake().product.utf8.count
        }

        let samples = (0 ..< metricSamples).map { _ in
            let started = DispatchTime.now().uptimeNanoseconds
            for _ in 0 ..< bridgeIterations {
                let handshake = coreHandshake()
                checksum &+= handshake.product.utf8.count
                checksum &+= Int(handshake.streamAbiVersion)
            }
            let elapsed = DispatchTime.now().uptimeNanoseconds - started
            return Double(elapsed) / Double(bridgeIterations) / 1_000
        }

        guard checksum > 0 else {
            throw HarnessError.invalidMeasurement("bridge work was optimized away")
        }
        return samples
    }

    private static func measureRequestSetup() throws -> [Double] {
        let draft = representativeRequest()
        var checksum: UInt64 = 0

        for _ in 0 ..< 100 {
            checksum &+= try prepareRequest(draft: draft).bodyBytes
        }

        let samples = try (0 ..< metricSamples).map { _ in
            let started = DispatchTime.now().uptimeNanoseconds
            for _ in 0 ..< requestIterations {
                let summary = try prepareRequest(draft: draft)
                checksum &+= summary.bodyBytes
                checksum &+= summary.headerCount
            }
            let elapsed = DispatchTime.now().uptimeNanoseconds - started
            return Double(elapsed) / Double(requestIterations) / 1_000
        }

        guard checksum > 0 else {
            throw HarnessError.invalidMeasurement("request work was optimized away")
        }
        return samples
    }

    private static func measureRequestDispatch() throws -> [Double] {
        guard wirebolt_runtime_warmup() == 1 else {
            throw HarnessError.invalidMeasurement("shared HTTP runtime failed to start")
        }
        let input = Data("{}".utf8)
        let callbacks = wirebolt_run_callbacks(
            on_prepared: performanceHeadCallback,
            on_head: performanceHeadCallback,
            on_chunk: performanceChunkCallback,
            on_complete: performanceTerminalCallback,
            on_error: performanceTerminalCallback
        )

        return try (0 ..< metricSamples).map { _ in
            let started = DispatchTime.now().uptimeNanoseconds
            let session = input.withUnsafeBytes { bytes in
                wirebolt_run_start(
                    bytes.bindMemory(to: UInt8.self).baseAddress,
                    UInt(bytes.count),
                    callbacks,
                    nil
                )
            }
            let elapsed = DispatchTime.now().uptimeNanoseconds - started
            guard let session else {
                throw HarnessError.invalidMeasurement("request dispatch failed")
            }
            wirebolt_run_free(session)
            return Double(elapsed) / 1_000
        }
    }

    private static func measureResponseFirstViewport() throws -> [Double] {
        let body = representativeResponseBody()
        var checksum = 0

        let samples = (0 ..< metricSamples).map { _ in
            autoreleasepool {
                let started = DispatchTime.now().uptimeNanoseconds
                let viewport = ResponseViewport.make(body: body)
                let storage = NSTextStorage(string: viewport.text)
                let layout = NSLayoutManager()
                let container = NSTextContainer(containerSize: NSSize(width: 1_024, height: 768))
                container.lineFragmentPadding = 0
                storage.addLayoutManager(layout)
                layout.addTextContainer(container)
                layout.ensureLayout(for: container)
                checksum &+= layout.glyphRange(for: container).length
                let elapsed = DispatchTime.now().uptimeNanoseconds - started
                return Double(elapsed) / 1_000_000
            }
        }

        guard checksum > 0 else {
            throw HarnessError.invalidMeasurement("response layout produced no glyphs")
        }
        return samples
    }

    private static func representativeRequest() -> BridgeRequestDraft {
        BridgeRequestDraft(
            method: "POST",
            url: "https://api.example.com/v1/items?limit=50&sort=created_at",
            headers: [
                HeaderField(name: "accept", value: "application/json"),
                HeaderField(name: "content-type", value: "application/json"),
                HeaderField(name: "cache-control", value: "no-cache"),
                HeaderField(name: "user-agent", value: "Wirebolt/0.1"),
                HeaderField(name: "x-request-id", value: "01234567-89ab-cdef-0123-456789abcdef"),
                HeaderField(name: "x-environment", value: "development"),
                HeaderField(name: "x-client-version", value: "0.1.0"),
                HeaderField(name: "x-correlation-id", value: "fedcba98-7654-3210-fedc-ba9876543210"),
            ],
            body: Data(repeating: 0x61, count: 1_024)
        )
    }

    private static func representativeResponseBody() -> Data {
        let line = Data("{\"id\":12345,\"name\":\"wirebolt\",\"ok\":true}\n".utf8)
        var body = Data(capacity: responseBodyBytes)
        while body.count + line.count <= responseBodyBytes {
            body.append(line)
        }
        body.append(line.prefix(responseBodyBytes - body.count))
        return body
    }

    private static func measureLaunch(
        executable: URL,
        count: Int
    ) throws -> (milliseconds: [Double], rssMebibytes: [Double]) {
        var milliseconds: [Double] = []
        var rssMebibytes: [Double] = []
        let fileManager = FileManager.default

        for _ in 0 ..< count {
            let readyFile = fileManager.temporaryDirectory
                .appendingPathComponent("wirebolt-ready-\(UUID().uuidString)")
            let process = Process()
            process.executableURL = executable
            var environment = ProcessInfo.processInfo.environment
            environment["WIREBOLT_READY_FILE"] = readyFile.path
            process.environment = environment

            let started = DispatchTime.now().uptimeNanoseconds
            try process.run()
            let deadline = DispatchTime.now().uptimeNanoseconds + 5_000_000_000

            while !fileManager.fileExists(atPath: readyFile.path),
                  process.isRunning,
                  DispatchTime.now().uptimeNanoseconds < deadline
            {
                usleep(500)
            }

            guard fileManager.fileExists(atPath: readyFile.path) else {
                if process.isRunning {
                    stop(process)
                    throw HarnessError.appReadinessTimedOut
                } else {
                    let status = process.terminationStatus
                    throw HarnessError.appExited(status)
                }
            }

            let elapsed = DispatchTime.now().uptimeNanoseconds - started
            milliseconds.append(Double(elapsed) / 1_000_000)
            usleep(250_000)
            rssMebibytes.append(try residentMemoryMebibytes(pid: process.processIdentifier))

            stop(process)
            try? fileManager.removeItem(at: readyFile)
            usleep(50_000)
        }

        return (milliseconds, rssMebibytes)
    }

    private static func stop(_ process: Process) {
        guard process.isRunning else {
            return
        }

        process.terminate()
        let deadline = DispatchTime.now().uptimeNanoseconds + 2_000_000_000
        while process.isRunning, DispatchTime.now().uptimeNanoseconds < deadline {
            usleep(10_000)
        }
        if process.isRunning {
            kill(process.processIdentifier, SIGKILL)
        }
        process.waitUntilExit()
    }

    private static func residentMemoryMebibytes(pid: Int32) throws -> Double {
        let output = commandOutput("/bin/ps", ["-o", "rss=", "-p", String(pid)])
        guard let kibibytes = Double(output) else {
            throw HarnessError.invalidMeasurement("could not read app RSS")
        }
        return kibibytes / 1_024
    }

    private static func commandOutput(_ executable: String, _ arguments: [String]) -> String {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice

        do {
            try process.run()
            process.waitUntilExit()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            return String(decoding: data, as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines)
        } catch {
            return "unknown"
        }
    }

    private static func loadBudgets(_ url: URL) throws -> PerformanceBudgets {
        do {
            return try JSONDecoder().decode(PerformanceBudgets.self, from: Data(contentsOf: url))
        } catch {
            throw HarnessError.invalidBudgetFile(error.localizedDescription)
        }
    }

    private static func percentile(_ values: [Double], _ percentile: Double) -> Double {
        let sorted = values.sorted()
        let index = max(0, min(sorted.count - 1, Int(ceil(percentile * Double(sorted.count))) - 1))
        return sorted[index]
    }

    private static func format(_ value: Double) -> String {
        String(format: "%.3f", value)
    }

    private static func printJSON<T: Encodable>(_ value: T) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(value)
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0A]))
    }
}
