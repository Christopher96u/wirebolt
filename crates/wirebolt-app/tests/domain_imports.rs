use std::fs;
use wirebolt_app::domain::{imports::*, request::*};
#[test]
fn collection_exports_keep_all_existing_importers() {
    for (extension, text, format) in [
        (
            "json",
            r#"{"_type":"export","__export_format":4,"resources":[]}"#,
            "insomnia",
        ),
        (
            "yaml",
            "type: collection.insomnia.rest/5.0\nschema_version: \"5.1\"\nname: Payments API",
            "insomnia",
        ),
        ("yaml", "_type: export\n__export_format: 4\n", "insomnia"),
        (
            "json",
            r#"{"name":"Library","version":"1","items":[],"environments":[]}"#,
            "bruno",
        ),
        ("bru", "meta {\n name: Ping\n}\n", "bruno_folder"),
        (
            "json",
            r#"{"info":{"_postman_id":"x"},"item":[]}"#,
            "postman_v2",
        ),
        ("json", r#"{"version":1,"nodes":[]}"#, "legacy_workspace_v1"),
    ] {
        assert_eq!(detect(extension, text).as_deref(), Some(format));
    }
    assert_eq!(detect("yaml", "openapi: 3.1.0\n"), None);
}
#[test]
fn bruno_folders_bundle_only_supported_files_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (path, contents) in [
        (
            "bruno.json",
            r#"{"version":"1","name":"Library","type":"collection"}"#,
        ),
        ("Users/folder.bru", "meta {\n name: Members\n}\n"),
        (
            "Users/Get member.bru",
            "get {\n url: {{baseUrl}}/members\n}\n",
        ),
        (
            "environments/Local.bru",
            "vars {\n baseUrl: http://127.0.0.1\n}\n",
        ),
        ("node_modules/dependency/ignored.bru", "get {\n}\n"),
        (".git/config.bru", "get {\n}\n"),
        ("assets/cover.png", "not read"),
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    assert!(is_bruno_collection(root));
    assert!(!is_bruno_collection(&root.join("assets")));
    let bundle = bruno_bundle(root).unwrap();
    let document: serde_json::Value = serde_json::from_str(&bundle).unwrap();
    assert_eq!(document["root"], root.to_string_lossy().as_ref());
    let paths: Vec<_> = document["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "Users/Get member.bru",
            "Users/folder.bru",
            "bruno.json",
            "environments/Local.bru"
        ]
    );
    assert!(
        bruno_bundle(&root.join("Users/Get member.bru"))
            .unwrap()
            .contains(r#""path":"Get member.bru""#)
    );
    assert_eq!(
        bruno_bundle(&root.join("assets")).unwrap_err(),
        "notACollection"
    );
}
const LEAF: &str =
    "-----BEGIN CERTIFICATE-----\nZml4dHVyZS1sZWFmLWNlcnRpZmljYXRl\n-----END CERTIFICATE-----";
const INTERMEDIATE: &str =
    "-----BEGIN CERTIFICATE-----\nZml4dHVyZS1pbnRlcm1lZGlhdGU=\n-----END CERTIFICATE-----";
const KEY: &str =
    "-----BEGIN PRIVATE KEY-----\nZml4dHVyZS1wcml2YXRlLWtleQ==\n-----END PRIVATE KEY-----";
#[test]
fn pem_certificate_chains_and_keys_combine_from_one_or_two_sources() {
    let identity = parse_pem(&[
        format!("{LEAF}\r\n{INTERMEDIATE}\n"),
        format!("Bag Attributes\n{KEY}"),
    ])
    .unwrap();
    assert_eq!(identity.certificates, [LEAF, INTERMEDIATE]);
    assert_eq!(identity.private_key, KEY);
    assert_eq!(
        identity.combined(),
        format!("{LEAF}\n{INTERMEDIATE}\n{KEY}\n")
    );
    assert_eq!(parse_pem(&[identity.combined()]).unwrap(), identity);
    let single = parse_pem(&[format!("{KEY}\n{LEAF}")]).unwrap();
    assert_eq!(single.certificates, [LEAF]);
    assert_eq!(single.private_key, KEY);
    let rsa = KEY.replace("PRIVATE KEY", "RSA PRIVATE KEY");
    assert_eq!(
        parse_pem(&[LEAF.into(), rsa.clone()]).unwrap().private_key,
        rsa
    );
}
#[test]
fn pem_rejects_missing_duplicate_and_encrypted_keys() {
    for(sources,error)in [(vec![KEY.into()],"noCertificate"),(vec![LEAF.into()],"noPrivateKey"),(vec!["not a certificate".into()],"noCertificate"),(vec![LEAF.into(),KEY.into(),KEY.into()],"multiplePrivateKeys"),(vec![LEAF.into(),KEY.replace("PRIVATE KEY","ENCRYPTED PRIVATE KEY")],"encryptedPrivateKey"),(vec![LEAF.into(),"-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: AES-128-CBC,00\n\nZml4dHVyZQ==\n-----END RSA PRIVATE KEY-----".into()],"encryptedPrivateKey")]{assert_eq!(parse_pem(&sources).unwrap_err(),error);}
}
#[test]
fn import_summary_preserves_counts_pluralization_and_environment_guidance() {
    assert_eq!(
        import_headline(&["Shop API".into()], 9, 3),
        "Imported 9 requests in 3 folders into “Shop API”."
    );
    assert_eq!(
        import_environment_line(&["Shop API".into()], "").as_deref(),
        Some("Created the environment “Shop API” for its variables.")
    );
    assert_eq!(
        import_headline(&["Imported cURL".into()], 1, 0),
        "Imported 1 request into “Imported cURL”."
    );
    assert_eq!(import_environment_line(&[], ""), None);
    assert_eq!(
        import_headline(&["A".into(), "B".into()], 2, 1),
        "Imported 2 requests in 1 folder into 2 collections."
    );
}
#[test]
fn git_status_words_keep_index_additions_and_conflicts() {
    for (staged, unstaged, conflicted, expected) in [
        ("none", "untracked", false, "New"),
        ("added", "modified", false, "New"),
        ("modified", "none", false, "Modified"),
        ("none", "deleted", false, "Deleted"),
        ("renamed", "none", false, "Renamed"),
        ("renamed", "modified", false, "Renamed"),
        ("unmerged", "unmerged", false, "Conflict"),
        ("modified", "modified", true, "Conflict"),
    ] {
        assert_eq!(git_change_kind(staged, unstaged, conflicted), expected);
    }
}
#[test]
fn git_push_requires_a_branch_and_commits_or_publishes_new_branches() {
    assert_eq!(
        push_availability(Some("main".into()), Some("origin/main"), 2),
        PushAvailability::Push
    );
    assert_eq!(
        push_availability(Some("main".into()), None, 0),
        PushAvailability::Publish {
            branch: "main".into()
        }
    );
    assert!(matches!(
        push_availability(Some("main".into()), Some("origin/main"), 0),
        PushAvailability::Unavailable { .. }
    ));
    assert!(matches!(
        push_availability(None, None, 0),
        PushAvailability::Unavailable { .. }
    ));
}
struct PackagePolicy;
impl CollectionDirectoryPolicy for PackagePolicy {
    fn is_package(&self, path: &std::path::Path) -> bool {
        path.extension().is_some_and(|e| e == "app")
    }
}
#[test]
fn bruno_skips_native_packages_and_checks_limits_while_traversing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("root.bru"), "get {}").unwrap();
    fs::create_dir_all(root.join("fixture.app/Contents")).unwrap();
    fs::write(root.join("fixture.app/Contents/hidden.bru"), "get {}").unwrap();
    let bundle = bruno_bundle_with_policy(root, &PackagePolicy).unwrap();
    assert!(!bundle.contains("hidden.bru"));
    let large = root.join("large.bru");
    let file = fs::File::create(&large).unwrap();
    file.set_len(2 * 1024 * 1024 + 1).unwrap();
    assert_eq!(bruno_bundle(root).unwrap_err(), "tooLarge");
}
