use wirebolt_app::response::FailureCategory;
mod support;
use support::*;
use wirebolt_app::domain::{models::*, proxy::*};
#[test]
fn precedence_replaces_the_whole_policy() {
    for app in [manual(), DomainProxy::System, DomainProxy::Direct] {
        assert_eq!(
            resolve(None, None, app.clone()),
            (app.clone(), DomainProxySource::AppDefault)
        );
        for workspace in [manual(), DomainProxy::System, DomainProxy::Direct] {
            assert_eq!(
                resolve(None, Some(workspace.clone()), app.clone()),
                (workspace.clone(), DomainProxySource::Workspace)
            );
            for request in [manual(), DomainProxy::System, DomainProxy::Direct] {
                assert_eq!(
                    resolve(Some(request.clone()), Some(workspace.clone()), app.clone()),
                    (request, DomainProxySource::Request)
                );
            }
        }
    }
}
#[test]
fn manual_editor_rejects_bad_ports_overlap_embedded_credentials_and_socks4_auth() {
    let mut form = form_input(Some(manual()));
    for port in ["70000", "abc", "0", "-1"] {
        form.routes[0].port = port.into();
        assert!(form_document(&form).is_err());
    }
    form.routes[0].port = "8080".into();
    form.routes[0].host = "user:password@localhost".into();
    assert!(form_document(&form).is_err());
    form.routes[0].host = "localhost".into();
    form.routes.push(form.routes[0].clone());
    assert!(form_document(&form).is_err());
    form.routes.truncate(1);
    form.routes[0].scheme = "socks4".into();
    form.routes[0].authenticated = true;
    form.routes[0].username = "user".into();
    form.routes[0].password = "sensitive-password".into();
    assert!(form_document(&form).is_err());
    form.routes[0].scheme = "socks5h".into();
    assert!(form_document(&form).unwrap().is_some());
}
#[test]
fn credentials_serialize_references_and_preserve_unchanged_names() {
    let mut form = form_input(Some(manual()));
    form.routes[0].authenticated = true;
    form.routes[0].username = "sensitive-user".into();
    form.routes[0].password = "sensitive-password".into();
    let document = form_document(&form).unwrap().unwrap();
    let encoded = serde_json::to_string(&document).unwrap();
    assert!(!encoded.contains("sensitive-user"));
    assert!(!encoded.contains("sensitive-password"));
    assert_eq!(form_secrets(&form).len(), 2);
    let reopened = form_input(Some(document.clone()));
    assert_eq!(form_document(&reopened).unwrap(), Some(document));
    assert!(form_secrets(&reopened).is_empty());
}
#[test]
fn summary_maps_websocket_schemes_and_bypasses_unmatched_routes() {
    let configuration = DomainProxy::Manual {
        routes: vec![DomainProxyRoute {
            destination: "https".into(),
            endpoint: "socks5h://localhost:1080".into(),
            credentials: None,
        }],
    };
    assert_eq!(
        summary(&configuration, Some("wss://example.com")),
        "SOCKS5H · localhost:1080"
    );
    assert_eq!(
        summary(&configuration, Some("http://example.com")),
        "Direct · No matching proxy route"
    );
    assert_eq!(summary(&DomainProxy::Direct, None), "Direct · No proxy");
    assert_eq!(summary(&DomainProxy::System, None), "System proxy");
}
#[test]
fn isolated_probe_sends_head_only_and_handles_407() {
    let mut probe = ConnectionProbe::default();
    let d = probe
        .start(manual(), "http://example.invalid/health".into())
        .unwrap();
    assert_eq!(d.method, "HEAD");
    assert_eq!(d.proxy, Some(manual()));
    assert!(d.headers.is_empty());
    assert_eq!(d.body, DomainBody::Empty);
    assert_eq!(d.authentication, DomainAuthentication::None);
    assert_eq!(d.transport.total_timeout_ms, 5000);
    assert!(!d.transport.follow_redirects);
    let id = probe.state.run_id.clone().unwrap();
    probe.complete(&id, Some(200), None);
    assert!(probe.state.succeeded);
    let _ = probe.start(manual(), "http://example.invalid/health".into());
    let id = probe.state.run_id.clone().unwrap();
    probe.complete(&id, Some(407), None);
    assert!(!probe.state.succeeded);
    assert!(probe.state.message.as_ref().unwrap().contains("407"));
}
#[test]
fn cancelling_a_queued_probe_invalidates_old_completions() {
    let mut probe = ConnectionProbe::default();
    let _ = probe.start(DomainProxy::Direct, "https://example.invalid".into());
    let id = probe.state.run_id.clone().unwrap();
    probe.cancel();
    assert!(!probe.active(&id));
    assert!(!probe.state.is_running);
    assert_eq!(probe.state.message.as_deref(), Some("Test cancelled."));
    probe.complete(&id, Some(200), None);
    assert!(!probe.state.succeeded);
    for url in [
        "file:///etc/passwd",
        "{{host}}",
        "https://user:password@example.invalid",
        "invalid",
    ] {
        assert!(probe.start(DomainProxy::Direct, url.into()).is_none());
        assert!(!probe.state.is_running);
    }
    probe.reset();
    assert!(probe.state.message.is_none());
}
#[test]
fn validation_rejects_paths_overlaps_empty_policies_and_unsupported_protocols() {
    assert!(validate(&DomainProxy::Manual { routes: vec![] }).is_err());
    let route = |destination: &str, endpoint: &str| DomainProxyRoute {
        destination: destination.into(),
        endpoint: endpoint.into(),
        credentials: None,
    };
    for endpoint in [
        "ftp://localhost:8080",
        "http://localhost:0",
        "http://localhost:70000",
        "http://localhost/path",
        "http://u:p@localhost:8080",
        "http://localhost/?q=1",
        "http://localhost/#x",
    ] {
        assert!(
            validate(&DomainProxy::Manual {
                routes: vec![route("all", endpoint)]
            })
            .is_err(),
            "{endpoint}"
        );
    }
    assert!(
        validate(&DomainProxy::Manual {
            routes: vec![
                route("all", "http://localhost:8080"),
                route("http", "http://localhost:8081")
            ]
        })
        .is_err()
    );
    assert!(
        validate(&DomainProxy::Manual {
            routes: vec![
                route("http", "http://localhost:8080"),
                route("https", "socks5h://localhost:1080")
            ]
        })
        .is_ok()
    );
}
#[test]
fn probe_failure_messages_keep_categories_and_timeouts() {
    for (category, expected) in [
        (
            FailureCategory::Timeout,
            "Connection timed out after 5 seconds.",
        ),
        (
            FailureCategory::Tls,
            "TLS verification failed. Check the certificate and trust settings.",
        ),
        (
            FailureCategory::Proxy,
            "Proxy configuration or credentials could not be loaded.",
        ),
        (
            FailureCategory::Secrets,
            "Proxy configuration or credentials could not be loaded.",
        ),
        (FailureCategory::Cancelled, "Test cancelled."),
        (
            FailureCategory::InternalError,
            "Connection failed. Check the proxy address, port and destination.",
        ),
    ] {
        assert_eq!(test_result(None, Some(category)), (false, expected.into()));
    }
    assert_eq!(
        test_result(None, None),
        (false, "No HTTP response received.".into())
    );
}
#[test]
fn composer_indicators_show_overrides_and_invalid_policies() {
    for app in [DomainProxy::System, DomainProxy::Direct, manual()] {
        assert!(!needs_indicator(&app, DomainProxySource::AppDefault));
        assert!(needs_indicator(
            &DomainProxy::System,
            DomainProxySource::Workspace
        ));
        assert!(needs_indicator(
            &DomainProxy::Direct,
            DomainProxySource::Request
        ));
    }
    assert!(needs_indicator(
        &DomainProxy::Manual { routes: vec![] },
        DomainProxySource::AppDefault
    ));
}
#[test]
fn proxy_routes_match_unresolved_url_authorities_and_reject_normalized_paths() {
    let configuration = DomainProxy::Manual {
        routes: vec![
            DomainProxyRoute {
                destination: "http".into(),
                endpoint: "http://plain:8080".into(),
                credentials: None,
            },
            DomainProxyRoute {
                destination: "https".into(),
                endpoint: "socks5h://secure:1080".into(),
                credentials: None,
            },
        ],
    };
    assert_eq!(
        summary(&configuration, Some("https://{{host}}/path")),
        "SOCKS5H · secure:1080"
    );
    for endpoint in [
        "HTTP://localhost:8080",
        "http://localhost/./",
        "http://localhost/../",
        "http://local\nhost:8080",
    ] {
        assert!(
            validate(&DomainProxy::Manual {
                routes: vec![DomainProxyRoute {
                    destination: "all".into(),
                    endpoint: endpoint.into(),
                    credentials: None
                }]
            })
            .is_err()
        );
    }
}
