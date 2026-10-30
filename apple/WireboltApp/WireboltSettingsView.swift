import AppKit
import SwiftUI

private enum SettingsSection: String, CaseIterable, Identifiable {
    case general, appearance, network, keyboard, privacy
    var id: String { rawValue }
    var title: String { rawValue.capitalized }
    var symbol: String {
        switch self {
        case .general: "slider.horizontal.3"
        case .appearance: "paintpalette"
        case .network: "network"
        case .keyboard: "keyboard"
        case .privacy: "checkmark.shield"
        }
    }
    var keywords: String {
        switch self {
        case .general: "workspace folder editor word wrap invisible characters scroll beyond"
        case .appearance: "theme black paper ember colors canvas accent density font reduce motion"
        case .network: "proxy connection manual system direct authentication credentials routes"
        case .keyboard: "shortcuts keybindings commands navigation"
        case .privacy: "analytics crash reports data keychain policy"
        }
    }
}

struct WireboltSettingsView: View {
    @Bindable var model: WireboltModel
    @Environment(\.wireboltTheme) private var theme
    @State private var visitedNetwork = false
    @State private var search = ""
    @FocusState private var searchIsFocused: Bool
    private var section: SettingsSection { SettingsSection(rawValue: model.settingsTab) ?? .appearance }
    private var filteredSections: [SettingsSection] {
        let query = search.trimmingCharacters(in: .whitespacesAndNewlines)
        return query.isEmpty ? SettingsSection.allCases : SettingsSection.allCases.filter {
            ($0.title + " " + $0.keywords).localizedCaseInsensitiveContains(query)
        }
    }

    var body: some View {
        GeometryReader { geometry in
            HStack(spacing: 0) {
                sidebar.frame(width: geometry.size.width >= 950 ? 260 : 208)
                Rectangle().fill(theme.hairline.color).frame(width: 1)
                VStack(spacing: 0) {
                    HStack(spacing: WireboltTheme.Spacing.medium) {
                        Text("Settings").foregroundStyle(theme.muted.color)
                        Text("/").foregroundStyle(theme.definition.dim.color)
                        Text(section.title).fontWeight(.medium)
                        Spacer()
                        Button("Close Settings", systemImage: "xmark", action: close)
                            .labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
                    }
                    .padding(.horizontal, WireboltTheme.Spacing.xxLarge)
                    .frame(height: 60)
                    Rectangle().fill(theme.hairline.color).frame(height: 1)
                    pages
                }
            }
        }
        .font(.system(size: theme.typography.settingsLabel))
        .controlSize(.large)
        .foregroundStyle(theme.primary.color)
        .background(theme.definition.window.color)
        .onChange(of: section, initial: true) { if section == .network { visitedNetwork = true } }
        .onChange(of: search) {
            if !filteredSections.contains(section), let first = filteredSections.first { model.settingsTab = first.rawValue }
        }
        .task(id: model.isShowingAppSettings) {
            if model.isShowingAppSettings { search = ""; searchIsFocused = true }
            else { searchIsFocused = false }
        }
        .onExitCommand(perform: close)
    }

    private var pages: some View {
        ZStack(alignment: .topLeading) {
            if filteredSections.isEmpty {
                ContentUnavailableView.search(text: search)
            } else {
                switch section {
                case .general: GeneralSettingsPage(model: model, openWorkspaceSettings: openWorkspaceSettings)
                case .appearance: AppearanceSettingsView()
                case .keyboard: KeyboardSettingsPage()
                case .privacy: PrivacySettingsPage()
                case .network: EmptyView()
                }
            }
            if visitedNetwork || section == .network {
                let active = model.isShowingAppSettings && section == .network && !filteredSections.isEmpty
                NetworkSettingsPage(model: model, scope: .app, isActive: active)
                    .opacity(section == .network && !filteredSections.isEmpty ? 1 : 0)
                    .allowsHitTesting(active)
                    .accessibilityHidden(!active)
                    .disabled(!active)
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: WireboltTheme.Spacing.medium) {
                Button("Search settings", systemImage: "magnifyingglass") { searchIsFocused = true }
                    .labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
                    .keyboardShortcut("f", modifiers: .command)
                TextField("Search", text: $search)
                    .textFieldStyle(.plain).focused($searchIsFocused)
                    .accessibilityLabel("Search settings")
                if !search.isEmpty {
                    Button("Clear settings search", systemImage: "xmark") { search = "" }
                        .labelStyle(.iconOnly).buttonStyle(GhostIconStyle())
                }
            }
            .padding(.horizontal, WireboltTheme.Spacing.large)
            .frame(height: 60)
            VStack(spacing: WireboltTheme.Spacing.xxSmall) {
                ForEach(filteredSections) { item in
                    Button { model.settingsTab = item.rawValue } label: {
                        Label(item.title, systemImage: item.symbol)
                            .labelStyle(.titleAndIcon)
                            .font(.system(size: theme.typography.settingsLabel, weight: section == item ? .medium : .regular))
                            .foregroundStyle(section == item ? theme.primary.color : theme.muted.color)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, WireboltTheme.Spacing.large)
                            .frame(minHeight: theme.density == .compact ? 32 : 40)
                            .background(section == item ? theme.definition.selected.color : .clear,
                                in: .rect(cornerRadius: theme.geometry.row))
                            .contentShape(.rect)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(item.title)
                    .accessibilityAddTraits(section == item ? .isSelected : [])
                }
            }
            .padding(.horizontal, WireboltTheme.Spacing.large)
            .onMoveCommand { direction in
                let items = filteredSections
                guard let index = items.firstIndex(of: section) else { return }
                if direction == .up { model.settingsTab = items[max(0, index - 1)].rawValue }
                else if direction == .down { model.settingsTab = items[min(items.count - 1, index + 1)].rawValue }
            }
            Spacer()
            Button(action: close) {
                HStack {
                    Text("Back to workspace")
                    Spacer()
                    Text("esc").font(.system(size: theme.typography.settingsDetail))
                        .padding(.horizontal, WireboltTheme.Spacing.small)
                        .padding(.vertical, WireboltTheme.Spacing.xxSmall)
                        .overlay { RoundedRectangle(cornerRadius: theme.geometry.chip).strokeBorder(theme.cardBorder.color) }
                }
                .foregroundStyle(theme.muted.color)
                .padding(WireboltTheme.Spacing.xxLarge)
                .frame(minHeight: theme.density.controlHeight)
                .contentShape(.rect)
            }.buttonStyle(.plain).accessibilityLabel("Back to workspace")
        }
        .background(theme.definition.card.color)
    }

    private func close() { commitPendingEdits(in: NSApp.keyWindow); model.isShowingAppSettings = false }
    private func openWorkspaceSettings() { close(); model.isShowingWorkspaceSettings = true }
}
