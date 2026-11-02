//! Versioned local theme library. Mutations publish only after an atomic write succeeds.
use super::{ThemeTokens, custom, vscode};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const VERSION: u32 = 1;
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
        custom::validate(&tokens)?;
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
    pub fn delete(&mut self, id: &str, selection: &str) -> Result<String, String> {
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
        Ok(if selection == id { "system" } else { selection }.into())
    }

    fn persist(&mut self, themes: Vec<CustomTheme>) -> Result<(), String> {
        if !self.snapshot.writable {
            return Err("This theme library was saved by a newer Wirebolt version. Update Wirebolt before changing custom themes.".into());
        }
        let document = Document {
            version: VERSION,
            themes,
        };
        let bytes = serde_json::to_vec_pretty(&document).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_LIBRARY_BYTES {
            return Err("The custom theme library is too large.".into());
        }
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let write = || -> Result<(), std::io::Error> {
            fs::create_dir_all(parent)?;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(&bytes)?;
            file.as_file().sync_all()?;
            if self.preserve_corrupt && self.path.exists() {
                let backup = self.path.with_extension(format!(
                    "corrupt-{}.json",
                    crate::domain::identity::unique_id()
                ));
                fs::rename(&self.path, backup)?;
            }
            file.persist(&self.path).map_err(|e| e.error)?;
            Ok(())
        };
        write().map_err(|e| format!("Could not save custom themes: {e}"))?;
        self.snapshot.themes = document.themes;
        self.preserve_corrupt = false;
        Ok(())
    }
}

fn load(path: &Path) -> Result<ThemeLibrarySnapshot, String> {
    let bytes = vscode::read_bounded(path, MAX_LIBRARY_BYTES)?;
    let header: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "The theme library is corrupt.")?;
    let version = header
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or("The theme library has no valid version.")?;
    if version > u64::from(VERSION) {
        return Ok(ThemeLibrarySnapshot { themes: vec![], writable: false, notices: vec!["Custom themes were saved by a newer Wirebolt version. Update Wirebolt to use them; the file has been left unchanged.".into()] });
    }
    if version != u64::from(VERSION) {
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
        custom::validate(&theme.tokens)?;
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
