//! Native theme adapters expose typed application commands; no policy lives here.
#![allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI lifts owned values at the boundary."
)]
use crate::{app_domain::DomainError, lock};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use wirebolt_app::domain::theme::{
    self, ResolvedTokens, RgbColor, ThemeTokens,
    custom::ThemePreview,
    library::{CustomTheme, ThemeLibrary, ThemeLibrarySnapshot},
};

#[uniffi::remote(Record)]
pub struct ThemePreview {
    pub name: String,
    pub tokens: ThemeTokens,
    pub notices: Vec<String>,
}
#[uniffi::remote(Record)]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub tokens: ThemeTokens,
}
#[uniffi::remote(Record)]
pub struct ThemeLibrarySnapshot {
    pub themes: Vec<CustomTheme>,
    pub notices: Vec<String>,
    pub writable: bool,
}

fn invalid(reason: String) -> DomainError {
    DomainError::Invalid { reason }
}

#[uniffi::export]
#[must_use]
pub fn domain_derive_theme(background: RgbColor, accent: RgbColor) -> ThemePreview {
    theme::custom::derive(background, accent)
}
#[uniffi::export]
#[must_use]
pub fn domain_resolve_custom_theme(tokens: ThemeTokens, increased: bool) -> ResolvedTokens {
    theme::custom::resolve(tokens, increased)
}
#[uniffi::export]
pub fn domain_import_vscode_theme(bytes: Vec<u8>) -> Result<ThemePreview, DomainError> {
    theme::vscode::parse(&bytes).map_err(invalid)
}
#[uniffi::export]
pub fn app_import_vscode_theme(path: String) -> Result<ThemePreview, DomainError> {
    theme::vscode::read(Path::new(&path)).map_err(invalid)
}

#[derive(Debug, uniffi::Object)]
pub struct AppThemeLibrary {
    inner: Mutex<ThemeLibrary>,
}
#[uniffi::export]
impl AppThemeLibrary {
    #[uniffi::constructor]
    #[must_use]
    pub fn open(path: String) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(ThemeLibrary::open(Path::new(&path))),
        })
    }
    #[must_use]
    pub fn snapshot(&self) -> ThemeLibrarySnapshot {
        lock(&self.inner).snapshot()
    }
    pub fn add(&self, name: String, tokens: ThemeTokens) -> Result<CustomTheme, DomainError> {
        lock(&self.inner).add(&name, tokens).map_err(invalid)
    }
    pub fn rename(&self, id: String, name: String) -> Result<(), DomainError> {
        lock(&self.inner).rename(&id, &name).map_err(invalid)
    }
    pub fn delete(&self, id: String, selection: String) -> Result<String, DomainError> {
        lock(&self.inner).delete(&id, &selection).map_err(invalid)
    }
}
