import SwiftUI
import UniformTypeIdentifiers

struct AppearanceSettingsView: View {
    @Environment(\.wireboltTheme) private var theme
    @Environment(\.customThemeStore) private var injectedStore
    private var store: ThemeStore { injectedStore ?? .shared }
    @State private var editing: ThemePreview?
    @State private var fromColors = false
    @State private var importing = false
    @State private var readingImport = false
    @State private var renaming: CustomTheme?
    @State private var deleting: CustomTheme?
    @AppStorage("interfaceAppearance") private var selection = "system"
    @AppStorage("interfaceDensity") private var density = "comfortable"
    @AppStorage("editor.fontSize") private var fontSize = 12.0
    @AppStorage("interfaceReduceMotion") private var reducesMotion = false

    /// Following the system still marks the preset currently in use, so one card is always selected.
    private var selectedPreset: ThemePreset? { activeCustom == nil ? theme.definition.preset : nil }

    private var activeCustom: CustomTheme? { store.customThemes.first { $0.id == selection } }

    var body: some View {
        SettingsPage(title: "Appearance", subtitle: "Choose a contrast-checked theme. Text ≥ 7:1, muted text ≥ 4.5:1.") {
            HStack {
                Spacer()
                Toggle("Follow System", isOn: Binding(get: { selection == "system" },
                    set: { selection = $0 ? "system" : theme.definition.preset.rawValue }))
                    .toggleStyle(.wireboltSwitch).controlSize(.small)
                    .font(.system(size: theme.typography.settingsDetail))
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 172, maximum: 172), spacing: 14)], alignment: .leading, spacing: 20) {
                ForEach(ThemePreset.allCases, id: \.self) { preset in
                    ThemePreviewCard(name: preset.name, preview: ResolvedTheme(preset: preset), selected: selectedPreset == preset && activeCustom == nil) { selection = preset.rawValue }
                }
                ForEach(store.customThemes, id: \.id) { custom in
                    ThemePreviewCard(name: custom.name, preview: ResolvedTheme(custom: custom), selected: selection == custom.id) { selection = custom.id }
                        .contextMenu {
                            Button("Rename…") { store.clearError(); renaming = custom }
                            Button("Delete…", role: .destructive) { deleting = custom }
                        }
                }
                customThemeCard
            }
            if let message = store.errorMessage {
                HStack {
                    Text(message).foregroundStyle(theme.definition.error.color).accessibilityLabel("Theme error: \(message)")
                    Spacer()
                    Button("Dismiss theme error", systemImage: "xmark") { store.clearError() }.labelStyle(.iconOnly)
                }
            }
            ForEach(store.notices, id: \.self) { Text($0).font(.callout).foregroundStyle(theme.muted.color) }
            if let custom = activeCustom {
                SettingsCard {
                    SettingsRow(title: custom.name, detail: "Custom theme", last: true) {
                        HStack {
                            Button("Rename…") { store.clearError(); renaming = custom }
                            Button("Delete…", role: .destructive) { deleting = custom }
                        }.disabled(!store.isWritable)
                    }
                }
            }
            SettingsCard {
                SettingsRow(title: "Canvas & variables", detail: "Theme colors; controls stay neutral.") {
                    HStack(spacing: WireboltTheme.Spacing.xLarge) {
                        swatch(theme.definition.window)
                        swatch(theme.definition.variableForeground)
                    }
                }
                SettingsRow(title: "Contrast", detail: "Checked against the window, sidebar and card surfaces.") {
                    let surfaces = [theme.definition.window, theme.definition.sidebar, theme.definition.card]
                    let primary = surfaces.map { theme.primary.contrast(with: $0) }.min() ?? 0
                    let muted = surfaces.map { theme.muted.contrast(with: $0) }.min() ?? 0
                    Label(String(format: "Text %.1f:1 · Muted %.1f:1", primary, muted), systemImage: "checkmark")
                        .foregroundStyle(theme.definition.success.color)
                }
                SettingsRow(title: "VS Code theme", detail: "Import a .json colour theme, or drop it here. Comments and trailing commas are accepted.") {
                    Button(readingImport ? "Importing…" : "Import…", systemImage: "doc.text") { store.clearError(); importing = true }
                        .disabled(readingImport || !store.isWritable)
                }
                SettingsRow(title: "Density", detail: "Row height in the sidebar, tabs and fields.") {
                    Picker("Density", selection: $density) {
                        Text("Compact").tag("compact")
                        Text("Comfortable").tag("comfortable")
                    }.labelsHidden().pickerStyle(.segmented).tint(WireboltTheme.controlAccent).frame(width: 220)
                }
                SettingsRow(title: "Editor font", detail: "Request and response editors.") {
                    HStack(spacing: WireboltTheme.Spacing.medium) {
                        Text("System monospaced").foregroundStyle(theme.definition.secondary.color)
                        Picker("Editor font size", selection: $fontSize) {
                            ForEach(Array(10...20) + [24, 28], id: \.self) { Text("\($0) pt").tag(Double($0)) }
                        }.labelsHidden().pickerStyle(.menu).tint(WireboltTheme.muted).frame(width: 90)
                    }
                }
                SettingsRow(title: "Reduce motion", detail: "macOS Reduce Motion always takes priority.", last: true) {
                    Toggle("Reduce motion", isOn: $reducesMotion).labelsHidden().toggleStyle(.wireboltSwitch)
                }
            }
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.json]) { result in
            switch result {
            case .success(let url): importTheme(url)
            case .failure(let error): store.report(error)
            }
        }
        .dropDestination(for: URL.self) { urls, _ in
            guard let url = urls.first, store.isWritable, !readingImport else { return false }
            importTheme(url)
            return true
        }
        .sheet(isPresented: Binding(get: { editing != nil }, set: { if !$0 { editing = nil } })) {
            if let editing {
                CustomThemeEditor(initial: editing, fromColors: fromColors, store: store) { selection = $0.id }
                    .environment(\.wireboltTheme, theme)
            }
        }
        .sheet(isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            if let renaming { RenameThemeSheet(custom: renaming, store: store) }
        }
        .confirmationDialog("Delete \(deleting?.name ?? "theme")?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }), titleVisibility: .visible, presenting: deleting) { custom in
            Button("Delete Theme", role: .destructive) {
                let previousSelection = selection
                Task {
                    if let fallback = await store.delete(id: custom.id, selection: previousSelection), selection == previousSelection { selection = fallback }
                }
            }
        } message: { _ in Text("Deleting the active theme returns to Follow System.") }
    }

    private func importTheme(_ url: URL) {
        store.clearError()
        readingImport = true
        Task {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() }; readingImport = false }
            if let preview = await store.importTheme(from: url) { fromColors = false; editing = preview }
        }
    }

    private var customThemeCard: some View {
        Button {
            store.clearError()
            fromColors = true
            editing = domainDeriveTheme(background: theme.definition.window, accent: theme.definition.variableForeground)
        } label: {
            VStack(spacing: WireboltTheme.Spacing.large) {
                Label("From 2 colors", systemImage: "plus")
                    .font(.system(size: theme.typography.settingsDetail))
                    .frame(width: 172, height: 112)
                    .overlay {
                        RoundedRectangle(cornerRadius: theme.geometry.row)
                            .strokeBorder(theme.definition.dim.color, style: StrokeStyle(lineWidth: 1, dash: [5, 4]))
                    }
                HStack {
                    Text("Custom")
                    Spacer()
                    Text("Generate").font(.system(size: theme.typography.settingsDetail))
                }
            }
            .foregroundStyle(theme.muted.color)
            .frame(width: 172)
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .disabled(!store.isWritable)
        .accessibilityLabel("Custom theme from 2 colors")
        .help("Derive a complete theme from a background and an accent colour.")
    }

    private func swatch(_ color: PaletteColor) -> some View {
        HStack(spacing: WireboltTheme.Spacing.medium) {
            RoundedRectangle(cornerRadius: theme.geometry.chip)
                .fill(color.color).frame(width: 18, height: 18)
                .overlay { RoundedRectangle(cornerRadius: theme.geometry.chip).strokeBorder(theme.cardBorder.color) }
            Text(String(format: "#%02X%02X%02X", Int(color.red), Int(color.green), Int(color.blue)))
                .font(.system(size: theme.typography.settingsDetail, design: .monospaced))
        }.foregroundStyle(theme.definition.secondary.color)
    }
}

struct ThemePreviewCard: View {
    let name: String
    let preview: ResolvedTheme
    let selected: Bool
    let action: () -> Void
    @Environment(\.wireboltTheme) private var theme

    var body: some View {
        Button(action: action) {
            VStack(spacing: WireboltTheme.Spacing.large) {
                Canvas { context, _ in
                    func rect(_ x: Double, _ y: Double, _ width: Double, _ height: Double, _ color: PaletteColor, radius: Double = 0) {
                        context.fill(Path(roundedRect: CGRect(x: x, y: y, width: width, height: height),
                            cornerRadius: radius), with: .color(color.color))
                    }
                    let p = preview.definition
                    rect(0, 0, 172, 112, p.window)
                    rect(0, 0, 50, 112, p.sidebar)
                    rect(50, 0, 1, 112, p.hairline)
                    rect(5, 24, 40, 12, p.selected, radius: 3)
                    for (y, width) in [(10.0, 22.0), (42, 30), (58, 26), (74, 32)] {
                        rect(9, y, width, 4, p.dim, radius: 2)
                    }
                    rect(9, 28, 24, 4, preview.primary, radius: 2)
                    rect(60, 10, 104, 34, p.card, radius: 7)
                    rect(67, 18, 54, 4, p.secondary, radius: 2)
                    rect(67, 32, 16, 6, p.methodBackgrounds[1], radius: 2)
                    context.fill(Path(ellipseIn: CGRect(x: 148, y: 30, width: 10, height: 10)), with: .color(p.sendBackground.color))
                    for (y, width, color) in [(56.0, 70.0, p.syntaxKey), (66, 88, p.syntaxString),
                        (76, 46, p.syntaxNumber), (86, 60, p.syntaxKey)] {
                        rect(64, y, width, 4, color, radius: 2)
                    }
                }
                .frame(width: 172, height: 112)
                .clipShape(.rect(cornerRadius: theme.geometry.row))
                .overlay {
                    RoundedRectangle(cornerRadius: theme.geometry.row)
                        .strokeBorder(selected ? theme.primary.color : preview.cardBorder.color, lineWidth: selected ? 2 : 1)
                }
                .accessibilityHidden(true)
                HStack {
                    Text(name).lineLimit(1)
                    Spacer()
                    Text(preview.isDark ? "Dark" : "Light")
                        .font(.system(size: theme.typography.settingsDetail)).foregroundStyle(theme.muted.color)
                }
            }
            .frame(width: 172)
            .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(name) theme")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
