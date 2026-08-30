import Foundation

struct ResponseViewport: Equatable, Sendable {
    static let defaultByteLimit = 32 * 1024

    let text: String
    let presentedBytes: Int
    let totalBytes: Int

    static func make(
        body: Data,
        byteLimit: Int = defaultByteLimit
    ) -> ResponseViewport {
        let presentedBytes = min(body.count, max(byteLimit, 0))
        let text = String(decoding: body.prefix(presentedBytes), as: UTF8.self)

        return ResponseViewport(
            text: text,
            presentedBytes: presentedBytes,
            totalBytes: body.count
        )
    }
}
