use super::*;
use crate::session::history::SessionRequestIdentity;
fn identity(request: &str) -> SessionRequestIdentity {
    SessionRequestIdentity {
        collection_id: "api".into(),
        request_id: request.into(),
    }
}
fn header(value: &str) -> ResponseHeaderValue {
    ResponseHeaderValue {
        name: "Set-Cookie".into(),
        value: value.into(),
    }
}
#[test]
fn response_cookies_exclude_prior_hosts() {
    let mut jar = ResponseCookieJar::new(None);
    let _ = jar.store(&[header("old=one; Path=/")], "https://old.example.com", 0.0);
    let received = jar.store(&[header("new=two; Path=/")], "https://new.example.com", 0.0);
    assert_eq!(
        received.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["new"]
    );
    assert_eq!(jar.all(0.0).len(), 2);
}
#[test]
fn streams_searches_and_exports_without_retaining_complete_body() {
    let root = tempfile::tempdir().unwrap();
    let mut store = BodyStore::new("body", root.path()).unwrap();
    let bytes = format!("{}needle", "0123456789".repeat(20_000)).into_bytes();
    store.append(&bytes[..91_000]).unwrap();
    store.append(&bytes[91_000..]).unwrap();
    store.finish().unwrap();
    assert_eq!(store.size(), super::super::count(bytes.len()));
    assert_eq!(store.viewport(0, 32 * 1024).unwrap().len(), 32 * 1024);
    assert_eq!(store.count_occurrences("needle").unwrap(), 1);
    let exported = root.path().join("export.body");
    store.export(&exported).unwrap();
    assert_eq!(std::fs::read(exported).unwrap(), bytes);
}
#[test]
fn history_survives_recreation_and_evicts_past_quota() {
    let root = tempfile::tempdir().unwrap();
    let mut history = RunHistory::new(root.path().into());
    let mut displayed = None;
    for index in 0..=100 {
        let mut store = BodyStore::new(&format!("run-{index}"), root.path()).unwrap();
        store.append(format!("run {index}").as_bytes()).unwrap();
        store.finish().unwrap();
        history
            .record(
                HistoryEntry {
                    id: format!("run-{index}"),
                    request_id: "health".into(),
                    identity: Some(identity("health")),
                    created_at: f64::from(index),
                    prepared: crate::session::run::SessionPrepared {
                        proxy: None,
                        method: "GET".into(),
                        url: "https://example.com".into(),
                        headers: vec![],
                        body: crate::session::run::SessionPreparedBody {
                            byte_count: 0,
                            content_type: None,
                            text_preview: None,
                            redacted: false,
                        },
                        transport: crate::domain::DomainTransport::default(),
                    },
                    response_head: None,
                    completion: Some(crate::session::run::SessionCompletion {
                        bytes_received: u64::try_from(index).unwrap(),
                        total_time_ns: 1,
                    }),
                    failure: None,
                    body_path: String::new(),
                },
                &store,
            )
            .unwrap();
        if index == 0 {
            let entry = history.list(&identity("health")).unwrap().remove(0);
            displayed = Some(BodyStore::retained(std::path::Path::new(&entry.body_path)).unwrap());
        }
        store.remove();
    }
    assert_eq!(
        displayed.as_ref().unwrap().viewport(0, 5).unwrap(),
        b"run 0"
    );
    let mut reloaded = RunHistory::new(root.path().into());
    let entries = reloaded.list(&identity("health")).unwrap();
    assert_eq!(entries.len(), 100);
    assert_eq!(reloaded.count(&identity("health")).unwrap(), 100);
    assert_eq!(reloaded.count(&identity("missing")).unwrap(), 0);
    assert!(
        entries
            .iter()
            .all(|e| std::path::Path::new(&e.body_path).exists())
    );
    reloaded.clear(&identity("health")).unwrap();
    assert_eq!(reloaded.count(&identity("health")).unwrap(), 0);
}
#[test]
fn cookies_honor_host_path_tls_expiry_and_persistence() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cookies.json");
    let mut jar = ResponseCookieJar::new(Some(path.clone()));
    let _ = jar.store(
        &[
            header("session=abc; Path=/account; Secure; HttpOnly; SameSite=Lax"),
            header("remember=1; Path=/account; Secure; Max-Age=3600"),
            header("gone=1; Max-Age=0"),
        ],
        "https://api.example.com/account/login",
        0.0,
    );
    assert_eq!(
        jar.header("https://api.example.com/account/me", 0.0),
        Some("session=abc; remember=1".into())
    );
    for url in [
        "http://api.example.com/account/me",
        "https://sub.api.example.com/account/me",
        "https://api.example.com/accounts",
    ] {
        assert_eq!(jar.header(url, 0.0), None);
    }
    let mut restored = ResponseCookieJar::new(Some(path.clone()));
    assert_eq!(
        restored.header("https://api.example.com/account/me", 0.0),
        Some("remember=1".into())
    );
    assert!(!std::fs::read_to_string(&path).unwrap().contains("session"));
}
#[test]
fn body_preview_waits_for_complete_utf8_and_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    let mut store = BodyStore::new("preview", root.path()).unwrap();
    store.append(&[0xf0, 0x9f]).unwrap();
    assert!(store.progress().text.is_empty());
    store.append(&[0x9a, 0x80]).unwrap();
    assert_eq!(store.progress().text, "🚀");
    store.append(&vec![b'x'; 40_000]).unwrap();
    assert_eq!(store.progress().data.len(), VIEWPORT_BYTES);
    assert!(store.progress().truncated);
    store.finish().unwrap();
    assert!(store.append(b"closed").is_err());
}
#[test]
fn cookie_domain_delete_and_dates() {
    let mut jar = ResponseCookieJar::new(None);
    let received = jar.store(
        &[
            header("a=1; Domain=.example.com; Expires=Wed, 09 Jun 2027 10:18:14 GMT"),
            header("b=2; Domain=elsewhere.com"),
        ],
        "https://api.example.com/a/b",
        0.0,
    );
    assert_eq!(received.len(), 1);
    assert_eq!(
        jar.header("https://sub.example.com/a/child", 0.0),
        Some("a=1".into())
    );
    assert_eq!(jar.header("https://sub.example.com/other", 0.0), None);
    jar.delete(&received[0].id());
    assert!(jar.all(0.0).is_empty());
    jar.remove_all();
}

#[test]
fn history_keeps_legacy_acronyms_and_omits_absent_optional_fields() {
    let source=br#"{"id":"run","requestID":"health","createdAt":42,"prepared":{"proxy":null,"method":"GET","url":"https://example.com","headers":[],"body":{"byte_count":0,"content_type":null,"text_preview":null,"redacted":false},"transport":{"validate_tls":true,"follow_redirects":true,"maximum_redirects":10,"total_timeout_ms":0,"read_timeout_ms":0,"client_certificate_reference":null,"custom_ca_path":null}},"bodyPath":"/runtime/body"}"#;
    let entry: HistoryEntry = serde_json::from_slice(source).unwrap();
    let encoded = serde_json::to_value(entry).unwrap();
    assert_eq!(encoded["requestID"], "health");
    assert!(encoded.get("requestId").is_none());
    for key in ["responseHead", "completion", "failure"] {
        assert!(encoded.get(key).is_none());
    }
}
#[test]
fn cookie_paths_preserve_encoded_urls_and_ignore_empty_attributes() {
    let mut jar = ResponseCookieJar::new(None);
    let received = jar.store(
        &[header("name=value; Domain=; Path=")],
        "https://api.example.com/caf%C3%A9/login",
        0.0,
    );
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].path, "/caf%C3%A9");
    assert!(received[0].host_only);
    assert_eq!(
        jar.header("https://api.example.com/caf%C3%A9/me", 0.0),
        Some("name=value".into())
    );
    assert_eq!(jar.header("https://api.example.com/other", 0.0), None);
}

#[test]
fn cookie_default_paths_preserve_directory_scope_and_encoded_separators() {
    for (path, expected) in [
        ("/", "/"),
        ("/admin", "/"),
        ("/admin/", "/admin"),
        ("/v1/users//", "/v1/users/"),
        ("/v1/users%2F", "/v1"),
        ("/admin%2Fprivate/login", "/admin%2Fprivate"),
    ] {
        let mut jar = ResponseCookieJar::new(None);
        let url = format!("https://h{path}");
        let received = jar.store(&[header("default=value")], &url, 0.0);
        assert_eq!(received[0].path, expected, "{url}");
        assert_eq!(jar.header(&url, 0.0).as_deref(), Some("default=value"));
        if expected != "/" {
            assert_eq!(jar.header("https://h/outside", 0.0), None);
        }
    }
}

#[test]
fn cookie_explicit_directory_paths_match_only_encoded_uri_scope() {
    let mut jar = ResponseCookieJar::new(None);
    let _ = jar.store(&[header("scoped=value; Path=/account/")], "https://h/", 0.0);
    for url in ["https://h/account/", "https://h/account//"] {
        assert_eq!(
            jar.header(url, 0.0).as_deref(),
            Some("scoped=value"),
            "{url}"
        );
    }
    assert_eq!(
        jar.header("https://h/account/me", 0.0).as_deref(),
        Some("scoped=value")
    );
    for path in ["/account", "/account%2F", "/account%2Fme", "/accounts/me"] {
        assert_eq!(jar.header(&format!("https://h{path}"), 0.0), None);
    }
    let mut jar = ResponseCookieJar::new(None);
    let _ = jar.store(
        &[header("encoded=value; Path=/admin%2Fprivate")],
        "https://h/",
        0.0,
    );
    assert_eq!(
        jar.header("https://h/admin%2Fprivate/me", 0.0).as_deref(),
        Some("encoded=value")
    );
    assert_eq!(jar.header("https://h/admin/private/me", 0.0), None);
}

#[test]
fn cookie_max_age_precedes_expires_and_invalid_attributes_do_not_erase_expiry() {
    for attributes in [
        "Max-Age=600; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        "Expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=600",
        "Max-Age=600; Max-Age=invalid; Expires=invalid",
        "Expires=Thu, 01 Jan 1970 00:00:00 GMT; Expires=invalid; Max-Age=invalid",
        "Max-Age=0; Expires=Wed, 09 Jun 2027 10:18:14 GMT",
    ] {
        let mut jar = ResponseCookieJar::new(None);
        let received = jar.store(
            &[header(&format!("session=fixture; {attributes}"))],
            "https://h/",
            1000.0,
        );
        let alive = attributes.contains("Max-Age=600");
        assert_eq!(
            jar.header("https://h/", 1001.0).is_some(),
            alive,
            "{attributes}"
        );
        if alive {
            assert_eq!(received[0].expires_at, Some(1600.0));
            assert_eq!(jar.header("https://h/", 1600.0), None);
        }
    }
}

#[test]
fn ipv6_cookies_keep_unbracketed_hosts_and_read_saved_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cookies.json");
    std::fs::write(&path, r#"[{"name":"saved","value":"old","domain":"::1","hostOnly":true,"path":"/","secure":false,"httpOnly":false,"sameSite":"","expiresAt":1000}]"#).unwrap();
    let mut jar = ResponseCookieJar::new(Some(path.clone()));
    assert_eq!(
        jar.header("http://[::1]/", 0.0).as_deref(),
        Some("saved=old")
    );
    let received = jar.store(
        &[header("new=value; Domain=::1; Max-Age=60")],
        "http://[::1]/",
        0.0,
    );
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].domain, "::1");
    let mut restored = ResponseCookieJar::new(Some(path));
    assert_eq!(
        restored.header("http://[::1]/", 0.0).as_deref(),
        Some("saved=old; new=value")
    );
    assert_eq!(restored.header("http://[::2]/", 0.0), None);
    for (url, host) in [
        ("http://[0:0:0:0:0:0:0:1]/", "0:0:0:0:0:0:0:1"),
        ("http://user:password@[::0001]:8080/", "::0001"),
        ("http://[::FFFF:192.0.2.1]/", "::ffff:192.0.2.1"),
    ] {
        let mut jar = ResponseCookieJar::new(None);
        let received = jar.store(&[header("ipv6=value")], url, 0.0);
        assert_eq!(received[0].domain, host);
        assert_eq!(jar.header(url, 0.0).as_deref(), Some("ipv6=value"));
        assert_eq!(jar.header("http://[::1]/", 0.0), None);
    }
}

#[test]
fn cookie_file_keys_resolve_symlinks_and_private_aliases() {
    use std::path::Path;
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&workspace, &alias).unwrap();
    assert_eq!(
        cookie_storage_path(&workspace, root.path()),
        cookie_storage_path(&alias, root.path())
    );
    assert_eq!(
        cookie_storage_path(Path::new("/tmp"), root.path())
            .file_name()
            .unwrap(),
        "e9671acd244849c57167c658fa2f9697.json"
    );
    assert_eq!(
        cookie_storage_path(Path::new("/private/tmp"), root.path()),
        cookie_storage_path(Path::new("/tmp"), root.path())
    );
    assert_ne!(
        cookie_storage_path(&alias.join("missing"), root.path()),
        cookie_storage_path(&workspace.join("missing"), root.path())
    );
}

#[test]
fn cookie_dates_preserve_pre_epoch_expiry_two_digit_years_and_named_timezones() {
    for (source, unix) in [
        ("Wed, 31 Dec 1969 23:59:59 GMT", -1.0),
        ("Sunday, 06-Nov-50 08:49:37 GMT", -604_422_623.0),
        ("Wed, 09 Jun 2027 10:18:14 PST", 1_812_565_094.0),
    ] {
        let parsed = crate::response::cookie_date::parse_cookie_date(source).unwrap();
        assert!((parsed - unix).abs() < 0.001, "{source}: {parsed}");
    }
    let mut jar = ResponseCookieJar::new(None);
    let _ = jar.store(
        &[header("old=value; Expires=Wed, 31 Dec 1969 23:59:59 GMT")],
        "https://example.com/",
        0.0,
    );
    assert!(jar.all(0.0).is_empty());
}

#[test]
fn damaged_history_is_reported_once_then_recording_and_clearing_resume() {
    for original in [
        br#"[{"id":"broken","requestID":"health","prepared":false}]"#.as_slice(),
        br#"{"version":2,"entries":[{"id":"broken"}]}"#,
        b"invalid JSON",
        br#"{"version":3,"entries":[]}"#,
    ] {
        let root = tempfile::tempdir().unwrap();
        let index = root.path().join("index.json");
        std::fs::write(&index, original).unwrap();
        let mut history = RunHistory::new(root.path().into());
        let error = history.take_load_problem().unwrap_err().to_string();
        assert!(error.starts_with("History could not be loaded."));
        assert!(error.contains("A backup is kept as"));
        assert_eq!(std::fs::read(&index).unwrap(), original);
        assert!(history.list(&identity("health")).unwrap().is_empty());
        history.take_load_problem().unwrap();
        let mut body = BodyStore::new("source", root.path()).unwrap();
        body.finish().unwrap();
        history
            .record(
                history_fixture("new", Some(identity("health")), String::new()),
                &body,
            )
            .unwrap();
        assert_eq!(history.count(&identity("health")).unwrap(), 1);
        history.clear_all().unwrap();
        assert!(
            RunHistory::new(root.path().into())
                .list(&identity("health"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::fs::read(root.path().join("index.json.backup")).unwrap(),
            original
        );
    }
}

#[test]
fn unreadable_history_index_remains_an_error_for_this_repository() {
    let root = tempfile::tempdir().unwrap();
    let index = root.path().join("index.json");
    std::fs::create_dir(&index).unwrap();
    let mut history = RunHistory::new(root.path().into());
    assert!(history.count(&identity("health")).is_err());
    std::fs::remove_dir(&index).unwrap();
    assert!(history.clear_all().is_err());
    assert!(!index.exists());
}

#[test]
fn history_preserves_typed_failures_and_unknown_legacy_codes() {
    let source = r#"{"id":"run","requestID":"health","createdAt":42,"prepared":{"proxy":null,"method":"GET","url":"https://example.com","headers":[],"body":{"byte_count":0,"content_type":null,"text_preview":null,"redacted":false},"transport":{"validate_tls":true,"follow_redirects":true,"maximum_redirects":10,"total_timeout_ms":0,"read_timeout_ms":0,"client_certificate_reference":null,"custom_ca_path":null}},"failure":{"kind":"future_failure","issues":[]},"bodyPath":"/runtime/body"}"#;
    let entry: HistoryEntry = serde_json::from_str(source).unwrap();
    assert_eq!(
        entry.failure.as_ref().unwrap().kind,
        ResponseFailureKind::Other {
            code: "future_failure".into()
        }
    );
    let encoded = serde_json::to_value(&entry).unwrap();
    assert_eq!(encoded["failure"]["kind"], "future_failure");
    assert_eq!(
        serde_json::from_value::<HistoryEntry>(encoded).unwrap(),
        entry
    );
    for (code, expected) in [
        ("cancelled", ResponseFailureKind::Cancelled),
        ("tls_configuration", ResponseFailureKind::TlsConfiguration),
        ("storage", ResponseFailureKind::Storage),
    ] {
        let kind: ResponseFailureKind = serde_json::from_value(serde_json::json!(code)).unwrap();
        assert_eq!(kind, expected);
        assert_eq!(serde_json::to_value(kind).unwrap(), code);
    }
}

#[test]
fn secure_cookies_accept_uppercase_https_schemes() {
    let mut jar = ResponseCookieJar::new(None);
    let received = jar.store(&[header("a=b; Secure")], "HTTPS://example.com/login", 0.0);
    assert!(received[0].secure);
    assert_eq!(jar.header("HTTPS://example.com/", 0.0), Some("a=b".into()));
    assert_eq!(jar.header("http://example.com/", 0.0), None);
}

#[test]
fn cookies_skip_a_blank_first_segment() {
    let mut jar = ResponseCookieJar::new(None);
    let received = jar.store(&[header(" ; a=b; Secure")], "https://example.com/", 0.0);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].name, "a");
    assert_eq!(received[0].value, "b");
    assert!(received[0].secure);
}

#[test]
fn a_cookie_attribute_with_an_empty_name_is_not_secure() {
    let mut jar = ResponseCookieJar::new(None);
    let received = jar.store(&[header("a=b; =secure")], "https://example.com/", 0.0);
    assert!(!received[0].secure);
    assert_eq!(jar.header("http://example.com/", 0.0), Some("a=b".into()));
}

#[test]
fn cookies_ignore_blank_trailing_segments_without_panicking() {
    let mut jar = ResponseCookieJar::new(None);
    let received = jar.store(&[header("a=b; ;")], "https://example.com/", 0.0);
    assert_eq!(received.len(), 1);
    assert_eq!(jar.header("https://example.com/", 0.0), Some("a=b".into()));
}

#[test]
fn failed_response_export_preserves_destination() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.body");
    let destination = directory.path().join("saved.body");
    std::fs::write(&source, "response").unwrap();
    std::fs::write(&destination, "previous export").unwrap();
    let body = BodyStore::existing(source.clone()).unwrap();
    std::fs::remove_file(source).unwrap();
    assert!(body.export(&destination).is_err());
    assert_eq!(
        std::fs::read_to_string(destination).unwrap(),
        "previous export"
    );
}

#[test]
fn response_export_to_itself_preserves_body() {
    let directory = tempfile::tempdir().unwrap();
    let mut body = BodyStore::new("run", directory.path()).unwrap();
    body.append(b"response").unwrap();
    body.export(&body.path()).unwrap();
    body.append(b"!").unwrap();
    body.finish().unwrap();
    assert_eq!(body.viewport(0, 9).unwrap(), b"response!");
}

fn history_fixture(
    id: &str,
    identity: Option<SessionRequestIdentity>,
    body_path: String,
) -> HistoryEntry {
    HistoryEntry {
        id: id.into(),
        request_id: "same".into(),
        identity,
        created_at: 42.0,
        body_path,
        prepared: crate::session::run::SessionPrepared {
            proxy: None,
            method: "GET".into(),
            url: "https://example.com/fixture".into(),
            headers: vec![],
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
    }
}

#[test]
fn history_identity_isolates_collections_and_requests() {
    let root = tempfile::tempdir().unwrap();
    let mut history = RunHistory::new(root.path().join("history"));
    let a = identity("same");
    let b = SessionRequestIdentity {
        collection_id: "other".into(),
        ..a.clone()
    };
    let c = identity("another-request");
    let mut body = BodyStore::new("source", root.path()).unwrap();
    body.append(b"fixture").unwrap();
    body.finish().unwrap();
    for (id, identity) in [("a", &a), ("b", &b), ("c", &c)] {
        history
            .record(
                history_fixture(id, Some(identity.clone()), String::new()),
                &body,
            )
            .unwrap();
    }
    history.clear(&b).unwrap();
    let mut reloaded = RunHistory::new(root.path().join("history"));
    assert_eq!(reloaded.list(&a).unwrap()[0].id, "a");
    assert!(reloaded.list(&b).unwrap().is_empty());
    assert_eq!(reloaded.list(&c).unwrap()[0].id, "c");
}

#[test]
fn history_migration_preserves_scoped_and_ambiguous_legacy_entries() {
    let root = tempfile::tempdir().unwrap();
    let key = identity("same");
    let entries: Vec<_> = ["legacy-a", "legacy-b", "scoped"]
        .into_iter()
        .map(|id| {
            let path = root.path().join(format!("{id}.body"));
            std::fs::write(&path, b"fixture").unwrap();
            history_fixture(
                id,
                (id == "scoped").then(|| key.clone()),
                path.to_string_lossy().into_owned(),
            )
        })
        .collect();
    std::fs::write(
        root.path().join("index.json"),
        serde_json::to_vec(&entries).unwrap(),
    )
    .unwrap();
    let mut history = RunHistory::new(root.path().into());
    assert_eq!(history.list(&key).unwrap().len(), 1);
    assert_eq!(history.legacy_list("same").unwrap().len(), 2);
    history.clear(&key).unwrap();
    let mut reloaded = RunHistory::new(root.path().into());
    assert!(reloaded.list(&key).unwrap().is_empty());
    let legacy = reloaded.legacy_list("same").unwrap();
    assert!(legacy.is_empty());
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("index.json")).unwrap()).unwrap();
    assert_eq!(index, serde_json::json!([]));
}

#[test]
fn previous_v2_history_reads_then_writes_an_rc1_array() {
    let root = tempfile::tempdir().unwrap();
    let legacy = history_fixture("legacy", None, String::new());
    std::fs::write(
        root.path().join("index.json"),
        serde_json::to_vec(&serde_json::json!({"version":2,"entries":[legacy]})).unwrap(),
    )
    .unwrap();
    let mut history = RunHistory::new(root.path().into());
    assert_eq!(history.legacy_list("same").unwrap().len(), 1);
    let mut body = BodyStore::new("new", root.path()).unwrap();
    body.append(b"fixture").unwrap();
    body.finish().unwrap();
    history
        .record(
            history_fixture("new", Some(identity("same")), String::new()),
            &body,
        )
        .unwrap();
    let entries: Vec<HistoryEntry> =
        serde_json::from_slice(&std::fs::read(root.path().join("index.json")).unwrap()).unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn export_uses_umask_and_preserves_existing_permissions() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let normal = root.path().join("normal");
    std::fs::write(&normal, b"normal").unwrap();
    let mut body = BodyStore::new("source", root.path()).unwrap();
    body.append(b"exported").unwrap();
    body.finish().unwrap();
    let destination = root.path().join("export");
    body.export(&destination).unwrap();
    assert_eq!(
        std::fs::metadata(&destination).unwrap().mode() & 0o777,
        std::fs::metadata(normal).unwrap().mode() & 0o777
    );
    std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o640)).unwrap();
    body.export(&destination).unwrap();
    assert_eq!(
        std::fs::metadata(&destination).unwrap().mode() & 0o777,
        0o640
    );
    assert_eq!(std::fs::read(destination).unwrap(), b"exported");
}

#[test]
fn retained_body_clone_cost_and_release() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("large.body");
    let mut file = std::fs::File::create(&path).unwrap();
    let chunk = vec![b'x'; 1024 * 1024];
    for _ in 0..300 {
        file.write_all(&chunk).unwrap();
    }
    file.sync_all().unwrap();
    // Measure per-tab retention after the process lease has been initialized.
    drop(BodyStore::retained(&path).unwrap());
    let start = std::time::Instant::now();
    let retained = BodyStore::retained(&path).unwrap();
    let clone_ms = start.elapsed().as_secs_f64() * 1000.0;
    let retained_path = retained.path();
    let retained_directory = retained_path.parent().unwrap().to_owned();
    assert_eq!(retained.size(), 300 * 1024 * 1024);
    assert_eq!(retained.viewport(0, 1).unwrap(), b"x");
    // An existing destination forces the previous streaming-copy behavior.
    let existing = root.path().join("existing.body");
    std::fs::File::create(&existing).unwrap();
    let start = std::time::Instant::now();
    std::fs::copy(&path, &existing).unwrap();
    let copy_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "retained_body_300_mib clone_ms={clone_ms:.3} existing_destination_copy_ms={copy_ms:.3}"
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(retained.viewport(0, 1).unwrap(), b"x");
    drop(retained);
    assert!(!retained_path.exists());
    assert!(!retained_directory.exists());
}

#[test]
fn development_history_namespace_is_ignored_when_reading_identity() {
    let root = tempfile::tempdir().unwrap();
    let entry = history_fixture("development", Some(identity("health")), String::new());
    let mut value = serde_json::to_value(&entry).unwrap();
    value["identity"]["workspace"] = "old-path-sha256.json".into();
    std::fs::write(
        root.path().join("index.json"),
        serde_json::to_vec(&vec![value]).unwrap(),
    )
    .unwrap();
    let mut history = RunHistory::new(root.path().into());
    assert_eq!(history.list(&identity("health")).unwrap(), [entry]);
}
