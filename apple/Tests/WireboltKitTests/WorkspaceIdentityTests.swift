import Foundation
import Testing
@testable import WireboltKit

struct WorkspaceIdentityTests {
    @Test("Renaming the workspace persists one trimmed command and updates the title source")
    @MainActor
    func renamesWorkspace() async {
        let persistence = KeychainRecorder()
        let model = WireboltModel(runner: SilentRunner(), persistence: persistence)
        await model.loadWorkspace()

        #expect(await model.renameWorkspace("  Payments API  "))
        #expect(await model.renameWorkspace("   ") == false)

        #expect(model.workspace.name == "Payments API")
        #expect(await persistence.commands == [.renameWorkspace(name: "Payments API")])
    }

    @Test("Each workspace folder gets its own cookie jar and session cookies end with the app")
    @MainActor
    func cookiesArePerWorkspace() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "wirebolt-cookies-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let cookieDirectory = root.appending(path: "Cookies")
        let legacy = cookieDirectory.appending(path: "cookies.json")
        try FileManager.default.createDirectory(at: cookieDirectory, withIntermediateDirectories: true)
        try Data("[]".utf8).write(to: legacy)
        let first = KeychainRecorder(location: root.appending(path: "Checkout"))
        let second = KeychainRecorder(location: root.appending(path: "Payments"))
        let model = WireboltModel(runner: CookieSettingRunner(), persistence: first, cookieDirectory: cookieDirectory)

        #expect(await model.openWorkspace(using: first))
        #expect(model.workspaceLocation == root.appending(path: "Checkout"))
        await model.send(model.sessions.open(draft: RequestDraft(url: "https://shop.example.com/cart")))
        #expect(await model.cookies().map(\.name) == ["remember", "session"])

        #expect(await model.openWorkspace(using: second))
        #expect(await model.cookies().isEmpty)

        // A relaunch (a new jar on the same file) keeps only cookies that have an expiry.
        let relaunched = WireboltModel(runner: SilentRunner(), cookieDirectory: cookieDirectory)
        relaunched.configurePersistence(first)
        #expect(await relaunched.cookies().map(\.name) == ["remember"])
        #expect(!FileManager.default.fileExists(atPath: legacy.path))
    }

    @Test("Deleting one cookie or clearing all updates the list and its revision")
    @MainActor
    func deletesCookies() async throws {
        let model = WireboltModel(runner: CookieSettingRunner(), cookieDirectory: nil)
        await model.send(model.sessions.open(draft: RequestDraft(url: "https://shop.example.com/cart")))
        let revision = model.cookieRevision
        let session = try #require(await model.cookies().first { $0.name == "session" })

        await model.deleteCookie(id: session.id)
        #expect(await model.cookies().map(\.name) == ["remember"])
        await model.clearCookies()
        #expect(await model.cookies().isEmpty)
        #expect(model.cookieRevision == revision + 2)
    }

    @Test("Cookie storage is keyed by workspace folder and lives outside it")
    func cookieStorageLocation() {
        let directory = URL(fileURLWithPath: "/tmp/wirebolt-runtime/Cookies", isDirectory: true)
        let checkout = URL(fileURLWithPath: "/Users/demo/Workspaces/Checkout", isDirectory: true)
        let payments = URL(fileURLWithPath: "/Users/demo/Workspaces/Payments", isDirectory: true)
        let url = CookieJar.storageURL(forWorkspaceAt: checkout, in: directory)
        #expect(url == CookieJar.storageURL(forWorkspaceAt: checkout.appending(path: "."), in: directory))
        #expect(url != CookieJar.storageURL(forWorkspaceAt: payments, in: directory))
        #expect(url.deletingLastPathComponent().standardizedFileURL == directory.standardizedFileURL)
        #expect(!url.path.hasPrefix(checkout.path))
    }
}

private struct CookieSettingRunner: RequestRunner {
    func events(for _: RunInput, runID _: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { stream in
            stream.yield(.cookies(ResponseCookies(url: "https://shop.example.com/cart", headers: [
                ResponseHeader(name: "Set-Cookie", value: "session=fixture-session; Path=/; Secure; HttpOnly"),
                ResponseHeader(name: "Set-Cookie", value: "remember=fixture-remember; Path=/; Max-Age=3600"),
            ])))
            stream.finish()
        }
    }

    func cancel(runID _: RunID) {}
}
