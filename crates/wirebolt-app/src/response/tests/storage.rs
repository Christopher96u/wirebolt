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
        history.record(HistoryEntry{id:format!("run-{index}"),request_id:"health".into(),created_at:f64::from(index),prepared:serde_json::json!({"method":"GET","url":"https://example.com","headers":[],"body":{"byte_count":0,"content_type":null,"text_preview":null,"redacted":false},"transport":{}}),response_head:None,completion:Some(serde_json::json!({"bytes_received":index,"total_time_ns":1})),failure:None,body_path:String::new()},&store).unwrap();
        store.remove();
    }
    let mut reloaded = RunHistory::new(root.path().into());
    let entries = reloaded.list("health");
    assert_eq!(entries.len(), 100);
    assert_eq!(reloaded.count("health"), 100);
    assert_eq!(reloaded.count("missing"), 0);
    assert!(
        entries
            .iter()
            .all(|e| std::path::Path::new(&e.body_path).exists())
    );
    reloaded.clear("health").unwrap();
    assert_eq!(reloaded.count("health"), 0);
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
    let source=br#"{"id":"run","requestID":"health","createdAt":42,"prepared":{"method":"GET"},"bodyPath":"/runtime/body"}"#;
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
