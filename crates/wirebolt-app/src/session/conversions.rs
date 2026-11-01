//! Explicit conversion between editable application records and validated storage records.
use super::persistence::WorkspaceError;
use crate::domain::{
    DomainApiKeyPlacement, DomainAuthentication, DomainBody, DomainEnvironment, DomainField,
    DomainGroup, DomainMultipartKind, DomainMultipartPart, DomainOAuthConfiguration,
    DomainOAuthGrant, DomainProxy, DomainProxyCredentials, DomainProxyRoute, DomainRequestDraft,
    DomainTransport, DomainValueSource, DomainVariable,
};
use wirebolt_core::{
    ApiKeyPlacement, Environment, EnvironmentVariable, Group, ManualProxy, MultipartPart,
    MultipartPartKind, Oauth2Configuration, Oauth2Grant, ProxyCredentials, ProxyEndpoint,
    ProxyMode, ProxyRoute, Request, RequestAuthentication, RequestBody, RequestHeader,
    RequestValueField, SecretName, TransportSettings, ValueSource,
};
fn invalid() -> WorkspaceError {
    WorkspaceError::operation("workspace snapshot could not be projected")
}
fn secret(value: String) -> Result<SecretName, WorkspaceError> {
    SecretName::new(value).map_err(|_| invalid())
}
pub fn value_to_core(value: DomainValueSource) -> Result<ValueSource, WorkspaceError> {
    Ok(match value {
        DomainValueSource::Literal { value } => ValueSource::Literal(value),
        DomainValueSource::Secret { secret: name } => ValueSource::Secret {
            secret: secret(name)?,
        },
    })
}
pub fn value_from_core(value: ValueSource) -> DomainValueSource {
    match value {
        ValueSource::Literal(value) => DomainValueSource::Literal { value },
        ValueSource::Secret { secret } => DomainValueSource::Secret {
            secret: secret.as_str().into(),
        },
    }
}
pub fn field_to_core(v: DomainField) -> Result<RequestValueField, WorkspaceError> {
    Ok(RequestValueField {
        id: v.id,
        name: v.name,
        value: value_to_core(v.value)?,
        enabled: v.enabled,
        sensitive: v.sensitive,
    })
}
pub fn field_from_core(v: RequestValueField) -> DomainField {
    DomainField {
        id: v.id,
        name: v.name,
        value: value_from_core(v.value),
        enabled: v.enabled,
        sensitive: v.sensitive,
    }
}
pub fn header_to_core(v: DomainField) -> Result<RequestHeader, WorkspaceError> {
    Ok(RequestHeader {
        id: v.id,
        name: v.name,
        value: value_to_core(v.value)?,
        enabled: v.enabled,
        sensitive: v.sensitive,
    })
}
pub fn header_from_core(v: RequestHeader) -> DomainField {
    DomainField {
        id: v.id,
        name: v.name,
        value: value_from_core(v.value),
        enabled: v.enabled,
        sensitive: v.sensitive,
    }
}
pub fn variable_to_core(v: DomainVariable) -> Result<EnvironmentVariable, WorkspaceError> {
    Ok(EnvironmentVariable {
        id: v.id,
        key: v.key,
        value: value_to_core(v.value)?,
        enabled: v.enabled,
        order: v.order,
    })
}
pub fn variable_from_core(v: EnvironmentVariable) -> DomainVariable {
    DomainVariable {
        id: v.id,
        key: v.key,
        value: value_from_core(v.value),
        enabled: v.enabled,
        order: v.order,
    }
}
pub fn oauth_to_core(v: DomainOAuthConfiguration) -> Result<Oauth2Configuration, WorkspaceError> {
    Ok(Oauth2Configuration {
        grant: match v.grant {
            DomainOAuthGrant::AuthorizationCodePkce => Oauth2Grant::AuthorizationCodePkce,
            DomainOAuthGrant::ClientCredentials => Oauth2Grant::ClientCredentials,
        },
        authorization_url: v.authorization_url,
        token_url: v.token_url,
        client_id: v.client_id,
        client_secret_reference: secret(v.client_secret_reference)?,
        scopes: v.scopes,
        audience: v.audience,
        redirect_uri: v.redirect_uri,
        access_token_reference: secret(v.access_token_reference)?,
    })
}
pub fn oauth_from_core(v: Oauth2Configuration) -> DomainOAuthConfiguration {
    DomainOAuthConfiguration {
        grant: match v.grant {
            Oauth2Grant::AuthorizationCodePkce => DomainOAuthGrant::AuthorizationCodePkce,
            Oauth2Grant::ClientCredentials => DomainOAuthGrant::ClientCredentials,
        },
        authorization_url: v.authorization_url,
        token_url: v.token_url,
        client_id: v.client_id,
        client_secret_reference: v.client_secret_reference.as_str().into(),
        scopes: v.scopes,
        audience: v.audience,
        redirect_uri: v.redirect_uri,
        access_token_reference: v.access_token_reference.as_str().into(),
    }
}
pub fn part_to_core(v: DomainMultipartPart) -> Result<MultipartPart, WorkspaceError> {
    Ok(MultipartPart {
        id: v.id,
        name: v.name,
        kind: match v.kind {
            DomainMultipartKind::Text => MultipartPartKind::Text,
            DomainMultipartKind::Binary => MultipartPartKind::Binary,
            DomainMultipartKind::File => MultipartPartKind::File,
        },
        value: value_to_core(v.value)?,
        file_path: v.file_path,
        file_name: v.file_name,
        content_type: v.content_type,
        enabled: v.enabled,
    })
}
pub fn part_from_core(v: MultipartPart) -> DomainMultipartPart {
    DomainMultipartPart {
        id: v.id,
        name: v.name,
        kind: match v.kind {
            MultipartPartKind::Text => DomainMultipartKind::Text,
            MultipartPartKind::Binary => DomainMultipartKind::Binary,
            MultipartPartKind::File => DomainMultipartKind::File,
        },
        value: value_from_core(v.value),
        file_path: v.file_path,
        file_name: v.file_name,
        content_type: v.content_type,
        enabled: v.enabled,
    }
}
pub fn transport_to_core(v: DomainTransport) -> Result<TransportSettings, WorkspaceError> {
    Ok(TransportSettings {
        validate_tls: v.validate_tls,
        follow_redirects: v.follow_redirects,
        maximum_redirects: u8::try_from(v.maximum_redirects).map_err(|_| invalid())?,
        total_timeout_ms: v.total_timeout_ms,
        read_timeout_ms: v.read_timeout_ms,
        client_certificate_reference: v.client_certificate_reference.map(secret).transpose()?,
        custom_ca_path: v.custom_ca_path,
    })
}
pub fn transport_from_core(v: TransportSettings) -> DomainTransport {
    DomainTransport {
        validate_tls: v.validate_tls,
        follow_redirects: v.follow_redirects,
        maximum_redirects: i64::from(v.maximum_redirects),
        total_timeout_ms: v.total_timeout_ms,
        read_timeout_ms: v.read_timeout_ms,
        client_certificate_reference: v.client_certificate_reference.map(|v| v.as_str().into()),
        custom_ca_path: v.custom_ca_path,
    }
}
pub fn auth_to_core(v: DomainAuthentication) -> Result<RequestAuthentication, WorkspaceError> {
    Ok(match v {
        DomainAuthentication::None => RequestAuthentication::None,
        DomainAuthentication::Basic { username, password } => RequestAuthentication::Basic {
            username: value_to_core(username)?,
            password: value_to_core(password)?,
        },
        DomainAuthentication::Bearer { token } => RequestAuthentication::Bearer {
            token: value_to_core(token)?,
        },
        DomainAuthentication::ApiKey {
            placement,
            name,
            value,
        } => RequestAuthentication::ApiKey {
            placement: match placement {
                DomainApiKeyPlacement::Header => ApiKeyPlacement::Header,
                DomainApiKeyPlacement::Query => ApiKeyPlacement::Query,
            },
            name,
            value: value_to_core(value)?,
        },
        DomainAuthentication::Oauth2 { configuration } => RequestAuthentication::Oauth2 {
            configuration: oauth_to_core(configuration)?,
        },
    })
}
pub fn auth_from_core(v: RequestAuthentication) -> DomainAuthentication {
    match v {
        RequestAuthentication::None => DomainAuthentication::None,
        RequestAuthentication::Basic { username, password } => DomainAuthentication::Basic {
            username: value_from_core(username),
            password: value_from_core(password),
        },
        RequestAuthentication::Bearer { token } => DomainAuthentication::Bearer {
            token: value_from_core(token),
        },
        RequestAuthentication::ApiKey {
            placement,
            name,
            value,
        } => DomainAuthentication::ApiKey {
            placement: match placement {
                ApiKeyPlacement::Header => DomainApiKeyPlacement::Header,
                ApiKeyPlacement::Query => DomainApiKeyPlacement::Query,
            },
            name,
            value: value_from_core(value),
        },
        RequestAuthentication::Oauth2 { configuration } => DomainAuthentication::Oauth2 {
            configuration: oauth_from_core(configuration),
        },
    }
}
pub fn body_to_core(v: DomainBody) -> Result<RequestBody, WorkspaceError> {
    Ok(match v {
        DomainBody::Empty => RequestBody::Empty,
        DomainBody::Text {
            content_type,
            value,
        } => RequestBody::Text {
            content_type,
            value,
        },
        DomainBody::Json { value } => RequestBody::Json { value },
        DomainBody::Xml { value } => RequestBody::Xml { value },
        DomainBody::Html { value } => RequestBody::Html { value },
        DomainBody::Raw {
            content_type,
            value,
        } => RequestBody::Raw {
            content_type,
            value,
        },
        DomainBody::FormUrlEncoded { fields } => RequestBody::FormUrlEncoded {
            fields: fields
                .into_iter()
                .map(field_to_core)
                .collect::<Result<_, _>>()?,
        },
        DomainBody::Multipart { parts } => RequestBody::Multipart {
            parts: parts
                .into_iter()
                .map(part_to_core)
                .collect::<Result<_, _>>()?,
        },
        DomainBody::File { path, content_type } => RequestBody::File { path, content_type },
    })
}
pub fn body_from_core(v: RequestBody) -> DomainBody {
    match v {
        RequestBody::Empty => DomainBody::Empty,
        RequestBody::Text {
            content_type,
            value,
        } => DomainBody::Text {
            content_type,
            value,
        },
        RequestBody::Json { value } => DomainBody::Json { value },
        RequestBody::Xml { value } => DomainBody::Xml { value },
        RequestBody::Html { value } => DomainBody::Html { value },
        RequestBody::Raw {
            content_type,
            value,
        } => DomainBody::Raw {
            content_type,
            value,
        },
        RequestBody::FormUrlEncoded { fields } => DomainBody::FormUrlEncoded {
            fields: fields.into_iter().map(field_from_core).collect(),
        },
        RequestBody::Multipart { parts } => DomainBody::Multipart {
            parts: parts.into_iter().map(part_from_core).collect(),
        },
        RequestBody::File { path, content_type } => DomainBody::File { path, content_type },
    }
}
pub fn proxy_to_core(v: DomainProxy) -> Result<ProxyMode, WorkspaceError> {
    Ok(match v {
        DomainProxy::System => ProxyMode::System,
        DomainProxy::Direct => ProxyMode::Direct,
        DomainProxy::Manual { routes } => ProxyMode::Manual(
            ManualProxy::new(
                routes
                    .into_iter()
                    .map(|r| {
                        let mut route = ProxyRoute::new(
                            r.destination,
                            ProxyEndpoint::new(&r.endpoint).map_err(|_| invalid())?,
                        );
                        if let Some(c) = r.credentials {
                            route = route.with_credentials(ProxyCredentials::new(
                                secret(c.username)?,
                                secret(c.password)?,
                            ));
                        }
                        Ok(route)
                    })
                    .collect::<Result<_, WorkspaceError>>()?,
            )
            .map_err(|_| invalid())?,
        ),
    })
}
pub fn proxy_from_core(v: ProxyMode) -> DomainProxy {
    match v {
        ProxyMode::System => DomainProxy::System,
        ProxyMode::Direct => DomainProxy::Direct,
        ProxyMode::Manual(manual) => DomainProxy::Manual {
            routes: manual
                .routes()
                .iter()
                .map(|r| DomainProxyRoute {
                    destination: r.destination(),
                    endpoint: r.endpoint().normalized(),
                    credentials: r.credentials().map(|c| DomainProxyCredentials {
                        username: c.username().as_str().into(),
                        password: c.password().as_str().into(),
                    }),
                })
                .collect(),
        },
    }
}
pub fn request_from_core(r: Request) -> DomainRequestDraft {
    DomainRequestDraft {
        id: r.id.to_string(),
        name: r.name,
        method: crate::domain::request::normalize_method(&r.method).unwrap_or_else(|| "GET".into()),
        url: r.url,
        web_socket: r.web_socket,
        note: r.note,
        query: r.query.into_iter().map(field_from_core).collect(),
        headers: r.headers.into_iter().map(header_from_core).collect(),
        authentication: auth_from_core(r.authentication),
        body: body_from_core(r.body),
        proxy: r.proxy_override.map(proxy_from_core),
        transport: transport_from_core(r.transport),
        inherits_workspace_transport: r.inherits_workspace_transport,
    }
}
pub fn environment_from_core(e: Environment) -> DomainEnvironment {
    DomainEnvironment {
        id: e.id.to_string(),
        name: e.name,
        variables: e.variables.into_iter().map(variable_from_core).collect(),
    }
}
pub fn group_from_core(g: Group) -> DomainGroup {
    DomainGroup {
        id: g.id.to_string(),
        name: g.name,
        parent_id: g.parent_id.map(|id| id.to_string()),
        order: g.order,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field() -> DomainField {
        DomainField {
            id: "stable-row".into(),
            name: "token".into(),
            value: DomainValueSource::Secret {
                secret: "api.token".into(),
            },
            enabled: false,
            sensitive: true,
        }
    }
    #[test]
    fn storage_conversions_keep_ids_flags_secret_references_and_all_body_variants() {
        let f = field();
        assert_eq!(field_from_core(field_to_core(f.clone()).unwrap()), f);
        assert_eq!(header_from_core(header_to_core(f.clone()).unwrap()), f);
        for body in [
            DomainBody::Empty,
            DomainBody::Text {
                content_type: None,
                value: "hi".into(),
            },
            DomainBody::Json { value: "{}".into() },
            DomainBody::Xml {
                value: "<x/>".into(),
            },
            DomainBody::Html {
                value: "<p>hi</p>".into(),
            },
            DomainBody::Raw {
                content_type: Some("text/plain".into()),
                value: "raw".into(),
            },
            DomainBody::FormUrlEncoded { fields: vec![f] },
            DomainBody::File {
                path: "/tmp/body".into(),
                content_type: None,
            },
        ] {
            assert_eq!(body_from_core(body_to_core(body.clone()).unwrap()), body);
        }
        for kind in [
            DomainMultipartKind::Text,
            DomainMultipartKind::Binary,
            DomainMultipartKind::File,
        ] {
            let body = DomainBody::Multipart {
                parts: vec![DomainMultipartPart {
                    id: "part".into(),
                    name: "data".into(),
                    kind,
                    value: DomainValueSource::literal("aGVsbG8="),
                    file_path: Some("/tmp/body".into()),
                    file_name: Some("body.txt".into()),
                    content_type: Some("text/plain".into()),
                    enabled: false,
                }],
            };
            assert_eq!(body_from_core(body_to_core(body.clone()).unwrap()), body);
        }
    }
    #[test]
    fn authentication_and_transport_projection_are_lossless_and_validate_references() {
        let value = DomainValueSource::Secret {
            secret: "api.token".into(),
        };
        for grant in [
            DomainOAuthGrant::AuthorizationCodePkce,
            DomainOAuthGrant::ClientCredentials,
        ] {
            let auth = DomainAuthentication::Oauth2 {
                configuration: DomainOAuthConfiguration {
                    grant,
                    client_secret_reference: "oauth.client".into(),
                    access_token_reference: "oauth.token".into(),
                    authorization_url: "https://example.com/auth".into(),
                    token_url: "https://example.com/token".into(),
                    client_id: "client".into(),
                    scopes: "read".into(),
                    audience: "api".into(),
                    redirect_uri: "wirebolt://oauth/callback".into(),
                },
            };
            assert_eq!(auth_from_core(auth_to_core(auth.clone()).unwrap()), auth);
        }
        for auth in [
            DomainAuthentication::None,
            DomainAuthentication::Basic {
                username: value.clone(),
                password: value.clone(),
            },
            DomainAuthentication::Bearer {
                token: value.clone(),
            },
            DomainAuthentication::ApiKey {
                placement: DomainApiKeyPlacement::Header,
                name: "X-Key".into(),
                value: value.clone(),
            },
            DomainAuthentication::ApiKey {
                placement: DomainApiKeyPlacement::Query,
                name: "key".into(),
                value,
            },
        ] {
            assert_eq!(auth_from_core(auth_to_core(auth.clone()).unwrap()), auth);
        }
        let transport = DomainTransport {
            client_certificate_reference: Some("tls.client".into()),
            custom_ca_path: Some("/tmp/ca.pem".into()),
            ..Default::default()
        };
        assert_eq!(
            transport_from_core(transport_to_core(transport.clone()).unwrap()),
            transport
        );
        assert!(
            transport_to_core(DomainTransport {
                maximum_redirects: -1,
                ..transport
            })
            .is_err()
        );
        assert!(
            value_to_core(DomainValueSource::Secret {
                secret: String::new()
            })
            .is_err()
        );
    }
    #[test]
    fn proxy_projection_keeps_credential_identity_and_rejects_invalid_routes() {
        for proxy in [
            DomainProxy::Direct,
            DomainProxy::System,
            DomainProxy::Manual {
                routes: vec![DomainProxyRoute {
                    destination: wirebolt_core::ProxyDestination::All,
                    endpoint: "http://localhost:8080/".into(),
                    credentials: Some(DomainProxyCredentials {
                        username: "proxy.user".into(),
                        password: "proxy.password".into(),
                    }),
                }],
            },
        ] {
            assert_eq!(
                proxy_from_core(proxy_to_core(proxy.clone()).unwrap()),
                proxy
            );
        }
        assert!(proxy_to_core(DomainProxy::Manual { routes: vec![] }).is_err());
        assert!(
            proxy_to_core(DomainProxy::Manual {
                routes: vec![DomainProxyRoute {
                    destination: wirebolt_core::ProxyDestination::All,
                    endpoint: "not a proxy endpoint".into(),
                    credentials: None
                }]
            })
            .is_err()
        );
    }
}
