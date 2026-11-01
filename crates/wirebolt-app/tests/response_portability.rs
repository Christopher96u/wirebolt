use wirebolt_app::response::*;

fn expires(source: &str) -> Option<f64> {
    ResponseCookieJar::new(None)
        .store(
            &[ResponseHeaderValue {
                name: "Set-Cookie".into(),
                value: format!("name=value; Expires={source}"),
            }],
            "https://example.com/",
            -20_000_000_000.0,
        )
        .first()
        .unwrap()
        .expires_at
}
#[test]
fn cookie_dates_use_unix_seconds_and_preserve_legacy_formats() {
    for (source, expected) in [
        ("Thu, 01 Jan 1970 00:00:00 GMT", 0.0),
        ("Wed, 31 Dec 1969 23:59:59 GMT", -1.0),
        ("Sunday, 06-Nov-50 08:49:37 GMT", -604_422_623.0),
        ("Sun, 06 Nov 49 08:49:37 GMT", 2_519_801_377.0),
        ("Wed, 09 Jun 2027 10:18:14 GMT", 1_812_536_294.0),
        ("Wed, 09 Jun 2027 10:18:14 HST", 1_812_572_294.0),
        ("Wed, 09 Jun 2027 10:18:14 AKST", 1_812_568_694.0),
        ("Wed, 09 Jun 2027 10:18:14 AKDT", 1_812_565_094.0),
        ("Wed, 09 Jun 2027 10:18:14 GMT+2", 1_812_529_094.0),
        ("Wed, 09 Jun 2027 10:18:14 gmt+0200", 1_812_529_094.0),
        ("Wednesday, 09-Jun-27 10:18:14 GMT", 1_812_536_294.0),
        ("Wed Jun 9 10:18:14 2027", 1_812_536_294.0),
        ("09/JUNE/2027 10:18:14 UTC", 1_812_536_294.0),
        ("Wed, 09 Jun 2027 10:18:14 PST", 1_812_565_094.0),
        ("Wed, 09 Jun 2027 10:18:14 MST", 1_812_561_494.0),
        ("Wed, 09 Jun 2027 10:18:14 GMT+0200", 1_812_529_094.0),
        ("Wed, 09 Jun 2027 10:18:14 +02:00", 1_812_529_094.0),
        ("Wed, 09 Jun 2027 10:18:14 -0530", 1_812_556_094.0),
        ("Tue, 29 Feb 2000 00:00:00 GMT", 951_782_400.0),
    ] {
        assert_eq!(expires(source), Some(expected), "{source}");
    }
}
#[test]
fn malformed_or_impossible_cookie_dates_are_ignored() {
    for source in [
        "",
        "nonsense",
        "09 Jun 2027",
        "09 Jun 2027 24:00:00 GMT",
        "09 Jun 2027 10:60:00 GMT",
        "09 Jun 2027 10:00:60 GMT",
        "00 Jun 2027 10:00:00 GMT",
        "31 Apr 2027 10:00:00 GMT",
        "29 Feb 1900 10:00:00 GMT",
        "29 Feb 2027 10:00:00 GMT",
        "09 Jun 1600 10:00:00 GMT",
        "09 Jun 2027 10:00:00 +abc",
        "09 Jun 2027 10:00:00 +",
    ] {
        assert_eq!(expires(source), None, "{source}");
    }
}
#[test]
fn cookie_files_keep_the_existing_epoch_while_bridge_records_use_unix() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("cookies.json");
    std::fs::write(&path, r#"[{"name":"name","value":"value","domain":"example.com","hostOnly":true,"path":"/","secure":false,"httpOnly":false,"sameSite":"Lax","expiresAt":834229094}]"#).unwrap();
    let mut jar = ResponseCookieJar::new(Some(path.clone()));
    assert_eq!(
        jar.all(1_812_536_293.0)[0].expires_at,
        Some(1_812_536_294.0)
    );
    let received = jar.store(
        &[ResponseHeaderValue {
            name: "Set-Cookie".into(),
            value: "name=value; Max-Age=1".into(),
        }],
        "https://example.com/",
        1_812_536_293.0,
    );
    assert_eq!(received[0].expires_at, Some(1_812_536_294.0));
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(persisted[0]["expiresAt"], 834_229_094.0);
    assert!(
        ResponseCookieJar::new(Some(path))
            .all(1_812_536_294.0)
            .is_empty()
    );
}
#[test]
fn portable_regex_handles_literals_word_boundaries_regions_and_surrogates() {
    let token = ResponseCancellation::default();
    let query = SearchQuery {
        text: "café".into(),
        whole_word: true,
        ..SearchQuery::default()
    };
    assert_eq!(
        query
            .matches("🚀 café CAFÉ café_ café1", None, &token)
            .unwrap(),
        [
            ResponseTextRange {
                location: 3,
                length: 4
            },
            ResponseTextRange {
                location: 8,
                length: 4
            }
        ]
    );
    assert_eq!(
        query
            .matches(
                "🚀 café CAFÉ",
                Some(ResponseTextRange {
                    location: 8,
                    length: 4
                }),
                &token
            )
            .unwrap(),
        [ResponseTextRange {
            location: 8,
            length: 4
        }]
    );
    assert!(
        query
            .matches(
                "🚀 café",
                Some(ResponseTextRange {
                    location: 1,
                    length: 5
                }),
                &token
            )
            .unwrap()
            .is_empty()
    );
    let literal = SearchQuery {
        text: r"\E[]".into(),
        ..SearchQuery::default()
    };
    assert_eq!(literal.matches(r"\E[]", None, &token).unwrap()[0].length, 4);
}
#[test]
fn searches_accept_a_complete_rust_engine_without_foreign_match_callbacks() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Engine(AtomicUsize);
    impl RegexEngine for Engine {
        fn matches(
            &self,
            query: &SearchQuery,
            source: &str,
            scope: Option<ResponseTextRange>,
            cancellation: &ResponseCancellation,
        ) -> ResponseResult<Vec<ResponseTextRange>> {
            self.0.fetch_add(1, Ordering::Relaxed);
            assert_eq!(query.text, "custom");
            assert_eq!(source, "🚀 text");
            cancellation.check()?;
            Ok(vec![scope.unwrap_or(ResponseTextRange {
                location: 3,
                length: 4,
            })])
        }
    }
    let engine = Engine(AtomicUsize::new(0));
    let query = SearchQuery {
        text: "custom".into(),
        ..SearchQuery::default()
    };
    let token = ResponseCancellation::default();
    assert_eq!(
        query
            .matches_with_engine("🚀 text", None, &token, &engine)
            .unwrap(),
        [ResponseTextRange {
            location: 3,
            length: 4
        }]
    );
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("body");
    std::fs::write(&path, "🚀 text").unwrap();
    let options = IndexOptions {
        path: path.to_string_lossy().into_owned(),
        columns: 120,
        prefix: String::new(),
        wrapping: None,
    };
    let index = TextIndex::new(options, u64::MAX, None, &token).unwrap();
    assert_eq!(
        index
            .search_with_engine(&query, None, &token, &engine)
            .unwrap(),
        [IndexedMatch {
            start: SearchPosition { row: 0, column: 3 },
            end: SearchPosition { row: 0, column: 7 }
        }]
    );
    assert_eq!(engine.0.load(Ordering::Relaxed), 2);
}

#[test]
fn legacy_timezone_prefixes_and_unicode_whitespace_preserve_the_same_instant() {
    for (suffix, offset) in [
        ("GMT (comment -05:00)", 0.0),
        ("GMT trailing +0200", 0.0),
        ("GMT-08:30 ignored+0200", -30_600.0),
        ("GMT +0200", 0.0),
        ("GMT+1:30", 5400.0),
        ("+130", 5400.0),
        ("GMT+02:00:30", 7230.0),
        ("+020030", 7230.0),
        ("+02:00:30", 7230.0),
        ("GMT+130", 46_800.0),
        ("UTC+130", 5400.0),
        ("UTC+0230", 9000.0),
        ("UTC+020030", 7230.0),
        ("GMT+020030", 7200.0),
        ("GMT+0230", 7200.0),
        ("+0260", 1560.0),
        ("+2500", 10_200.0),
        ("+013060", 786.0),
        ("+1:03", 36_000.0),
        ("GMT+1:03", 3780.0),
    ] {
        assert_eq!(
            expires(&format!("Wed, 09 Jun 2027 10:18:14 {suffix}")),
            Some(1_812_536_294.0 - offset),
            "{suffix}"
        );
    }
    for separator in ['\u{a0}', '\u{202f}'] {
        assert_eq!(
            expires(&format!("Wed, 09{separator}Jun 2027 10:18:14 GMT")),
            Some(1_812_536_294.0)
        );
    }
}
