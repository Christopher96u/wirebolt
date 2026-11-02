//! Bounded deterministic malformed-input coverage for every supported importer.
use wirebolt_core::{ImportEngine, ImportFormat};

#[test]
fn bounded_import_mutations_never_panic() {
    let seeds = [
        (ImportFormat::Curl, "curl 'https://example.invalid/a' -H 'X-Test: café' --data 'hello'".to_owned()),
        (ImportFormat::Har, r#"{"log":{"version":"1.2","entries":[{"request":{"method":"GET","url":"https://example.invalid/","headers":[],"queryString":[]}}]}}"#.to_owned()),
        (ImportFormat::PostmanV2, r#"{"info":{"name":"Fixture","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[{"name":"Ping","request":{"method":"GET","url":"https://example.invalid/"}}]}"#.to_owned()),
        (ImportFormat::Insomnia, r#"{"_type":"export","__export_format":4,"resources":[{"_id":"wrk","_type":"workspace","name":"Fixture"},{"_id":"req","_type":"request","parentId":"wrk","url":"https://example.invalid/","method":"GET"}]}"#.to_owned()),
        (ImportFormat::Insomnia, "type: collection.insomnia.rest/5.0\nname: Fixture\ncollection:\n  - name: Ping\n    url: https://example.invalid/\n    method: GET\n".to_owned()),
        (ImportFormat::Bruno, r#"{"name":"Fixture","version":"1","items":[{"type":"http","name":"Ping","request":{"method":"GET","url":"https://example.invalid/"}}]}"#.to_owned()),
    ];
    let mut count = 0;
    for (format, seed) in seeds {
        let chars: Vec<char> = seed.chars().collect();
        for i in 0..=chars.len() {
            let prefix: String = chars[..i].iter().collect();
            assert!(
                std::panic::catch_unwind(|| ImportEngine::parse_import(format, &prefix, None))
                    .is_ok(),
                "prefix {format:?} {i}"
            );
            count += 1;
        }
        for i in 0..chars.len() {
            for replacement in ['\0', '\r', '\n', '"', '\\', '{', ']', 'é', '東', '🦀'] {
                let mut mutation = chars.clone();
                mutation[i] = replacement;
                let source: String = mutation.into_iter().collect();
                assert!(
                    std::panic::catch_unwind(|| ImportEngine::parse_import(format, &source, None))
                        .is_ok(),
                    "mutation {format:?} {i} {replacement:?}"
                );
                count += 1;
            }
        }
    }
    let bru = "meta {\n name: Ping\n type: http\n}\nget {\n url: https://example.invalid/\n}\nheaders {\n X-Test: café\n}\n";
    let chars: Vec<_> = bru.chars().collect();
    for i in 0..=chars.len() {
        for replacement in [None, Some('\0'), Some('}'), Some('東')] {
            let mut source: String = chars[..i].iter().collect();
            if let Some(c) = replacement {
                source.push(c);
                source.extend(chars[i..].iter());
            }
            let bundle = wirebolt_core::BrunoFolderBundle {
                root: "synthetic".into(),
                files: vec![
                    wirebolt_core::BrunoSourceFile {
                        path: "bruno.json".into(),
                        contents: r#"{"name":"Fixture","version":"1"}"#.into(),
                    },
                    wirebolt_core::BrunoSourceFile {
                        path: "request.bru".into(),
                        contents: source,
                    },
                ],
            };
            assert!(
                std::panic::catch_unwind(|| ImportEngine::parse_bruno_folder(&bundle, None))
                    .is_ok(),
                "bru {i}"
            );
            count += 1;
        }
    }
    for input in [
        ";",
        "curl ; ;",
        "curl -H",
        "curl '\\",
        "curl https://example.invalid/ --data-urlencode 'é=x'",
        "a: &a [*a]",
        "type: collection.insomnia.rest/5.0\ncollection: [",
        "\u{feff}{}",
    ] {
        for format in [
            ImportFormat::Curl,
            ImportFormat::Har,
            ImportFormat::PostmanV2,
            ImportFormat::Insomnia,
            ImportFormat::Bruno,
        ] {
            assert!(
                std::panic::catch_unwind(|| ImportEngine::parse_import(format, input, None))
                    .is_ok()
            );
            count += 1;
        }
    }
    eprintln!("bounded_import_mutations_never_panic: {count} parser invocations, no panics");
}
