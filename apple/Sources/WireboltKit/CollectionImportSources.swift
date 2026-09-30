import Foundation

public extension ImportFormat {
    /// Recognises Insomnia and Bruno exports. `root` is the parsed JSON object, or nil for
    /// text that is not JSON (Insomnia v5 and v4 YAML exports).
    static func detectCollectionExport(fileExtension: String, contents: String, root: [String: Any]?) -> ImportFormat? {
        if fileExtension.lowercased() == "bru" { return .brunoFolder }
        guard let root else { return isInsomniaYAML(contents) ? .insomnia : nil }
        if root["__export_format"] != nil, root["resources"] != nil || root["_type"] as? String == "export" {
            return .insomnia
        }
        if let version = root["version"] as? String, !version.isEmpty,
           root["name"] is String, root["items"] is [Any] {
            return .bruno
        }
        return nil
    }

    private static func isInsomniaYAML(_ contents: String) -> Bool {
        // Only the first lines matter; exports start with their type or export format.
        for line in contents.split(separator: "\n", maxSplits: 40, omittingEmptySubsequences: true).prefix(40) {
            let text = line.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "\"", with: "")
                .replacingOccurrences(of: "'", with: "")
            if text.hasPrefix("type: collection.insomnia.rest/5")
                || text.hasPrefix("type: spec.insomnia.rest/5")
                || text.hasPrefix("type: environment.insomnia.rest/5")
                || text.hasPrefix("__export_format:") {
                return true
            }
        }
        return false
    }
}

/// Reads a Bruno collection folder (or one `.bru` file) into the bundle the importer parses.
/// Only Bruno's text files are read; hidden folders and `node_modules` are skipped.
public enum BrunoCollectionSource {
    public enum ReadError: Error, Equatable {
        case notACollection
        case tooLarge
    }

    static let maximumFileCount = 5_000
    static let maximumFileBytes = 2 * 1_024 * 1_024
    static let maximumTotalBytes = 64 * 1_024 * 1_024

    /// True for a folder with `bruno.json` or top-level `.bru` files.
    public static func isCollection(_ url: URL) -> Bool {
        let manager = FileManager.default
        if manager.fileExists(atPath: url.appendingPathComponent("bruno.json").path) { return true }
        let names = (try? manager.contentsOfDirectory(atPath: url.path)) ?? []
        return names.contains { $0.lowercased().hasSuffix(".bru") }
    }

    /// Builds the source for `ImportFormat.brunoFolder` from a collection folder or a `.bru` file.
    public static func bundle(at url: URL) throws -> String {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory) else {
            throw ReadError.notACollection
        }
        if !isDirectory.boolValue {
            let contents = try String(contentsOf: url, encoding: .utf8)
            return try encode(root: url.deletingLastPathComponent(), files: [(url.lastPathComponent, contents)])
        }
        return try encode(root: url, files: collectionFiles(in: url))
    }

    private static func collectionFiles(in root: URL) throws -> [(String, String)] {
        let root = root.resolvingSymlinksInPath().standardizedFileURL
        let keys: [URLResourceKey] = [.isDirectoryKey, .fileSizeKey]
        guard let enumerator = FileManager.default.enumerator(
            at: root,
            includingPropertiesForKeys: keys,
            options: [.skipsHiddenFiles, .skipsPackageDescendants]
        ) else { throw ReadError.notACollection }
        var files: [(String, String)] = []
        var totalBytes = 0
        for case let url as URL in enumerator {
            let values = try url.resourceValues(forKeys: Set(keys))
            if values.isDirectory == true {
                if url.lastPathComponent == "node_modules" { enumerator.skipDescendants() }
                continue
            }
            let name = url.lastPathComponent.lowercased()
            guard name.hasSuffix(".bru") || name == "bruno.json" || name == "opencollection.yml" else { continue }
            let size = values.fileSize ?? 0
            totalBytes += size
            guard files.count < maximumFileCount, size <= maximumFileBytes, totalBytes <= maximumTotalBytes else {
                throw ReadError.tooLarge
            }
            let path = url.resolvingSymlinksInPath().standardizedFileURL.path
            guard path.hasPrefix(root.path + "/") else { continue }
            files.append((String(path.dropFirst(root.path.count + 1)), try String(contentsOf: url, encoding: .utf8)))
        }
        guard !files.isEmpty else { throw ReadError.notACollection }
        return files.sorted { $0.0 < $1.0 }
    }

    private static func encode(root: URL, files: [(String, String)]) throws -> String {
        let document: [String: Any] = [
            "root": root.path,
            "files": files.map { ["path": $0.0, "contents": $0.1] },
        ]
        let data = try JSONSerialization.data(withJSONObject: document, options: [.withoutEscapingSlashes])
        return String(decoding: data, as: UTF8.self)
    }
}
