//! Typed bridge for the response engine. All algorithms and retained state live in wirebolt-app.
// UniFFI lifts owned arguments and shared object handles at the foreign boundary.
#![allow(clippy::needless_pass_by_value)]
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use wirebolt_app::response::{
    BodyProgress, BodyStore, CodeDocument, CodeSyntax, FailureCategory,
    FailureIssue as CoreFailureIssue, FailureMessage, Fold, IndexOptions, IndexedMatch,
    JsonDocument, JsonNodeKind, JsonTreeNode, LineIndex, LineIndexChange, MarkdownCache,
    MarkdownCacheStats, MarkdownDocument, MarkdownRun, MarkdownStyle, ResponseCancellation,
    ResponseCookieJar, ResponseError, ResponseFailureKind, ResponseHeaderValue, ResponseTextRange,
    RunHistory, SearchPosition, SearchQuery, ShapingGroup, ShapingMetrics, ShapingRequest,
    StoredCookie, TextIndex, TextIndexCache, TextRow, TextShaper, WrapBreakIndex,
    WrapBreakSnapshot, WrapLayout, completion_summary, cookie_storage_path, describe_issue,
    failure_host, failure_message, format_duration, format_elapsed, json_tree, json_tree_file,
    reason_phrase, sentence, space_runs, status_line, unix_time, uses_text_index,
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

#[derive(Debug, uniffi::Record)]
pub struct FailureIssue {
    pub path: String,
    pub kind: ResponseIssueKind,
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
    pub kind: JsonNodeKind,
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

pub(crate) use wirebolt_app::lock;
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
    query.matches_with_engine(&source, scope, &cancellation.inner, regex_engine())
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
    pub(crate) inner: Arc<Mutex<BodyStore>>,
}
#[uniffi::export]
impl AppResponseBody {
    #[uniffi::constructor]
    pub fn new(run_id: String, directory: String) -> Result<Arc<Self>, ResponseError> {
        Ok(Arc::new(Self {
            inner: Arc::new(Mutex::new(BodyStore::new(&run_id, Path::new(&directory))?)),
        }))
    }
    #[uniffi::constructor]
    pub fn existing(path: String) -> Result<Arc<Self>, ResponseError> {
        Ok(Arc::new(Self {
            inner: Arc::new(Mutex::new(BodyStore::existing(PathBuf::from(path))?)),
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
    pub(crate) inner: Arc<Mutex<RunHistory>>,
}
#[uniffi::export]
impl AppRunHistory {
    #[uniffi::constructor]
    pub fn new(root: String) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Mutex::new(RunHistory::new(PathBuf::from(root)))),
        })
    }
    pub fn record(&self, entry: Vec<u8>, body: Arc<AppResponseBody>) -> Result<(), ResponseError> {
        let entry = serde_json::from_slice(&entry).map_err(|e| ResponseError::Io {
            reason: e.to_string(),
        })?;
        lock(&self.inner).record(entry, &lock(&body.inner))
    }
    pub fn list(&self, request_id: String) -> Result<Vec<u8>, ResponseError> {
        let entries = lock(&self.inner).list(&request_id)?;
        serde_json::to_vec(&entries).map_err(|error| ResponseError::Io {
            reason: error.to_string(),
        })
    }
    pub fn count(&self, request_id: String) -> Result<u64, ResponseError> {
        lock(&self.inner).count(&request_id)
    }
    pub fn clear(&self, request_id: String) -> Result<(), ResponseError> {
        lock(&self.inner).clear(&request_id)
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppCookieJar {
    pub(crate) inner: Arc<Mutex<ResponseCookieJar>>,
}
#[uniffi::export]
impl AppCookieJar {
    #[uniffi::constructor]
    pub fn new(storage: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Mutex::new(ResponseCookieJar::new(
                storage.map(PathBuf::from),
            ))),
        })
    }
    pub fn store(
        &self,
        headers: Vec<ResponseHeaderValue>,
        request_url: String,
    ) -> Vec<StoredCookie> {
        lock(&self.inner).store(&headers, &request_url, unix_time())
    }
    pub fn header(&self, url: String) -> Option<String> {
        lock(&self.inner).header(&url, unix_time())
    }
    pub fn all(&self) -> Vec<StoredCookie> {
        lock(&self.inner).all(unix_time())
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
    kind: ResponseFailureKind,
    issues: Vec<FailureIssue>,
    host: Option<String>,
) -> FailureMessage {
    failure_message(
        &kind,
        &issues.into_iter().map(Into::into).collect::<Vec<_>>(),
        host.as_deref(),
    )
}
#[uniffi::export]
pub fn app_failure_host(url: Option<String>) -> Option<String> {
    failure_host(url.as_deref())
}
#[uniffi::export]
pub fn app_describe_issue(issue: FailureIssue) -> String {
    describe_issue(&issue.into())
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
            .search_with_engine(&query, start.zip(end), &cancellation.inner, regex_engine())
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
    failure: Option<ResponseFailureKind>,
    issues: Vec<FailureIssue>,
    host: Option<String>,
) -> Option<ResponseBadge> {
    ResponseBadge::new(
        status,
        failure.as_ref(),
        &issues.into_iter().map(Into::into).collect::<Vec<_>>(),
        host.as_deref(),
    )
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
#[uniffi::export]
impl AppRunHistory {
    pub fn record_typed(
        &self,
        entry: wirebolt_app::session::history::SessionHistoryEntry,
        body: Arc<AppResponseBody>,
    ) -> Result<(), ResponseError> {
        lock(&self.inner).record(entry, &lock(&body.inner))
    }
    pub fn list_typed(
        &self,
        request_id: String,
    ) -> Result<Vec<wirebolt_app::session::history::SessionHistoryEntry>, ResponseError> {
        lock(&self.inner).list(&request_id)
    }
}

fn regex_engine() -> &'static dyn wirebolt_app::response::RegexEngine {
    #[cfg(target_vendor = "apple")]
    {
        &apple_regex::AppleRegexEngine
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        &wirebolt_app::response::PortableRegexEngine
    }
}

// Keep the native ICU dialect and Unicode data at the platform boundary.
#[cfg(target_vendor = "apple")]
mod apple_regex {
    use std::{
        ffi::{CStr, c_char, c_void},
        ptr::{self, NonNull},
    };
    use wirebolt_app::response::{
        RegexEngine, ResponseCancellation, ResponseError, ResponseResult, ResponseTextRange,
        SearchQuery,
    };

    const MULTILINE: u32 = 8;
    const CASE_INSENSITIVE: u32 = 2;

    // Opaque ICU handle. Pattern/text buffers and callback context remain borrowed
    // until the expression is dropped; the handle never leaves this synchronous call.
    struct Expression(NonNull<c_void>);
    impl Drop for Expression {
        fn drop(&mut self) {
            // SAFETY: This is the sole owner of the successfully opened ICU handle.
            unsafe {
                uregex_close(self.0.as_ptr());
            }
        }
    }
    impl Expression {
        fn range(&self, status: &mut i32) -> ResponseResult<ResponseTextRange> {
            // SAFETY: A successful match has capture group zero and live bounds.
            let (start, end) = unsafe {
                (
                    uregex_start64(self.0.as_ptr(), 0, status),
                    uregex_end64(self.0.as_ptr(), 0, status),
                )
            };
            check_status(*status)?;
            let start = u64::try_from(start).map_err(|_| invalid_pattern("Invalid match start"))?;
            let end = u64::try_from(end).map_err(|_| invalid_pattern("Invalid match end"))?;
            Ok(ResponseTextRange {
                location: start,
                length: end.saturating_sub(start),
            })
        }
    }
    #[derive(Debug)]
    pub(super) struct AppleRegexEngine;
    impl RegexEngine for AppleRegexEngine {
        fn matches(
            &self,
            query: &SearchQuery,
            source: &str,
            scope: Option<ResponseTextRange>,
            cancellation: &ResponseCancellation,
        ) -> ResponseResult<Vec<ResponseTextRange>> {
            matches(query, source, scope, cancellation)
        }
    }
    fn matches(
        query: &SearchQuery,
        source: &str,
        scope: Option<ResponseTextRange>,
        cancellation: &ResponseCancellation,
    ) -> ResponseResult<Vec<ResponseTextRange>> {
        cancellation.check()?;
        if query.text.is_empty() {
            return Ok(vec![]);
        }
        let pattern = if query.regular_expression {
            query.text.clone()
        } else {
            format!(r"\Q{}\E", query.text.replace(r"\E", r"\E\\E\Q"))
        };
        let pattern = if query.whole_word {
            format!(r"(?<![\p{{L}}\p{{N}}_])(?:{pattern})(?![\p{{L}}\p{{N}}_])")
        } else {
            pattern
        };
        let pattern: Vec<_> = pattern.encode_utf16().collect();
        let units: Vec<_> = source.encode_utf16().collect();
        let pattern_length =
            i32::try_from(pattern.len()).map_err(|_| invalid_pattern("Pattern too long"))?;
        let length = i32::try_from(units.len()).map_err(|_| invalid_pattern("Text too long"))?;
        let mut status = 0;
        // SAFETY: ICU copies the pattern during open. A null parse-error pointer is
        // explicitly supported. Both buffers contain their declared UTF-16 lengths.
        let raw = unsafe {
            uregex_open(
                pattern.as_ptr(),
                pattern_length,
                MULTILINE
                    | if query.match_case {
                        0
                    } else {
                        CASE_INSENSITIVE
                    },
                ptr::null_mut(),
                &raw mut status,
            )
        };
        let expression = NonNull::new(raw)
            .map(Expression)
            .ok_or_else(|| status_error(status))?;
        check_status(status)?;
        let region = scope.unwrap_or(ResponseTextRange {
            location: 0,
            length: u64::try_from(units.len()).unwrap_or(u64::MAX),
        });
        let start = i32::try_from(
            region
                .location
                .min(u64::try_from(units.len()).unwrap_or(u64::MAX)),
        )
        .unwrap_or(length);
        let end = i32::try_from(
            region
                .end()
                .min(u64::try_from(units.len()).unwrap_or(u64::MAX)),
        )
        .unwrap_or(length)
        .max(start);
        // SAFETY: Text and cancellation context outlive the expression. ICU invokes
        // progress callbacks synchronously; it does not retain them after close.
        unsafe {
            uregex_setText(
                expression.0.as_ptr(),
                units.as_ptr(),
                length,
                &raw mut status,
            );
            uregex_setRegion(expression.0.as_ptr(), start, end, &raw mut status);
            uregex_setMatchCallback(
                expression.0.as_ptr(),
                Some(match_progress),
                ptr::from_ref(cancellation).cast(),
                &raw mut status,
            );
            uregex_setFindProgressCallback(
                expression.0.as_ptr(),
                Some(find_progress),
                ptr::from_ref(cancellation).cast(),
                &raw mut status,
            );
        }
        check_status(status)?;
        let mut result = Vec::new();
        loop {
            cancellation.check()?;
            // SAFETY: The handle, text and callback context are live for the call.
            let found = unsafe { uregex_findNext(expression.0.as_ptr(), &raw mut status) };
            cancellation.check()?;
            check_status(status)?;
            if found == 0 {
                break;
            }
            result.push(expression.range(&mut status)?);
            if result.len() == SearchQuery::MAXIMUM_MATCHES {
                break;
            }
        }
        Ok(result)
    }
    fn invalid_pattern(reason: &str) -> ResponseError {
        ResponseError::InvalidPattern {
            reason: reason.into(),
        }
    }
    fn status_error(status: i32) -> ResponseError {
        // SAFETY: ICU returns a static, NUL-terminated name for every status code.
        let reason = unsafe { CStr::from_ptr(u_errorName(status)) }.to_string_lossy();
        invalid_pattern(&reason)
    }
    fn check_status(status: i32) -> ResponseResult<()> {
        if status > 0 {
            Err(status_error(status))
        } else {
            Ok(())
        }
    }
    unsafe extern "C" fn match_progress(context: *const c_void, _steps: i32) -> i8 {
        // SAFETY: `matches` installs a live borrowed cancellation token as context.
        i8::from(
            unsafe { &*context.cast::<ResponseCancellation>() }
                .check()
                .is_ok(),
        )
    }
    unsafe extern "C" fn find_progress(context: *const c_void, _offset: i64) -> i8 {
        // SAFETY: The callback has the same live context as `match_progress`.
        unsafe { match_progress(context, 0) }
    }
    #[link(name = "icucore")]
    unsafe extern "C" {
        fn uregex_open(
            pattern: *const u16,
            length: i32,
            flags: u32,
            parse_error: *mut c_void,
            status: *mut i32,
        ) -> *mut c_void;
        fn uregex_close(expression: *mut c_void);
        fn uregex_setText(expression: *mut c_void, text: *const u16, length: i32, status: *mut i32);
        fn uregex_setRegion(expression: *mut c_void, start: i32, end: i32, status: *mut i32);
        fn uregex_findNext(expression: *mut c_void, status: *mut i32) -> i8;
        fn uregex_start64(expression: *mut c_void, group: i32, status: *mut i32) -> i64;
        fn uregex_end64(expression: *mut c_void, group: i32, status: *mut i32) -> i64;
        fn uregex_setMatchCallback(
            expression: *mut c_void,
            callback: Option<unsafe extern "C" fn(*const c_void, i32) -> i8>,
            context: *const c_void,
            status: *mut i32,
        );
        fn uregex_setFindProgressCallback(
            expression: *mut c_void,
            callback: Option<unsafe extern "C" fn(*const c_void, i64) -> i8>,
            context: *const c_void,
            status: *mut i32,
        );
        fn u_errorName(status: i32) -> *const c_char;
    }
}

#[cfg(all(test, target_vendor = "apple"))]
mod regex_tests {
    use super::*;
    fn query(text: &str) -> SearchQuery {
        SearchQuery {
            text: text.into(),
            ..SearchQuery::default()
        }
    }
    fn cancellation() -> ResponseCancellation {
        ResponseCancellation::default()
    }
    #[test]
    fn search_preserves_icu_unicode_dialect_and_line_endings() {
        let source = "a\rb\nc\r\nd\u{85}e\u{2028}f\u{2029}g";
        for pattern in [".", "^.", ".$"] {
            let hits = SearchQuery {
                regular_expression: true,
                ..query(pattern)
            }
            .matches_with_engine(source, None, &cancellation(), regex_engine())
            .unwrap();
            assert_eq!(
                hits.iter().map(|r| r.location).collect::<Vec<_>>(),
                [0, 2, 4, 7, 9, 11, 13]
            );
        }
        assert_eq!(
            SearchQuery {
                regular_expression: true,
                ..query(r"\Qabc\E")
            }
            .matches_with_engine("abc", None, &cancellation(), regex_engine())
            .unwrap(),
            [ResponseTextRange {
                location: 0,
                length: 3
            }]
        );
        assert_eq!(
            SearchQuery {
                regular_expression: true,
                ..query("(?i)strasse")
            }
            .matches_with_engine("Straße STRASSE", None, &cancellation(), regex_engine())
            .unwrap()
            .len(),
            2
        );
        assert_eq!(
            SearchQuery {
                regular_expression: true,
                ..query("[[:alpha:]]+")
            }
            .matches_with_engine("αβ ΑΒ 東京_1", None, &cancellation(), regex_engine())
            .unwrap()
            .len(),
            3
        );
        assert_eq!(
            SearchQuery {
                regular_expression: true,
                ..query(r"\X")
            }
            .matches_with_engine("👨‍👩‍👧‍👦e\u{301}", None, &cancellation(), regex_engine())
            .unwrap()
            .len(),
            2
        );
        assert_eq!(
            query(r"\E[]")
                .matches_with_engine(r"\E[]", None, &cancellation(), regex_engine())
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn cancellation_interrupts_icu_backtracking() {
        let cancellation = Arc::new(cancellation());
        let worker_cancellation = Arc::clone(&cancellation);
        let worker = std::thread::spawn(move || {
            SearchQuery {
                regular_expression: true,
                ..query("(a|aa)+$")
            }
            .matches_with_engine(
                &("a".repeat(2000) + "!"),
                None,
                &worker_cancellation,
                regex_engine(),
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(2));
        cancellation.cancel();
        assert_eq!(worker.join().unwrap(), Err(ResponseError::Cancelled));
    }

    #[test]
    fn native_zero_length_matches_terminate() {
        assert_eq!(
            SearchQuery {
                regular_expression: true,
                ..query("(?=x)")
            }
            .matches_with_engine("xx", None, &cancellation(), regex_engine())
            .unwrap()
            .len(),
            2
        );
    }
}

#[uniffi::remote(Enum)]
pub enum JsonNodeKind {
    Object,
    Array,
    String,
    Number,
    Boolean,
    Null,
    Truncated,
}

#[uniffi::remote(Enum)]
pub enum ResponseFailureKind {
    Cancelled,
    Connection,
    ConnectTimeout,
    Timeout,
    TotalTimeout,
    ReadTimeout,
    Tls,
    TlsConfiguration,
    Proxy,
    ProxyConfiguration,
    Keychain,
    Request,
    ResponseBody,
    ResponseTooLarge,
    StreamStopped,
    Transport,
    InvalidRequest,
    InvalidInput,
    UnresolvedValue,
    Internal,
    Bridge,
    Storage,
    Export,
    Workspace,
    Other { code: String },
}

#[derive(Debug, uniffi::Enum)]
pub enum ResponseIssueKind {
    InvalidMethod,
    InvalidUrl,
    InvalidHeaderName,
    InvalidHeaderValue,
    ConflictingHeader,
    InvalidJson,
    InvalidBody,
    FileUnavailable,
    InvalidTemplate,
    TemplateTooDeep,
    ResolvedValueTooLarge,
    MissingVariable,
    UnresolvedValue,
    CyclicVariable,
    MissingSecret,
    Other { code: String },
}
impl ResponseIssueKind {
    fn as_str(&self) -> &str {
        match self {
            Self::InvalidMethod => "invalid_method",
            Self::InvalidUrl => "invalid_url",
            Self::InvalidHeaderName => "invalid_header_name",
            Self::InvalidHeaderValue => "invalid_header_value",
            Self::ConflictingHeader => "conflicting_header",
            Self::InvalidJson => "invalid_json",
            Self::InvalidBody => "invalid_body",
            Self::FileUnavailable => "file_unavailable",
            Self::InvalidTemplate => "invalid_template",
            Self::TemplateTooDeep => "template_too_deep",
            Self::ResolvedValueTooLarge => "resolved_value_too_large",
            Self::MissingVariable => "missing_variable",
            Self::UnresolvedValue => "unresolved_value",
            Self::CyclicVariable => "cyclic_variable",
            Self::MissingSecret => "missing_secret",
            Self::Other { code } => code,
        }
    }
}

impl From<FailureIssue> for CoreFailureIssue {
    fn from(issue: FailureIssue) -> Self {
        Self {
            path: issue.path,
            kind: issue.kind.as_str().to_owned(),
            reference: issue.reference,
        }
    }
}

#[cfg(test)]
mod choice_tests {
    use super::*;
    #[test]
    fn unknown_diagnostics_and_issue_references_are_preserved() {
        let issue = FailureIssue {
            path: "body".into(),
            kind: ResponseIssueKind::MissingVariable,
            reference: Some("token".into()),
        };
        assert!(app_describe_issue(issue).contains("“token”"));
        let unknown = ResponseFailureKind::Other {
            code: "workspace_reload".into(),
        };
        assert_eq!(
            app_failure_message(unknown, vec![], None).message,
            "Workspace reload."
        );
        let unknown = FailureIssue {
            path: "body".into(),
            kind: ResponseIssueKind::Other {
                code: "future_issue".into(),
            },
            reference: None,
        };
        assert_eq!(app_describe_issue(unknown), "Future issue. (body)");
        assert_eq!(
            app_failure_message(ResponseFailureKind::Cancelled, vec![], None).category,
            FailureCategory::Cancelled
        );
        assert_eq!(
            app_failure_message(ResponseFailureKind::TlsConfiguration, vec![], None).category,
            FailureCategory::Tls
        );
    }
    #[test]
    fn shared_lock_recovers_the_value_after_poisoning() {
        let mutex = Arc::new(Mutex::new(42));
        let worker = Arc::clone(&mutex);
        assert!(
            std::thread::spawn(move || {
                let _guard = worker.lock().unwrap();
                panic!("poison fixture");
            })
            .join()
            .is_err()
        );
        assert_eq!(*lock(&mutex), 42);
    }
}
