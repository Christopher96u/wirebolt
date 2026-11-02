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
            jar.delete(&before[0].id())
        };
        assert!(result.is_err());
        assert_eq!(jar.all(unix_time()), before);
        std::fs::remove_file(&directory).unwrap();
        std::fs::rename(&backup, &directory).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), disk);
        if clear {
            jar.remove_all().unwrap();
        } else {
            jar.delete(&before[0].id()).unwrap();
        }
        assert!(
            ResponseCookieJar::new(Some(path))
                .all(unix_time())
                .is_empty()
        );
    }
}
