use super::super::websocket::*;
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
