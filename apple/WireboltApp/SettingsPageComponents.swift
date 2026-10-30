import SwiftUI

struct SettingsPage<Content: View>: View {
    let title: String
    let subtitle: String
    @ViewBuilder var content: Content
    @Environment(\.wireboltTheme) private var theme

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: WireboltTheme.Spacing.section) {
                VStack(alignment: .leading, spacing: WireboltTheme.Spacing.small) {
                    Text(title).font(WireboltTheme.Typography.title)
                    Text(subtitle).font(.system(size: theme.typography.settingsLabel))
                        .foregroundStyle(theme.muted.color)
                        .fixedSize(horizontal: false, vertical: true)
                }
                content
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(.horizontal, WireboltTheme.Spacing.canvas)
            .padding(.vertical, WireboltTheme.Spacing.section)
            .frame(maxWidth: .infinity, alignment: .top)
        }
        .font(.system(size: theme.typography.settingsLabel))
        .background(theme.definition.window.color)
    }
}

struct SettingsCard<Content: View>: View {
    @ViewBuilder var content: Content
    @Environment(\.wireboltTheme) private var theme

    var body: some View {
        VStack(spacing: 0) { content }
            .background(theme.definition.card.color)
            .clipShape(.rect(cornerRadius: theme.geometry.card))
            .overlay { RoundedRectangle(cornerRadius: theme.geometry.card).strokeBorder(theme.cardBorder.color) }
    }
}

struct SettingsRow<Control: View>: View {
    let title: String
    let detail: String
    var last = false
    @ViewBuilder var control: Control
    @Environment(\.wireboltTheme) private var theme

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: WireboltTheme.Spacing.xLarge) {
                VStack(alignment: .leading, spacing: WireboltTheme.Spacing.xSmall) {
                    Text(title).font(.system(size: theme.typography.settingsLabel))
                    if !detail.isEmpty {
                        Text(detail).font(.system(size: theme.typography.settingsDetail))
                            .foregroundStyle(theme.muted.color)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: WireboltTheme.Spacing.medium)
                control
                    .font(.system(size: theme.typography.settingsDetail))
                    .frame(minHeight: theme.density.controlHeight)
            }
            .padding(.horizontal, WireboltTheme.Spacing.xxLarge)
            .padding(.vertical, theme.density == .compact ? WireboltTheme.Spacing.large : 14)
            if !last { Rectangle().fill(theme.hairline.color).frame(height: 1) }
        }
    }
}
