//! Versioned local theme library. Mutations publish only after an atomic write succeeds.
use super::{ThemeSelection, ThemeTokens, custom};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const VERSION: u32 = 2;
const NEWER_LIBRARY_MESSAGE: &str = "Custom themes were saved by a newer Wirebolt version. Update Wirebolt before changing custom themes; the file has been left unchanged.";
const MAX_LIBRARY_BYTES: usize = 4 * 1_048_576;
const MAX_THEMES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub tokens: ThemeTokens,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeLibrarySnapshot {
    pub themes: Vec<CustomTheme>,
    pub notices: Vec<String>,
    pub writable: bool,
}
#[derive(Debug, Serialize, Deserialize)]
struct Document {
    version: u32,
    themes: Vec<CustomTheme>,
}

#[derive(Debug)]
pub struct ThemeLibrary {
    path: PathBuf,
    snapshot: ThemeLibrarySnapshot,
    preserve_corrupt: bool,
}
impl ThemeLibrary {
    #[must_use]
    pub fn open(path: &Path) -> Self {
        let mut library = Self {
            path: path.to_owned(),
            snapshot: ThemeLibrarySnapshot {
                themes: vec![],
                notices: vec![],
                writable: true,
            },
            preserve_corrupt: false,
        };
        if !path.exists() {
            return library;
        }
        match load(path) {
            Ok(snapshot) => library.snapshot = snapshot,
            Err(message) => {
                library.snapshot.notices.push(format!("Custom themes could not be loaded. {message} Built-in themes are still available. The original file will be preserved when you save a theme."));
                library.preserve_corrupt = true;
            }
        }
        library
    }
    #[must_use]
    pub fn snapshot(&self) -> ThemeLibrarySnapshot {
        self.snapshot.clone()
    }

    /// # Errors
    /// Returns an error for invalid names/tokens, library limits or failed persistence.
    pub fn add(&mut self, name: &str, tokens: ThemeTokens) -> Result<CustomTheme, String> {
        let name = checked_name(name)?;
        if self.snapshot.themes.len() >= MAX_THEMES {
            return Err(
                "You can save up to 64 custom themes. Delete a theme before adding another.".into(),
            );
        }
        let theme = CustomTheme {
            id: format!("custom:{}", crate::domain::identity::unique_id()),
            name,
            tokens: custom::checked(tokens, false).0,
        };
        let mut themes = self.snapshot.themes.clone();
        themes.push(theme.clone());
        self.persist(themes)?;
        Ok(theme)
    }
    /// # Errors
    /// Returns an error for missing themes, invalid names or failed persistence.
    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = checked_name(name)?;
        let mut themes = self.snapshot.themes.clone();
        let theme = themes
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("This custom theme no longer exists.")?;
        theme.name = name;
        self.persist(themes)
    }
    /// Preference storage is a platform adapter; fallback policy stays here.
    /// # Errors
    /// Returns an error for missing themes or failed persistence.
    pub fn delete(
        &mut self,
        id: &str,
        selection: &ThemeSelection,
    ) -> Result<ThemeSelection, String> {
        let themes: Vec<_> = self
            .snapshot
            .themes
            .iter()
            .filter(|t| t.id != id)
            .cloned()
            .collect();
        if themes.len() == self.snapshot.themes.len() {
            return Err("This custom theme no longer exists.".into());
        }
        self.persist(themes)?;
        Ok(
            if matches!(selection, ThemeSelection::Custom { id: selected } if selected == id) {
                ThemeSelection::System
            } else {
                selection.clone()
            },
        )
    }

    fn persist(&mut self, themes: Vec<CustomTheme>) -> Result<(), String> {
        if !self.snapshot.writable {
            return Err(NEWER_LIBRARY_MESSAGE.into());
        }
        let document = Document {
            version: VERSION,
            themes,
        };
        let bytes = serde_json::to_vec_pretty(&document).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_LIBRARY_BYTES {
            return Err("The custom theme library is too large.".into());
        }
        if self.preserve_corrupt && self.path.exists() {
            let backup = self.path.with_extension(format!(
                "corrupt-{}.json",
                crate::domain::identity::unique_id()
            ));
            fs::copy(&self.path, backup)
                .map_err(|e| format!("Could not preserve custom themes: {e}"))?;
        }
        crate::response::atomic_write(&self.path, &bytes)
            .map_err(|e| format!("Could not save custom themes: {e}"))?;
        self.snapshot.themes = document.themes;
        self.preserve_corrupt = false;
        Ok(())
    }
}

fn load(path: &Path) -> Result<ThemeLibrarySnapshot, String> {
    let bytes = super::read_bounded(path, MAX_LIBRARY_BYTES, "custom theme library")?;
    let mut header: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "The theme library is corrupt.")?;
    let version = header
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or("The theme library has no valid version.")?;
    if version > u64::from(VERSION) {
        return Ok(ThemeLibrarySnapshot {
            themes: vec![],
            writable: false,
            notices: vec![NEWER_LIBRARY_MESSAGE.into()],
        });
    }
    if version == 1 {
        migrate_method_tokens(&mut header)?;
    } else if version != u64::from(VERSION) {
        return Err(format!("Theme library version {version} is not supported."));
    }
    let mut document: Document =
        serde_json::from_value(header).map_err(|_| "The theme library is corrupt.")?;
    if document.themes.len() > MAX_THEMES {
        return Err("The theme library contains too many themes.".into());
    }
    let mut ids = HashSet::new();
    for theme in &mut document.themes {
        if !theme.id.starts_with("custom:") || theme.id.len() != 43 || !ids.insert(theme.id.clone())
        {
            return Err("The theme library contains invalid theme identifiers.".into());
        }
        theme.name = checked_name(&theme.name)?;
        theme.tokens = custom::checked(theme.tokens.clone(), false).0;
    }
    Ok(ThemeLibrarySnapshot {
        themes: document.themes,
        notices: vec![],
        writable: true,
    })
}
fn checked_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("Give the theme a name of 1–64 characters, without control characters.".into());
    }
    Ok(name.into())
}

fn migrate_method_tokens(document: &mut serde_json::Value) -> Result<(), String> {
    let themes = document
        .get_mut("themes")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("The theme library is corrupt.")?;
    for theme in themes {
        let tokens = theme
            .get_mut("tokens")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("The theme library is corrupt.")?;
        let backgrounds = tokens
            .remove("method_backgrounds")
            .and_then(|value| value.as_array().cloned())
            .ok_or("The custom theme has an incomplete method palette.")?;
        let foregrounds = tokens
            .remove("method_foregrounds")
            .and_then(|value| value.as_array().cloned())
            .ok_or("The custom theme has an incomplete method palette.")?;
        let names = [
            "get",
            "post",
            "put",
            "patch",
            "delete",
            "head",
            "options",
            "query",
            "custom",
            "web_socket",
        ];
        if backgrounds.len() != names.len() || foregrounds.len() != names.len() {
            return Err("The custom theme has an incomplete method palette.".into());
        }
        let methods: serde_json::Map<_, _> = names
            .into_iter()
            .zip(backgrounds.into_iter().zip(foregrounds))
            .map(|(name, (background, foreground))| {
                (
                    name.into(),
                    serde_json::json!({"background": background, "foreground": foreground}),
                )
            })
            .collect();
        tokens.insert("methods".into(), methods.into());
    }
    document["version"] = VERSION.into();
    Ok(())
}
