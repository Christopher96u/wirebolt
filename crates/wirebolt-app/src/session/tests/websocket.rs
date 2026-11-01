use super::super::websocket::*;
use crate::domain::DomainBody;
#[test]
fn generation_rejects_late_events_after_disconnect() {
    let mut socket = SessionSocket::default();
    let generation = socket.begin().unwrap();
    assert!(socket.begin().is_none());
    socket.connected(&generation, vec![], "wss://example.com");
    socket.disconnect();
    socket.append(&generation, b"late".to_vec(), false, false, None);
    socket.finish(&generation, Some("late".into()));
    let snapshot = socket.snapshot();
    assert_eq!(snapshot.status, SessionSocketStatus::Disconnected);
    assert_eq!(snapshot.messages.len(), 2);
    assert!(snapshot.error.is_none());
}
#[test]
fn retention_limits_messages_and_bytes() {
    let mut socket = SessionSocket::default();
    let generation = socket.begin().unwrap();
    for _ in 0..1002 {
        socket.append(&generation, vec![65], false, false, None);
    }
    assert_eq!(socket.snapshot().messages.len(), 1000);
    socket.append(&generation, vec![0; MESSAGE_BYTES], true, false, None);
    assert_eq!(socket.snapshot().messages.len(), 1);
    socket.clear();
    assert!(socket.snapshot().messages.is_empty());
}
#[test]
fn completion_records_failure_notice_and_reconnect_clears_error() {
    let mut socket = SessionSocket::default();
    let generation = socket.begin().unwrap();
    socket.finish(&generation, Some("failed".into()));
    assert_eq!(
        socket.snapshot().messages[0].notice,
        Some(SessionSocketNotice::Failed)
    );
    assert!(socket.begin().is_some());
    assert!(socket.snapshot().error.is_none());
}
#[test]
fn binary_formats_accept_whitespace_and_reject_invalid_input() {
    assert_eq!(decode_binary(" aG Vs bG8=\n", false).unwrap(), b"hello");
    assert_eq!(decode_binary(" 00 ff 7A\n", true).unwrap(), [0, 255, 122]);
    for text in ["0", "gg", "é0"] {
        assert_eq!(
            decode_binary(text, true).unwrap_err(),
            "Enter a valid hexadecimal binary message."
        );
    }
    assert_eq!(
        decode_binary("???", false).unwrap_err(),
        "Enter a valid Base64 binary message."
    );
}

#[test]
fn binary_base64_accepts_nonzero_trailing_bits() {
    assert_eq!(decode_binary(" Z h = =\n", false).unwrap(), b"f");
    assert_eq!(decode_binary("Zm9=", false).unwrap(), b"fo");
}

#[test]
fn json_messages_preserve_foundation_tolerance_and_original_bytes() {
    for value in [
        r#"{"a":1,}"#.to_owned(),
        "\u{feff}[1,]".into(),
        r#"{"s":"comma,} and escaped \" quote","a":[true,],}"#.into(),
        format!("{}0{}", "[".repeat(512), "]".repeat(512)),
        "null".into(),
    ] {
        let (data, binary) = message_body(&DomainBody::Json {
            value: value.clone(),
        })
        .unwrap();
        assert_eq!(data, value.as_bytes());
        assert!(!binary);
    }
    for value in [
        "[,]".to_owned(),
        "{,}".into(),
        "[1,,]".into(),
        " \u{feff}{}".into(),
        format!("{}0{}", "[".repeat(513), "]".repeat(513)),
    ] {
        assert_eq!(
            message_body(&DomainBody::Json { value }).unwrap_err(),
            "The data couldn’t be read because it isn’t in the correct format."
        );
    }
}

#[test]
fn normal_server_close_preserves_the_last_send_error_and_failed_notice() {
    let mut socket = SessionSocket::default();
    let generation = socket.begin().unwrap();
    socket.connected(&generation, vec![], "wss://example.com");
    let error = message_body(&DomainBody::Json { value: "{".into() }).unwrap_err();
    socket.set_error(Some(error.clone()));
    socket.closed(&generation);
    socket.finish(&generation, None);
    let snapshot = socket.snapshot();
    assert_eq!(snapshot.status, SessionSocketStatus::Disconnected);
    assert_eq!(snapshot.error.as_deref(), Some(error.as_str()));
    let notice = snapshot.messages.last().unwrap();
    assert_eq!(notice.notice, Some(SessionSocketNotice::Failed));
    assert_eq!(notice.text, error);
}

#[test]
fn receive_error_replaces_a_send_error_but_a_clean_close_stays_disconnected() {
    for previous in [None, Some("send failed".into())] {
        let mut socket = SessionSocket::default();
        let generation = socket.begin().unwrap();
        socket.set_error(previous);
        socket.finish(&generation, Some("receive failed".into()));
        let snapshot = socket.snapshot();
        assert_eq!(snapshot.error.as_deref(), Some("receive failed"));
        assert_eq!(snapshot.messages.last().unwrap().text, "receive failed");
    }
    let mut socket = SessionSocket::default();
    let generation = socket.begin().unwrap();
    socket.finish(&generation, None);
    let snapshot = socket.snapshot();
    assert!(snapshot.error.is_none());
    assert_eq!(
        snapshot.messages[0].notice,
        Some(SessionSocketNotice::Disconnected)
    );
    assert_eq!(snapshot.messages[0].text, "Disconnected");
}
