use super::{
    Arc, IndexOptions, ResponseCancellation, ResponseResult, TextIndex, TextShaper, fs, size,
};
use crate::response::ResponseError;
use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex},
    time::SystemTime,
};
#[derive(Clone, Debug, PartialEq)]
struct CacheKey {
    options: IndexOptions,
    size: u64,
    modified: Option<SystemTime>,
}
#[derive(Debug)]
struct Flight {
    key: CacheKey,
    result: Mutex<Option<ResponseResult<Arc<TextIndex>>>>,
    ready: Condvar,
}
#[derive(Debug, Default)]
struct CacheState {
    entries: VecDeque<(CacheKey, Arc<TextIndex>)>,
    flights: Vec<Arc<Flight>>,
}
#[derive(Debug)]
pub struct TextIndexCache {
    capacity: usize,
    state: Mutex<CacheState>,
}
impl TextIndexCache {
    #[must_use]
    pub fn new(capacity: u64) -> Self {
        Self {
            capacity: size(capacity),
            state: Mutex::default(),
        }
    }
    /// # Errors
    /// Returns I/O or cancellation errors. Cancelled callers never publish a partial index.
    pub fn index(
        &self,
        options: IndexOptions,
        first_viewport: bool,
        shaper: Option<Arc<dyn TextShaper>>,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Arc<TextIndex>> {
        cancellation.check()?;
        let metadata = fs::metadata(&options.path)?;
        let key = CacheKey {
            options: options.clone(),
            size: metadata.len(),
            modified: metadata.modified().ok(),
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(position) = state.entries.iter().position(|(k, _)| *k == key)
            && let Some(entry) = state.entries.remove(position)
        {
            let index = Arc::clone(&entry.1);
            state.entries.push_back(entry);
            return Ok(index);
        }
        // Full scans share a flight. An individual cancelled waiter cannot cancel
        // the owner's scan or poison the result for other callers.
        if !first_viewport
            && let Some(flight) = state.flights.iter().find(|f| f.key == key).cloned()
        {
            drop(state);
            let mut result = flight
                .result
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                cancellation.check()?;
                if let Some(completed) = &*result {
                    if matches!(completed, Err(ResponseError::Cancelled)) {
                        drop(result);
                        return self.index(options, first_viewport, shaper, cancellation);
                    }
                    return completed.clone();
                }
                result = flight
                    .ready
                    .wait_timeout(result, std::time::Duration::from_millis(10))
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
        }
        let flight = Arc::new(Flight {
            key: key.clone(),
            result: Mutex::new(None),
            ready: Condvar::new(),
        });
        if !first_viewport {
            state.flights.push(Arc::clone(&flight));
        }
        drop(state);
        let result = TextIndex::new(
            options,
            if first_viewport { 256 } else { u64::MAX },
            shaper,
            cancellation,
        )
        .map(Arc::new);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Ok(index) = &result
            && !first_viewport
            && self.capacity > 0
            && index.row_count < 4_000_000
        {
            state.entries.retain(|(k, _)| *k != key);
            state.entries.push_back((key, Arc::clone(index)));
            while state.entries.len() > self.capacity {
                state.entries.pop_front();
            }
        }
        *flight
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result.clone());
        flight.ready.notify_all();
        state.flights.retain(|f| !Arc::ptr_eq(f, &flight));
        drop(state);
        result
    }
}
