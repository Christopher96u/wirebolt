import Foundation

public enum BrunoCollectionSource {
    public typealias ReadError = CollectionSourceError
    public static func isCollection(_ url: URL) -> Bool { domainIsBrunoCollection(path: url.path) }
    public static func bundle(at url: URL) throws -> BrunoFolderBundle {
        try domainBrunoBundle(path: url.path, policy: NativeCollectionPackages())
    }
}

extension CollectionSourceError {
    public static var notACollection: Self { .NotACollection }
    public static var tooLarge: Self { .TooLarge }
}

/// File packages are an Apple filesystem presentation concept.
private final class NativeCollectionPackages: CollectionPackageServices, Sendable {
    func isPackage(path: String) -> Bool {
        (try? URL(fileURLWithPath: path).resourceValues(forKeys: [.isPackageKey]).isPackage) == true
    }
}
