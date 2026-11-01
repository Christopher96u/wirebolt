import Foundation

/// Rust owns the disposable file; Swift retains its presentation snapshot.
public final class JSONResponseDocument: Sendable {
    public let url: URL
    public let byteCount: UInt64
    public let preview: String
    public let containsUnicodeEscapes: Bool
    private let rust: AppJsonDocument
    init(_ rust: AppJsonDocument) {
        self.rust = rust
        let metadata = rust.metadata()
        url = URL(fileURLWithPath: metadata.path); byteCount = metadata.byteCount
        preview = metadata.preview; containsUnicodeEscapes = metadata.containsUnicodeEscapes
    }
    public convenience init(sourceURL: URL, decodesUnicodeEscapes: Bool = false) throws {
        let document = try responseCall { try AppJsonDocument(path: sourceURL.path, decode: decodesUnicodeEscapes, cancellation: $0) }
        self.init(document)
    }
}
