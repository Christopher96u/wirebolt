//! Wirebolt's application core: the state and logic behind the interface, without any
//! interface or platform API. The macOS app renders this state and implements the platform
//! services it needs; see `docs/specs/rust-app-core.md`.

pub mod domain;
pub mod response;
pub mod session;

/// Recovers application state after a worker panic.
pub fn lock<T>(value: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_lock_recovers_poisoned_state() {
        let state = std::sync::Mutex::new(vec![1]);
        let _ = std::panic::catch_unwind(|| {
            let mut value = super::lock(&state);
            value.push(2);
            panic!("worker failed");
        });
        assert!(state.is_poisoned());
        assert_eq!(*super::lock(&state), [1, 2]);
    }
}
