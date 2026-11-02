import AppKit
import SwiftUI

/// Sheet state is presentation only; every palette change is derived and checked in Rust.
struct CustomThemeEditor: View {
    let fromColors: Bool
    let store: ThemeStore
    let save: (CustomTheme) -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.wireboltTheme) private var theme
    @State private var name: String
    @State private var preview: ThemePreview
    @State private var background: Color
    @State private var accent: Color
    @State private var saving = false
    @State private var increasedContrast = false

    init(initial: ThemePreview, fromColors: Bool, store: ThemeStore, save: @escaping (CustomTheme) -> Void) {
        self.fromColors = fromColors
        self.store = store
        self.save = save
        // A new sheet owns a one-time editable seed, never a changing parent selection.
        _name = State(initialValue: initial.name)
        _preview = State(initialValue: initial)
        _background = State(initialValue: initial.tokens.window.color)
        _accent = State(initialValue: initial.tokens.variableForeground.color)
    }
    private var resolved: ResolvedTheme {
        ResolvedTheme(custom: CustomTheme(id: "preview", name: name, tokens: preview.tokens), increasedContrast: increasedContrast)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text(fromColors ? "From 2 colors" : "Import VS Code theme").font(.title2.weight(.semibold))
            TextField("Theme name", text: $name).textFieldStyle(.roundedBorder)
                .accessibilityLabel("Theme name")
            if fromColors {
                HStack(spacing: 28) {
                    ColorPicker("Background", selection: $background, supportsOpacity: false)
                    ColorPicker("Accent", selection: $accent, supportsOpacity: false)
                }
                .onChange(of: background, derive)
                .onChange(of: accent, derive)
            }
            ThemeSample().environment(\.wireboltTheme, resolved)
            Toggle("Preview Increase Contrast", isOn: $increasedContrast)
            if !preview.notices.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Import and contrast adjustments").font(.callout.weight(.medium))
                    ScrollView {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(preview.notices, id: \.self) { Text($0).frame(maxWidth: .infinity, alignment: .leading) }
                        }
                    }.frame(maxHeight: 110)
                }.font(.callout).foregroundStyle(theme.muted.color)
            }
            if let message = store.errorMessage { Text(message).foregroundStyle(theme.definition.error.color).accessibilityLabel("Theme error: \(message)") }
            HStack {
                Button("Cancel") { store.clearError(); dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button(saving ? "Saving…" : "Save Theme") {
                    saving = true
                    Task {
                        if let saved = await store.save(name: name, preview: preview) { save(saved); dismiss() }
                        saving = false
                    }
                }.keyboardShortcut(.defaultAction).disabled(saving || !store.isWritable)
            }
        }
        .padding(24).frame(width: 540)
        .foregroundStyle(theme.primary.color).background(theme.definition.window.color)
        .interactiveDismissDisabled(saving)
    }
    private func derive() {
        guard let background = NSColor(background).usingColorSpace(.sRGB),
              let accent = NSColor(accent).usingColorSpace(.sRGB) else { return }
        func rgb(_ c: NSColor) -> RgbColor {
            func channel(_ value: CGFloat) -> UInt8 {
                guard value.isFinite else { return 0 }
                return UInt8(min(255, max(0, (value * 255).rounded())))
            }
            return RgbColor(red: channel(c.redComponent), green: channel(c.greenComponent), blue: channel(c.blueComponent))
        }
        preview = domainDeriveTheme(background: rgb(background), accent: rgb(accent))
    }
}

struct ThemeSample: View {
    @Environment(\.wireboltTheme) private var theme
    var body: some View {
        let d = theme.definition
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Preview").font(.headline)
                Spacer()
                Text(theme.isDark ? "Dark" : "Light").foregroundStyle(theme.muted.color)
            }
            HStack {
                ForEach([HTTPMethod.get, .post, .put, .patch, .delete], id: \.self) { MethodChip(method: $0) }
                Spacer()
            }
            HStack {
                Text("{{base_url}}").foregroundStyle(d.variableForeground.color)
                    .padding(5).background(d.variableBackground.color, in: .rect(cornerRadius: 4))
                Text("/v1/example").foregroundStyle(theme.primary.color)
                Spacer()
                Text("Send").foregroundStyle(d.sendForeground.color).padding(.horizontal, 14).padding(.vertical, 5)
                    .background(d.sendBackground.color, in: .rect(cornerRadius: 6))
            }
            VStack(alignment: .leading, spacing: 5) {
                Text("// A response in your theme").foregroundStyle(d.dim.color)
                Text("{").foregroundStyle(d.syntaxPunctuation.color)
                HStack(spacing: 0) {
                    Text("  \"message\": ").foregroundStyle(d.syntaxKey.color)
                    Text("\"Hello, Wirebolt\"").foregroundStyle(d.syntaxString.color)
                    Text(",").foregroundStyle(d.syntaxPunctuation.color)
                }
                HStack(spacing: 0) {
                    Text("  \"count\": ").foregroundStyle(d.syntaxKey.color)
                    Text("42").foregroundStyle(d.syntaxNumber.color)
                    Text(", \"ready\": ").foregroundStyle(d.syntaxKey.color)
                    Text("true").foregroundStyle(d.syntaxKeyword.color)
                }
                Text("}").foregroundStyle(d.syntaxPunctuation.color)
            }.font(.system(size: 13, design: .monospaced))
            HStack(spacing: 20) {
                Label("200 OK", systemImage: "checkmark.circle.fill").foregroundStyle(d.success.color)
                Label("Warning", systemImage: "exclamationmark.triangle.fill").foregroundStyle(d.warning.color)
                Label("Error", systemImage: "xmark.octagon.fill").foregroundStyle(d.error.color)
            }.font(.caption)
        }
        .padding(20).background(d.card.color, in: .rect(cornerRadius: 12))
        .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(theme.cardBorder.color) }
        .accessibilityElement(children: .contain).accessibilityLabel("Theme live preview")
    }
}

struct RenameThemeSheet: View {
    let custom: CustomTheme
    let store: ThemeStore
    @Environment(\.dismiss) private var dismiss
    @State private var name: String
    @State private var saving = false
    init(custom: CustomTheme, store: ThemeStore) {
        self.custom = custom; self.store = store
        _name = State(initialValue: custom.name)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("Rename Theme").font(.title2.weight(.semibold))
            TextField("Theme name", text: $name).textFieldStyle(.roundedBorder).accessibilityLabel("Theme name")
            if let error = store.errorMessage { Text(error).foregroundStyle(.red) }
            HStack {
                Button("Cancel") { store.clearError(); dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("Rename") {
                    saving = true
                    Task { if await store.rename(id: custom.id, name: name) { dismiss() }; saving = false }
                }.keyboardShortcut(.defaultAction).disabled(saving)
            }
        }.padding(24).frame(width: 380).interactiveDismissDisabled(saving)
    }
}
