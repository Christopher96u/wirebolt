import Foundation

extension ImportFormat {
    public static func detectCollectionExport(fileExtension: String, contents: String, root: [String: Any]?) -> ImportFormat? {
        let json = root.flatMap { try? JSONSerialization.data(withJSONObject: $0) }.map { String(decoding: $0, as: UTF8.self) }
        return domainDetectCollection(extension: fileExtension, contents: contents, rootJson: json)
    }
}
public enum BrunoCollectionSource {
    public enum ReadError: Error, Equatable { case notACollection, tooLarge }
    public static func isCollection(_ url: URL) -> Bool { domainIsBrunoCollection(path: url.path) }
    public static func bundle(at url: URL) throws -> String {
        do { return try domainBrunoBundle(path: url.path, policy: NativeCollectionPackages()) }
        catch let error as CollectionSourceError {
            switch error {
            case .NotACollection: throw ReadError.notACollection
            case .TooLarge: throw ReadError.tooLarge
            }
        }
    }
}

/// File packages are an Apple filesystem presentation concept.
private final class NativeCollectionPackages: CollectionPackageServices, Sendable {
    func isPackage(path: String) -> Bool {
        (try? URL(fileURLWithPath: path).resourceValues(forKeys: [.isPackageKey]).isPackage) == true
    }
}
