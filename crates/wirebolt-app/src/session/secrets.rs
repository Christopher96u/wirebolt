//! Secret editing and release policy. Material is never part of a workspace snapshot.
use super::run::lock;
use crate::domain::{DomainValueSource, request::owned_reference};
use async_trait::async_trait;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::Path,
    sync::Mutex,
};
#[async_trait]
pub trait SessionSecretPlatform: Send + Sync {
    async fn read(&self, name: String) -> Result<Option<String>, ()>;
    async fn save(&self, name: String, value: String) -> Result<(), ()>;
    async fn remove(&self, name: String) -> Result<(), ()>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecretAccessError;
impl fmt::Display for SecretAccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("secret storage failed")
    }
}
impl std::error::Error for SecretAccessError {}
impl From<()> for SecretAccessError {
    fn from((): ()) -> Self {
        Self
    }
}
#[derive(Default)]
struct SecretState {
    edited: HashMap<String, String>,
    dirty: HashSet<String>,
    observed: HashSet<String>,
}
impl fmt::Debug for SecretState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretState")
            .field("edited_count", &self.edited.len())
            .field("dirty_count", &self.dirty.len())
            .field("observed_count", &self.observed.len())
            .finish()
    }
}
#[derive(Debug, Default)]
pub struct SessionSecrets {
    state: Mutex<SecretState>,
}
impl SessionSecrets {
    pub fn edit_batch(&self, values: HashMap<String, String>) {
        let mut state = lock(&self.state);
        for (name, value) in values {
            state.dirty.insert(name.clone());
            state.edited.insert(name, value);
        }
    }
    pub fn reset_editor(&self) {
        let mut s = lock(&self.state);
        s.edited.clear();
        s.dirty.clear();
    }
    pub fn clear_observed(&self) {
        lock(&self.state).observed.clear();
    }
    pub fn observe(&self, names: Vec<String>) {
        lock(&self.state)
            .observed
            .extend(names.into_iter().filter(|name| owned_reference(name)));
    }
    #[must_use]
    pub fn has_observed(&self) -> bool {
        !lock(&self.state).observed.is_empty()
    }
    #[must_use]
    pub fn material(&self, source: &DomainValueSource) -> String {
        match source {
            DomainValueSource::Literal { value } => value.clone(),
            DomainValueSource::Secret { secret } => lock(&self.state)
                .edited
                .get(secret)
                .cloned()
                .unwrap_or_default(),
        }
    }
    /// # Errors
    /// Returns a platform credential error.
    pub async fn load(
        &self,
        name: String,
        platform: &dyn SessionSecretPlatform,
    ) -> Result<String, SecretAccessError> {
        if let Some(value) = lock(&self.state).edited.get(&name).cloned() {
            return Ok(value);
        }
        let value = platform.read(name.clone()).await?.unwrap_or_default();
        Ok(lock(&self.state)
            .edited
            .entry(name)
            .or_insert(value)
            .clone())
    }
    /// # Errors
    /// Stops on the first failed platform write; successful writes stay acknowledged.
    pub async fn flush(
        &self,
        platform: &dyn SessionSecretPlatform,
    ) -> Result<(), SecretAccessError> {
        loop {
            let pending: Vec<_> = {
                let s = lock(&self.state);
                s.dirty
                    .iter()
                    .filter_map(|n| Some((n.clone(), s.edited.get(n)?.clone())))
                    .collect()
            };
            if pending.is_empty() {
                return Ok(());
            }
            for (name, value) in pending {
                self.observe(vec![name.clone()]);
                platform.save(name.clone(), value.clone()).await?;
                let mut s = lock(&self.state);
                if s.edited.get(&name) == Some(&value) {
                    s.dirty.remove(&name);
                }
            }
        }
    }
    /// # Errors
    /// Stops on the first failed platform write.
    pub async fn save(
        &self,
        values: HashMap<String, String>,
        platform: &dyn SessionSecretPlatform,
    ) -> Result<(), SecretAccessError> {
        let mut values: Vec<_> = values.into_iter().collect();
        values.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in values {
            self.observe(vec![name.clone()]);
            platform.save(name.clone(), value.clone()).await?;
            let mut s = lock(&self.state);
            s.dirty.remove(&name);
            s.edited.insert(name, value);
        }
        Ok(())
    }
    /// # Errors
    /// Returns a platform credential error. Missing credentials remain missing.
    pub async fn copy(
        &self,
        renames: HashMap<String, String>,
        platform: &dyn SessionSecretPlatform,
    ) -> Result<HashMap<String, String>, SecretAccessError> {
        let mut renames: Vec<_> = renames.into_iter().collect();
        renames.sort_by(|a, b| a.0.cmp(&b.0));
        let mut copied = HashMap::new();
        for (old, new) in renames {
            let (dirty, cached) = {
                let s = lock(&self.state);
                (s.dirty.contains(&old), s.edited.get(&old).cloned())
            };
            let value = if dirty {
                cached
            } else {
                platform.read(old).await?.or(cached)
            };
            if let Some(value) = value {
                platform.save(new.clone(), value.clone()).await?;
                lock(&self.state).edited.insert(new.clone(), value.clone());
                self.observe(vec![new.clone()]);
                copied.insert(new, value);
            }
        }
        Ok(copied)
    }
    pub async fn purge(
        &self,
        in_use: Vec<String>,
        others: Vec<String>,
        platform: &dyn SessionSecretPlatform,
    ) {
        let mut released: HashSet<_> = lock(&self.state)
            .observed
            .difference(&in_use.into_iter().collect())
            .cloned()
            .collect();
        let shared = scan_references(&released.iter().cloned().collect::<Vec<_>>(), &others);
        lock(&self.state)
            .observed
            .retain(|name| !shared.contains(name));
        released.retain(|name| !shared.contains(name));
        let mut released: Vec<_> = released.into_iter().collect();
        released.sort();
        for name in released {
            if platform.remove(name.clone()).await.is_ok() {
                let mut s = lock(&self.state);
                s.edited.remove(&name);
                s.dirty.remove(&name);
                s.observed.remove(&name);
            }
        }
    }
}
#[must_use]
pub fn scan_references(candidates: &[String], workspaces: &[String]) -> HashSet<String> {
    let mut remaining: HashSet<_> = candidates.iter().cloned().collect();
    let mut found = HashSet::new();
    for root in workspaces {
        let root = Path::new(root);
        let mut files = toml_files(&root.join("environments"));
        if let Ok(collections) = std::fs::read_dir(root.join("collections")) {
            for collection in collections.flatten() {
                files.extend(toml_files(&collection.path().join("requests")));
            }
        }
        for file in files {
            if remaining.is_empty() {
                return found;
            }
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            for name in &remaining {
                if text.contains(&format!("\"{name}\"")) || text.contains(&format!("'{name}'")) {
                    found.insert(name.clone());
                }
            }
            remaining.retain(|name| !found.contains(name));
        }
    }
    found
}
fn toml_files(directory: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect()
}
impl SessionSecrets {
    #[must_use]
    pub fn cached(&self) -> HashMap<String, String> {
        lock(&self.state).edited.clone()
    }
    #[must_use]
    pub fn dirty(&self) -> Vec<String> {
        lock(&self.state).dirty.iter().cloned().collect()
    }
}
