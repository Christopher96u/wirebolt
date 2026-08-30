use std::collections::BTreeMap;

use wirebolt_core::{
    ApiKeyPlacement, DocumentId, Environment, Request, RequestAuthentication, RequestBody,
    RequestHeader, RequestIssue, RequestIssueKind, RequestPipeline, RequestValueField,
    ResolvedSecret, SecretName, SecretResolutionError, SecretResolutionErrorKind, SecretResolver,
    ValueSource,
};

#[derive(Default)]
struct FixtureSecrets(BTreeMap<String, String>);

impl SecretResolver for FixtureSecrets {
    fn resolve(&self, name: &SecretName) -> Result<ResolvedSecret, SecretResolutionError> {
        self.0
            .get(name.as_str())
            .cloned()
            .map(ResolvedSecret::new)
            .ok_or_else(|| SecretResolutionError::new(SecretResolutionErrorKind::NotFound))
    }
}

#[test]
fn prepares_basic_auth_and_urlencoded_form_without_disabled_fields() {
    let mut request = Request::new(
        id("create-user"),
        "Create user",
        "POST",
        "https://api.example.com/users",
    );
    request.authentication = RequestAuthentication::Basic {
        username: ValueSource::literal("Chris"),
        password: ValueSource::literal("wirebolt"),
    };
    request.query.push(RequestValueField {
        name: "ignored".to_owned(),
        value: ValueSource::literal("ignored"),
        enabled: false,
    });
    request.body = RequestBody::FormUrlEncoded {
        fields: vec![
            RequestValueField::enabled("display name", ValueSource::literal("Chris M.")),
            RequestValueField {
                name: "ignored".to_owned(),
                value: ValueSource::literal("ignored"),
                enabled: false,
            },
        ],
    };

    let prepared = RequestPipeline::new(None, &FixtureSecrets::default())
        .prepare(&request)
        .expect("valid form request");

    assert_eq!(prepared.uri().to_string(), "https://api.example.com/users");
    assert_eq!(
        prepared.headers()["authorization"],
        "Basic Q2hyaXM6d2lyZWJvbHQ="
    );
    assert_eq!(
        prepared.headers()["content-type"],
        "application/x-www-form-urlencoded"
    );
    assert_eq!(prepared.body(), b"display+name=Chris+M.");
}

#[test]
fn prepares_an_api_key_in_the_query() {
    let mut request = Request::new(
        id("health"),
        "Health",
        "GET",
        "https://api.example.com/health",
    );
    request.authentication = RequestAuthentication::ApiKey {
        placement: ApiKeyPlacement::Query,
        name: "api key".to_owned(),
        value: ValueSource::literal("secret value"),
    };

    let prepared = RequestPipeline::new(None, &FixtureSecrets::default())
        .prepare(&request)
        .expect("valid API-key request");

    assert_eq!(
        prepared.uri().to_string(),
        "https://api.example.com/health?api+key=secret+value"
    );
}

#[test]
fn missing_secret_reports_only_its_reference_and_field() {
    let secret = SecretName::new("MISSING_TOKEN").expect("secret name");
    let mut request = Request::new(
        id("private"),
        "Private",
        "GET",
        "https://api.example.com/private",
    );
    request.authentication = RequestAuthentication::Bearer {
        token: ValueSource::secret(secret),
    };

    let error = RequestPipeline::new(None, &FixtureSecrets::default())
        .prepare(&request)
        .expect_err("missing secret must fail");

    assert_eq!(
        error.issues,
        vec![RequestIssue {
            path: "authentication.token".to_owned(),
            kind: RequestIssueKind::MissingSecret,
            reference: Some("MISSING_TOKEN".to_owned()),
        }]
    );
    assert_eq!(error.to_string(), "request has 1 issue(s)");
}

#[test]
fn cyclic_variables_are_rejected_without_recursing_forever() {
    let environment = Environment::new(
        id("cycle"),
        "Cycle".to_owned(),
        BTreeMap::from([
            ("one".to_owned(), ValueSource::literal("{{two}}")),
            ("two".to_owned(), ValueSource::literal("{{one}}")),
        ]),
    );
    let request = Request::new(id("cycle"), "Cycle", "GET", "https://{{one}}.example.com");

    let error = RequestPipeline::new(Some(&environment), &FixtureSecrets::default())
        .prepare(&request)
        .expect_err("cycle must fail");

    assert_eq!(error.issues[0].path, "url");
    assert_eq!(error.issues[0].kind, RequestIssueKind::CyclicVariable);
    assert_eq!(error.issues[0].reference.as_deref(), Some("one"));
}

#[test]
fn deeply_nested_variables_are_rejected_before_exhausting_the_stack() {
    let mut variables = BTreeMap::new();
    for index in 0..70 {
        variables.insert(
            format!("v{index}"),
            ValueSource::literal(format!("{{{{v{}}}}}", index + 1)),
        );
    }
    variables.insert("v70".to_owned(), ValueSource::literal("example"));
    let environment = Environment::new(id("deep"), "Deep".to_owned(), variables);
    let request = Request::new(id("deep"), "Deep", "GET", "https://{{v0}}.com");

    let error = RequestPipeline::new(Some(&environment), &FixtureSecrets::default())
        .prepare(&request)
        .expect_err("deep expansion must be bounded");

    assert_eq!(error.issues[0].path, "url");
    assert_eq!(error.issues[0].kind, RequestIssueKind::TemplateTooDeep);
}

#[test]
fn invalid_json_is_scoped_to_the_body() {
    let mut request = Request::new(
        id("invalid-json"),
        "Invalid JSON",
        "POST",
        "https://api.example.com",
    );
    request.body = RequestBody::Json {
        value: "{not-json}".to_owned(),
    };

    let error = RequestPipeline::new(None, &FixtureSecrets::default())
        .prepare(&request)
        .expect_err("invalid JSON must fail");

    assert_eq!(
        error.issues,
        vec![RequestIssue {
            path: "body".to_owned(),
            kind: RequestIssueKind::InvalidJson,
            reference: None,
        }]
    );
}

#[test]
fn two_authorization_sources_are_rejected() {
    let mut request = Request::new(
        id("ambiguous-auth"),
        "Ambiguous auth",
        "GET",
        "https://api.example.com",
    );
    request.headers.push(RequestHeader::enabled(
        "Authorization",
        ValueSource::literal("Bearer existing"),
    ));
    request.authentication = RequestAuthentication::Bearer {
        token: ValueSource::literal("generated"),
    };

    let error = RequestPipeline::new(None, &FixtureSecrets::default())
        .prepare(&request)
        .expect_err("ambiguous auth must fail");

    assert_eq!(error.issues[0].path, "authentication");
    assert_eq!(error.issues[0].kind, RequestIssueKind::ConflictingHeader);
}

fn id(value: &str) -> DocumentId {
    DocumentId::new(value).expect("valid fixture ID")
}

#[test]
fn prepares_environment_query_bearer_and_json_as_one_request() {
    let mut variables = BTreeMap::new();
    variables.insert(
        "base_url".to_owned(),
        ValueSource::literal("https://api.example.com"),
    );
    variables.insert("user_id".to_owned(), ValueSource::literal("Chris M."));
    let environment = Environment::new(id("local"), "Local".to_owned(), variables);
    let token = SecretName::new("API_TOKEN").expect("secret name");
    let secrets = FixtureSecrets(BTreeMap::from([(
        token.as_str().to_owned(),
        "super-secret".to_owned(),
    )]));
    let mut request = Request::new(id("get-user"), "Get user", "POST", "{{base_url}}/users");
    request.query.push(RequestValueField::enabled(
        "display name",
        ValueSource::literal("{{user_id}}"),
    ));
    request.headers.push(RequestHeader::enabled(
        "accept",
        ValueSource::literal("application/json"),
    ));
    request.authentication = RequestAuthentication::Bearer {
        token: ValueSource::secret(token),
    };
    request.body = RequestBody::Json {
        value: r#"{"user":"{{user_id}}"}"#.to_owned(),
    };

    let prepared = RequestPipeline::new(Some(&environment), &secrets)
        .prepare(&request)
        .expect("valid request");

    assert_eq!(
        prepared.uri().to_string(),
        "https://api.example.com/users?display+name=Chris+M."
    );
    assert_eq!(prepared.headers()["authorization"], "Bearer super-secret");
    assert_eq!(prepared.headers()["content-type"], "application/json");
    assert_eq!(prepared.body(), br#"{"user":"Chris M."}"#);
}
