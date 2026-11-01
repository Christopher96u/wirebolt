//! Typed bridge for the response engine. All algorithms and retained state live in wirebolt-app.
// UniFFI lifts owned arguments and shared object handles at the foreign boundary.
#![allow(clippy::needless_pass_by_value)]
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};
use wirebolt_app::response::{
    BodyProgress, BodyStore, CodeDocument, CodeSyntax, FailureCategory, FailureIssue,
    FailureMessage, Fold, IndexOptions, IndexedMatch, JsonDocument, JsonTreeNode, LineIndex,
    LineIndexChange, MarkdownCache, MarkdownCacheStats, MarkdownDocument, MarkdownRun,
    MarkdownStyle, ResponseCancellation, ResponseCookieJar, ResponseError, ResponseHeaderValue,
    ResponseTextRange, RunHistory, SearchPosition, SearchQuery, ShapingGroup, ShapingMetrics,
    ShapingRequest, StoredCookie, TextIndex, TextIndexCache, TextRow, TextShaper, WrapBreakIndex,
    WrapBreakSnapshot, WrapLayout, completion_summary, cookie_storage_path, describe_issue,
    failure_host, failure_message, format_duration, format_elapsed, json_tree, json_tree_file,
    reason_phrase, reference_time, sentence, space_runs, status_line, uses_text_index,
};

#[uniffi::remote(Error)]
pub enum ResponseError {
    Io { reason: String },
    InvalidJson,
    PresentationLimit,
    InvalidPattern { reason: String },
    Cancelled,
}

#[uniffi::remote(Record)]
pub struct ResponseTextRange {
    pub location: u64,
    pub length: u64,
}

#[uniffi::remote(Record)]
pub struct LineIndex {
    pub starts: Vec<u64>,
    pub length: u64,
}

#[uniffi::remote(Record)]
pub struct SearchQuery {
    pub text: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regular_expression: bool,
}

#[uniffi::remote(Enum)]
pub enum CodeSyntax {
    Json,
    Xml,
    Html,
    None,
}

#[uniffi::remote(Record)]
pub struct Fold {
    pub line: u64,
    pub range: ResponseTextRange,
}

#[uniffi::remote(Record)]
pub struct FailureIssue {
    pub path: String,
    pub kind: String,
    pub reference: Option<String>,
}

#[uniffi::remote(Enum)]
pub enum FailureCategory {
    Cancelled,
    Connection,
    Timeout,
    Tls,
    Proxy,
    Secrets,
    InvalidRequest,
    Response,
    InternalError,
}

#[uniffi::remote(Record)]
pub struct FailureMessage {
    pub category: FailureCategory,
    pub title: String,
    pub message: String,
    pub system_image: String,
    pub suggests_network_settings: bool,
}

#[uniffi::remote(Record)]
pub struct JsonTreeNode {
    pub id: String,
    pub key: String,
    pub kind: String,
    pub value: String,
    pub children: Option<Vec<JsonTreeNode>>,
}

#[uniffi::remote(Record)]
pub struct BodyProgress {
    pub bytes: u64,
    pub data: Vec<u8>,
    pub text: String,
    pub truncated: bool,
}

#[uniffi::remote(Record)]
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

#[uniffi::remote(Record)]
pub struct ResponseHeaderValue {
    pub name: String,
    pub value: String,
}

#[uniffi::remote(Record)]
pub struct WrapLayout {
    pub font_name: String,
    pub font_size: f64,
    pub width: f64,
    pub advance: f64,
}

#[uniffi::remote(Record)]
pub struct ShapingRequest {
    pub text: String,
    pub boundaries: Vec<u64>,
}

#[uniffi::remote(Record)]
pub struct TextRow {
    pub number: u64,
    pub line: u64,
    pub continuation: bool,
    pub byte_offset: u64,
    pub text: String,
    pub columns: u64,
    pub indent: f64,
    pub spaces: Vec<ResponseTextRange>,
}

#[uniffi::remote(Record)]
pub struct SearchPosition {
    pub row: u64,
    pub column: u64,
}

#[uniffi::remote(Record)]
pub struct IndexedMatch {
    pub start: SearchPosition,
    pub end: SearchPosition,
}

#[uniffi::remote(Record)]
pub struct IndexOptions {
    pub path: String,
    pub columns: u64,
    pub prefix: String,
    pub wrapping: Option<WrapLayout>,
}

#[uniffi::remote(Record)]
pub struct MarkdownStyle {
    pub heading: u32,
    pub code: bool,
    pub quote: bool,
    pub depth: u32,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub link: Option<String>,
}

#[uniffi::remote(Record)]
pub struct MarkdownRun {
    pub text: String,
    pub style: MarkdownStyle,
}

#[uniffi::remote(Record)]
pub struct MarkdownCacheStats {
    pub retained_bytes: u64,
    pub parse_count: u64,
    pub retained_ids: Vec<u64>,
}

#[uniffi::remote(Record)]
pub struct WrapBreakSnapshot {
    pub previous: Vec<u64>,
    pub next_content: Vec<u64>,
    pub length: u64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
#[derive(Debug, uniffi::Object)]
pub struct AppResponseCancellation {
    inner: Arc<ResponseCancellation>,
}
#[uniffi::export]
impl AppResponseCancellation {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::default(),
        })
    }
    pub fn cancel(&self) {
        self.inner.cancel();
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppLineIndex {
    inner: LineIndex,
}
#[uniffi::export]
impl AppLineIndex {
    #[uniffi::constructor]
    pub fn new(text: String) -> Arc<Self> {
        Arc::new(Self {
            inner: LineIndex::new(&text),
        })
    }
    pub fn snapshot(&self) -> LineIndex {
        self.inner.clone()
    }
    pub fn line(&self, offset: u64) -> u64 {
        self.inner.line(offset)
    }
    pub fn replacing(
        &self,
        range: ResponseTextRange,
        text: String,
    ) -> Result<AppLineIndexEdit, ResponseError> {
        let mut inner = self.inner.clone();
        let change = inner.replace(range, &text)?;
        Ok(AppLineIndexEdit {
            index: Arc::new(Self { inner }),
            change,
        })
    }
}
#[uniffi::export]
pub fn app_text_search(
    query: SearchQuery,
    source: String,
    scope: Option<ResponseTextRange>,
    cancellation: Arc<AppResponseCancellation>,
) -> Result<Vec<ResponseTextRange>, ResponseError> {
    query.matches(&source, scope, &cancellation.inner)
}
#[uniffi::export]
pub fn app_space_runs(text: String) -> Vec<ResponseTextRange> {
    space_runs(&text)
}
#[uniffi::export]
pub fn app_uses_text_index(bytes: u64, preview: String) -> bool {
    uses_text_index(bytes, &preview)
}
#[uniffi::export]
pub fn app_wrap_breaks(text: String) -> WrapBreakSnapshot {
    WrapBreakIndex::new(&text).snapshot()
}
#[derive(Debug, Clone, uniffi::Record)]
pub struct AppProjectionSnapshot {
    pub text: Option<String>,
    pub folds: Vec<Fold>,
    pub collapsed: Vec<u64>,
}
#[derive(Debug, uniffi::Object)]
pub struct AppCodeProjection {
    inner: CodeDocument,
}
#[uniffi::export]
impl AppCodeProjection {
    #[uniffi::constructor]
    pub fn new(
        source: String,
        collapsed: Vec<u64>,
        syntax: CodeSyntax,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<Arc<Self>, ResponseError> {
        Ok(Arc::new(Self {
            inner: CodeDocument::new(source, collapsed, syntax, &cancellation.inner)?,
        }))
    }
    #[uniffi::constructor]
    pub fn unfolded(source: String, folds: Vec<Fold>) -> Arc<Self> {
        Arc::new(Self {
            inner: CodeDocument::unfolded(source, folds),
        })
    }
    pub fn snapshot(&self, include_text: bool) -> AppProjectionSnapshot {
        AppProjectionSnapshot {
            text: include_text.then(|| self.inner.text.as_ref().clone()),
            folds: self.inner.folds.clone(),
            collapsed: self.inner.collapsed.clone(),
        }
    }
    pub fn updating_edit(
        &self,
        range: ResponseTextRange,
        replacement: String,
        removed: String,
    ) -> Option<Arc<Self>> {
        self.inner
            .updating_edit(range, &replacement, &removed)
            .map(|inner| Arc::new(Self { inner }))
    }
    pub fn source_offsets(&self, offsets: Vec<u64>) -> Vec<u64> {
        self.inner.source_offsets(offsets)
    }
    pub fn source_offset(&self, display: u64, after_placeholder: bool) -> u64 {
        self.inner.source_offset(display, after_placeholder)
    }
    pub fn display_offset(&self, source: u64) -> u64 {
        self.inner.display_offset(source)
    }
    pub fn replacing(&self, range: ResponseTextRange, replacement: String) -> String {
        self.inner.replacing(range, &replacement)
    }
}
#[derive(Debug, Clone, uniffi::Record)]
pub struct AppJsonMetadata {
    pub path: String,
    pub byte_count: u64,
    pub preview: String,
    pub contains_unicode_escapes: bool,
}
#[derive(Debug, uniffi::Object)]
pub struct AppJsonDocument {
    inner: Arc<JsonDocument>,
}
#[uniffi::export]
impl AppJsonDocument {
    #[uniffi::constructor]
    pub fn new(
        path: String,
        decode: bool,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<Arc<Self>, ResponseError> {
        Ok(Arc::new(Self {
            inner: Arc::new(JsonDocument::new(
                Path::new(&path),
                decode,
                &cancellation.inner,
            )?),
        }))
    }
    pub fn metadata(&self) -> AppJsonMetadata {
        AppJsonMetadata {
            path: self.inner.path().to_string_lossy().into_owned(),
            byte_count: self.inner.byte_count,
            preview: self.inner.preview.clone(),
            contains_unicode_escapes: self.inner.contains_unicode_escapes,
        }
    }
}
#[uniffi::export]
pub fn app_json_tree(
    data: Vec<u8>,
    node_limit: u64,
    cancellation: Arc<AppResponseCancellation>,
) -> Result<JsonTreeNode, ResponseError> {
    json_tree(&data, node_limit, &cancellation.inner)
}
#[derive(Debug, uniffi::Object)]
pub struct AppResponseBody {
    inner: Mutex<BodyStore>,
}
#[uniffi::export]
impl AppResponseBody {
    #[uniffi::constructor]
    pub fn new(run_id: String, directory: String) -> Result<Arc<Self>, ResponseError> {
        Ok(Arc::new(Self {
            inner: Mutex::new(BodyStore::new(&run_id, Path::new(&directory))?),
        }))
    }
    #[uniffi::constructor]
    pub fn existing(path: String) -> Result<Arc<Self>, ResponseError> {
        Ok(Arc::new(Self {
            inner: Mutex::new(BodyStore::existing(PathBuf::from(path))?),
        }))
    }
    pub fn path(&self) -> String {
        lock(&self.inner).path().to_string_lossy().into_owned()
    }
    pub fn append(&self, data: Vec<u8>) -> Result<(), ResponseError> {
        lock(&self.inner).append(&data)
    }
    pub fn append_batch(&self, chunks: Vec<Vec<u8>>) -> Result<(), ResponseError> {
        let mut body = lock(&self.inner);
        for chunk in chunks {
            body.append(&chunk)?;
        }
        Ok(())
    }
    pub fn progress(&self) -> BodyProgress {
        lock(&self.inner).progress()
    }
    pub fn finish(&self) -> Result<(), ResponseError> {
        lock(&self.inner).finish()
    }
    pub fn size(&self) -> u64 {
        lock(&self.inner).size()
    }
    pub fn viewport(&self, offset: u64, length: u64) -> Result<Vec<u8>, ResponseError> {
        lock(&self.inner).viewport(offset, length)
    }
    pub fn count_occurrences(&self, query: String) -> Result<u64, ResponseError> {
        lock(&self.inner).count_occurrences(&query)
    }
    pub fn export(&self, destination: String) -> Result<(), ResponseError> {
        lock(&self.inner).export(Path::new(&destination))
    }
    pub fn remove(&self) {
        lock(&self.inner).remove();
    }
    pub fn formatted_json(
        &self,
        decode: bool,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<Option<Arc<AppJsonDocument>>, ResponseError> {
        Ok(lock(&self.inner)
            .formatted_json(decode, &cancellation.inner)?
            .map(|inner| Arc::new(AppJsonDocument { inner })))
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppRunHistory {
    inner: Mutex<RunHistory>,
}
#[uniffi::export]
impl AppRunHistory {
    #[uniffi::constructor]
    pub fn new(root: String) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(RunHistory::new(PathBuf::from(root))),
        })
    }
    pub fn record(&self, entry: Vec<u8>, body: Arc<AppResponseBody>) -> Result<(), ResponseError> {
        let entry = serde_json::from_slice(&entry).map_err(|e| ResponseError::Io {
            reason: e.to_string(),
        })?;
        lock(&self.inner).record(entry, &lock(&body.inner))
    }
    pub fn list(&self, request_id: String) -> Vec<u8> {
        serde_json::to_vec(&lock(&self.inner).list(&request_id)).expect("history JSON values")
    }
    pub fn count(&self, request_id: String) -> u64 {
        lock(&self.inner).count(&request_id)
    }
    pub fn clear(&self, request_id: String) -> Result<(), ResponseError> {
        lock(&self.inner).clear(&request_id)
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppCookieJar {
    inner: Mutex<ResponseCookieJar>,
}
#[uniffi::export]
impl AppCookieJar {
    #[uniffi::constructor]
    pub fn new(storage: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(ResponseCookieJar::new(storage.map(PathBuf::from))),
        })
    }
    pub fn store(
        &self,
        headers: Vec<ResponseHeaderValue>,
        request_url: String,
    ) -> Vec<StoredCookie> {
        lock(&self.inner).store(&headers, &request_url, reference_time())
    }
    pub fn header(&self, url: String) -> Option<String> {
        lock(&self.inner).header(&url, reference_time())
    }
    pub fn all(&self) -> Vec<StoredCookie> {
        lock(&self.inner).all(reference_time())
    }
    pub fn delete(&self, id: String) {
        lock(&self.inner).delete(&id);
    }
    pub fn remove_all(&self) {
        lock(&self.inner).remove_all();
    }
}
#[uniffi::export]
pub fn app_cookie_storage_path(workspace: String, directory: String) -> String {
    cookie_storage_path(Path::new(&workspace), Path::new(&directory))
        .to_string_lossy()
        .into_owned()
}
#[uniffi::export]
pub fn app_format_duration(seconds: f64) -> String {
    format_duration(seconds)
}
#[uniffi::export]
pub fn app_format_elapsed(seconds: f64) -> String {
    format_elapsed(seconds)
}
#[uniffi::export]
pub fn app_reason_phrase(status: u16) -> String {
    reason_phrase(status).into()
}
#[uniffi::export]
pub fn app_status_line(status: u16) -> String {
    status_line(status)
}
#[uniffi::export]
pub fn app_completion_summary(
    status: Option<u16>,
    nanoseconds: Option<u64>,
    formatted_bytes: String,
) -> String {
    completion_summary(status, nanoseconds, formatted_bytes)
}
#[uniffi::export]
pub fn app_failure_message(
    kind: String,
    issues: Vec<FailureIssue>,
    host: Option<String>,
) -> FailureMessage {
    failure_message(&kind, &issues, host.as_deref())
}
#[uniffi::export]
pub fn app_failure_host(url: Option<String>) -> Option<String> {
    failure_host(url.as_deref())
}
#[uniffi::export]
pub fn app_describe_issue(issue: FailureIssue) -> String {
    describe_issue(&issue)
}
#[uniffi::export]
pub fn app_failure_sentence(kind: String) -> String {
    sentence(&kind)
}
#[uniffi::export(with_foreign)]
pub trait AppTextShaper: Send + Sync {
    fn measure(&self, requests: Vec<ShapingRequest>, layout: WrapLayout) -> Vec<ShapingMetrics>;
}
struct ShaperAdapter(Arc<dyn AppTextShaper>);
impl std::fmt::Debug for ShaperAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ShaperAdapter")
    }
}
impl TextShaper for ShaperAdapter {
    fn measure(&self, requests: Vec<ShapingRequest>, layout: WrapLayout) -> Vec<ShapingMetrics> {
        self.0.measure(requests, layout)
    }
}
fn shaper(adapter: Option<Arc<dyn AppTextShaper>>) -> Option<Arc<dyn TextShaper>> {
    adapter.map(|a| Arc::new(ShaperAdapter(a)) as Arc<dyn TextShaper>)
}
#[derive(Debug, Clone, uniffi::Record)]
pub struct AppIndexMetadata {
    pub row_count: u64,
    pub line_count: u64,
    pub is_complete: bool,
    pub maximum_columns: u64,
}
#[derive(Debug, uniffi::Object)]
pub struct AppTextIndex {
    inner: Arc<TextIndex>,
}
#[uniffi::export]
impl AppTextIndex {
    #[uniffi::constructor]
    pub fn new(
        options: IndexOptions,
        row_limit: u64,
        adapter: Option<Arc<dyn AppTextShaper>>,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<Arc<Self>, ResponseError> {
        Ok(Arc::new(Self {
            inner: Arc::new(TextIndex::new(
                options,
                row_limit,
                shaper(adapter),
                &cancellation.inner,
            )?),
        }))
    }
    pub fn metadata(&self) -> AppIndexMetadata {
        AppIndexMetadata {
            row_count: self.inner.row_count,
            line_count: self.inner.line_count,
            is_complete: self.inner.is_complete,
            maximum_columns: self.inner.maximum_columns,
        }
    }
    pub fn rows(
        &self,
        start: u64,
        count: u64,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<Vec<TextRow>, ResponseError> {
        self.inner.rows(start, count, &cancellation.inner)
    }
    pub fn row_at_byte(
        &self,
        offset: u64,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<u64, ResponseError> {
        self.inner.row_at_byte(offset, &cancellation.inner)
    }
    pub fn preserving(
        &self,
        anchor: TextRow,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<u64, ResponseError> {
        self.inner.preserving(&anchor, &cancellation.inner)
    }
    pub fn search(
        &self,
        query: SearchQuery,
        start: Option<SearchPosition>,
        end: Option<SearchPosition>,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<Vec<IndexedMatch>, ResponseError> {
        self.inner
            .search(&query, start.zip(end), &cancellation.inner)
    }
    pub fn first_match(
        &self,
        query: String,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<Option<u64>, ResponseError> {
        self.inner.first_match(&query, &cancellation.inner)
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppResponseIndexCache {
    inner: TextIndexCache,
}
#[uniffi::export]
impl AppResponseIndexCache {
    #[uniffi::constructor]
    pub fn new(capacity: u64) -> Arc<Self> {
        Arc::new(Self {
            inner: TextIndexCache::new(capacity),
        })
    }
    pub fn index(
        &self,
        options: IndexOptions,
        first_viewport: bool,
        adapter: Option<Arc<dyn AppTextShaper>>,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<Arc<AppTextIndex>, ResponseError> {
        Ok(Arc::new(AppTextIndex {
            inner: self.inner.index(
                options,
                first_viewport,
                shaper(adapter),
                &cancellation.inner,
            )?,
        }))
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppMarkdownDocument {
    inner: Arc<MarkdownDocument>,
}
#[uniffi::export]
impl AppMarkdownDocument {
    pub fn id(&self) -> u64 {
        self.inner.id
    }
    pub fn runs(&self) -> Vec<MarkdownRun> {
        self.inner.runs.clone()
    }
    pub fn cost(&self) -> u64 {
        self.inner.cost
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppMarkdownCache {
    inner: Mutex<MarkdownCache>,
}
#[uniffi::export]
impl AppMarkdownCache {
    #[uniffi::constructor]
    pub fn new(byte_limit: u64) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(MarkdownCache::new(byte_limit)),
        })
    }
    pub fn stats(&self) -> MarkdownCacheStats {
        lock(&self.inner).stats()
    }
    pub fn render(
        &self,
        source: String,
        cancellation: Arc<AppResponseCancellation>,
    ) -> Result<Arc<AppMarkdownDocument>, ResponseError> {
        Ok(Arc::new(AppMarkdownDocument {
            inner: lock(&self.inner).render(&source, &cancellation.inner)?,
        }))
    }
}

use wirebolt_app::response::ResponseBadge;
#[uniffi::remote(Enum)]
pub enum ResponseBadge {
    Status { status: u16 },
    Failed { title: String },
    Cancelled,
}
#[uniffi::export]
pub fn app_response_badge(
    status: Option<u16>,
    failure: Option<String>,
    issues: Vec<FailureIssue>,
    host: Option<String>,
) -> Option<ResponseBadge> {
    ResponseBadge::new(status, failure.as_deref(), &issues, host.as_deref())
}
#[uniffi::export]
pub fn app_response_badge_label(badge: ResponseBadge) -> String {
    badge.label()
}
#[uniffi::export]
pub fn app_response_badge_detail(badge: ResponseBadge) -> String {
    badge.detail()
}

#[uniffi::export]
pub fn app_json_tree_file(
    path: String,
    node_limit: u64,
    cancellation: Arc<AppResponseCancellation>,
) -> Result<JsonTreeNode, ResponseError> {
    json_tree_file(Path::new(&path), node_limit, &cancellation.inner)
}

#[uniffi::remote(Record)]
pub struct ShapingMetrics {
    pub advances: Vec<f64>,
    pub groups: Vec<ShapingGroup>,
}
#[uniffi::remote(Record)]
pub struct ShapingGroup {
    pub first: u64,
    pub count: u64,
    pub widths: Vec<f64>,
}

#[uniffi::remote(Record)]
pub struct LineIndexChange {
    pub first: u64,
    pub removed: u64,
    pub inserted: Vec<u64>,
    pub delta: i64,
    pub length: u64,
}
#[derive(Debug, uniffi::Record)]
pub struct AppLineIndexEdit {
    pub index: Arc<AppLineIndex>,
    pub change: LineIndexChange,
}
