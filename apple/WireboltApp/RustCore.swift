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
        let runtimeReady = wirebolt_runtime_warmup()

        precondition(
            handshake.streamAbiVersion == streamABIVersion && runtimeReady == 1,
            "Swift bindings and stream ABI are out of sync"
        )

        return CoreStatus(
            product: handshake.product,
            coreVersion: handshake.coreVersion,
            streamABIVersion: streamABIVersion
        )
    }
}
