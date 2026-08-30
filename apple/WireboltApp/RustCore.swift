import WireboltStreamFFI

struct CoreStatus: Equatable, Sendable {
    let product: String
    let coreVersion: String
    let streamABIVersion: UInt32
}

struct RustCore: Sendable {
    func status() -> CoreStatus {
        let handshake = coreHandshake()
        let streamABIVersion = wirebolt_stream_abi_version()

        precondition(
            handshake.streamAbiVersion == streamABIVersion,
            "Swift bindings and stream ABI are out of sync"
        )

        return CoreStatus(
            product: handshake.product,
            coreVersion: handshake.coreVersion,
            streamABIVersion: streamABIVersion
        )
    }
}
