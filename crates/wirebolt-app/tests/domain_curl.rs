use wirebolt_app::domain::DomainAuthenticationKind;
mod support;
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    process::Command,
};
use support::*;
use wirebolt_app::domain::{curl::*, models::*};
fn command(
    d: &DomainRequestDraft,
    vars: &HashMap<String, DomainValueSource>,
    keys: &HashMap<String, String>,
) -> String {
    export(
        d,
        vars,
        &keys
            .iter()
            .filter(|(_, v)| prints_as_hex(v))
            .map(|(k, _)| k.clone())
            .collect(),
    )
    .command
}
fn run(command: &str, keys: &HashMap<String, String>) -> Vec<String> {
    let mut entries = String::new();
    for (name, value) in keys {
        let printed = if prints_as_hex(value) {
            let mut hex = String::new();
            for b in value.bytes() {
                write!(hex, "{b:02x}").unwrap();
            }
            hex
        } else {
            value.clone()
        };
        writeln!(
            entries,
            "{}) printf '%s\\n' {};;",
            shell_quote(name),
            shell_quote(&printed)
        )
        .unwrap();
    }
    let script = format!(
        r#"security() {{
 local service="" account=""
 while [ $# -gt 0 ]; do case "$1" in -s) service="$2"; shift 2;; -a) account="$2"; shift 2;; *) shift;; esac; done
 [ "$service" = 'io.github.christopher96u.wirebolt' ] || return 44
 case "$account" in
 {entries}
 *) return 44;; esac
}}
curl() {{
 for a in "$@"; do
  printf '%s\0' "$a"
  if [[ $a =~ '/dev/fd/([0-9]+)' ]]; then printf 'fd%s=%s\0' "$match[1]" "$(cat /dev/fd/$match[1])"; fi
 done
}}
{command}"#
    );
    let output = Command::new("/bin/zsh")
        .args(["-f", "-c", &script])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .split_terminator('\0')
        .map(str::to_owned)
        .collect()
}
fn keys<const N: usize>(pairs: [(&str, &str); N]) -> HashMap<String, String> {
    pairs
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}
#[test]
fn shell_quoting_preserves_apostrophes_substitutions_unicode_and_credentials() {
    let mut d = request("https://example.com/echo");
    d.method = "POST".into();
    d.authentication = DomainAuthentication::Basic {
        username: DomainValueSource::literal("O'Brien"),
        password: secret("test.password"),
    };
    let payload = "café 東京 $(printf SHOULD_NOT_RUN) `printf SHOULD_NOT_RUN` 'quoted'";
    d.body = DomainBody::Json {
        value: payload.into(),
    };
    let k = keys([("test.password", "p@ss w'rd $(x)")]);
    let cmd = command(&d, &HashMap::new(), &k);
    assert!(!cmd.contains("p@ss"));
    assert_eq!(
        run(&cmd, &k),
        [
            "--request",
            "POST",
            "https://example.com/echo",
            "--user",
            "O'Brien:p@ss w'rd $(x)",
            "--header",
            "Content-Type: application/json",
            "--data-raw",
            payload
        ]
    );
}
#[test]
fn auth_headers_query_bodies_and_variables_read_keychain_at_execution() {
    let k = keys([
        ("env.token", "tok3n/+&=?"),
        ("auth.bearer", "bearer-value"),
        ("header.secret", "header-value"),
        ("form.secret", "form value"),
        ("unicode.secret", "café ☕"),
    ]);
    let vars = HashMap::from([
        ("token".into(), secret("env.token")),
        ("host".into(), DomainValueSource::literal("api.example.com")),
        ("wrapped".into(), DomainValueSource::literal("id-{{token}}")),
    ]);
    let mut d = request("https://{{host}}/items?fixed=1");
    d.method = "POST".into();
    d.query = vec![field("q", "a b"), field("key", "{{wrapped}}")];
    let mut header = field("X-Secret", "");
    header.value = secret("header.secret");
    let mut unicode = field("X-Unicode", "");
    unicode.value = secret("unicode.secret");
    d.headers = vec![header, unicode];
    d.authentication = DomainAuthentication::Bearer {
        token: secret("auth.bearer"),
    };
    let mut f = field("f", "");
    f.value = secret("form.secret");
    d.body = DomainBody::FormUrlEncoded {
        fields: vec![f, field("t", "{{token}}")],
    };
    let cmd = command(&d, &vars, &k);
    for value in k.values() {
        assert!(!cmd.contains(value));
    }
    assert!(cmd.contains("-a 'unicode.secret' -w | xxd -r -p"));
    assert!(!cmd.contains("-a 'env.token' -w | xxd"));
    assert_eq!(
        run(&cmd, &k),
        [
            "--request",
            "POST",
            "https://api.example.com/items?fixed=1",
            "--url-query",
            "q=a b",
            "--url-query",
            "key=id-tok3n/+&=?",
            "--header",
            "Authorization: Bearer bearer-value",
            "--header",
            "X-Secret: header-value",
            "--header",
            "X-Unicode: café ☕",
            "--header",
            "Content-Type: application/x-www-form-urlencoded",
            "--data-urlencode",
            "f=form value",
            "--data-urlencode",
            "t=tok3n/+&=?"
        ]
    );
    assert_eq!(
        export(&d, &vars, &HashSet::new()).secret_names,
        [
            "env.token",
            "auth.bearer",
            "header.secret",
            "unicode.secret",
            "form.secret"
        ]
    );
}
fn part(
    name: &str,
    kind: DomainMultipartKind,
    value: DomainValueSource,
    mime: Option<&str>,
    filename: Option<&str>,
) -> DomainMultipartPart {
    DomainMultipartPart {
        id: name.into(),
        name: name.into(),
        kind,
        value,
        file_path: None,
        file_name: filename.map(str::to_owned),
        content_type: mime.map(str::to_owned),
        enabled: true,
    }
}
#[test]
fn api_keys_oauth_and_multipart_keep_secret_material_out_of_exports() {
    let k = keys([
        ("api.key", "k3y"),
        ("oauth.token", "oauth-value"),
        ("part.secret", "part value"),
        ("binary.secret", "aGk="),
    ]);
    let mut d = request("https://example.com/");
    d.authentication = DomainAuthentication::ApiKey {
        placement: DomainApiKeyPlacement::Query,
        name: "api_key".into(),
        value: secret("api.key"),
    };
    assert_eq!(
        run(&command(&d, &HashMap::new(), &k), &k),
        [
            "--request",
            "GET",
            "https://example.com/",
            "--url-query",
            "api_key=k3y"
        ]
    );
    let DomainAuthentication::Oauth2 { mut configuration } =
        wirebolt_app::domain::request::new_auth(DomainAuthenticationKind::Oauth2)
    else {
        panic!("OAuth")
    };
    configuration.access_token_reference = "oauth.token".into();
    d.authentication = DomainAuthentication::Oauth2 { configuration };
    d.body = DomainBody::Multipart {
        parts: vec![
            part(
                "note",
                DomainMultipartKind::Text,
                secret("part.secret"),
                Some("text/plain"),
                None,
            ),
            part(
                "plain",
                DomainMultipartKind::Text,
                DomainValueSource::literal("visible"),
                None,
                None,
            ),
            part(
                "blob",
                DomainMultipartKind::Binary,
                secret("binary.secret"),
                None,
                Some("a.bin"),
            ),
        ],
    };
    let cmd = command(&d, &HashMap::new(), &k);
    for v in k.values() {
        assert!(!cmd.contains(v));
    }
    assert_eq!(
        run(&cmd, &k),
        [
            "--request",
            "GET",
            "https://example.com/",
            "--header",
            "Authorization: Bearer oauth-value",
            "--form",
            "note=</dev/fd/3;type=\"text/plain\"",
            "fd3=part value",
            "--form",
            "plain=\"visible\"",
            "--form",
            "blob=@/dev/fd/4;filename=\"a.bin\"",
            "fd4=hi"
        ]
    );
}
#[test]
fn forms_keep_repeated_enabled_fields_and_binary_multipart_bytes() {
    let mut d = request("http://localhost/echo");
    let mut off = field("off", "hidden");
    off.enabled = false;
    d.body = DomainBody::FormUrlEncoded {
        fields: vec![field("name", "café"), field("name", "東京"), off],
    };
    let cmd = command(&d, &HashMap::new(), &HashMap::new());
    assert!(cmd.contains("--data-urlencode 'name=café' --data-urlencode 'name=東京'"));
    assert!(!cmd.contains("hidden"));
    d.headers = vec![field("Content-Type", "multipart/form-data; boundary=stale")];
    d.body = DomainBody::Multipart {
        parts: vec![part(
            "file",
            DomainMultipartKind::Binary,
            DomainValueSource::literal("AAH/"),
            None,
            Some("a.bin"),
        )],
    };
    let cmd = command(&d, &HashMap::new(), &HashMap::new());
    assert!(cmd.contains("@/dev/fd/3"));
    assert!(cmd.contains("printf %s 'AAH/' | base64 --decode"));
    assert!(!cmd.contains("boundary=stale"));
}
#[test]
fn client_identity_is_read_through_file_descriptors() {
    let mut d = request("https://mtls.example.com/");
    d.transport.client_certificate_reference = Some("tls.fixture.client-identity".into());
    d.transport.custom_ca_path = Some("/tmp/fixture ca.pem".into());
    assert_eq!(
        value_sources(&d).last(),
        Some(&secret("tls.fixture.client-identity"))
    );
    let identity = "-----BEGIN CERTIFICATE-----\nZml4dHVyZQ==\n-----END CERTIFICATE-----\n-----BEGIN PRIVATE KEY-----\nZml4dHVyZQ==\n-----END PRIVATE KEY-----\n";
    let k = keys([("tls.fixture.client-identity", identity)]);
    let cmd = command(&d, &HashMap::new(), &k);
    assert!(!cmd.contains("BEGIN"));
    assert!(cmd.contains("--cert /dev/fd/3 --key /dev/fd/4"));
    assert!(cmd.contains("3< <(security find-generic-password -s 'io.github.christopher96u.wirebolt' -a 'tls.fixture.client-identity' -w | xxd -r -p)"));
    let stored = identity.trim_end();
    assert_eq!(
        run(&cmd, &k),
        vec![
            "--request".into(),
            "GET".into(),
            "https://mtls.example.com/".into(),
            "--cacert".into(),
            "/tmp/fixture ca.pem".into(),
            "--cert".into(),
            "/dev/fd/3".into(),
            format!("fd3={stored}"),
            "--key".into(),
            "/dev/fd/4".into(),
            format!("fd4={stored}")
        ]
    );
}
#[test]
fn binary_stream_descriptors_follow_identity_descriptors() {
    let mut d = request("https://mtls.example.com/");
    d.transport.client_certificate_reference = Some("tls.fixture.client-identity".into());
    d.body = DomainBody::Multipart {
        parts: vec![part(
            "file",
            DomainMultipartKind::Binary,
            DomainValueSource::literal("AAH/"),
            None,
            Some("a.bin"),
        )],
    };
    let cmd = command(&d, &HashMap::new(), &HashMap::new());
    assert!(cmd.contains("--cert /dev/fd/3 --key /dev/fd/4"));
    assert!(cmd.contains("--form 'file=@/dev/fd/5;filename=\"a.bin\"'"));
    assert!(cmd.contains("5< <(printf %s 'AAH/' | base64 --decode)"));
}
#[test]
fn proxy_arguments_quote_keychain_credentials_and_preserve_bypass() {
    assert!(
        proxy_arguments(&DomainProxy::System, "https://example.com", &HashSet::new()).is_empty()
    );
    assert_eq!(
        proxy_arguments(&DomainProxy::Direct, "https://example.com", &HashSet::new()),
        " --noproxy '*'"
    );
    let config = DomainProxy::Manual {
        routes: vec![DomainProxyRoute {
            destination: wirebolt_core::ProxyDestination::Https,
            endpoint: "http://localhost:8080".into(),
            credentials: Some(DomainProxyCredentials {
                username: "proxy.user".into(),
                password: "proxy.password".into(),
            }),
        }],
    };
    assert_eq!(
        proxy_arguments(&config, "http://example.com", &HashSet::new()),
        " --noproxy '*'"
    );
    let args = proxy_arguments(&config, "https://example.com", &HashSet::new());
    assert!(args.contains("--proxy 'http://localhost:8080' --noproxy ''"));
    assert!(args.contains("--proxy-user"));
    assert!(args.contains("-a 'proxy.user'"));
    assert!(args.contains("-a 'proxy.password'"));
}
