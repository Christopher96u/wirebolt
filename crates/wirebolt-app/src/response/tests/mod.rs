//! Behavioral cases ported from the Swift response suites. Native font and
//! attributed-string assertions remain in Swift as platform-adapter coverage.
mod folding;
mod formatting;
mod indexes;
mod json;
mod markdown;
mod storage;
mod text;
use super::*;
use std::sync::Arc;
fn cancellation() -> ResponseCancellation {
    ResponseCancellation::default()
}
fn body(data: &[u8]) -> (tempfile::TempDir, BodyStore) {
    let root = tempfile::tempdir().unwrap();
    let mut body = BodyStore::new("test", root.path()).unwrap();
    body.append(data).unwrap();
    body.finish().unwrap();
    (root, body)
}
fn options(
    path: &std::path::Path,
    columns: u64,
    prefix: &str,
    wrapping: Option<WrapLayout>,
) -> IndexOptions {
    IndexOptions {
        path: path.to_string_lossy().into_owned(),
        columns,
        prefix: prefix.into(),
        wrapping,
    }
}
fn layout(width: f64) -> WrapLayout {
    WrapLayout {
        font_name: "Test monospace".into(),
        font_size: 12.0,
        width,
        advance: 7.2,
    }
}
#[derive(Debug)]
struct TestShaper;
impl TextShaper for TestShaper {
    fn measure(&self, requests: Vec<ShapingRequest>, layout: WrapLayout) -> Vec<ShapingMetrics> {
        requests
            .into_iter()
            .map(|request| ShapingMetrics {
                advances: request
                    .boundaries
                    .windows(2)
                    .map(|_| layout.advance)
                    .collect(),
                groups: vec![],
            })
            .collect()
    }
}
fn shaper() -> Arc<dyn TextShaper> {
    Arc::new(TestShaper)
}
