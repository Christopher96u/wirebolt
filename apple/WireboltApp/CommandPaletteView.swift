import AppKit
import SwiftUI

/// Mounted only while presented. Query observation stays inside this subtree.
struct CommandPaletteView: View {
    let controller: CommandPaletteController
    @Environment(\.wireboltTheme) private var theme
    @Environment(\.wireboltReduceMotion) private var reduceMotion
    @Environment(\.colorSchemeContrast) private var contrast

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .top) {
                theme.definition.modalScrim.color.opacity(theme.isDark ? 0.60 : 0.30)
                    .contentShape(.rect)
                    .onTapGesture { controller.dismiss(restoringFocus: true) }
                    .accessibilityHidden(true)
                if let snapshot = controller.projection.snapshot {
                    panel(snapshot, height: geometry.size.height)
                        .frame(width: min(600, max(0, geometry.size.width - 32)))
                        .background(theme.definition.overlay.color, in: .rect(cornerRadius: theme.geometry.card))
                        .overlay { RoundedRectangle(cornerRadius: theme.geometry.card).strokeBorder(contrast == .increased ? theme.primary.color : theme.cardBorder.color, lineWidth: contrast == .increased ? 2 : 1) }
                        .shadow(color: theme.definition.modalScrim.color.opacity(0.35), radius: 24, y: 8)
                        .padding(.top, min(64, max(16, geometry.size.height * 0.08)))
                        .accessibilityElement(children: .contain)
                        .accessibilityLabel("Command Palette")
                        .accessibilityAddTraits(.isModal)
                }
            }
            .transaction { if reduceMotion { $0.animation = nil } }
            .onAppear { controller.announceSelection() }
        }
    }
    private func panel(_ snapshot: PaletteSnapshot, height: CGFloat) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                if snapshot.page != .root {
                    Button { controller.backOrDismiss() } label: { Image(systemName: "chevron.left") }
                        .buttonStyle(.plain).accessibilityLabel("Back to actions")
                }
                Text(snapshot.pageTitle)
                    .font(.system(size: 11, weight: .medium)).foregroundStyle(theme.muted.color)
                Spacer()
                Button { controller.dismiss(restoringFocus: true) } label: { Image(systemName: "xmark") }
                    .buttonStyle(.plain).accessibilityLabel("Close Command Palette")
            }.padding(.horizontal, 16).padding(.top, 14)
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass").foregroundStyle(theme.muted.color).accessibilityHidden(true)
                CommandPaletteSearch(controller: controller, query: snapshot.query, placeholder: snapshot.searchPlaceholder, label: snapshot.searchLabel, color: theme.primary.nsColor)
                    .frame(height: 30)
                    .accessibilityLabel(snapshot.searchLabel)
            }.padding(.horizontal, 16).padding(.vertical, 12)
            if snapshot.page == .root {
                HStack(spacing: 4) {
                    filter("All", .all, snapshot: snapshot)
                    filter("Requests", .requests, snapshot: snapshot)
                    filter("Actions", .actions, snapshot: snapshot)
                    Spacer()
                }.padding(.horizontal, 12).padding(.bottom, 10)
            }
            Divider().overlay(theme.hairline.color)
            ScrollViewReader { scroll in
                ScrollView {
                    LazyVStack(spacing: 2) {
                        if snapshot.results.isEmpty {
                            VStack(spacing: 8) {
                                Text("No results").font(.system(size: 14, weight: .medium))
                                Text("Try a request name, method, or action.").font(.system(size: 12)).foregroundStyle(theme.muted.color)
                            }.frame(maxWidth: .infinity).padding(.vertical, 36).accessibilityElement(children: .combine)
                        }
                        ForEach(snapshot.results, id: \.id) { result in
                            row(result, selected: result.id == snapshot.selectedId).id(result.id)
                        }
                    }.padding(6)
                }.frame(height: min(max(100, height - 240), min(340, CGFloat(max(1, snapshot.results.count)) * 52 + 12)))
                .onChange(of: snapshot.selectedId) {
                    if let id = snapshot.selectedId { scroll.scrollTo(id, anchor: nil) }
                }
            }
            if let failure = controller.failure {
                Text(failure).font(.system(size: 12)).foregroundStyle(theme.definition.error.color)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(12).accessibilityLabel("Unable to confirm. \(failure)")
            }
            Divider().overlay(theme.hairline.color)
            HStack(spacing: 14) {
                Text("↑ ↓ navigate")
                Text("↵ confirm")
                Text(snapshot.page == .root ? "esc close" : "⌫ / esc back")
                Spacer(minLength: 0)
                Text(snapshot.resultSummary)
            }.font(.system(size: 10)).foregroundStyle(theme.muted.color).padding(.horizontal, 16).padding(.vertical, 11)
        }.foregroundStyle(theme.primary.color)
    }
    private func filter(_ title: String, _ filter: PaletteFilter, snapshot: PaletteSnapshot) -> some View {
        let selected = snapshot.effectiveFilter == filter
        return Button(title) { controller.query(snapshot.query, filter: filter) }
            .buttonStyle(.plain).font(.system(size: 11, weight: selected ? .medium : .regular))
            .padding(.horizontal, 10).padding(.vertical, 5)
            .background(selected ? theme.definition.selected.color : .clear, in: .rect(cornerRadius: 5))
            .accessibilityLabel("\(title) filter").accessibilityAddTraits(selected ? .isSelected : [])
    }
    private func row(_ result: PaletteResult, selected: Bool) -> some View {
        Button { controller.confirm(result.id) } label: {
            HStack(spacing: 10) {
                if !result.method.isEmpty {
                    MethodChip(method: HTTPMethod(rawValue: result.method)!, webSocket: result.webSocket)
                        .frame(width: 46)
                } else {
                    Image(systemName: symbol(result.destination)).font(.system(size: 15))
                        .foregroundStyle(theme.muted.color).frame(width: 46).accessibilityHidden(true)
                }
                VStack(alignment: .leading, spacing: 4) {
                    Text(result.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    if let reason = result.unavailableReason {
                        Text(reason).font(.system(size: 11)).foregroundStyle(theme.muted.color).lineLimit(2)
                    } else if !result.subtitle.isEmpty {
                        Text(result.subtitle).font(.system(size: 11)).foregroundStyle(theme.muted.color).lineLimit(2)
                    }
                }
                Spacer(minLength: 4)
                if !result.marker.isEmpty { Text(result.marker).font(.system(size: 10)).foregroundStyle(theme.muted.color) }
                if result.current { Image(systemName: "checkmark").accessibilityHidden(true) }
                else { Text(result.shortcut).font(.system(size: 10, design: .monospaced)).foregroundStyle(theme.muted.color) }
            }.padding(.horizontal, 10).frame(minHeight: 50).frame(maxWidth: .infinity, alignment: .leading)
                .background(selected ? theme.definition.selected.color : .clear, in: .rect(cornerRadius: 6))
                .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .onHover { if $0 { controller.highlight(result.id) } }
        .accessibilityLabel(result.accessibilityLabel)
        .accessibilityValue(result.unavailableReason == nil ? "Available" : "Unavailable")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
    private func symbol(_ destination: PaletteDestination) -> String {
        switch destination {
        case .environment: "globe"
        case .theme: "circle.lefthalf.filled"
        case .tab: "rectangle.on.rectangle"
        default: "command"
        }
    }
}

/// AppKit keeps marked-text composition and native search editing intact.
struct CommandPaletteSearch: NSViewRepresentable {
    let controller: CommandPaletteController
    let query: String
    let placeholder: String
    let label: String
    let color: NSColor
    func makeCoordinator() -> Coordinator { Coordinator(controller: controller) }
    func makeNSView(context: Context) -> NSTextField {
        let field = PaletteSearchField()
        field.delegate = context.coordinator
        field.placeholderString = placeholder
        field.setAccessibilityLabel(label)
        field.font = .systemFont(ofSize: 17)
        field.isBordered = false
        field.drawsBackground = false
        field.focusRingType = .none
        field.stringValue = query
        return field
    }
    func updateNSView(_ field: NSTextField, context: Context) {
        field.placeholderString = placeholder
        field.setAccessibilityLabel(label)
        field.textColor = color
        if field.stringValue != query, (field.currentEditor() as? NSTextView)?.hasMarkedText() != true { field.stringValue = query }
    }
    final class Coordinator: NSObject, NSTextFieldDelegate {
        let controller: CommandPaletteController
        init(controller: CommandPaletteController) { self.controller = controller }
        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSTextField else { return }
            controller.query(field.stringValue)
        }
    }
    private final class PaletteSearchField: NSTextField {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            guard let window else { return }
            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window, self.window === window else { return }
                window.makeFirstResponder(self)
            }
        }
    }
}
