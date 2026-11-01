import Darwin
import Foundation

// Fresh processes per sample keep the two-query peak meaningful. Index construction
// and fixture writes are outside query timings; searches use the production ICU bridge.
@main enum SearchWorkloads {
    static func main() throws {
        let arguments = CommandLine.arguments
        precondition(arguments.count >= 3, "size-in-MiB output")
        let mebibytes = Int(arguments[1])!
        precondition([10, 100].contains(mebibytes))
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("wirebolt-search-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: url) }
        FileManager.default.createFile(atPath: url.path, contents: nil)
        let writer = try FileHandle(forWritingTo: url)
        let line = "needle " + String(repeating: "x", count: 56) + "\n"
        let block = Data(String(repeating: line, count: 16_384).utf8)
        for _ in 0..<mebibytes { try writer.write(contentsOf: block) }
        try writer.seek(toOffset: UInt64(mebibytes * 1_048_576 - 1))
        try writer.write(contentsOf: Data("!".utf8))
        try writer.close()
        let index = try AppTextIndex(
            options: IndexOptions(path: url.path, columns: 100, prefix: "", wrapping: nil),
            rowLimit: UInt64.max, adapter: nil, cancellation: AppResponseCancellation())
        var measurements: [[String: Any]] = []
        var twoQueryPeak: Int64 = 0
        for (name, text, expected) in [("absent", "not-present-in-file", 0), ("capped", "needle", 20_000), ("last-byte", "!", 1)] {
            let start = ContinuousClock.now
            let matches = try index.search(
                query: SearchQuery(text: text, matchCase: false, wholeWord: false, regularExpression: false),
                start: nil, end: nil, cancellation: AppResponseCancellation())
            let elapsed = start.duration(to: .now).components
            let milliseconds = Double(elapsed.seconds) * 1000 + Double(elapsed.attoseconds) / 1e15
            precondition(matches.count == expected)
            if name == "capped" {
                precondition(matches.last?.start == SearchPosition(row: 19_999, column: 0))
                precondition(matches.last?.end == SearchPosition(row: 19_999, column: 6))
            } else if name == "last-byte" {
                precondition(matches[0].start == SearchPosition(row: UInt64(mebibytes * 16_384 - 1), column: 63))
                precondition(matches[0].end.column == 64)
            }
            measurements.append(["name": name, "samples_ms": [milliseconds]])
            if name == "capped" {
                var usage = rusage()
                getrusage(RUSAGE_SELF, &usage)
                twoQueryPeak = Int64(usage.ru_maxrss)
            }
        }
        let result: [String: Any] = ["size_mib": mebibytes, "measurements": measurements,
                                     "two_query_peak_rss_bytes": twoQueryPeak]
        try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
            .write(to: URL(fileURLWithPath: arguments[2]))
    }
}
