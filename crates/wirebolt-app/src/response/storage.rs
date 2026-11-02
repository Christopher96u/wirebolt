use super::{JsonDocument, ResponseCancellation, ResponseError, ResponseResult, count, size};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_segmentation::UnicodeSegmentation;
use url::Url;

pub const VIEWPORT_BYTES: usize = 32 * 1024;
pub const SEARCH_CHUNK_BYTES: usize = 256 * 1024;
#[derive(Clone, Debug, Default)]
pub struct BodyPreview {
    bytes: u64,
    preview: Vec<u8>,
    decoded: Option<(Vec<u8>, String)>,
}
impl BodyPreview {
    pub fn append(&mut self, data: &[u8]) {
        self.bytes += count(data.len());
        if self.preview.len() < VIEWPORT_BYTES {
            self.preview
                .extend_from_slice(&data[..data.len().min(VIEWPORT_BYTES - self.preview.len())]);
            self.decoded = None;
        }
    }
    #[must_use]
    pub fn progress(&mut self) -> BodyProgress {
        let (data, text) = self.decoded.get_or_insert_with(|| {
            let mut end = self.preview.len();
            if end > 0 {
                let mut lead = end - 1;
                while lead > 0 && self.preview[lead] & 0xc0 == 0x80 {
                    lead -= 1;
                }
                let byte = self.preview[lead];
                let expected = if byte & 0xf8 == 0xf0 {
                    4
                } else if byte & 0xf0 == 0xe0 {
                    3
                } else if byte & 0xe0 == 0xc0 {
                    2
                } else {
                    1
                };
                if end - lead < expected {
                    end = lead;
                }
            }
            let data = self.preview[..end].to_vec();
            let text = String::from_utf8_lossy(&data).into_owned();
            (data, text)
        });
        BodyProgress {
            bytes: self.bytes,
            data: data.clone(),
            text: text.clone(),
            truncated: self.bytes > count(data.len()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyProgress {
    pub bytes: u64,
    pub data: Vec<u8>,
    pub text: String,
    pub truncated: bool,
}
#[derive(Clone, Debug, Default)]
enum JsonCache {
    #[default]
    Unparsed,
    Parsed(Option<Arc<JsonDocument>>),
}
#[derive(Debug)]
pub struct BodyStore {
    path: PathBuf,
    owns_file: bool,
    writer: Option<File>,
    preview: BodyPreview,
    json: [JsonCache; 2],
    retained_directory: Option<tempfile::TempDir>,
}
impl BodyStore {
    /// # Errors
    /// Returns an I/O error when the body file cannot be created.
    pub fn new(run_id: &str, directory: &Path) -> ResponseResult<Self> {
        let path = directory.join(format!("wirebolt-response-{run_id}.body"));
        let writer = File::create(&path)?;
        Ok(Self {
            path,
            owns_file: true,
            writer: Some(writer),
            preview: BodyPreview::default(),
            json: [JsonCache::Unparsed, JsonCache::Unparsed],
            retained_directory: None,
        })
    }
    /// # Errors
    /// Returns an I/O error when the existing body cannot be inspected.
    pub fn existing(path: PathBuf) -> ResponseResult<Self> {
        let bytes = fs::metadata(&path)?.len();
        Ok(Self {
            path,
            owns_file: false,
            writer: None,
            preview: BodyPreview {
                bytes,
                ..BodyPreview::default()
            },
            json: [JsonCache::Unparsed, JsonCache::Unparsed],
            retained_directory: None,
        })
    }
    /// Copies a historical body into a temporary file owned by the displayed run.
    /// # Errors
    /// Returns an I/O error if the source cannot be copied.
    pub fn retained(path: &Path) -> ResponseResult<Self> {
        let directory = tempfile::tempdir_in(retained_root()?)?;
        let destination = directory.path().join("body");
        // The destination must not exist: fs::copy can then clone on APFS.
        fs::copy(path, &destination)?;
        let mut body = Self::existing(destination)?;
        body.owns_file = true;
        body.retained_directory = Some(directory);
        Ok(body)
    }
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    #[must_use]
    pub fn size(&self) -> u64 {
        self.preview.bytes
    }
    /// # Errors
    /// Returns an error for closed stores or failed disk writes.
    pub fn append(&mut self, data: &[u8]) -> ResponseResult<()> {
        self.json = [JsonCache::Unparsed, JsonCache::Unparsed];
        self.writer
            .as_mut()
            .ok_or_else(|| ResponseError::Io {
                reason: "Response body is closed".into(),
            })?
            .write_all(data)?;
        self.preview.append(data);
        Ok(())
    }
    #[must_use]
    pub fn progress(&mut self) -> BodyProgress {
        self.preview.progress()
    }
    /// # Errors
    /// Returns an I/O error if synchronising the body fails.
    pub fn finish(&mut self) -> ResponseResult<()> {
        if let Some(writer) = self.writer.take() {
            writer.sync_all()?;
        }
        Ok(())
    }
    /// # Errors
    /// Returns an I/O error if reading the viewport fails.
    pub fn viewport(&self, offset: u64, length: u64) -> ResponseResult<Vec<u8>> {
        let mut reader = File::open(&self.path)?;
        reader.seek(SeekFrom::Start(offset.min(self.preview.bytes)))?;
        let mut bytes = Vec::with_capacity(size(length.min(self.preview.bytes)));
        reader.take(length).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
    /// # Errors
    /// Returns I/O and cancellation errors. Invalid JSON has no presentation.
    pub fn formatted_json(
        &mut self,
        decode: bool,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Option<Arc<JsonDocument>>> {
        if self.writer.is_some() {
            return Ok(None);
        }
        let index = usize::from(decode);
        if let JsonCache::Parsed(cached) = &self.json[index] {
            return Ok(cached.clone());
        }
        let document = match JsonDocument::new(&self.path, decode, cancellation) {
            Ok(document) => Some(Arc::new(document)),
            Err(ResponseError::InvalidJson | ResponseError::PresentationLimit) => None,
            Err(error) => return Err(error),
        };
        if document
            .as_ref()
            .is_none_or(|d| !d.contains_unicode_escapes)
        {
            self.json[1 - index] = JsonCache::Parsed(document.clone());
        }
        self.json[index] = JsonCache::Parsed(document.clone());
        Ok(document)
    }
    /// # Errors
    /// Returns an I/O error when searching the body fails.
    pub fn count_occurrences(&self, query: &str) -> ResponseResult<u64> {
        let needle = query.as_bytes();
        if needle.is_empty() {
            return Ok(0);
        }
        let mut reader = File::open(&self.path)?;
        let mut buffer = vec![0; SEARCH_CHUNK_BYTES];
        let mut carry = Vec::new();
        let mut matches = 0;
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            carry.extend_from_slice(&buffer[..read]);
            let mut cursor = 0;
            while let Some(offset) = find_bytes(&carry[cursor..], needle) {
                cursor += offset + needle.len();
                matches += 1;
            }
            let keep = needle.len().saturating_sub(1).min(carry.len());
            carry.drain(..carry.len() - keep);
        }
        Ok(matches)
    }
    /// # Errors
    /// Returns an I/O error if the destination cannot be replaced.
    pub fn export(&self, destination: &Path) -> ResponseResult<()> {
        if let Some(writer) = &self.writer {
            writer.sync_all()?;
        }
        let source_path = fs::canonicalize(&self.path)?;
        if fs::canonicalize(destination).ok().as_ref() == Some(&source_path) {
            return Ok(());
        }
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::Builder::new().make_in(parent, |path| {
            File::options()
                .write(true)
                .read(true)
                .create_new(true)
                .open(path)
        })?;
        if destination.is_file() {
            // Copying onto the staging file preserves platform metadata where
            // fs::copy supports it, including extended attributes on macOS.
            fs::copy(destination, temporary.path())?;
            temporary.as_file().set_len(0)?;
        }
        let mut source = File::open(&self.path)?;
        std::io::copy(&mut source, &mut temporary)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(destination)
            .map_err(|error| ResponseError::from(error.error))?;
        Ok(())
    }
    pub fn remove(&mut self) {
        self.writer = None;
        if self.owns_file {
            let _ = fs::remove_file(&self.path);
        }
    }
}
impl Drop for BodyStore {
    fn drop(&mut self) {
        self.remove();
    }
}
pub(super) fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Shared session records are persisted directly; the repository owns ordering,
/// quotas and disk I/O while preserving the existing history file format.
pub use crate::session::history::SessionHistoryEntry as HistoryEntry;
use crate::session::history::SessionRequestIdentity;

const PREVIOUS_HISTORY_INDEX_VERSION: u32 = 2;

#[derive(Deserialize)]
struct HistoryIndex {
    version: u32,
    entries: Vec<HistoryEntry>,
}
#[derive(Debug)]
pub struct RunHistory {
    root: PathBuf,
    entries: Option<ResponseResult<Vec<HistoryEntry>>>,
    maximum_entries: usize,
    maximum_bytes: u64,
}
impl RunHistory {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            entries: None,
            maximum_entries: 100,
            maximum_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
    fn entries(&mut self) -> ResponseResult<&mut Vec<HistoryEntry>> {
        if self.entries.is_none() {
            let _ = retained_root();
            let path = self.root.join("index.json");
            let entries = match fs::read(&path) {
                Ok(data) => decode_history_index(&data)
                    .map_err(|error| history_backup_error(&self.root, &data, &error)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(error) => Err(error.into()),
            };
            self.entries = Some(entries);
        }
        self.entries
            .as_mut()
            .expect("history was loaded")
            .as_mut()
            .map_err(|error| error.clone())
    }
    /// # Errors
    /// Returns an I/O error if the body or index cannot be stored.
    pub fn record(&mut self, mut entry: HistoryEntry, body: &BodyStore) -> ResponseResult<()> {
        self.entries()?;
        fs::create_dir_all(&self.root)?;
        let destination = self.root.join(format!("{}.body", entry.id));
        body.export(&destination)?;
        entry.body_path = destination.to_string_lossy().into_owned();
        let identity = entry.identity.clone();
        let request_id = entry.request_id.clone();
        let mut entries = self.entries()?.clone();
        entries.retain(|previous| previous.id != entry.id);
        entries.push(entry);
        entries.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
        let mut matching = 0;
        entries.retain(|entry| {
            if entry.identity == identity && entry.request_id == request_id {
                matching += 1;
                matching <= self.maximum_entries
            } else {
                true
            }
        });
        let mut total: u64 = entries
            .iter()
            .map(|entry| file_size(&entry.body_path))
            .sum();
        while total > self.maximum_bytes {
            let Some(entry) = entries.pop() else {
                break;
            };
            total = total.saturating_sub(file_size(&entry.body_path));
        }
        let retained = entries
            .iter()
            .any(|entry| Path::new(&entry.body_path) == destination);
        if let Err(error) = self.replace_entries(entries) {
            let _ = fs::remove_file(destination);
            return Err(error);
        }
        if !retained {
            let _ = fs::remove_file(destination);
        }
        Ok(())
    }
    /// # Errors
    /// Returns an error for unreadable or damaged indexes.
    pub fn list(&mut self, identity: &SessionRequestIdentity) -> ResponseResult<Vec<HistoryEntry>> {
        let mut entries: Vec<_> = self
            .entries()?
            .iter()
            .filter(|e| e.identity.as_ref() == Some(identity))
            .cloned()
            .collect();
        entries.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
        Ok(entries)
    }
    /// # Errors
    /// Returns an error for unreadable or damaged indexes.
    pub fn count(&mut self, identity: &SessionRequestIdentity) -> ResponseResult<u64> {
        Ok(count(
            self.entries()?
                .iter()
                .filter(|e| e.identity.as_ref() == Some(identity))
                .count(),
        ))
    }
    /// # Errors
    /// Returns an I/O error if writing the index fails.
    pub fn clear(&mut self, identity: &SessionRequestIdentity) -> ResponseResult<()> {
        let entries = self
            .entries()?
            .iter()
            .filter(|entry| {
                entry.identity.as_ref() != Some(identity)
                    && !(entry.identity.is_none() && entry.request_id == identity.request_id)
            })
            .cloned()
            .collect();
        self.replace_entries(entries)
    }
    /// Entries without a complete identity are available separately for recovery.
    /// # Errors
    /// Returns an error for an unsupported or damaged versioned index.
    pub fn legacy_list(&mut self, request_id: &str) -> ResponseResult<Vec<HistoryEntry>> {
        let mut entries: Vec<_> = self
            .entries()?
            .iter()
            .filter(|entry| entry.identity.is_none() && entry.request_id == request_id)
            .cloned()
            .collect();
        entries.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
        Ok(entries)
    }
    /// # Errors
    /// Returns an I/O error if the migrated index cannot be saved.
    pub fn relocate(
        &mut self,
        from: &SessionRequestIdentity,
        to: &SessionRequestIdentity,
    ) -> ResponseResult<()> {
        let mut entries = self.entries()?.clone();
        for entry in &mut entries {
            if entry.identity.as_ref() == Some(from) {
                entry.identity = Some(to.clone());
                entry.request_id.clone_from(&to.request_id);
            }
        }
        self.replace_entries(entries)
    }
    /// Assign legacy IDs only when this workspace has exactly one matching request.
    /// # Errors
    /// Returns an error if the index cannot be read or migration cannot be saved.
    pub fn migrate_legacy(
        &mut self,
        namespace: &str,
        workspace: &crate::domain::DomainWorkspace,
    ) -> ResponseResult<()> {
        let mut matches = std::collections::HashMap::new();
        for collection in &workspace.collections {
            for location in &collection.requests {
                matches
                    .entry(location.request.id.as_str())
                    .and_modify(|value| *value = None)
                    .or_insert_with(|| {
                        Some(SessionRequestIdentity {
                            workspace: namespace.into(),
                            collection_id: collection.id.clone(),
                            request_id: location.request.id.clone(),
                        })
                    });
            }
        }
        let mut changed = false;
        let mut entries = self.entries()?.clone();
        for entry in &mut entries {
            if entry.identity.is_none()
                && let Some(Some(identity)) = matches.get(entry.request_id.as_str())
            {
                entry.identity = Some(identity.clone());
                changed = true;
            }
        }
        if changed {
            self.replace_entries(entries)?;
        }
        Ok(())
    }
    /// # Errors
    /// Returns a storage error without replacing a damaged index.
    pub fn clear_all(&mut self) -> ResponseResult<()> {
        self.replace_entries(Vec::new())
    }
    /// # Errors
    /// Returns an error for an unreadable index.
    pub fn workspace_entries(&mut self, namespace: &str) -> ResponseResult<Vec<HistoryEntry>> {
        Ok(self
            .entries()?
            .iter()
            .filter(|entry| {
                entry
                    .identity
                    .as_ref()
                    .is_some_and(|identity| identity.workspace == namespace)
            })
            .cloned()
            .collect())
    }
    fn replace_entries(&mut self, entries: Vec<HistoryEntry>) -> ResponseResult<()> {
        // Persist before unlinking anything: failed writes keep every old body.
        let previous = self.entries()?.clone();
        let data = serde_json::to_vec(&entries).map_err(|e| json_io(&e))?;
        atomic_write(&self.root.join("index.json"), &data)?;
        let retained_paths: std::collections::HashSet<_> = entries
            .iter()
            .map(|entry| entry.body_path.as_str())
            .collect();
        for entry in previous {
            if !retained_paths.contains(entry.body_path.as_str()) {
                let _ = fs::remove_file(&entry.body_path);
            }
        }
        self.entries = Some(Ok(entries));
        Ok(())
    }
}
fn decode_history_index(data: &[u8]) -> ResponseResult<Vec<HistoryEntry>> {
    let unreadable = || ResponseError::Io {
        reason: "The history index could not be read; it has been left unchanged.".into(),
    };
    let value: serde_json::Value = serde_json::from_slice(data).map_err(|_| unreadable())?;
    if value.is_array() {
        return serde_json::from_value(value).map_err(|_| unreadable());
    }
    let index: HistoryIndex = serde_json::from_value(value).map_err(|_| unreadable())?;
    if index.version != PREVIOUS_HISTORY_INDEX_VERSION {
        return Err(unreadable());
    }
    Ok(index.entries)
}

fn history_backup_error(root: &Path, data: &[u8], error: &ResponseError) -> ResponseError {
    let message = match backup_history_index(root, data) {
        Ok(path) => format!(
            "A backup is kept as {}.",
            path.file_name().unwrap_or_default().to_string_lossy()
        ),
        Err(backup_error) => format!(
            "A backup could not be saved: {backup_error}. The original index is still unchanged."
        ),
    };
    ResponseError::Io {
        reason: format!("{error} {message}"),
    }
}
fn backup_history_index(root: &Path, data: &[u8]) -> std::io::Result<PathBuf> {
    let destination = root.join("index.json.backup");
    if fs::read(&destination).ok().as_deref() == Some(data) {
        return Ok(destination);
    }
    let mut backup = tempfile::Builder::new()
        .prefix("index.json.backup-")
        .tempfile_in(root)?;
    backup.write_all(data)?;
    backup.as_file().sync_all()?;
    // Never replace a previous backup. If its name is occupied (or exclusive
    // rename is unsupported), keep the already exclusive temporary name instead.
    match backup.persist_noclobber(&destination) {
        Ok(_) => Ok(destination),
        Err(error) => error
            .file
            .keep()
            .map(|(_, path)| path)
            .map_err(|error| error.error),
    }
}

struct RetainedRoot {
    directory: tempfile::TempDir,
    _lease: File,
}
// Process leases keep a second running instance's displayed bodies alive. A later
// launch removes directories whose process exited, including after a crash.
fn retained_root() -> std::io::Result<&'static Path> {
    static ROOT: OnceLock<Result<RetainedRoot, std::io::Error>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join("wirebolt-retained-bodies");
        fs::create_dir_all(&root)?;
        let directory = tempfile::tempdir_in(&root)?;
        let lease = File::options()
            .create_new(true)
            .write(true)
            .open(directory.path().join("lease"))?;
        lease.lock()?;
        cleanup_retained_bodies(&root, directory.path());
        Ok(RetainedRoot {
            directory,
            _lease: lease,
        })
    })
    .as_ref()
    .map(|root| root.directory.path())
    .map_err(|error| std::io::Error::new(error.kind(), error.to_string()))
}
fn cleanup_retained_bodies(root: &Path, active: &Path) {
    let Ok(directories) = fs::read_dir(root) else {
        return;
    };
    for entry in directories.flatten() {
        if entry.path() == active {
            continue;
        }
        if let Ok(lease) = File::options().write(true).open(entry.path().join("lease"))
            && lease.try_lock().is_ok()
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
fn file_size(path: &str) -> u64 {
    fs::metadata(path).map_or(0, |m| m.len())
}
pub(super) fn json_io(error: &serde_json::Error) -> ResponseError {
    ResponseError::Io {
        reason: error.to_string(),
    }
}
fn atomic_write(path: &Path, bytes: &[u8]) -> ResponseResult<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| ResponseError::from(e.error))?;
    Ok(())
}
#[must_use]
pub fn unix_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
/// Legacy history/session timestamps retain their existing persisted representation.
#[must_use]
pub fn reference_time() -> f64 {
    unix_time() - 978_307_200.0
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: String,
    /// Unix seconds in memory; legacy cookie files use the original reference epoch.
    #[serde(default, with = "legacy_cookie_expiry")]
    pub expires_at: Option<f64>,
}
impl StoredCookie {
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}\0{}\0{}", self.domain, self.path, self.name)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseHeaderValue {
    pub name: String,
    pub value: String,
}
#[derive(Debug)]
pub struct ResponseCookieJar {
    storage: Option<PathBuf>,
    cookies: Option<Vec<StoredCookie>>,
}
impl ResponseCookieJar {
    #[must_use]
    pub fn new(storage: Option<PathBuf>) -> Self {
        Self {
            storage,
            cookies: None,
        }
    }
    fn cookies(&mut self) -> &mut Vec<StoredCookie> {
        self.cookies.get_or_insert_with(|| {
            self.storage
                .as_ref()
                .and_then(|p| fs::read(p).ok())
                .and_then(|data| serde_json::from_slice::<Vec<StoredCookie>>(&data).ok())
                .unwrap_or_default()
                .into_iter()
                .filter(|c| c.expires_at.is_some())
                .collect()
        })
    }
    #[must_use]
    pub fn store(
        &mut self,
        headers: &[ResponseHeaderValue],
        request_url: &str,
        now: f64,
    ) -> Vec<StoredCookie> {
        let Ok(url) = Url::parse(request_url) else {
            return vec![];
        };
        let host = cookie_host(&url, request_url);
        let mut received = Vec::new();
        for header in headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("set-cookie"))
        {
            if let Some(cookie) = parse_cookie(&header.value, &url, &host, now) {
                self.cookies().retain(|c| c.id() != cookie.id());
                if cookie.expires_at.is_none_or(|t| t > now) {
                    self.cookies().push(cookie.clone());
                }
                received.push(cookie);
            }
        }
        self.expire(now);
        self.persist();
        received
    }
    #[must_use]
    pub fn header(&mut self, request_url: &str, now: f64) -> Option<String> {
        self.expire(now);
        let url = Url::parse(request_url).ok()?;
        let host = cookie_host(&url, request_url);
        let path = if url.path().is_empty() {
            "/"
        } else {
            url.path()
        };
        let mut cookies: Vec<_> = self
            .cookies()
            .iter()
            .filter(|c| {
                (host == c.domain || !c.host_only && host.ends_with(&format!(".{}", c.domain)))
                    && path_matches(path, &c.path)
                    && (!c.secure || url.scheme() == "https")
            })
            .collect();
        cookies.sort_by_key(|c| std::cmp::Reverse(c.path.graphemes(true).count()));
        let header = cookies
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");
        (!header.is_empty()).then_some(header)
    }
    #[must_use]
    pub fn all(&mut self, now: f64) -> Vec<StoredCookie> {
        self.expire(now);
        self.cookies().clone()
    }
    pub fn delete(&mut self, id: &str) {
        self.cookies().retain(|c| c.id() != id);
        self.persist();
    }
    pub fn remove_all(&mut self) {
        self.cookies = Some(vec![]);
        self.persist();
    }
    fn expire(&mut self, now: f64) {
        self.cookies()
            .retain(|c| c.expires_at.is_none_or(|t| t > now));
    }
    fn persist(&mut self) {
        if let Some(path) = self.storage.clone() {
            let cookies: Vec<_> = self
                .cookies()
                .iter()
                .filter(|c| c.expires_at.is_some())
                .collect();
            if let Ok(data) = serde_json::to_vec(&cookies) {
                let _ = atomic_write(&path, &data);
            }
        }
    }
}
#[must_use]
pub fn cookie_storage_path(workspace: &Path, directory: &Path) -> PathBuf {
    directory.join(workspace_namespace(workspace))
}

/// Stable history identity independent of cookie storage layout. The suffix
/// preserves namespaces written by pre-rc2 builds.
#[must_use]
pub fn workspace_namespace(workspace: &Path) -> String {
    let path = resolved_workspace_path(workspace);
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    let mut hex = String::with_capacity(32);
    for byte in &digest[..16] {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    format!("{hex}.json")
}
fn resolved_workspace_path(workspace: &Path) -> PathBuf {
    use std::path::Component;
    let absolute = if workspace.is_absolute() {
        workspace.to_owned()
    } else {
        std::env::current_dir().unwrap_or_default().join(workspace)
    };
    let mut standardized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                standardized.pop();
            }
            Component::CurDir => (),
            other => standardized.push(other),
        }
    }
    let Ok(resolved) = standardized.canonicalize() else {
        return standardized;
    };
    // Foundation uses the public /tmp, /var and /etc aliases after resolving
    // existing paths. An unresolved path keeps its standardized spelling.
    if let Ok(relative) = resolved.strip_prefix("/private") {
        let public = Path::new("/").join(relative);
        if public.canonicalize().ok().as_ref() == Some(&resolved) {
            return public;
        }
    }
    resolved
}
fn path_matches(request: &str, cookie: &str) -> bool {
    request.starts_with(cookie)
        && (request.len() == cookie.len()
            || cookie.ends_with('/')
            || request.as_bytes().get(cookie.len()) == Some(&b'/'))
}
fn parse_cookie(source: &str, url: &Url, host: &str, now: f64) -> Option<StoredCookie> {
    let mut segments = source.split(';').map(str::trim).filter(|s| !s.is_empty());
    let (name, value) = segments.next()?.split_once('=')?;
    let mut cookie = StoredCookie {
        name: name.into(),
        value: value.into(),
        domain: host.into(),
        host_only: true,
        path: url
            .path()
            .rsplit_once('/')
            .filter(|(parent, _)| !parent.is_empty())
            .map_or_else(|| "/".into(), |(parent, _)| parent.into()),
        secure: false,
        http_only: false,
        same_site: String::new(),
        expires_at: None,
    };
    let mut max_age = None;
    let mut expires = None;
    for attribute in segments {
        let (key, value) = attribute
            .split_once('=')
            .map_or((attribute, None), |(k, v)| {
                (k, (!v.is_empty()).then_some(v))
            });
        match (key.to_lowercase().as_str(), value) {
            ("domain", Some(v)) => {
                cookie.domain = v.trim_matches('.').to_lowercase();
                cookie.host_only = false;
            }
            ("path", Some(v)) if v.starts_with('/') => cookie.path = v.into(),
            ("secure", _) => cookie.secure = true,
            ("httponly", _) => cookie.http_only = true,
            ("samesite", Some(v)) => cookie.same_site = v.into(),
            ("max-age", Some(v)) => {
                let digits = v.strip_prefix('-').unwrap_or(v);
                if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                    let age = v.parse::<i64>().unwrap_or(if v.starts_with('-') {
                        i64::MIN
                    } else {
                        i64::MAX
                    });
                    max_age = Some(if age <= 0 {
                        now
                    } else {
                        now + std::time::Duration::from_secs(age.unsigned_abs()).as_secs_f64()
                    });
                }
            }
            ("expires", Some(v)) => {
                if let Some(date) = super::cookie_date::parse_cookie_date(v) {
                    expires = Some(date);
                }
            }
            _ => (),
        }
    }
    cookie.expires_at = max_age.or(expires);
    (cookie.host_only || host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain)))
        .then_some(cookie)
}

fn cookie_host(url: &Url, source: &str) -> String {
    // Foundation preserves the spelling of IPv6 addresses instead of compressing
    // them. Cookie files and host-only matching use that unbracketed spelling.
    let ipv6 = if matches!(url.host(), Some(url::Host::Ipv6(_))) {
        source.split_once("://").and_then(|(_, rest)| {
            let authority = rest.split(['/', '?', '#']).next()?;
            let host = authority.rsplit('@').next()?;
            host.strip_prefix('[')?
                .split_once(']')
                .map(|(host, _)| host)
        })
    } else {
        None
    };
    ipv6.or_else(|| url.host_str())
        .unwrap_or_default()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_lowercase()
}

mod legacy_cookie_expiry {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    const OFFSET: f64 = 978_307_200.0;
    #[expect(
        clippy::ref_option,
        reason = "Serde field serializers receive a borrowed field."
    )]
    pub fn serialize<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
        value.map(|v| v - OFFSET).serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<f64>, D::Error> {
        Option::<f64>::deserialize(deserializer).map(|value| value.map(|v| v + OFFSET))
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    #[test]
    fn crash_cleanup_removes_only_unleased_retained_directories() {
        let root = tempfile::tempdir().unwrap();
        let crashed = root.path().join("crashed");
        let live = root.path().join("live");
        fs::create_dir(&crashed).unwrap();
        fs::create_dir(&live).unwrap();
        fs::write(crashed.join("lease"), b"").unwrap();
        fs::write(crashed.join("body"), b"leftover").unwrap();
        let lease = File::create(live.join("lease")).unwrap();
        lease.lock().unwrap();
        cleanup_retained_bodies(root.path(), &root.path().join("active"));
        assert!(!crashed.exists());
        assert!(live.exists());
    }
    #[test]
    fn legacy_history_counts_toward_byte_quota() {
        let root = tempfile::tempdir().unwrap();
        let mut history = RunHistory::new(root.path().into());
        history.maximum_bytes = 1;
        let legacy_path = root.path().join("legacy.body");
        fs::write(&legacy_path, b"legacy body").unwrap();
        let entry = crate::session::history::SessionHistoryEntry {
            id: "legacy".into(),
            request_id: "old".into(),
            identity: None,
            created_at: 0.0,
            prepared: crate::session::run::SessionPrepared {
                method: "GET".into(),
                url: "https://example.com".into(),
                headers: vec![],
                proxy: None,
                body: crate::session::run::SessionPreparedBody {
                    byte_count: 0,
                    content_type: None,
                    text_preview: None,
                    redacted: false,
                },
                transport: crate::domain::DomainTransport::default(),
            },
            response_head: None,
            completion: None,
            failure: None,
            body_path: legacy_path.to_string_lossy().into_owned(),
        };
        fs::write(
            root.path().join("index.json"),
            serde_json::to_vec(&vec![entry.clone()]).unwrap(),
        )
        .unwrap();
        let mut body = BodyStore::new("source", root.path()).unwrap();
        body.append(b"x").unwrap();
        body.finish().unwrap();
        history
            .record(
                HistoryEntry {
                    id: "new".into(),
                    created_at: 1.0,
                    identity: Some(SessionRequestIdentity {
                        workspace: "fixture".into(),
                        collection_id: "api".into(),
                        request_id: "new".into(),
                    }),
                    ..entry
                },
                &body,
            )
            .unwrap();
        assert!(history.legacy_list("old").unwrap().is_empty());
        assert!(!legacy_path.exists());
    }
}

#[cfg(test)]
mod history_backup_tests {
    use super::*;
    #[test]
    fn damaged_history_reuses_identical_backups_and_preserves_distinct_damage() {
        let root = tempfile::tempdir().unwrap();
        let first = backup_history_index(root.path(), b"first damaged index").unwrap();
        assert_eq!(first, root.path().join("index.json.backup"));
        assert_eq!(
            backup_history_index(root.path(), b"first damaged index").unwrap(),
            first
        );
        let second = backup_history_index(root.path(), b"second damaged index").unwrap();
        assert_ne!(second, first);
        assert_eq!(fs::read(first).unwrap(), b"first damaged index");
        assert_eq!(fs::read(second).unwrap(), b"second damaged index");
    }
    #[test]
    fn damaged_history_reports_backup_failure_without_claiming_success() {
        let root = tempfile::tempdir().unwrap();
        let error = history_backup_error(
            &root.path().join("missing"),
            b"damaged",
            &ResponseError::Io {
                reason: "The index is unchanged.".into(),
            },
        );
        let reason = error.to_string();
        assert!(reason.contains("backup could not be saved"));
        assert!(!reason.contains("backup is kept"));
    }
}
