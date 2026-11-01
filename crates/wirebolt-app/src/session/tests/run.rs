use super::super::run::*;
use crate::lock;
use crate::response::{StoredCookie, VIEWPORT_BYTES};
use crate::{
    domain::proxy::{ProxySource, ResolvedDomainProxy},
    response::{FailureIssue, ResponseIssueKind},
};
use std::time::{Duration, Instant};
fn head(status: u16) -> SessionRunEvent {
    SessionRunEvent::Head {
        head: SessionHead {
            status,
            version: "HTTP/1.1".into(),
            headers: vec![],
            time_to_headers_ns: 1,
        },
    }
}
fn complete(bytes: u64) -> SessionRunEvent {
    SessionRunEvent::Complete {
        completion: SessionCompletion {
            bytes_received: bytes,
            total_time_ns: 1,
        },
    }
}
fn completed() -> (tempfile::TempDir, DocumentRun) {
    let directory = tempfile::tempdir().unwrap();
    let mut run = DocumentRun::default();
    run.begin("previous", directory.path());
    run.consume("previous", head(200));
    run.append_batch("previous", &[b"previous".to_vec()], Instant::now());
    run.consume("previous", complete(8));
    (directory, run)
}
#[test]
fn previous_response_stays_visible_until_new_head() {
    let (directory, mut run) = completed();
    let previous = run.body().unwrap();
    run.begin("next", directory.path());
    let snapshot = run.snapshot();
    assert!(snapshot.awaiting_head && snapshot.started_at.is_some());
    assert_eq!(snapshot.text, "previous");
    assert_eq!(snapshot.head.unwrap().status, 200);
    assert!(std::sync::Arc::ptr_eq(&previous, &run.body().unwrap()));
    run.consume("next", head(404));
    let snapshot = run.snapshot();
    assert!(!snapshot.awaiting_head);
    assert_eq!(snapshot.head.unwrap().status, 404);
    assert!(snapshot.text.is_empty() && snapshot.completion.is_none());
    assert!(!std::sync::Arc::ptr_eq(&previous, &run.body().unwrap()));
    run.append_batch("next", &[b"next".to_vec()], Instant::now());
    run.consume("next", complete(4));
    assert_eq!(run.snapshot().text, "next");
    assert!(run.snapshot().active_run_id.is_none() && run.snapshot().started_at.is_none());
}
#[test]
fn failure_before_head_replaces_previous_response() {
    let (directory, mut run) = completed();
    run.begin("next", directory.path());
    run.finish(
        "next",
        Some(SessionFailure::new(
            crate::response::ResponseFailureKind::Connection,
        )),
    );
    let s = run.snapshot();
    assert_eq!(
        s.failure.unwrap().kind,
        crate::response::ResponseFailureKind::Connection
    );
    assert!(s.head.is_none() && s.text.is_empty() && run.body().is_some());
    assert!(!s.awaiting_head && s.started_at.is_none());
}
#[test]
fn cancellation_before_head_presents_cancelled_run_and_rejects_late_head() {
    let (directory, mut run) = completed();
    run.begin("next", directory.path());
    run.cancel("next");
    run.consume("next", head(200));
    assert_eq!(
        run.snapshot().failure.unwrap().kind,
        crate::response::ResponseFailureKind::Cancelled
    );
    assert!(run.snapshot().head.is_none() && run.snapshot().text.is_empty());
}
fn cookie() -> StoredCookie {
    StoredCookie {
        name: "id".into(),
        value: "1".into(),
        domain: "example.com".into(),
        host_only: true,
        path: "/".into(),
        secure: false,
        http_only: false,
        same_site: String::new(),
        expires_at: None,
    }
}
#[test]
fn cookies_before_head_belong_to_new_response() {
    let (directory, mut run) = completed();
    run.append_cookies(vec![cookie()]);
    run.begin("next", directory.path());
    run.append_cookies(vec![cookie(), cookie()]);
    assert_eq!(run.snapshot().cookies, vec![cookie()]);
    run.consume("next", head(302));
    assert_eq!(run.snapshot().cookies, vec![cookie(), cookie()]);
}
#[test]
fn slow_chunks_publish_at_sixteen_ms_and_preserve_utf8() {
    let directory = tempfile::tempdir().unwrap();
    let mut run = DocumentRun::default();
    run.begin("run", directory.path());
    run.consume("run", head(200));
    let bytes = "first 🌍 line\n".as_bytes();
    let start = Instant::now();
    assert_eq!(
        run.append_batch("run", &[bytes[..8].to_vec()], start),
        Some(PUBLICATION_INTERVAL)
    );
    run.publish_due("run", start + Duration::from_millis(15));
    assert_eq!(run.snapshot().bytes, 0);
    run.publish_due("run", start + PUBLICATION_INTERVAL);
    assert_eq!(run.snapshot().bytes, 8);
    assert_eq!(run.snapshot().text, "first ");
    assert!(run.accepts("run"));
    run.append_batch("run", &[bytes[8..].to_vec()], start);
    run.publish_due("run", start + PUBLICATION_INTERVAL);
    assert_eq!(run.snapshot().text, "first 🌍 line\n");
    assert_eq!(lock(&run.body().unwrap()).viewport(0, 100).unwrap(), bytes);
    run.consume("run", complete(bytes.len() as u64));
    assert!(run.snapshot().started_at.is_none());
}
#[test]
fn preview_is_bounded_and_file_retains_all_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let mut run = DocumentRun::default();
    run.begin("run", directory.path());
    let bytes = vec![65; VIEWPORT_BYTES * 4];
    for chunk in bytes.chunks(1024) {
        run.append_batch("run", &[chunk.to_vec()], Instant::now());
    }
    run.consume("run", complete(bytes.len() as u64));
    let snapshot = run.snapshot();
    assert_eq!(snapshot.data.len(), VIEWPORT_BYTES);
    assert_eq!(snapshot.text.len(), VIEWPORT_BYTES);
    assert_eq!(snapshot.bytes, bytes.len() as u64);
    assert!(snapshot.truncated);
    assert_eq!(
        lock(&run.body().unwrap())
            .viewport(0, bytes.len() as u64)
            .unwrap(),
        bytes
    );
}
#[test]
fn cancellation_retains_partial_file_and_rejects_old_generation() {
    let directory = tempfile::tempdir().unwrap();
    let mut run = DocumentRun::default();
    run.begin("run", directory.path());
    run.append_batch("run", &[b"partial".to_vec()], Instant::now());
    run.cancel("run");
    run.append_batch("run", &[b"late".to_vec()], Instant::now());
    assert_eq!(run.snapshot().text, "partial");
    assert_eq!(
        run.snapshot().failure.unwrap().kind,
        crate::response::ResponseFailureKind::Cancelled
    );
    assert_eq!(
        lock(&run.body().unwrap()).viewport(0, 100).unwrap(),
        b"partial"
    );
    run.begin("new", directory.path());
    run.append_batch("new", &[b"new".to_vec()], Instant::now());
    run.consume("run", complete(999));
    run.consume("new", complete(3));
    assert_eq!(run.snapshot().text, "new");
    assert!(run.snapshot().failure.is_none());
}
#[test]
fn completion_flushes_queued_publication() {
    let directory = tempfile::tempdir().unwrap();
    let mut run = DocumentRun::default();
    run.begin("run", directory.path());
    run.append_batch("run", &[b"done".to_vec()], Instant::now());
    run.consume("run", complete(4));
    assert_eq!(run.snapshot().text, "done");
    assert_eq!(run.snapshot().bytes, 4);
}
#[test]
fn storage_failure_ends_run_without_accepting_completion() {
    let directory = tempfile::tempdir().unwrap();
    let mut run = DocumentRun::default();
    run.begin("run", directory.path());
    run.consume("run", head(200));
    lock(&run.body().unwrap()).finish().unwrap();
    run.append_batch("run", &[b"lost".to_vec()], Instant::now());
    run.consume("run", complete(999));
    assert_eq!(
        run.snapshot().failure.unwrap().kind,
        crate::response::ResponseFailureKind::Storage
    );
    assert!(run.snapshot().completion.is_none());
}
#[test]
fn copy_shares_completed_body_but_does_not_copy_active_run() {
    let (directory, mut source) = completed();
    let mut copy = DocumentRun::default();
    copy.copy_completed(&source);
    assert!(std::sync::Arc::ptr_eq(
        &source.body().unwrap(),
        &copy.body().unwrap()
    ));
    source.begin("next", directory.path());
    let before = copy.snapshot();
    copy.copy_completed(&source);
    assert_eq!(copy.snapshot(), before);
}

#[test]
fn shared_transport_projection_preserves_redaction_proxy_source_and_timing() {
    use crate::session::run::{SessionCompletion, SessionFailure, SessionPrepared};
    use crate::session::transport::{
        PreparedBodyDocument, PreparedHeaderDocument, PreparedRunSnapshotDocument,
        RunFailureDocument,
    };
    let snapshot = PreparedRunSnapshotDocument {
        method: "POST".into(),
        url: "[REDACTED]".into(),
        headers: vec![PreparedHeaderDocument {
            name: "Authorization".into(),
            value: "••••••••".into(),
            redacted: true,
        }],
        body: PreparedBodyDocument {
            byte_count: 42,
            content_type: Some("application/json".into()),
            text_preview: None,
            redacted: true,
        },
        transport: wirebolt_core::TransportSettings::default(),
        proxy: ResolvedDomainProxy {
            configuration: crate::domain::DomainProxy::Direct,
            source: ProxySource::AppDefault,
        },
    };
    let projected = SessionPrepared::from(&snapshot);
    assert_eq!(projected.proxy.unwrap().source, ProxySource::AppDefault);
    assert_eq!(projected.url, "[REDACTED]");
    assert!(projected.headers[0].redacted);
    assert_eq!(projected.headers[0].value, "••••••••");
    assert_eq!(projected.body.byte_count, 42);
    assert!(projected.body.redacted && projected.body.text_preview.is_none());
    assert_eq!(projected.transport.total_timeout_ms, 30_000);
    let completion = SessionCompletion::from_transport(42, std::time::Duration::from_millis(3));
    assert_eq!(
        (completion.bytes_received, completion.total_time_ns),
        (42, 3_000_000)
    );
    let failure = SessionFailure::from(&RunFailureDocument {
        kind: crate::response::ResponseFailureKind::InvalidRequest,
        issues: vec![FailureIssue {
            path: "url".into(),
            kind: ResponseIssueKind::MissingSecret,
            reference: Some("api.token".into()),
        }],
    });
    assert_eq!(
        failure.kind,
        crate::response::ResponseFailureKind::InvalidRequest
    );
    assert_eq!(failure.issues[0].reference.as_deref(), Some("api.token"));
}
