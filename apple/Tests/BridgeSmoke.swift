import WireboltStreamFFI

@main
enum BridgeSmoke {
    static func main() {
        let handshake = coreHandshake()
        let streamABIVersion = wirebolt_stream_abi_version()

        guard handshake.product == "Wirebolt" else {
            fatalError("unexpected product")
        }
        guard handshake.streamAbiVersion == streamABIVersion else {
            fatalError("bridge versions disagree")
        }

        print("bridge_smoke=passed core=\(handshake.coreVersion) stream_abi=\(streamABIVersion)")
    }
}
