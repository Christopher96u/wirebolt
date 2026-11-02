import Testing
@testable import WireboltKit

struct SemanticPaletteTests {
    private static var colors: [(String, AdaptivePaletteColor)] {
        HTTPMethod.allCases.map { ($0.rawValue, SemanticPalette.method($0)) }
            + [("WS", SemanticPalette.method(.get, webSocket: true)), ("CUSTOM", SemanticPalette.method(HTTPMethod(rawValue: "PROPFIND")!))]
            + [101, 200, 302, 404, 503].map { ("\($0)", domainStatusColor(status: UInt16($0))) }
    }

    @Test("Method and status text meets WCAG AA, and AAA with Increase Contrast, on every background")
    func contrast() {
        for (name, color) in Self.colors {
            for background in SemanticPalette.lightBackgrounds {
                #expect(domainContrast(color: color.light, other: background) >= 4.5, "\(name) light")
                #expect(domainContrast(color: color.lightHighContrast, other: background) >= 7, "\(name) light, increased contrast")
            }
            for background in SemanticPalette.darkBackgrounds {
                #expect(domainContrast(color: color.dark, other: background) >= 4.5, "\(name) dark")
                #expect(domainContrast(color: color.darkHighContrast, other: background) >= 7, "\(name) dark, increased contrast")
            }
        }
    }

    @Test("Every method, including WebSocket, has its own color")
    func distinctMethodColors() {
        let methods = HTTPMethod.allCases.map { SemanticPalette.method($0) } + [SemanticPalette.method(.get, webSocket: true)]
        #expect(Set(methods).count == methods.count)
        #expect(SemanticPalette.method(.post, webSocket: true) == SemanticPalette.method(.get, webSocket: true))
    }

    @Test("Status classes map to distinct colors")
    func statusClasses() {
        #expect(HTTPStatusClass(status: 101) == .informational)
        #expect(HTTPStatusClass(status: 204) == .success)
        #expect(HTTPStatusClass(status: 308) == .redirection)
        #expect(HTTPStatusClass(status: 422) == .clientError)
        #expect(HTTPStatusClass(status: 500) == .serverError)
        let colors = [101, 200, 301, 404, 500].map { domainStatusColor(status: UInt16($0)) }
        #expect(Set(colors).count == colors.count)
    }

    @Test("Contrast math matches WCAG reference values")
    func wcagReference() {
        let black = PaletteColor(hex: 0x000000), white = PaletteColor(hex: 0xFFFFFF)
        #expect(abs(domainContrast(color: black, other: white) - 21) < 0.001)
        #expect(abs(domainContrast(color: PaletteColor(hex: 0x777777), other: white) - 4.48) < 0.01)
    }
}
