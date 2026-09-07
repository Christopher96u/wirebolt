use std::{
    ffi::c_void,
    ptr,
    sync::{Mutex, mpsc},
    time::Duration,
};

use tokio::sync::mpsc::{Receiver, Sender};
use tokio_util::sync::CancellationToken;
use wirebolt_core::{RequestBody, WebSocketConnection, WebSocketFrame};

use super::{RunInput, SHARED_RUNTIME, prepare_protocol_run};

type EventCallback = extern "C" fn(*mut c_void, u8, *const u8, usize) -> u8;

#[derive(Debug)]
pub struct WireboltSocketSession {
    sender: Sender<WebSocketFrame>,
    cancellation: CancellationToken,
    completion: Mutex<Option<mpsc::Receiver<()>>>,
}

/// Copies input and starts a bounded, serialized WebSocket stream.
///
/// # Safety
/// Input must be readable for length bytes. Callback/context must remain alive
/// until the terminal callback returns. Free the result exactly once afterward.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_socket_start(
    json: *const u8,
    length: usize,
    callback: Option<EventCallback>,
    context: *mut c_void,
) -> *mut WireboltSocketSession {
    let Some(callback) = callback else {
        return ptr::null_mut();
    };
    if json.is_null() || length > 16 * 1024 * 1024 {
        return ptr::null_mut();
    }
    let Some(runtime) = SHARED_RUNTIME.as_ref() else {
        return ptr::null_mut();
    };
    // SAFETY: The caller supplies a readable input buffer; it is copied now.
    let input = unsafe { std::slice::from_raw_parts(json, length) }.to_vec();
    let cancellation = CancellationToken::new();
    let cancelled = cancellation.clone();
    let (sender, receiver) = tokio::sync::mpsc::channel(4);
    let (done, completion) = mpsc::sync_channel(1);
    let address = context as usize;
    runtime.spawn(async move {
        let worker = tokio::spawn(run(input, receiver, cancelled, callback, address));
        let result = worker.await;
        match result {
            Ok(Ok(())) => {
                emit(callback, address, 3, &[]);
            }
            Ok(Err(reason)) => {
                emit(callback, address, 4, reason.as_bytes());
            }
            Err(_) => {
                emit(callback, address, 4, b"The WebSocket connection failed.");
            }
        }
        let _ = done.send(());
    });
    Box::into_raw(Box::new(WireboltSocketSession {
        sender,
        cancellation,
        completion: Mutex::new(Some(completion)),
    }))
}

/// Enqueues one message; 0 means the queue is full or the session has closed.
///
/// # Safety
/// Session must be live and bytes readable for length bytes. Do not race free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_socket_send(
    session: *mut WireboltSocketSession,
    binary: u8,
    bytes: *const u8,
    length: usize,
) -> u8 {
    if session.is_null()
        || (bytes.is_null() && length != 0)
        || length > WebSocketConnection::MAX_MESSAGE_BYTES
    {
        return 0;
    }
    // SAFETY: The caller guarantees session lifetime and the readable payload.
    let session = unsafe { &*session };
    let bytes = if length == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, length) }
    };
    let message = if binary != 0 {
        WebSocketFrame::Binary(bytes.to_vec())
    } else {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return 0;
        };
        WebSocketFrame::Text(text.to_owned())
    };
    u8::from(session.sender.try_send(message).is_ok())
}

/// # Safety
/// Session must be null or live, with no concurrent free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_socket_cancel(session: *mut WireboltSocketSession) {
    if !session.is_null() {
        // SAFETY: The caller guarantees session lifetime.
        unsafe { &*session }.cancellation.cancel();
    }
}

/// Cancels, joins and releases one session. Never call from its callback.
///
/// # Safety
/// Session must be null or a live start result whose ownership is transferred
/// exactly once; no further calls may use the pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirebolt_socket_free(session: *mut WireboltSocketSession) {
    if session.is_null() {
        return;
    }
    // SAFETY: The caller transfers unique ownership back exactly once.
    let session = unsafe { Box::from_raw(session) };
    session.cancellation.cancel();
    if let Some(done) = session
        .completion
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        let _ = done.recv();
    }
}

fn emit(callback: EventCallback, address: usize, kind: u8, payload: &[u8]) -> bool {
    callback(
        address as *mut c_void,
        kind,
        payload.as_ptr(),
        payload.len(),
    ) != 0
}

async fn run(
    input: Vec<u8>,
    mut messages: Receiver<WebSocketFrame>,
    cancellation: CancellationToken,
    callback: EventCallback,
    address: usize,
) -> Result<(), String> {
    let mut input: RunInput =
        serde_json::from_slice(&input).map_err(|_| "The connection settings are invalid.")?;
    "GET".clone_into(&mut input.method);
    input.body = RequestBody::Empty;
    let timeout = input.total_timeout_ms;
    let preparation = tokio::task::spawn_blocking(move || {
        #[cfg(target_vendor = "apple")]
        let secrets = wirebolt_core::KeychainSecretResolver::default();
        #[cfg(not(target_vendor = "apple"))]
        let secrets = wirebolt_core::NoSecrets;
        prepare_protocol_run(input, &secrets, true)
            .map_err(|_| "The URL, headers or credentials could not be prepared.")
    });
    let prepared = tokio::select! {
        () = cancellation.cancelled() => return Ok(()),
        result = preparation => result.map_err(|_| "The connection could not be prepared.")??,
    };
    let connect = WebSocketConnection::connect(&prepared.1, &prepared.0);
    let mut socket = tokio::select! {
        () = cancellation.cancelled() => return Ok(()),
        result = async {
            if timeout == 0 { connect.await.map_err(|error| error.to_string()) }
            else { tokio::time::timeout(Duration::from_millis(timeout), connect).await
                .map_err(|_| "The connection timed out.".to_owned())?.map_err(|error| error.to_string()) }
        } => result?,
    };
    let headers = socket
        .handshake_headers
        .iter()
        .map(|header| serde_json::json!({"name": header.name, "value": header.value}))
        .collect::<Vec<_>>();
    let headers =
        serde_json::to_vec(&headers).map_err(|_| "The handshake headers could not be read.")?;
    if !emit(callback, address, 0, &headers) {
        return Ok(());
    }
    loop {
        tokio::select! {
            () = cancellation.cancelled() => {
                let _ = tokio::time::timeout(Duration::from_millis(500), socket.send(WebSocketFrame::Closed { code: None, reason: String::new() })).await;
                return Ok(());
            }
            message = messages.recv() => {
                let Some(message) = message else { return Ok(()) };
                let (kind, payload) = match &message {
                    WebSocketFrame::Text(value) => (5, value.as_bytes().to_vec()),
                    WebSocketFrame::Binary(value) => (6, value.clone()),
                    WebSocketFrame::Closed { .. } | WebSocketFrame::Ping(_) | WebSocketFrame::Pong(_) => return Ok(()),
                };
                tokio::select! {
                    () = cancellation.cancelled() => return Ok(()),
                    result = socket.send(message) => result.map_err(|error| error.to_string())?,
                }
                if !emit(callback, address, kind, &payload) { return Err("Messages arrived faster than they could be displayed.".into()); }
            }
            result = socket.next() => match result.map_err(|error| error.to_string())? {
                Some(WebSocketFrame::Text(value)) => { if !emit(callback, address, 1, value.as_bytes()) { return Err("Messages arrived faster than they could be displayed.".into()); } }
                Some(WebSocketFrame::Binary(value)) => { if !emit(callback, address, 2, &value) { return Err("Messages arrived faster than they could be displayed.".into()); } }
                Some(WebSocketFrame::Ping(value)) => {
                    if !emit(callback, address, 7, &value) || !emit(callback, address, 9, &value) { return Ok(()); }
                }
                Some(WebSocketFrame::Pong(value)) => { if !emit(callback, address, 8, &value) { return Ok(()); } }
                Some(WebSocketFrame::Closed { .. }) | None => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, net::TcpListener, thread};

    extern "C" fn collect(context: *mut c_void, kind: u8, bytes: *const u8, length: usize) -> u8 {
        // SAFETY: Each test retains this sender until free has joined the worker.
        let sender = unsafe { &*context.cast::<mpsc::Sender<(u8, Vec<u8>)>>() };
        // SAFETY: The ABI supplies a readable borrowed payload for this call.
        let payload = unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec();
        u8::from(sender.send((kind, payload)).is_ok())
    }

    #[test]
    fn malformed_input_has_one_terminal_callback_and_can_be_freed() {
        let (sender, receiver) = mpsc::channel::<(u8, Vec<u8>)>();
        let context = &raw const sender;
        // SAFETY: Input and context are valid until the joined session is freed.
        let session = unsafe {
            wirebolt_socket_start(b"{".as_ptr(), 1, Some(collect), context.cast_mut().cast())
        };
        assert!(!session.is_null());
        let event = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event.0, 4);
        // SAFETY: This is the unique owner; free joins outside the callback.
        unsafe { wirebolt_socket_free(session) };
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn cancel_interrupts_a_stalled_handshake_without_waiting_for_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted, waiting) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = [0; 4096];
            assert!(stream.read(&mut buffer).unwrap() > 0);
            accepted.send(()).unwrap();
            let _ = released.recv_timeout(Duration::from_secs(5));
        });
        let input = serde_json::to_vec(&serde_json::json!({ "method":"GET", "url":format!("ws://{address}/echo"), "total_timeout_ms":0, "workspace_proxy":{"mode":"direct"} })).unwrap();
        let (sender, receiver) = mpsc::channel::<(u8, Vec<u8>)>();
        let context = &raw const sender;
        // SAFETY: Input is copied and context remains alive until free joins.
        let session = unsafe {
            wirebolt_socket_start(
                input.as_ptr(),
                input.len(),
                Some(collect),
                context.cast_mut().cast(),
            )
        };
        assert!(!session.is_null());
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        // SAFETY: The test exclusively owns the live session.
        unsafe { wirebolt_socket_cancel(session) };
        assert_eq!(receiver.recv_timeout(Duration::from_secs(2)).unwrap().0, 3);
        // SAFETY: Ownership is returned once, outside the callback thread.
        unsafe { wirebolt_socket_free(session) };
        assert!(receiver.try_recv().is_err());
        release.send(()).unwrap();
        peer.join().unwrap();
    }

    #[test]
    fn invalid_buffers_are_rejected_before_dereference() {
        // SAFETY: Nulls and over-limit sizes must be rejected without reads.
        unsafe {
            assert!(
                wirebolt_socket_start(ptr::null(), 0, Some(collect), ptr::null_mut()).is_null()
            );
            assert!(
                wirebolt_socket_start(ptr::null(), usize::MAX, None, ptr::null_mut()).is_null()
            );
            assert_eq!(wirebolt_socket_send(ptr::null_mut(), 0, ptr::null(), 0), 0);
            wirebolt_socket_cancel(ptr::null_mut());
            wirebolt_socket_free(ptr::null_mut());
        }
    }
}
