uniffi::setup_scaffolding!();

#[derive(Debug, Eq, PartialEq, uniffi::Record)]
pub struct CoreHandshake {
    pub product: String,
    pub core_version: String,
    pub stream_abi_version: u32,
}

#[uniffi::export]
#[must_use]
pub fn core_handshake() -> CoreHandshake {
    wirebolt_core::handshake().into()
}

#[unsafe(no_mangle)]
pub extern "C" fn wirebolt_stream_abi_version() -> u32 {
    wirebolt_core::STREAM_ABI_VERSION
}

impl From<wirebolt_core::CoreHandshake> for CoreHandshake {
    fn from(value: wirebolt_core::CoreHandshake) -> Self {
        Self {
            product: value.product.to_owned(),
            core_version: value.core_version.to_owned(),
            stream_abi_version: value.stream_abi_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_bridge_surfaces_share_the_same_version() {
        let handshake = core_handshake();

        assert_eq!(handshake.product, "Wirebolt");
        assert_eq!(handshake.stream_abi_version, wirebolt_stream_abi_version());
    }
}
