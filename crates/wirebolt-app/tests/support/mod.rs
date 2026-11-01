#![allow(dead_code)]
use wirebolt_app::domain::models::*;
pub fn field(name: &str, value: &str) -> DomainField {
    DomainField {
        id: name.into(),
        name: name.into(),
        value: DomainValueSource::literal(value),
        enabled: true,
        sensitive: false,
    }
}
pub fn secret(name: &str) -> DomainValueSource {
    DomainValueSource::Secret {
        secret: name.into(),
    }
}
pub fn request(url: &str) -> DomainRequestDraft {
    DomainRequestDraft {
        url: url.into(),
        ..DomainRequestDraft::default()
    }
}
pub fn group(id: &str, name: &str, parent: Option<&str>) -> DomainGroup {
    DomainGroup {
        id: id.into(),
        name: name.into(),
        parent_id: parent.map(str::to_owned),
        order: 0,
    }
}
pub fn location(id: &str, name: &str, url: &str, parent: Option<&str>) -> DomainLocation {
    DomainLocation {
        collection_id: "c".into(),
        group_id: parent.map(str::to_owned),
        order: 0,
        request: DomainRequestDraft {
            id: id.into(),
            name: name.into(),
            url: url.into(),
            ..DomainRequestDraft::default()
        },
    }
}
pub fn collection() -> DomainCollection {
    DomainCollection {
        id: "c".into(),
        name: "Collection".into(),
        order: 0,
        groups: vec![
            group("nested", "Nested", Some("folder")),
            group("folder", "Folder", None),
        ],
        requests: vec![
            location(
                "inside",
                "Café 東京",
                "https://example.invalid/needle",
                Some("nested"),
            ),
            location("root", "Root", "", None),
        ],
    }
}
pub fn manual() -> DomainProxy {
    DomainProxy::Manual {
        routes: vec![DomainProxyRoute {
            destination: "all".into(),
            endpoint: "http://localhost:8080".into(),
            credentials: None,
        }],
    }
}
pub fn environments() -> Vec<DomainEnvironment> {
    vec![
        environment(
            "global",
            "Global",
            &[
                (
                    "host",
                    DomainValueSource::literal("global.example.com"),
                    true,
                ),
                ("version", DomainValueSource::literal("v1"), true),
                ("disabled", DomainValueSource::literal("x"), false),
            ],
        ),
        environment(
            "staging",
            "Staging",
            &[
                (
                    "host",
                    DomainValueSource::literal("staging.example.com"),
                    true,
                ),
                ("token", secret("staging.token"), true),
            ],
        ),
        environment(
            "production",
            "Production",
            &[("host", DomainValueSource::literal("api.example.com"), true)],
        ),
    ]
}
pub fn environment(
    id: &str,
    name: &str,
    rows: &[(&str, DomainValueSource, bool)],
) -> DomainEnvironment {
    DomainEnvironment {
        id: id.into(),
        name: name.into(),
        variables: rows
            .iter()
            .zip(0_i64..)
            .map(|((key, value, enabled), order)| DomainVariable {
                id: order.to_string(),
                key: (*key).into(),
                value: value.clone(),
                enabled: *enabled,
                order,
            })
            .collect(),
    }
}
