import Foundation
import Testing
@testable import WireboltKit

@Suite("Variable templates")
struct VariableTemplateTests {
    private let environments = [
        EnvironmentDraft(id: WorkspaceDraft.globalEnvironmentID, name: "Global", variables: [
            EnvironmentVariableDraft(key: "host", value: .literal("global.example.com"), order: 0),
            EnvironmentVariableDraft(key: "version", value: .literal("v1"), order: 1),
            EnvironmentVariableDraft(key: "disabled", value: .literal("x"), enabled: false, order: 2),
        ]),
        EnvironmentDraft(id: "staging", name: "Staging", variables: [
            EnvironmentVariableDraft(key: "host", value: .literal("staging.example.com"), order: 0),
            EnvironmentVariableDraft(key: "token", value: .secret("staging.token"), order: 1),
        ]),
        EnvironmentDraft(id: "production", name: "Production", variables: [
            EnvironmentVariableDraft(key: "host", value: .literal("api.example.com"), order: 0),
        ]),
    ]

    @Test("References match the resolver: trimmed names, empty and unclosed ones ignored")
    func references() {
        let text = "https://{{host}}/{{ version }}/{{}}/x?{{open"
        let found = VariableTemplate.references(in: text)
        #expect(found.map(\.name) == ["host", "version"])
        #expect((text as NSString).substring(with: found[1].range) == "{{ version }}")
        #expect(VariableTemplate.references(in: "plain").isEmpty)
    }

    @Test("UTF-16 ranges stay correct after non-ASCII text")
    func utf16Ranges() throws {
        let text = "café 🚀 {{host}}"
        let reference = try #require(VariableTemplate.references(in: text).first)
        #expect((text as NSString).substring(with: reference.range) == "{{host}}")
    }

    @Test("An unfinished reference before the caret is offered for completion")
    func openReference() {
        let text = "https://{{ho"
        let open = VariableTemplate.openReference(in: text, caret: (text as NSString).length)
        #expect(open?.prefix == "ho")
        #expect(open?.replacementRange == NSRange(location: 10, length: 2))
        #expect(open?.isClosed == false)

        let closed = "{{}}/path"
        #expect(VariableTemplate.openReference(in: closed, caret: 2)?.isClosed == true)
        #expect(VariableTemplate.openReference(in: closed, caret: 2)?.prefix == "")
        #expect(VariableTemplate.openReference(in: "{{host}}/x", caret: 10) == nil)
        #expect(VariableTemplate.openReference(in: "{{a b", caret: 5) == nil)
        #expect(VariableTemplate.openReference(in: "{", caret: 1) == nil)
    }

    @Test("The selected environment overrides globals and disabled rows are hidden")
    func catalogMerging() {
        let catalog = VariableCatalog(environments: environments, selectedEnvironmentID: "staging")
        #expect(catalog.entries.map(\.name) == ["host", "token", "version"])
        #expect(catalog.entry(named: "host")?.displayValue == "staging.example.com")
        #expect(catalog.entry(named: "host")?.environmentName == "Staging")
        #expect(catalog.entry(named: "version")?.environmentName == "Global")
        #expect(catalog.entry(named: "disabled") == nil)

        let global = VariableCatalog(environments: environments, selectedEnvironmentID: nil)
        #expect(global.entry(named: "host")?.displayValue == "global.example.com")
        #expect(global.entry(named: "token") == nil)
    }

    @Test("Secret references never show secret material")
    func secretsAreMasked() {
        let catalog = VariableCatalog(environments: environments, selectedEnvironmentID: "staging")
        #expect(catalog.entry(named: "token")?.displayValue == VariableCatalog.secretPlaceholder)
        let summary = catalog.summary(for: "Bearer {{token}}") ?? ""
        #expect(summary == "token = •••• (secret) (Staging)")
        #expect(!summary.contains("staging.token"))
    }

    @Test("Summaries list each reference once and flag undefined names")
    func summary() {
        let catalog = VariableCatalog(environments: environments, selectedEnvironmentID: "production")
        #expect(catalog.summary(for: "{{host}}/{{host}}/{{missing}}") == "host = api.example.com (Production)\nmissing: not defined in the active environments")
        #expect(catalog.summary(for: "no variables") == nil)
    }

    @Test("Completions rank prefix matches before substring matches")
    func completions() {
        let catalog = VariableCatalog(environments: environments, selectedEnvironmentID: "staging")
        #expect(catalog.completions(matching: "").map(\.name) == ["host", "token", "version"])
        #expect(catalog.completions(matching: "o").map(\.name) == ["host", "token", "version"])
        #expect(catalog.completions(matching: "T").map(\.name) == ["token", "host"])
    }
}
