use std::collections::BTreeMap;

use tempfile::tempdir;
use wirebolt_core::{
    HttpEngine, HttpEngineConfig, ResolvedSecret, SecretName, SecretResolutionError,
    SecretResolutionErrorKind, SecretResolver,
};

struct Secrets(BTreeMap<String, String>);

impl SecretResolver for Secrets {
    fn resolve(&self, name: &SecretName) -> Result<ResolvedSecret, SecretResolutionError> {
        self.0
            .get(name.as_str())
            .cloned()
            .map(ResolvedSecret::new)
            .ok_or_else(|| SecretResolutionError::new(SecretResolutionErrorKind::NotFound))
    }
}

#[test]
fn tls_material_is_resolved_but_never_rendered() {
    let reference = SecretName::new("mtls.identity").expect("secret name");
    let material = "-----BEGIN PRIVATE KEY-----\nprivate-material\n-----END PRIVATE KEY-----";
    let secrets = Secrets(BTreeMap::from([(
        reference.as_str().to_owned(),
        material.to_owned(),
    )]));

    let configuration = HttpEngineConfig::default()
        .resolve_tls(Some(&reference), None, &secrets)
        .expect("resolve identity");
    let rendered = format!("{configuration:?}");
    assert!(rendered.contains("[REDACTED]"));
    assert!(!rendered.contains("private-material"));

    let error =
        HttpEngine::new(&configuration).expect_err("fixture identity is intentionally invalid");
    let rendered = format!("{error:?}");
    assert!(!rendered.contains("private-material"));
}

#[test]
fn custom_ca_is_read_without_placing_its_bytes_in_diagnostics() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("ca.pem");
    std::fs::write(&path, "public-ca-material").expect("write CA fixture");
    let configuration = HttpEngineConfig::default()
        .resolve_tls(None, path.to_str(), &Secrets(BTreeMap::new()))
        .expect("read CA file");
    let rendered = format!("{configuration:?}");
    assert!(rendered.contains("[CONFIGURED]"));
    assert!(!rendered.contains("public-ca-material"));
}
