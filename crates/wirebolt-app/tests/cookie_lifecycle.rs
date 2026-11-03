use std::sync::{Arc, Mutex};
use wirebolt_app::{
    response::{ResponseCookieJar, ResponseHeaderValue, RunHistory, unix_time},
    session::runtime::RuntimeState,
};

fn header(value: &str) -> ResponseHeaderValue {
    ResponseHeaderValue {
        name: "set-cookie".into(),
        value: value.into(),
    }
}

#[test]
fn workspace_switch_keeps_each_process_cookie_jar() {
    let root = tempfile::tempdir().unwrap();
    let runtime = RuntimeState::new(
        Arc::new(Mutex::new(RunHistory::new(root.path().join("history")))),
        Arc::new(Mutex::new(ResponseCookieJar::new(None))),
    );
    let directory = root.path().to_str().unwrap();
    runtime.configure_cookies(Some("/fixture/a"), Some(directory));
    let first = runtime.cookie_jar();
    let _ = first.lock().unwrap().store(
        &[header("session=a; Path=/")],
        "https://example.invalid/",
        unix_time(),
    );
    runtime.configure_cookies(Some("/fixture/b"), Some(directory));
    assert!(runtime.cookies().is_empty());
    let _ = runtime.cookie_jar().lock().unwrap().store(
        &[header("session=b; Path=/")],
        "https://example.invalid/",
        unix_time(),
    );
    runtime.configure_cookies(Some("/fixture/a/."), Some(directory));
    assert!(Arc::ptr_eq(&first, &runtime.cookie_jar()));
    assert_eq!(runtime.cookies()[0].value, "a");
    runtime.configure_cookies(Some("/fixture/b"), Some(directory));
    assert_eq!(runtime.cookies()[0].value, "b");
    let mut relaunched = ResponseCookieJar::new(Some(wirebolt_app::response::cookie_storage_path(
        std::path::Path::new("/fixture/a"),
        root.path(),
    )));
    assert!(relaunched.all(unix_time()).is_empty());
}

#[test]
fn failed_cookie_delete_and_clear_restore_memory_and_disk() {
    for clear in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("cookies");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("jar.json");
        let mut jar = ResponseCookieJar::new(Some(path.clone()));
        let _ = jar.store(
            &[
                header("persistent=fixture; Max-Age=600; Path=/"),
                header("session=fixture; Path=/"),
            ],
            "https://example.invalid/",
            unix_time(),
        );
        let before = jar.all(unix_time());
        let disk = std::fs::read(&path).unwrap();
        let backup = root.path().join("backup");
        std::fs::rename(&directory, &backup).unwrap();
        std::fs::write(&directory, "blocks directory creation").unwrap();
        let result = if clear {
            jar.remove_all()
        } else {
            jar.delete(&[before[0].id()])
        };
        assert!(result.is_err());
        assert_eq!(jar.all(unix_time()), before);
        std::fs::remove_file(&directory).unwrap();
        std::fs::rename(&backup, &directory).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), disk);
        if clear {
            jar.remove_all().unwrap();
        } else {
            jar.delete(&[before[0].id()]).unwrap();
        }
        assert!(
            ResponseCookieJar::new(Some(path))
                .all(unix_time())
                .is_empty()
        );
    }
}

#[test]
fn multi_cookie_delete_is_atomic_on_failure_and_persists_one_complete_selection() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("jar.json");
    let mut jar = ResponseCookieJar::new(Some(path.clone()));
    let _ = jar.store(
        &[
            header("first=1; Max-Age=600"),
            header("second=2; Max-Age=600"),
            header("keep=3; Max-Age=600"),
        ],
        "https://example.invalid/",
        unix_time(),
    );
    let previous = jar.all(unix_time());
    let bytes = std::fs::read(&path).unwrap();
    let ids = previous[..2]
        .iter()
        .map(wirebolt_app::response::StoredCookie::id)
        .collect::<Vec<_>>();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(jar.delete(&ids).is_err());
    assert_eq!(
        jar.all(unix_time()),
        previous,
        "Every selected cookie is restored"
    );
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    jar.delete(&ids).unwrap();
    assert_eq!(jar.all(unix_time()), previous[2..]);
    assert_eq!(
        ResponseCookieJar::new(Some(path)).all(unix_time()),
        previous[2..]
    );
}
