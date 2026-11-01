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
fn cookie_dates_match_the_two_foundation_formatter_patterns() {
    // Generated with the unmodified 081c6d0 en_US_POSIX DateFormatter patterns:
    // EEE, dd MMM yyyy HH:mm:ss zzz and EEEE, dd-MMM-yy HH:mm:ss zzz.
    let cases: Vec<(String, Option<f64>)> =
        serde_json::from_str(include_str!("fixtures/cookie-dates.json")).unwrap();
    let differences: Vec<_> = cases
        .into_iter()
        .filter_map(|(source, expected)| {
            let actual = expires(&source);
            (actual != expected).then(|| format!("{source:?}: {actual:?}, expected {expected:?}"))
        })
        .collect();
    assert!(differences.is_empty(), "{}", differences.join("\n"));
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
