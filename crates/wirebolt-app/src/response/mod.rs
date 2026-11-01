//! File-backed response presentation and local runtime repositories.
//!
//! Coordinates exposed to text editors are UTF-16; body anchors are byte offsets.
//! Native font measurement is supplied in batches through `TextShaper`.
mod cookie_date;
mod folding;
mod formatting;
mod index;
mod json;
mod markdown;
mod regex;
mod storage;
mod text;
mod wrapping;

pub use folding::*;
pub use formatting::*;
pub use index::*;
pub use json::*;
pub use markdown::*;
pub use regex::{PortableRegexEngine, RegexEngine};
pub use storage::*;
pub use text::*;
pub use wrapping::*;

use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResponseError {
    Io { reason: String },
    InvalidJson,
    PresentationLimit,
    InvalidPattern { reason: String },
    Cancelled,
}
impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { reason } | Self::InvalidPattern { reason } => f.write_str(reason),
            Self::InvalidJson => f.write_str("Invalid JSON"),
            Self::PresentationLimit => f.write_str("JSON presentation limit exceeded"),
            Self::Cancelled => f.write_str("Cancelled"),
        }
    }
}
impl std::error::Error for ResponseError {}
impl From<std::io::Error> for ResponseError {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            reason: error.to_string(),
        }
    }
}
pub type ResponseResult<T> = Result<T, ResponseError>;

#[derive(Debug, Default)]
pub struct ResponseCancellation(AtomicBool);
impl ResponseCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    /// # Errors
    /// Returns `Cancelled` after cancellation is requested.
    pub fn check(&self) -> ResponseResult<()> {
        if self.0.load(Ordering::Relaxed) {
            Err(ResponseError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResponseTextRange {
    pub location: u64,
    pub length: u64,
}
impl ResponseTextRange {
    #[must_use]
    pub fn end(self) -> u64 {
        self.location.saturating_add(self.length)
    }
}

// The supported macOS target has 64-bit pointers. Foreign lengths are clamped so
// invalid bridge input cannot wrap an allocation or a slice bound.
fn size(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}
fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
