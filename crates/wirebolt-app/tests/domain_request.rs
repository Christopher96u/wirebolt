use wirebolt_app::domain::DomainAuthenticationKind;
mod support;
use support::*;
use wirebolt_app::domain::{chrome::*, models::*, request::*, url::*};

#[test]
fn imported_url_parameters_are_separated() {
    let d = edit_url(request(""), "https://example.com/?a=1&a=2");
    assert_eq!(d.url, "https://example.com/");
    assert_eq!(
        d.query.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["a", "a"]
    );
    assert_eq!(display_url(&d), "https://example.com/?a=1&a=2");
}
#[test]
fn variable_query_values_remain_readable() {
    let mut d = request("{{base_url}}/products");
    d.query = vec![field("category", "{{category}}"), field("q", "a b&c")];
    assert_eq!(
        display_url(&d),
        "{{base_url}}/products?category={{category}}&q=a%20b%26c"
    );
    let original = d.query.clone();
    d = edit_url(d.clone(), &display_url(&d));
    assert_eq!(d.query, original);
    d = request("https://example.com");
    d.query = vec![field("json", "{a}")];
    assert_eq!(display_url(&d), "https://example.com?json=%7Ba%7D");
}
#[test]
fn incremental_typing_never_reencodes_keys() {
    let text = "http://127.0.0.1:48173/json?limit=10&search=wirebolt";
    let mut d = request("");
    let mut typed = String::new();
    for c in text.chars() {
        typed.push(c);
        d = edit_url(d, &typed);
    }
    assert_eq!(d.url, "http://127.0.0.1:48173/json");
    assert_eq!(
        d.query.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["limit", "search"]
    );
    assert_eq!(
        d.query
            .iter()
            .map(|f| f.value.editable())
            .collect::<Vec<_>>(),
        ["10", "wirebolt"]
    );
    assert_eq!(display_url(&d), text);
}
#[test]
fn duplicate_keys_disabled_fields_and_unicode_keep_identity() {
    let mut d = request("");
    let mut off = field("off", "hidden");
    off.enabled = false;
    d.query.push(off);
    d = edit_url(d, "https://example.com/p?a=1&a=2&name=caf%C3%A9#anchor");
    let fields = d.query.clone();
    d = edit_url(d.clone(), &display_url(&d));
    assert_eq!(d.query, fields);
    assert_eq!(d.query[2].value.editable(), "café");
    assert_eq!(d.url, "https://example.com/p#anchor");
    d = edit_url(d, "https://example.com/p");
    assert_eq!(d.query.len(), 1);
    assert_eq!(d.query[0].name, "off");
}
fn protected() -> Vec<DomainField> {
    vec![
        DomainField {
            id: "secret".into(),
            name: "X-Token".into(),
            value: secret("fixture.token"),
            enabled: true,
            sensitive: true,
        },
        DomainField {
            id: "private".into(),
            name: "X-Private".into(),
            value: DomainValueSource::literal("fixture"),
            enabled: true,
            sensitive: true,
        },
    ]
}
#[test]
fn bulk_unrelated_edits_preserve_credentials_and_redaction() {
    let original = protected();
    let result = parse_bulk(
        "X-Token: fixture.token\nX-Private: fixture\nX-New: added",
        original.clone(),
    );
    assert_eq!(result[..2], original);
    assert_eq!(result[2].value.editable(), "added");
}
#[test]
fn bulk_toggle_rename_and_reference_edits_keep_metadata() {
    let original = protected();
    let r = parse_bulk("# X-Renamed: fixture.token", vec![original[0].clone()]);
    assert_eq!(r[0].id, "secret");
    assert_eq!(r[0].value, secret("fixture.token"));
    assert!(r[0].sensitive);
    assert!(!r[0].enabled);
    let changed = parse_bulk("X-Token: fixture.replacement", vec![original[0].clone()]);
    assert_eq!(changed[0].value, secret("fixture.replacement"));
    assert!(changed[0].sensitive);
}
#[test]
fn bulk_duplicate_reordering_keeps_protection() {
    let mut a = field("X", "one");
    a.id = "a".into();
    let mut b = field("X", "two");
    b.id = "b".into();
    b.value = secret("two");
    b.sensitive = true;
    assert_eq!(
        parse_bulk("X: two\nX: one", vec![a.clone(), b.clone()]),
        vec![b, a]
    );
}
#[test]
fn each_auth_type_has_unique_keychain_names() {
    for kind in [
        DomainAuthenticationKind::Basic,
        DomainAuthenticationKind::Bearer,
        DomainAuthenticationKind::ApiKey,
        DomainAuthenticationKind::Oauth2,
    ] {
        let a = new_auth(kind);
        let b = new_auth(kind);
        assert_eq!(auth_kind(&a), kind);
        let first = auth_references(&a);
        assert!(!first.is_empty());
        for name in first {
            assert!(!auth_references(&b).contains(&name));
        }
    }
}
#[test]
fn authentication_defaults_never_use_shared_names() {
    for kind in [
        DomainAuthenticationKind::ApiKey,
        DomainAuthenticationKind::Oauth2,
    ] {
        for name in auth_references(&new_auth(kind)) {
            assert!(!legacy_reference(&name));
        }
    }
    if let DomainAuthentication::Oauth2 { configuration: c } =
        new_auth(DomainAuthenticationKind::Oauth2)
    {
        assert_ne!(c.client_secret_reference, c.access_token_reference);
    } else {
        panic!("OAuth configuration")
    }
}
#[test]
fn credential_references_are_valid_secret_names() {
    for kind in [
        DomainAuthenticationKind::None,
        DomainAuthenticationKind::Basic,
        DomainAuthenticationKind::Bearer,
        DomainAuthenticationKind::ApiKey,
        DomainAuthenticationKind::Oauth2,
    ] {
        for name in auth_references(&new_auth(kind)) {
            assert!((1..=128).contains(&name.len()));
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            );
            assert!(owned_reference(&name));
        }
    }
}
#[test]
fn api_key_serializes_only_its_reference() {
    let auth = new_auth(DomainAuthenticationKind::ApiKey);
    let encoded = serde_json::to_string(&auth).unwrap();
    assert!(encoded.contains("\"kind\":\"api_key\""));
    assert!(encoded.contains(&auth_references(&auth)[0]));
}
#[test]
fn environment_secret_names_are_unique_and_owned() {
    let a = credential_reference("stage/name", "env");
    let b = credential_reference("stage/name", "env");
    assert_ne!(a, b);
    assert!(a.ends_with(".stage-name"));
    assert!(owned_reference(&a));
    assert!(a.len() <= 128);
}
#[test]
fn owned_reference_recognizes_legacy_and_import_names() {
    for n in [
        "auth.12345678-1234-1234-1234-123456789abc.token",
        "request.12345678-1234-1234-1234-123456789ABC.password",
        "env.12345678-1234-1234-1234-123456789abc.dev",
        "import-ab-cd-ef-request-2-token",
    ] {
        assert!(owned_reference(n), "{n}");
    }
    for n in [
        "auth.api-key",
        "oauth.client-secret",
        "oauth.access-token",
        "proxy.user",
        "personal-token",
    ] {
        assert!(!owned_reference(n));
    }
}
#[test]
fn auth_renames_preserve_sharing_within_one_copy() {
    let auth = DomainAuthentication::Basic {
        username: secret("auth.api-key"),
        password: secret("auth.api-key"),
    };
    let (fresh, names) = own_auth(auth, true);
    assert_eq!(names.len(), 1);
    let refs = auth_references(&fresh);
    assert_eq!(refs[0], refs[1]);
    assert!(owned_reference(&refs[0]));
    let unchanged = DomainAuthentication::Bearer {
        token: secret("personal"),
    };
    assert_eq!(own_auth(unchanged.clone(), true).0, unchanged);
    assert_ne!(own_auth(unchanged.clone(), false).0, unchanged);
}
#[test]
fn method_normalization_accepts_custom_tokens() {
    for input in ["get", " GET ", "pRopFind", "QUERY", "X-42"] {
        assert!(normalize_method(input).is_some());
    }
    for input in ["", " \n", "BAD METHOD", "GET/", "💥"] {
        assert!(normalize_method(input).is_none());
    }
}
#[test]
fn window_names_preserve_exact_text() {
    assert_eq!(workspace_name("Payments"), "Payments");
    assert_eq!(workspace_name(""), "Wirebolt");
    assert_eq!(workspace_name("  "), "Wirebolt");
    assert_eq!(
        window_title("Payments", Some("Capture payment")),
        "Payments — Capture payment"
    );
    assert_eq!(window_title("Payments", None), "Payments");
    assert_eq!(window_title("Payments", Some("")), "Payments");
    assert_eq!(window_title("", Some("Refund")), "Wirebolt — Refund");
}
#[test]
fn tab_accessibility_includes_method_edits_and_preview() {
    assert_eq!(tab_description("PUT", false, false, false), "PUT");
    assert_eq!(
        tab_description("POST", false, true, true),
        "POST, Edited, Preview"
    );
    assert_eq!(
        tab_description("GET", true, false, true),
        "WebSocket, Preview"
    );
}
#[test]
fn active_fields_require_enabled_nonempty_name() {
    let mut fields = vec![
        field("limit", "20"),
        field("page", "2"),
        field("  ", "v"),
        field("X-New", ""),
    ];
    fields[1].enabled = false;
    assert_eq!(active_count(&fields), 2);
}
#[test]
fn every_authentication_kind_has_its_composer_title() {
    assert_eq!(
        [
            DomainAuthenticationKind::None,
            DomainAuthenticationKind::Basic,
            DomainAuthenticationKind::Bearer,
            DomainAuthenticationKind::ApiKey,
            DomainAuthenticationKind::Oauth2
        ]
        .map(auth_title),
        ["No auth", "Basic", "Bearer", "API key", "OAuth 2"]
    );
}
#[test]
fn timeout_units_and_fractional_seconds_match_native_labels() {
    for (ms, text) in [(0, "No limit"), (500, "500 ms"), (120_000, "2 min")] {
        assert_eq!(timeout_presentation(ms).text.as_deref(), Some(text));
    }
    for (ms, seconds) in [(30_000, 30.0), (1500, 1.5), (90_000, 90.0)] {
        let label = timeout_presentation(ms);
        assert_eq!(label.seconds, Some(seconds));
        assert_eq!(label.suffix, " s");
    }
}
#[test]
fn switching_authentication_keeps_existing_credentials_or_starts_empty() {
    let bearer = DomainAuthentication::Bearer {
        token: secret("request.token"),
    };
    assert_eq!(
        switch_auth(bearer.clone(), DomainAuthenticationKind::Bearer),
        bearer
    );
    let basic = switch_auth(bearer.clone(), DomainAuthenticationKind::Basic);
    assert_eq!(auth_kind(&basic), DomainAuthenticationKind::Basic);
    let DomainAuthentication::Basic { username, .. } = basic else {
        panic!("Basic")
    };
    assert_eq!(username, DomainValueSource::literal(""));
    assert_eq!(
        switch_auth(bearer, DomainAuthenticationKind::None),
        DomainAuthentication::None
    );
    assert_eq!(
        auth_kind(&switch_auth(
            DomainAuthentication::None,
            DomainAuthenticationKind::ApiKey
        )),
        DomainAuthenticationKind::ApiKey
    );
}
#[test]
fn run_inputs_project_one_immutable_transport_snapshot() {
    let mut d = request("https://example.invalid");
    d.proxy = Some(DomainProxy::Direct);
    d.transport.total_timeout_ms = 5000;
    let input = run_input(
        d.clone(),
        std::collections::HashMap::new(),
        Some(DomainProxy::System),
        Some(manual()),
    );
    d.url = "changed".into();
    assert_eq!(input.url, "https://example.invalid");
    assert_eq!(input.request_proxy, Some(DomainProxy::Direct));
    assert_eq!(input.total_timeout_ms, 5000);
    assert_eq!(input.workspace_proxy, Some(DomainProxy::System));
    assert_eq!(input.app_proxy, Some(manual()));
    assert!(input.max_response_bytes.is_none());
}
#[test]
fn legacy_environment_rows_sort_keys_and_keep_stable_ids() {
    let values = std::collections::BTreeMap::from([
        ("b".into(), DomainValueSource::literal("two")),
        ("a".into(), secret("fixture")),
    ]);
    let env = legacy_environment("legacy".into(), "Legacy".into(), values);
    assert_eq!(
        env.variables
            .iter()
            .map(|v| v.key.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(env.variables[0].id, "variable-0");
    assert_eq!(env.variables[1].order, 1);
    assert!(env.variables.iter().all(|v| v.enabled));
}
#[test]
fn url_bridge_results_include_display_and_preserve_field_identity() {
    let edited = edit_url_query(
        String::new(),
        vec![field("off", "hidden")],
        "https://example.invalid?a=1&a=2",
    );
    assert_eq!(edited.display_url, "https://example.invalid?a=1&a=2");
    let again = edit_url_query(
        edited.url.clone(),
        edited.query.clone(),
        &edited.display_url,
    );
    assert_eq!(again, edited);
    assert_eq!(
        separate_url_query_values("https://example.invalid?a=1".into(), vec![field("b", "2")])
            .display_url,
        "https://example.invalid?a=1&b=2"
    );
}
#[test]
fn transport_labels_use_workspace_settings_unless_request_overrides() {
    let workspace = DomainTransport {
        total_timeout_ms: 5000,
        ..DomainTransport::default()
    };
    let inherited = DomainTransport {
        total_timeout_ms: 30_000,
        ..DomainTransport::default()
    };
    assert_eq!(
        effective_transport(inherited, workspace.clone(), true).total_timeout_ms,
        5000
    );
    let overridden = DomainTransport {
        total_timeout_ms: 60_000,
        ..DomainTransport::default()
    };
    assert_eq!(
        effective_transport(overridden, workspace, false).total_timeout_ms,
        60_000
    );
}
#[test]
fn credential_role_sanitizing_uses_extended_grapheme_clusters() {
    assert_eq!(sanitized("e\u{301}👨‍👩‍👧"), "--");
    assert_eq!(sanitized(""), "value");
}

#[test]
fn credential_edits_copy_shared_names_and_reuse_private_names() {
    for name in [
        "auth.api-key",
        "oauth.client-secret",
        "oauth.access-token",
        "",
    ] {
        let source = secret(name);
        let first = editable_credential_reference(&source, "api-key");
        let second = editable_credential_reference(&source, "api-key");
        assert_ne!(first, name);
        assert_ne!(first, second);
        assert!(owned_reference(&first));
        assert!(!legacy_reference(&first));
    }
    assert_eq!(
        editable_credential_reference(&secret("chosen.name"), "token"),
        "chosen.name"
    );
    assert!(owned_reference(&editable_credential_reference(
        &DomainValueSource::literal("material"),
        "password"
    )));
}
