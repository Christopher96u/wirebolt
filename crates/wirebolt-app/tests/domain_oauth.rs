use std::{collections::BTreeMap, sync::Arc};
use wirebolt_app::domain::DomainAuthenticationKind;
use wirebolt_app::domain::{models::*, oauth::*, request::new_auth};
fn configuration() -> DomainOAuthConfiguration {
    let DomainAuthentication::Oauth2 { configuration } = new_auth(DomainAuthenticationKind::Oauth2)
    else {
        panic!("OAuth")
    };
    configuration
}
#[test]
fn form_encoding_round_trips_plus_ampersand_equals_and_unicode() {
    let original = BTreeMap::from([
        ("client_secret".into(), "a+b&c=d café".into()),
        ("scope".into(), "read write".into()),
    ]);
    let encoded = encode_form(&original);
    assert!(encoded.contains("%2B"));
    let decoded: BTreeMap<_, _> = url::form_urlencoded::parse(encoded.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(decoded, original);
}
#[test]
fn pkce_challenge_matches_rfc7636_vector() {
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let verifier = random_url_safe(48).unwrap();
    assert_eq!(verifier.len(), 64);
    assert!(
        verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    );
    assert_ne!(verifier, random_url_safe(48).unwrap());
}
#[test]
fn authorization_preserves_query_items_and_builds_pkce_request() {
    let mut c = configuration();
    c.authorization_url = "https://auth.example/authorize?tenant=fixture".into();
    c.client_id = "client".into();
    c.scopes = "read write".into();
    c.audience = "audience".into();
    let (address, callback) = authorization_url(&c, "challenge", "state").unwrap();
    assert_eq!(callback, "wirebolt");
    let url = url::Url::parse(&address).unwrap();
    let query: BTreeMap<_, _> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    for (key, value) in [
        ("tenant", "fixture"),
        ("response_type", "code"),
        ("client_id", "client"),
        ("redirect_uri", "wirebolt://oauth/callback"),
        ("code_challenge", "challenge"),
        ("code_challenge_method", "S256"),
        ("state", "state"),
        ("scope", "read write"),
        ("audience", "audience"),
    ] {
        assert_eq!(query[key], value);
    }
    c.authorization_url = "http://remote.example/authorize".into();
    assert_eq!(
        authorization_url(&c, "c", "s").unwrap_err(),
        DomainOAuthError::InvalidConfiguration
    );
    c.authorization_url = "http://localhost/authorize".into();
    assert!(authorization_url(&c, "c", "s").is_ok());
}
#[test]
fn callback_validates_state_before_code_and_uses_first_duplicate() {
    assert_eq!(
        authorization_code(
            "wirebolt://oauth/callback?state=s&code=first&code=second",
            "s"
        )
        .unwrap(),
        "first"
    );
    assert_eq!(
        authorization_code("wirebolt://oauth/callback?state=wrong&code=c", "s").unwrap_err(),
        DomainOAuthError::StateMismatch
    );
    assert_eq!(
        authorization_code("wirebolt://oauth/callback?state=s", "s").unwrap_err(),
        DomainOAuthError::MissingAuthorizationCode
    );
    assert_eq!(
        authorization_code("wirebolt://oauth/callback?state=s&code=", "s").unwrap_err(),
        DomainOAuthError::MissingAuthorizationCode
    );
}
#[test]
fn token_parsing_preserves_optional_expiry_scope_and_rejection_rules() {
    let receipt = parse_token(
        200,
        r#"{"access_token":"fixture","expires_in":60,"scope":"read","refresh_token":"ignored"}"#,
        1000.0,
    )
    .unwrap();
    assert_eq!(receipt.token, "fixture");
    assert_eq!(receipt.expires_at, Some(1060.0));
    assert_eq!(receipt.scope.as_deref(), Some("read"));
    assert_eq!(
        parse_token(401, "{}", 0.0).unwrap_err(),
        DomainOAuthError::TokenEndpointRejected { status: 401 }
    );
    for body in ["{}", r#"{"access_token":""}"#, "invalid"] {
        assert_eq!(
            parse_token(200, body, 0.0).unwrap_err(),
            DomainOAuthError::InvalidTokenResponse
        );
    }
    let receipt = parse_token(200, r#"{"access_token":"fixture"}"#, 0.0).unwrap();
    assert!(receipt.expires_at.is_none());
    assert!(receipt.scope.is_none());
}
struct NoPlatform;
#[async_trait::async_trait]
impl DomainOAuthPlatform for NoPlatform {
    async fn authorize(
        &self,
        _url: String,
        _callback_scheme: String,
    ) -> Result<String, DomainOAuthError> {
        panic!("invalid configurations never open a browser")
    }
    async fn read_secret(&self, _name: String) -> Option<String> {
        None
    }
}
#[tokio::test]
async fn invalid_token_endpoints_and_missing_secrets_fail_before_exchange() {
    for url in ["", "http://remote.example/token"] {
        let mut c = configuration();
        c.token_url = url.into();
        c.client_id = "client".into();
        assert_eq!(
            acquire(c, Arc::new(NoPlatform)).await.unwrap_err(),
            DomainOAuthError::InvalidConfiguration
        );
    }
    let mut c = configuration();
    c.grant = DomainOAuthGrant::ClientCredentials;
    c.token_url = "https://auth.example/token".into();
    c.client_id = "client".into();
    assert_eq!(
        acquire(c, Arc::new(NoPlatform)).await.unwrap_err(),
        DomainOAuthError::ClientSecretUnavailable
    );
}
#[test]
fn existing_authorization_query_keeps_literal_plus_flags_and_duplicates() {
    let mut c = configuration();
    c.authorization_url = "https://auth.example/authorize?tenant=a+b&flag&key=one&key=two".into();
    let (url, _) = authorization_url(&c, "challenge", "state").unwrap();
    assert!(url.contains("tenant=a+b&flag&key=one&key=two&response_type=code"));
}

fn token_server() -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = vec![];
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            assert_ne!(count, 0);
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(head) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..head]);
                let length: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .map(|(_, v)| v.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= head + 4 + length {
                    break;
                }
            }
        }
        let body = r#"{"access_token":"fixture-token","expires_in":60,"scope":"read"}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        String::from_utf8(bytes).unwrap()
    });
    (format!("http://{address}/token"), worker)
}
#[derive(Default)]
struct Platform {
    challenge: std::sync::Mutex<String>,
}
#[async_trait::async_trait]
impl DomainOAuthPlatform for Platform {
    async fn authorize(
        &self,
        address: String,
        _scheme: String,
    ) -> Result<String, DomainOAuthError> {
        let url = url::Url::parse(&address).unwrap();
        let query: BTreeMap<_, _> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        self.challenge
            .lock()
            .unwrap()
            .clone_from(&query["code_challenge"]);
        Ok(format!(
            "wirebolt://oauth/callback?state={}&code=code+value",
            query["state"]
        ))
    }
    async fn read_secret(&self, _name: String) -> Option<String> {
        Some("a+b&c=d".into())
    }
}
fn captured_form(request: &str) -> BTreeMap<String, String> {
    assert!(request.starts_with("POST /token HTTP/1.1\r\n"));
    let body = request.split_once("\r\n\r\n").unwrap().1;
    url::form_urlencoded::parse(body.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}
#[tokio::test]
async fn client_credentials_exchange_preserves_form_and_receipt() {
    let (endpoint, server) = token_server();
    let mut c = configuration();
    c.grant = DomainOAuthGrant::ClientCredentials;
    c.token_url = endpoint;
    c.client_id = "client".into();
    c.scopes = "read write".into();
    c.audience = "fixture".into();
    let receipt = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        acquire(c, Arc::new(Platform::default())),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(receipt.token, "fixture-token");
    assert_eq!(receipt.scope.as_deref(), Some("read"));
    assert!(receipt.expires_at.is_some());
    let form = captured_form(&server.join().unwrap());
    assert_eq!(form["grant_type"], "client_credentials");
    assert_eq!(form["client_secret"], "a+b&c=d");
    assert_eq!(form["scope"], "read write");
    assert_eq!(form["audience"], "fixture");
}
#[tokio::test]
async fn pkce_exchange_uses_browser_code_and_the_original_verifier() {
    let (endpoint, server) = token_server();
    let mut c = configuration();
    c.token_url = endpoint;
    c.client_id = "client".into();
    c.authorization_url = "https://auth.example/authorize".into();
    let platform = Arc::new(Platform::default());
    let receipt = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        acquire(c, platform.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(receipt.token, "fixture-token");
    let form = captured_form(&server.join().unwrap());
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["code"], "code+value");
    assert_eq!(form["redirect_uri"], "wirebolt://oauth/callback");
    assert_eq!(
        pkce_challenge(&form["code_verifier"]),
        *platform.challenge.lock().unwrap()
    );
    assert!(!form.contains_key("client_secret"));
}
