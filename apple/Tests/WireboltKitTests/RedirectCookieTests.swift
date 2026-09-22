import Foundation
import Testing
@testable import WireboltKit

struct RedirectCookieTests {
    @Test @MainActor func cookiesKeepResponseOriginAndSurviveFailure() async {
        let jar = CookieJar()
        let model = WireboltModel(runner: CookieRedirectRunner(), cookieJar: jar)
        let session = model.sessions.open(draft: RequestDraft(url: "https://original.example/start"))
        await model.send(session)
        #expect(session.failure?.kind == "connection")
        #expect(await jar.header(for: URL(string: "https://original.example/")!) == "first=fixture")
        #expect(await jar.header(for: URL(string: "https://redirected.example/")!) == "second=fixture")
        #expect(await jar.header(for: URL(string: "https://unrelated.example/")!) == nil)
        #expect(session.responseCookies.count == 2)
    }
}

private struct CookieRedirectRunner: RequestRunner {
    func events(for input: RunInput, runID: RunID) -> AsyncThrowingStream<RunEvent, any Error> {
        AsyncThrowingStream { stream in
            stream.yield(.cookies(ResponseCookies(url: "https://original.example/start", headers: [ResponseHeader(name: "Set-Cookie", value: "first=fixture; Path=/; Secure")])))
            stream.yield(.cookies(ResponseCookies(url: "https://redirected.example/final", headers: [ResponseHeader(name: "Set-Cookie", value: "second=fixture; Path=/; Secure")])))
            stream.finish(throwing: RunFailure(kind: "connection", issues: []))
        }
    }
    func cancel(runID: RunID) {}
}
