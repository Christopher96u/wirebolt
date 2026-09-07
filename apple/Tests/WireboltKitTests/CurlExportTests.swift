import Foundation
import Testing
@testable import WireboltKit

@Suite("cURL export")
struct CurlExportTests {
    private func literal(_ source: ValueSource) -> String {
        switch source { case let .literal(value): value; case .secret: "fixture-secret" }
    }

    @Test("Shell quoting preserves apostrophes, substitutions, Unicode and credentials as literal arguments")
    func shellQuoting() throws {
        var request = RequestDraft(url: "https://example.com/echo")
        request.method = .post
        request.authentication = .basic(username: .literal("O'Brien"), password: .secret("test.password"))
        let payload = "café 東京 $(printf SHOULD_NOT_RUN) `printf SHOULD_NOT_RUN` 'quoted'"
        request.body = .json(value: payload)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-f", "-c", "curl() { printf '%s\\0' \"$@\"; }\n" + request.curlCommand(resolve: literal)]
        let output = Pipe()
        process.standardOutput = output
        try process.run()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
        let arguments = String(decoding: data, as: UTF8.self).split(separator: "\0").map(String.init)
        #expect(arguments == ["--request", "POST", "https://example.com/echo", "--user", "O'Brien:fixture-secret",
                              "--header", "Content-Type: application/json", "--data-raw", payload])
    }

    @Test("Forms retain enabled repeated fields and binary multipart keeps its bytes")
    func forms() {
        var request = RequestDraft(url: "http://localhost/echo")
        request.body = .formURLEncoded(fields: [RequestField(name: "name", value: .literal("café")),
            RequestField(name: "name", value: .literal("東京")), RequestField(name: "off", value: .literal("hidden"), enabled: false)])
        let command = request.curlCommand(resolve: literal)
        #expect(command.contains("--data-urlencode 'name=café' --data-urlencode 'name=東京'"))
        #expect(!command.contains("hidden"))
        request.headers = [RequestField(name: "Content-Type", value: .literal("multipart/form-data; boundary=stale"))]
        request.body = .multipart(parts: [MultipartPart(name: "file", kind: .binary, value: .literal("AAH/"), fileName: "a.bin")])
        let multipart = request.curlCommand(resolve: literal)
        #expect(multipart.contains("@/dev/fd/3"))
        #expect(multipart.contains("printf %s 'AAH/' | base64 --decode"))
        #expect(!multipart.contains("boundary=stale"))
    }
}
