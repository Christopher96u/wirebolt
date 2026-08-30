import AppKit
import Darwin
import Foundation

private struct PerformanceBudgets: Decodable {
    let referenceMachine: String
    let processLaunchP95Milliseconds: Double
    let idleRssP95Mebibytes: Double
    let bridgeHandshakeP95Microseconds: Double
    let requestSetupP95Microseconds: Double
    let responseColdFirstViewportMilliseconds: Double
    let responseWarmFirstViewportP95Milliseconds: Double
    let responseWarmMaximumMainThreadSliceMilliseconds: Double
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
    let responseColdFirstViewportMilliseconds: Double
    let responseWarmFirstViewportP50Milliseconds: Double
    let responseWarmFirstViewportP95Milliseconds: Double
    let responseWarmMaximumMainThreadSliceMilliseconds: Double
    let raw: RawSamples
    let violations: [String]

    struct RawSamples: Encodable {
        let processLaunchMilliseconds: [Double]
        let idleRssMebibytes: [Double]
        let bridgeHandshakeMicroseconds: [Double]
        let requestSetupMicroseconds: [Double]
        let responseFirstViewportMilliseconds: [Double]
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
    private static let launchSamples = 12
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
        let response = try measureResponseFirstViewport()

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
            responseCold: response[0],
            responseWarmP50: percentile(warmResponse, 0.50),
            responseWarmP95: percentile(warmResponse, 0.95),
            responseWarmMaximum: warmResponse.max() ?? .infinity
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
            responseColdFirstViewportMilliseconds: values.responseCold,
            responseWarmFirstViewportP50Milliseconds: values.responseWarmP50,
            responseWarmFirstViewportP95Milliseconds: values.responseWarmP95,
            responseWarmMaximumMainThreadSliceMilliseconds: values.responseWarmMaximum,
            raw: PerformanceResults.RawSamples(
                processLaunchMilliseconds: launch.milliseconds,
                idleRssMebibytes: launch.rssMebibytes,
                bridgeHandshakeMicroseconds: bridge,
                requestSetupMicroseconds: request,
                responseFirstViewportMilliseconds: response
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
        let responseCold: Double
        let responseWarmP50: Double
        let responseWarmP95: Double
        let responseWarmMaximum: Double
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
            ("response cold first viewport (ms)", values.responseCold, budgets.responseColdFirstViewportMilliseconds),
            ("response warm first viewport p95 (ms)", values.responseWarmP95, budgets.responseWarmFirstViewportP95Milliseconds),
            ("response warm maximum main-thread slice (ms)", values.responseWarmMaximum, budgets.responseWarmMaximumMainThreadSliceMilliseconds),
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

    private static func representativeRequest() -> RequestDraft {
        RequestDraft(
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
