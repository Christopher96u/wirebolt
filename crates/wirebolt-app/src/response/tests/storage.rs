use super::*;
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
    for index in 0..=100 {
        let mut store = BodyStore::new(&format!("run-{index}"), root.path()).unwrap();
        store.append(format!("run {index}").as_bytes()).unwrap();
        store.finish().unwrap();
        history
            .record(
                HistoryEntry {
                    id: format!("run-{index}"),
                    request_id: "health".into(),
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
        store.remove();
    }
    let mut reloaded = RunHistory::new(root.path().into());
    let entries = reloaded.list("health").unwrap();
    assert_eq!(entries.len(), 100);
    assert_eq!(reloaded.count("health").unwrap(), 100);
    assert_eq!(reloaded.count("missing").unwrap(), 0);
    assert!(
        entries
            .iter()
            .all(|e| std::path::Path::new(&e.body_path).exists())
    );
    reloaded.clear("health").unwrap();
    assert_eq!(reloaded.count("health").unwrap(), 0);
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
fn cookie_paths_decode_urls_and_ignore_empty_attributes() {
    let mut jar = ResponseCookieJar::new(None);
    let received = jar.store(
        &[header("name=value; Domain=; Path=")],
        "https://api.example.com/caf%C3%A9/login",
        0.0,
    );
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].path, "/café");
    assert!(received[0].host_only);
    assert_eq!(
        jar.header("https://api.example.com/caf%C3%A9/me", 0.0),
        Some("name=value".into())
    );
    assert_eq!(jar.header("https://api.example.com/other", 0.0), None);
}

#[test]
fn cookie_paths_strip_trailing_slashes_like_foundation() {
    for url in [
        "https://h/v1/users/",
        "https://h/v1/users//",
        "https://h/v1/users%2F",
    ] {
        let mut jar = ResponseCookieJar::new(None);
        let received = jar.store(&[header("default=value")], url, 0.0);
        assert_eq!(received[0].path, "/v1", "{url}");
        assert_eq!(
            jar.header("https://h/v1/other", 0.0).as_deref(),
            Some("default=value")
        );
    }
    let mut jar = ResponseCookieJar::new(None);
    let _ = jar.store(&[header("scoped=value; Path=/account/")], "https://h/", 0.0);
    for url in [
        "https://h/account/",
        "https://h/account//",
        "https://h/account%2F",
    ] {
        assert_eq!(jar.header(url, 0.0), None, "{url}");
    }
    assert_eq!(
        jar.header("https://h/account/me", 0.0).as_deref(),
        Some("scoped=value")
    );
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
fn corrupt_history_loads_empty_and_next_write_repairs_the_index() {
    let root = tempfile::tempdir().unwrap();
    let index = root.path().join("index.json");
    let original = br#"[{"id":"broken","requestID":"health","prepared":false}]"#;
    std::fs::write(&index, original).unwrap();
    let mut history = RunHistory::new(root.path().into());
    assert!(history.list("health").unwrap().is_empty());
    assert_eq!(history.count("health").unwrap(), 0);
    assert_eq!(std::fs::read(&index).unwrap(), original);
    history.clear("health").unwrap();
    assert_eq!(std::fs::read(&index).unwrap(), b"[]");

    std::fs::write(&index, original).unwrap();
    let mut history = RunHistory::new(root.path().into());
    let source = r#"{"id":"new-run","requestID":"health","createdAt":42,"prepared":{"proxy":null,"method":"GET","url":"https://example.com","headers":[],"body":{"byte_count":0,"content_type":null,"text_preview":null,"redacted":false},"transport":{"validate_tls":true,"follow_redirects":true,"maximum_redirects":10,"total_timeout_ms":0,"read_timeout_ms":0,"client_certificate_reference":null,"custom_ca_path":null}},"bodyPath":""}"#;
    let mut body = BodyStore::new("source", root.path()).unwrap();
    body.append(b"saved response").unwrap();
    body.finish().unwrap();
    history
        .record(serde_json::from_str(source).unwrap(), &body)
        .unwrap();
    let entries = RunHistory::new(root.path().into()).list("health").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "new-run");
    assert_eq!(
        std::fs::read(&entries[0].body_path).unwrap(),
        b"saved response"
    );
}

#[test]
fn failed_body_export_does_not_load_or_cache_an_unreadable_index() {
    let root = tempfile::tempdir().unwrap();
    let index = root.path().join("index.json");
    std::fs::write(&index, b"corrupt index").unwrap();
    let source = r#"{"id":"saved","requestID":"health","createdAt":42,"prepared":{"proxy":null,"method":"GET","url":"https://example.com","headers":[],"body":{"byte_count":0,"content_type":null,"text_preview":null,"redacted":false},"transport":{"validate_tls":true,"follow_redirects":true,"maximum_redirects":10,"total_timeout_ms":0,"read_timeout_ms":0,"client_certificate_reference":null,"custom_ca_path":null}},"bodyPath":""}"#;
    let saved: HistoryEntry = serde_json::from_str(source).unwrap();
    let mut history = RunHistory::new(root.path().into());
    let mut body = BodyStore::new("source", root.path()).unwrap();
    body.finish().unwrap();
    std::fs::remove_file(body.path()).unwrap();
    assert!(history.record(saved.clone(), &body).is_err());
    std::fs::write(&index, serde_json::to_vec(&vec![saved.clone()]).unwrap()).unwrap();
    assert_eq!(history.list("health").unwrap(), vec![saved]);
}

#[test]
fn unreadable_history_index_is_cached_as_empty() {
    let root = tempfile::tempdir().unwrap();
    let index = root.path().join("index.json");
    std::fs::create_dir(&index).unwrap();
    let mut history = RunHistory::new(root.path().into());
    assert_eq!(history.count("health").unwrap(), 0);
    std::fs::remove_dir(&index).unwrap();
    history.clear("health").unwrap();
    assert_eq!(std::fs::read(&index).unwrap(), b"[]");
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
