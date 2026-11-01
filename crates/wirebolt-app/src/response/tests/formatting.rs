use super::*;
#[test]
fn durations() {
    for (n, expected) in [
        (0, "<0.1 ms"),
        (1, "<0.1 ms"),
        (400_000, "0.4 ms"),
        (949_000, "0.9 ms"),
        (960_000, "1 ms"),
        (4_000_000, "4 ms"),
        (186_400_000, "186 ms"),
        (999_400_000, "999 ms"),
        (999_600_000, "1.00 s"),
        (1_234_000_000, "1.23 s"),
        (4_006_000_000, "4.01 s"),
        (12_340_000_000, "12.3 s"),
        (125_000_000_000, "2 min 5 s"),
    ] {
        assert_eq!(
            format_duration(std::time::Duration::from_nanos(n).as_secs_f64()),
            expected
        );
    }
}
#[test]
fn elapsed() {
    assert_eq!(format_elapsed(0.0), "0.0 s");
    assert_eq!(format_elapsed(1.26), "1.3 s");
    assert_eq!(format_elapsed(61.0), "1 min 1 s");
}
#[test]
fn reason_phrases() {
    for (status, expected) in [
        (200, "200 OK"),
        (302, "302 Found"),
        (422, "422 Unprocessable Content"),
        (599, "599 Server Error"),
        (799, "799 Response"),
    ] {
        assert_eq!(status_line(status), expected);
    }
}
#[test]
fn summary() {
    assert_eq!(
        completion_summary(Some(200), Some(4_000_000), "468 KB".into()).to_lowercase(),
        "200 ok, 4 ms, 468 kb"
    );
}
#[test]
fn every_failure_kind_has_specific_copy() {
    for kind in [
        "cancelled",
        "total_timeout",
        "read_timeout",
        "connect_timeout",
        "stream_stopped",
        "response_too_large",
        "connection",
        "request",
        "response_body",
        "proxy",
        "transport",
        "tls_configuration",
        "invalid_request",
        "invalid_input",
        "internal",
        "bridge",
        "keychain",
        "unresolved_value",
    ] {
        let message = failure_message(kind, &[], Some("127.0.0.1:18880"));
        assert!(!message.title.is_empty() && !message.message.is_empty());
        assert!(!message.title.contains('_') && !message.message.contains('_'));
        assert!(!message.title.contains("Tls"));
    }
}
#[test]
fn connection_names_host_and_offers_network_settings() {
    let host = failure_host(Some("http://127.0.0.1:18880/echo"));
    let message = failure_message("connection", &[], host.as_deref());
    assert_eq!(message.title, "Couldn’t Connect to 127.0.0.1:18880");
    assert!(message.suggests_network_settings);
    assert_eq!(
        failure_message("tls_configuration", &[], None).category,
        FailureCategory::Tls
    );
    assert!(!failure_message("cancelled", &[], None).suggests_network_settings);
}
#[test]
fn hosts_require_concrete_absolute_urls() {
    assert_eq!(
        failure_host(Some("https://example.com/a")),
        Some("example.com".into())
    );
    for url in [Some("{{base}}/a"), Some("not a url"), None] {
        assert_eq!(failure_host(url), None);
    }
}
#[test]
fn issues_and_unknown_kinds_are_sentences() {
    let issue = FailureIssue {
        path: "url".into(),
        kind: "missing_variable".into(),
        reference: Some("base".into()),
    };
    assert_eq!(
        failure_message("invalid_request", &[issue], None).message,
        "The variable “base” isn’t defined in the active environment. (url)"
    );
    assert_eq!(
        failure_message("workspace_reload", &[], None).message,
        "Workspace reload."
    );
    assert_eq!(
        describe_issue(&FailureIssue {
            path: String::new(),
            kind: "invalid_template".into(),
            reference: None
        }),
        "A {{variable}} template isn’t valid."
    );
}

#[test]
fn run_outcome_badges() {
    assert_eq!(ResponseBadge::new(None, None, &[], None), None);
    let status = ResponseBadge::new(Some(404), None, &[], None).unwrap();
    assert_eq!(status, ResponseBadge::Status { status: 404 });
    assert_eq!(status.label(), "404 Not Found");
    let refused =
        ResponseBadge::new(None, Some("connection"), &[], Some("127.0.0.1:18880")).unwrap();
    assert_eq!(
        refused,
        ResponseBadge::Failed {
            title: "Couldn’t Connect to 127.0.0.1:18880".into()
        }
    );
    assert_eq!(refused.label(), "Failed");
    assert_eq!(refused.detail(), "Couldn’t Connect to 127.0.0.1:18880");
    let cancelled = ResponseBadge::new(Some(200), Some("cancelled"), &[], None).unwrap();
    assert_eq!(cancelled, ResponseBadge::Cancelled);
    assert_eq!(cancelled.label(), "Cancelled");
}

#[test]
fn failure_hosts_preserve_display_spelling_and_explicit_default_ports() {
    for (url, host) in [
        ("https://EXAMPLE.COM:443/a", "EXAMPLE.COM:443"),
        ("https://user:pass@HOST:443?x=y", "HOST:443"),
        ("https://[::1]:443/a", "[::1]:443"),
        ("https://café.example/a", "café.example"),
        ("https://xn--caf-dma.example/a", "café.example"),
        ("//EXAMPLE.COM/a", "EXAMPLE.COM"),
    ] {
        assert_eq!(failure_host(Some(url)).as_deref(), Some(host));
    }
}
