use super::{JsonDocument, ResponseCancellation, ResponseError, ResponseResult, count, size};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_segmentation::UnicodeSegmentation;
use url::Url;

pub const VIEWPORT_BYTES: usize = 32 * 1024;
pub const SEARCH_CHUNK_BYTES: usize = 256 * 1024;
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
    bytes: u64,
    preview: Vec<u8>,
    decoded: Option<(Vec<u8>, String)>,
    json: [JsonCache; 2],
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
            bytes: 0,
            preview: vec![],
            decoded: None,
            json: [JsonCache::Unparsed, JsonCache::Unparsed],
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
            bytes,
            preview: vec![],
            decoded: None,
            json: [JsonCache::Unparsed, JsonCache::Unparsed],
        })
    }
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    #[must_use]
    pub fn size(&self) -> u64 {
        self.bytes
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
        self.bytes += count(data.len());
        if self.preview.len() < VIEWPORT_BYTES {
            self.preview
                .extend_from_slice(&data[..data.len().min(VIEWPORT_BYTES - self.preview.len())]);
            self.decoded = None;
        }
        Ok(())
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
        reader.seek(SeekFrom::Start(offset.min(self.bytes)))?;
        let mut bytes = Vec::with_capacity(size(length.min(self.bytes)));
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
        if destination.exists() {
            fs::remove_file(destination)?;
        }
        fs::copy(&self.path, destination)?;
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

/// Domain snapshots stay opaque at this boundary until the domain/session waves
/// supply their shared records. The repository owns ordering, quotas and disk I/O.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    #[serde(rename = "requestID")]
    pub request_id: String,
    pub created_at: f64,
    pub prepared: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_head: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<serde_json::Value>,
    pub body_path: String,
}
#[derive(Debug)]
pub struct RunHistory {
    root: PathBuf,
    entries: Option<Vec<HistoryEntry>>,
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
    fn entries(&mut self) -> &mut Vec<HistoryEntry> {
        self.entries.get_or_insert_with(|| {
            fs::read(self.root.join("index.json"))
                .ok()
                .and_then(|data| serde_json::from_slice(&data).ok())
                .unwrap_or_default()
        })
    }
    /// # Errors
    /// Returns an I/O error if the body or index cannot be stored.
    pub fn record(&mut self, mut entry: HistoryEntry, body: &BodyStore) -> ResponseResult<()> {
        fs::create_dir_all(&self.root)?;
        let destination = self.root.join(format!("{}.body", entry.id));
        body.export(&destination)?;
        entry.body_path = destination.to_string_lossy().into_owned();
        let request_id = entry.request_id.clone();
        self.entries().push(entry);
        let mut matching: Vec<_> = self
            .entries()
            .iter()
            .filter(|e| e.request_id == request_id)
            .cloned()
            .collect();
        matching.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
        for entry in matching.into_iter().skip(self.maximum_entries) {
            self.evict(&entry);
        }
        let mut total: u64 = self.entries().iter().map(|e| file_size(&e.body_path)).sum();
        let mut oldest = self.entries().clone();
        oldest.sort_by(|a, b| a.created_at.total_cmp(&b.created_at));
        for entry in oldest {
            if total <= self.maximum_bytes {
                break;
            }
            total = total.saturating_sub(file_size(&entry.body_path));
            self.evict(&entry);
        }
        self.persist()
    }
    fn evict(&mut self, entry: &HistoryEntry) {
        self.entries().retain(|e| e.id != entry.id);
        let _ = fs::remove_file(&entry.body_path);
    }
    #[must_use]
    pub fn list(&mut self, request_id: &str) -> Vec<HistoryEntry> {
        let mut entries: Vec<_> = self
            .entries()
            .iter()
            .filter(|e| e.request_id == request_id)
            .cloned()
            .collect();
        entries.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
        entries
    }
    #[must_use]
    pub fn count(&mut self, request_id: &str) -> u64 {
        count(
            self.entries()
                .iter()
                .filter(|e| e.request_id == request_id)
                .count(),
        )
    }
    /// # Errors
    /// Returns an I/O error if writing the index fails.
    pub fn clear(&mut self, request_id: &str) -> ResponseResult<()> {
        for entry in self.list(request_id) {
            self.evict(&entry);
        }
        self.persist()
    }
    fn persist(&mut self) -> ResponseResult<()> {
        let data = serde_json::to_vec(self.entries()).map_err(|e| json_io(&e))?;
        atomic_write(&self.root.join("index.json"), &data)
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
    file.persist(path)
        .map_err(|e| ResponseError::from(e.error))?;
    Ok(())
}
#[must_use]
pub fn reference_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        - 978_307_200.0
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
        let mut received = Vec::new();
        for header in headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case("set-cookie"))
        {
            if let Some(cookie) = parse_cookie(&header.value, &url, now) {
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
        let host = url.host_str().unwrap_or_default().to_lowercase();
        let decoded_path = decoded_url_path(&url);
        let path = if decoded_path.is_empty() {
            "/"
        } else {
            &decoded_path
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
    let path = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_owned());
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    let mut hex = String::with_capacity(32);
    for byte in &digest[..16] {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    directory.join(format!("{hex}.json"))
}
fn path_matches(request: &str, cookie: &str) -> bool {
    request.starts_with(cookie)
        && (request.len() == cookie.len()
            || cookie.ends_with('/')
            || request.as_bytes().get(cookie.len()) == Some(&b'/'))
}
fn parse_cookie(source: &str, url: &Url, now: f64) -> Option<StoredCookie> {
    let mut segments = source.split(';').map(str::trim).filter(|s| !s.is_empty());
    let (name, value) = segments.next()?.split_once('=')?;
    let mut cookie = StoredCookie {
        name: name.into(),
        value: value.into(),
        domain: url.host_str().unwrap_or_default().to_lowercase(),
        host_only: true,
        path: decoded_url_path(url)
            .rsplit_once('/')
            .filter(|(parent, _)| !parent.is_empty())
            .map_or_else(|| "/".into(), |(parent, _)| parent.into()),
        secure: false,
        http_only: false,
        same_site: String::new(),
        expires_at: None,
    };
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
            ("path", Some(v)) => cookie.path = v.into(),
            ("secure", _) => cookie.secure = true,
            ("httponly", _) => cookie.http_only = true,
            ("samesite", Some(v)) => cookie.same_site = v.into(),
            ("max-age", Some(v)) => {
                cookie.expires_at = v.parse::<i64>().ok().map(|age| {
                    now + std::time::Duration::from_secs(age.unsigned_abs()).as_secs_f64()
                        * if age < 0 { -1.0 } else { 1.0 }
                });
            }
            ("expires", Some(v)) => {
                cookie.expires_at = super::cookie_date::parse_cookie_date(v);
            }
            _ => (),
        }
    }
    let host = url.host_str().unwrap_or_default().to_lowercase();
    (cookie.host_only || host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain)))
        .then_some(cookie)
}

fn decoded_url_path(url: &Url) -> String {
    percent_encoding::percent_decode_str(url.path())
        .decode_utf8_lossy()
        .into_owned()
}
