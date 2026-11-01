//! Wirebolt's application core: the state and logic behind the interface, without any
//! interface or platform API. The macOS app renders this state and implements the platform
//! services it needs; see `docs/specs/rust-app-core.md`.

pub mod domain;
pub mod response;
pub mod session;
