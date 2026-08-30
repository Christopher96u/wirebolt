import SwiftUI

struct ContentView: View {
    let status: CoreStatus

    var body: some View {
        VStack(spacing: 12) {
            Text(status.product)
                .font(.system(size: 34, weight: .bold, design: .rounded))

            Text("Native shell connected to Rust \(status.coreVersion)")
                .foregroundStyle(.secondary)

            Text("Stream ABI \(status.streamABIVersion)")
                .font(.caption.monospaced())
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(40)
    }
}
