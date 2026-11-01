//! Collection-source recognition, bounded Bruno filesystem bundles and import summary labels.
use serde::Serialize;
use std::fmt::Write as _;
use std::{fs, path::Path};
pub use wirebolt_core::ImportFormat;
#[must_use]
pub fn detect_collection(
    extension: &str,
    contents: &str,
    root: Option<&serde_json::Value>,
) -> Option<ImportFormat> {
    if extension.eq_ignore_ascii_case("bru") {
        return Some(ImportFormat::BrunoFolder);
    }
    let Some(root) = root else {
        return contents
            .split('\n')
            .filter(|s| !s.is_empty())
            .take(40)
            .any(|l| {
                let l = l.trim().replace(['\"', '\''], "");
                [
                    "type: collection.insomnia.rest/5",
                    "type: spec.insomnia.rest/5",
                    "type: environment.insomnia.rest/5",
                    "__export_format:",
                ]
                .iter()
                .any(|p| l.starts_with(p))
            })
            .then_some(ImportFormat::Insomnia);
    };
    if root.get("__export_format").is_some()
        && (root.get("resources").is_some()
            || root.get("_type").and_then(serde_json::Value::as_str) == Some("export"))
    {
        return Some(ImportFormat::Insomnia);
    }
    if root
        .get("version")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| !s.is_empty())
        && root.get("name").is_some_and(serde_json::Value::is_string)
        && root.get("items").is_some_and(serde_json::Value::is_array)
    {
        return Some(ImportFormat::Bruno);
    }
    None
}
#[must_use]
pub fn detect(extension: &str, contents: &str) -> Option<ImportFormat> {
    let trimmed = contents.trim();
    let command = trimmed
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or_default();
    if command.starts_with("curl ") || command.starts_with("curl\t") {
        return Some(ImportFormat::Curl);
    }
    let root: Option<serde_json::Value> = serde_json::from_str(trimmed)
        .ok()
        .filter(serde_json::Value::is_object);
    let Some(root) = root else {
        return detect_collection(extension, trimmed, None);
    };
    if root
        .get("log")
        .and_then(|l| l.get("entries"))
        .is_some_and(serde_json::Value::is_array)
    {
        return Some(ImportFormat::Har);
    }
    if root.get("info").is_some_and(|i| {
        i.is_object() && (i.get("schema").is_some() || i.get("_postman_id").is_some())
    }) {
        return Some(ImportFormat::PostmanV2);
    }
    if root.get("version").and_then(serde_json::Value::as_i64) == Some(1)
        && root.get("nodes").is_some_and(serde_json::Value::is_array)
    {
        return Some(ImportFormat::LegacyWorkspaceV1);
    }
    detect_collection(extension, trimmed, Some(&root)).or_else(|| {
        extension
            .eq_ignore_ascii_case("har")
            .then_some(ImportFormat::Har)
    })
}
#[must_use]
pub fn is_bruno_collection(path: &Path) -> bool {
    path.join("bruno.json").exists()
        || fs::read_dir(path).is_ok_and(|entries| {
            entries.flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .ends_with(".bru")
            })
        })
}
#[derive(Debug, Serialize)]
struct SourceFile {
    path: String,
    contents: String,
}
#[derive(Debug, Serialize)]
struct Bundle {
    root: String,
    files: Vec<SourceFile>,
}
/// A native package-folder decision; the core never imports an Apple API.
pub trait CollectionDirectoryPolicy: Send + Sync {
    fn is_package(&self, path: &Path) -> bool;
}
struct NoPackages;
impl CollectionDirectoryPolicy for NoPackages {
    fn is_package(&self, _path: &Path) -> bool {
        false
    }
}
/// # Errors
/// Returns `notACollection` for unreadable sources or `tooLarge` for a bundle over its limits.
pub fn bruno_bundle(path: &Path) -> Result<String, CollectionSourceError> {
    bruno_bundle_with_policy(path, &NoPackages)
}
/// # Errors
/// Returns `notACollection` for unreadable sources or `tooLarge` for a bundle over its limits.
pub fn bruno_bundle_with_policy(
    path: &Path,
    policy: &dyn CollectionDirectoryPolicy,
) -> Result<String, CollectionSourceError> {
    if !path.exists() {
        return Err(CollectionSourceError::NotACollection);
    }
    let files = if path.is_dir() {
        let root = path
            .canonicalize()
            .map_err(|_| CollectionSourceError::NotACollection)?;
        let mut files = vec![];
        let mut total = 0;
        collect(&root, &root, &mut files, &mut total, policy)?;
        if files.is_empty() {
            return Err(CollectionSourceError::NotACollection);
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files
    } else {
        vec![SourceFile {
            path: path
                .file_name()
                .ok_or(CollectionSourceError::NotACollection)?
                .to_string_lossy()
                .into_owned(),
            contents: fs::read_to_string(path)
                .map_err(|_| CollectionSourceError::NotACollection)?,
        }]
    };
    let root = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or_else(|| Path::new(""))
    };
    serde_json::to_string(&Bundle {
        root: root.to_string_lossy().into_owned(),
        files,
    })
    .map_err(|_| CollectionSourceError::NotACollection)
}
fn collect(
    root: &Path,
    current: &Path,
    files: &mut Vec<SourceFile>,
    total: &mut u64,
    policy: &dyn CollectionDirectoryPolicy,
) -> Result<(), CollectionSourceError> {
    for entry in fs::read_dir(current).map_err(|_| CollectionSourceError::NotACollection)? {
        let entry = entry.map_err(|_| CollectionSourceError::NotACollection)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|_| CollectionSourceError::NotACollection)?;
        if kind.is_dir() {
            if !policy.is_package(&path) {
                collect(root, &path, files, total, policy)?;
            }
            continue;
        }
        let name = name.to_lowercase();
        if !Path::new(&name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bru"))
            && name != "bruno.json"
            && name != "opencollection.yml"
        {
            continue;
        }
        let size = fs::metadata(&path)
            .map_err(|_| CollectionSourceError::NotACollection)?
            .len();
        *total = total.saturating_add(size);
        if files.len() >= 5000 || size > 2 * 1024 * 1024 || *total > 64 * 1024 * 1024 {
            return Err(CollectionSourceError::TooLarge);
        }
        let canonical = path
            .canonicalize()
            .map_err(|_| CollectionSourceError::NotACollection)?;
        let Ok(relative) = canonical.strip_prefix(root) else {
            continue;
        };
        files.push(SourceFile {
            path: relative.to_string_lossy().into_owned(),
            contents: fs::read_to_string(&path)
                .map_err(|_| CollectionSourceError::NotACollection)?,
        });
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PemIdentity {
    pub certificates: Vec<String>,
    pub private_key: String,
}
/// # Errors
/// Returns the missing, duplicate or encrypted identity component.
pub fn parse_pem(sources: &[String]) -> Result<PemIdentity, PemError> {
    let mut certificates = vec![];
    let mut keys = vec![];
    for source in sources {
        let mut label: Option<String> = None;
        let mut lines = vec![];
        for line in source.lines() {
            let line = line.trim();
            if label.is_none() {
                if let Some(name) = line
                    .strip_prefix("-----BEGIN ")
                    .and_then(|s| s.strip_suffix("-----"))
                {
                    label = Some(name.into());
                    lines = vec![line];
                }
            } else {
                lines.push(line);
                if label
                    .as_ref()
                    .is_some_and(|s| line == format!("-----END {s}-----"))
                {
                    let name = label.take().unwrap_or_default();
                    let text = lines.join("\n");
                    if name == "CERTIFICATE" {
                        certificates.push(text);
                    } else if name == "ENCRYPTED PRIVATE KEY"
                        || (name.ends_with("PRIVATE KEY")
                            && text.contains("Proc-Type: 4,ENCRYPTED"))
                    {
                        return Err(PemError::EncryptedPrivateKey);
                    } else if ["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY"]
                        .contains(&name.as_str())
                    {
                        keys.push(text);
                    }
                }
            }
        }
    }
    if certificates.is_empty() {
        return Err(PemError::NoCertificate);
    }
    if keys.is_empty() {
        return Err(PemError::NoPrivateKey);
    }
    if keys.len() != 1 {
        return Err(PemError::MultiplePrivateKeys);
    }
    Ok(PemIdentity {
        certificates,
        private_key: keys.remove(0),
    })
}
impl PemIdentity {
    #[must_use]
    pub fn combined(&self) -> String {
        self.certificates
            .iter()
            .chain(std::iter::once(&self.private_key))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }
}
#[must_use]
pub fn pem_error_message(kind: PemError) -> String {
    match kind {
        PemError::NoCertificate => "Choose a PEM file with a certificate (BEGIN CERTIFICATE).",
        PemError::MultiplePrivateKeys => "Choose only one private key.",
        PemError::EncryptedPrivateKey => {
            "Encrypted private keys aren’t supported. Export the key without a passphrase."
        }
        PemError::NoPrivateKey => {
            "Choose the certificate’s private key too, in the same PEM file or a second one."
        }
    }
    .into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PemError {
    NoCertificate,
    NoPrivateKey,
    MultiplePrivateKeys,
    EncryptedPrivateKey,
}
impl std::fmt::Display for PemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&pem_error_message(*self))
    }
}
impl std::error::Error for PemError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionSourceError {
    NotACollection,
    TooLarge,
}
impl std::fmt::Display for CollectionSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotACollection => "notACollection",
            Self::TooLarge => "tooLarge",
        })
    }
}
impl std::error::Error for CollectionSourceError {}

#[must_use]
pub fn import_headline(names: &[String], requests: i64, groups: i64) -> String {
    fn counted(n: i64, noun: &str) -> String {
        format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
    }
    let mut text = format!("Imported {}", counted(requests, "request"));
    if groups > 0 {
        let _ = write!(text, " in {}", counted(groups, "folder"));
    }
    match names {
        [] => text.push('.'),
        [name] => {
            let _ = write!(text, " into “{name}”.");
        }
        _ => {
            let _ = write!(text, " into {} collections.", names.len());
        }
    }
    text
}

#[must_use]
pub fn import_environment_line(names: &[String], localized_list: &str) -> Option<String> {
    match names {
        [] => None,
        [name] => Some(format!(
            "Created the environment “{name}” for its variables."
        )),
        _ => Some(format!(
            "Created {} environments: {localized_list}.",
            names.len()
        )),
    }
}
