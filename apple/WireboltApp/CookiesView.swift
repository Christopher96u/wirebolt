import SwiftUI

/// Workspace ▸ Cookies…: the cookies the open workspace's requests received.
struct CookiesView: View {
    static let windowID = "cookies"

    let model: WireboltModel
    @State private var cookies: [CookieSnapshot] = []
    @State private var selection: Set<CookieSnapshot.ID> = []
    @State private var showsValues = false
    @State private var isConfirmingClear = false

    var body: some View {
        VStack(spacing: 0) {
            Table(cookies, selection: $selection) {
                TableColumn("Domain") { cookie in
                    Text(cookie.hostOnly ? cookie.domain : "." + cookie.domain)
                        .help(cookie.hostOnly ? "Sent only to \(cookie.domain)" : "Sent to \(cookie.domain) and its subdomains")
                }
                TableColumn("Name", value: \.name)
                TableColumn("Value") { cookie in
                    Text(showsValues ? cookie.value : String(repeating: "•", count: min(max(cookie.value.count, 4), 12)))
                        .lineLimit(1)
                        .textSelection(.enabled)
                        .accessibilityLabel(showsValues ? cookie.value : "Hidden value")
                }
                TableColumn("Path", value: \.path)
                TableColumn("Expires") { cookie in
                    Text(cookie.expiresAt?.formatted(date: .abbreviated, time: .shortened) ?? "End of session")
                        .foregroundStyle(cookie.expiresAt == nil ? .secondary : .primary)
                }
                TableColumn("Flags") { cookie in
                    Text(flags(cookie)).foregroundStyle(.secondary)
                }
            }
            .contextMenu(forSelectionType: CookieSnapshot.ID.self) { ids in
                Button(ids.count > 1 ? "Delete \(ids.count) Cookies" : "Delete") { delete(ids) }
                    .disabled(ids.isEmpty)
            }
            .onDeleteCommand { delete(selection) }
            .overlay {
                if cookies.isEmpty {
                    ContentUnavailableView(
                        "No Cookies",
                        systemImage: "tray",
                        description: Text("Cookies set by responses in this workspace appear here. Session cookies are kept until Wirebolt quits.")
                    )
                }
            }
            Divider()
            HStack {
                Toggle("Show Values", isOn: $showsValues)
                Spacer()
                Text(cookies.count == 1 ? "1 cookie" : "\(cookies.count) cookies")
                    .foregroundStyle(.secondary)
                Button("Delete") { delete(selection) }
                    .disabled(selection.isEmpty)
                Button("Clear All…") { isConfirmingClear = true }
                    .disabled(cookies.isEmpty)
            }
            .padding(12)
        }
        .frame(minWidth: 640, idealWidth: 760, minHeight: 300, idealHeight: 420)
        .navigationTitle(model.hasLoadedWorkspace ? "Cookies — \(model.workspace.name)" : "Cookies")
        .task(id: model.cookieRevision) {
            cookies = await model.cookies()
            selection.formIntersection(cookies.map(\.id))
        }
        .confirmationDialog("Delete all cookies in this workspace?", isPresented: $isConfirmingClear) {
            Button("Clear All", role: .destructive) { Task { await model.clearCookies() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Requests stop sending these cookies. Other workspaces keep their own cookies.")
        }
    }

    private func flags(_ cookie: CookieSnapshot) -> String {
        var flags: [String] = []
        if cookie.secure { flags.append("Secure") }
        if cookie.httpOnly { flags.append("HttpOnly") }
        if !cookie.sameSite.isEmpty { flags.append("SameSite=\(cookie.sameSite)") }
        return flags.joined(separator: " · ")
    }

    private func delete(_ ids: Set<CookieSnapshot.ID>) {
        guard !ids.isEmpty else { return }
        Task {
            for id in ids { await model.deleteCookie(id: id) }
        }
    }
}
