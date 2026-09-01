use std::{collections::BTreeMap, hint::black_box};

use criterion::{Criterion, criterion_group, criterion_main};
use wirebolt_core::{
    DocumentId, Environment, Request, RequestAuthentication, RequestBody, RequestHeader,
    RequestPipeline, RequestValueField, ResolvedSecret, SecretName, SecretResolutionError,
    SecretResolver, ValueSource,
};

struct StaticSecrets;

impl SecretResolver for StaticSecrets {
    fn resolve(&self, _: &SecretName) -> Result<ResolvedSecret, SecretResolutionError> {
        Ok(ResolvedSecret::new("sk-live-benchmark-token"))
    }
}

fn id(value: &str) -> DocumentId {
    DocumentId::new(value).expect("benchmark document ID")
}

/// A saved request shaped like a typical API call: templated URL, eight
/// headers (two templated), three query fields, bearer secret, JSON body.
fn representative_request() -> Request {
    let mut request = Request::new(
        id("create-order"),
        "Create order",
        "POST",
        "{{base_url}}/v1/orders/{{tenant}}",
    );
    request.query = vec![
        RequestValueField::enabled("expand", ValueSource::literal("items,customer")),
        RequestValueField::enabled("locale", ValueSource::literal("{{locale}}")),
        RequestValueField::enabled("dry run", ValueSource::literal("false")),
    ];
    request.headers = (0..8)
        .map(|index| {
            let value = match index {
                0 => "application/json".to_owned(),
                1 => "{{tenant}}".to_owned(),
                2 => "{{locale}}".to_owned(),
                _ => format!("value-{index}"),
            };
            RequestHeader::enabled(format!("x-bench-{index}"), ValueSource::literal(value))
        })
        .collect();
    request.authentication = RequestAuthentication::Bearer {
        token: ValueSource::secret(SecretName::new("api.token").expect("secret name")),
    };
    request.body = RequestBody::Json {
        value: r#"{"tenant":"{{tenant}}","items":[{"sku":"A1","qty":2},{"sku":"B2","qty":1}]}"#
            .to_owned(),
    };
    request
}

fn benchmark_request_pipeline(c: &mut Criterion) {
    let environment = Environment::new(
        id("production"),
        "Production".to_owned(),
        BTreeMap::from([
            (
                "base_url".to_owned(),
                ValueSource::literal("https://api.example.com"),
            ),
            ("tenant".to_owned(), ValueSource::literal("acme")),
            ("locale".to_owned(), ValueSource::literal("es-ES")),
        ]),
    );
    let request = representative_request();
    let pipeline = RequestPipeline::new(Some(&environment), &StaticSecrets);

    c.bench_function("request_pipeline_prepare", |b| {
        b.iter(|| {
            let prepared = pipeline
                .prepare(black_box(&request))
                .expect("benchmark request prepares");
            black_box(prepared.headers().len())
        });
    });

    let plain = Request::new(
        id("health"),
        "Health",
        "GET",
        "https://api.example.com/health",
    );
    c.bench_function("request_pipeline_prepare_plain_get", |b| {
        b.iter(|| {
            let prepared = pipeline
                .prepare(black_box(&plain))
                .expect("plain request prepares");
            black_box(prepared.url().as_str().len())
        });
    });
}

criterion_group!(benches, benchmark_request_pipeline);
criterion_main!(benches);
