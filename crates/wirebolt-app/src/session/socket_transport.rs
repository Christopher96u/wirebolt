//! WebSocket execution and cancellation, independent of native callbacks.
use super::transport::prepare_protocol_run;
use crate::{domain::DomainRunInput, response::ResponseHeaderValue};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketEvent {
    Connected {
        headers: Vec<ResponseHeaderValue>,
    },
    Message {
        data: Vec<u8>,
        binary: bool,
        outgoing: bool,
        control: Option<String>,
    },
}
fn socket_message(kind: u8, data: &[u8]) -> SocketEvent {
    SocketEvent::Message {
        data: data.to_vec(),
        binary: matches!(kind, 2 | 6..=9),
        outgoing: matches!(kind, 5 | 6 | 9),
        control: match kind {
            7 => Some("Ping".into()),
            8 | 9 => Some("Pong".into()),
            _ => None,
        },
    }
}
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use wirebolt_core::{RequestBody, WebSocketConnection, WebSocketFrame};
/// # Errors
/// Returns a connection, request-preparation or receive error.
pub async fn run_websocket<R>(
    input: DomainRunInput,
    mut messages: Receiver<WebSocketFrame>,
    cancellation: CancellationToken,
    emit: impl Fn(SocketEvent) -> bool + Send + Sync,
    secrets: R,
) -> Result<(), String>
where
    R: wirebolt_core::SecretResolver + Send + 'static,
{
    let mut input = super::runtime::transport_input(input)
        .map_err(|_| "The connection settings are invalid.")?;
    "GET".clone_into(&mut input.method);
    input.body = RequestBody::Empty;
    let timeout = input.total_timeout_ms;
    let preparation = tokio::task::spawn_blocking(move || {
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
        .map(|header| ResponseHeaderValue {
            name: header.name.clone(),
            value: header.value.clone(),
        })
        .collect();
    if !emit(SocketEvent::Connected { headers }) {
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
                if !emit(socket_message(kind, &payload)) { return Err("Messages arrived faster than they could be displayed.".into()); }
            }
            result = socket.next() => match result.map_err(|error| error.to_string())? {
                Some(WebSocketFrame::Text(value)) => { if !emit(socket_message(1, value.as_bytes())) { return Err("Messages arrived faster than they could be displayed.".into()); } }
                Some(WebSocketFrame::Binary(value)) => { if !emit(socket_message(2, &value)) { return Err("Messages arrived faster than they could be displayed.".into()); } }
                Some(WebSocketFrame::Ping(value)) => {
                    if !emit(socket_message(7, &value)) || !emit(socket_message(9, &value)) { return Ok(()); }
                }
                Some(WebSocketFrame::Pong(value)) => { if !emit(socket_message(8, &value)) { return Ok(()); } }
                Some(WebSocketFrame::Closed { .. }) | None => return Ok(()),
            }
        }
    }
}

/// Updates the retained socket in Rust and publishes at most one delta per frame.
///
/// # Errors
/// Returns the connection, preparation or receive error after publishing its final state.
pub async fn run_websocket_session<R>(
    input: DomainRunInput,
    messages: Receiver<WebSocketFrame>,
    cancellation: CancellationToken,
    state: std::sync::Arc<std::sync::Mutex<super::websocket::SessionSocket>>,
    generation: String,
    emit: impl Fn(super::websocket::SessionSocketSnapshot) -> bool + Send + Sync,
    secrets: R,
) -> Result<(), String>
where
    R: wirebolt_core::SecretResolver + Send + 'static,
{
    let dirty = std::sync::atomic::AtomicBool::new(false);
    let url = input.url.clone();
    let connection = run_websocket(
        input,
        messages,
        cancellation,
        |event| {
            let mut state = crate::lock(&state);
            if !state.accepts(&generation) {
                return false;
            }
            match event {
                SocketEvent::Connected { headers } => state.connected(&generation, headers, &url),
                SocketEvent::Message {
                    data,
                    binary,
                    outgoing,
                    control,
                } => state.append(&generation, data, binary, outgoing, control),
            }
            dirty.store(true, std::sync::atomic::Ordering::Release);
            true
        },
        secrets,
    );
    publish_connection(connection, &state, &generation, &dirty, emit).await
}
async fn publish_connection(
    connection: impl std::future::Future<Output = Result<(), String>>,
    state: &std::sync::Mutex<super::websocket::SessionSocket>,
    generation: &str,
    dirty: &std::sync::atomic::AtomicBool,
    emit: impl Fn(super::websocket::SessionSocketSnapshot) -> bool,
) -> Result<(), String> {
    tokio::pin!(connection);
    let mut sequence = crate::lock(state).snapshot_since(u64::MAX).next_sequence;
    loop {
        tokio::select! {
            result = &mut connection => {
                let snapshot = {
                    let mut state = crate::lock(state);
                    if !state.accepts(generation) { return result; }
                    state.finish(generation, result.as_ref().err().cloned());
                    state.snapshot_since(sequence)
                };
                emit(snapshot);
                return result;
            }
            () = tokio::time::sleep(super::run::PUBLICATION_INTERVAL) => {
                if dirty.swap(false, std::sync::atomic::Ordering::AcqRel) {
                    let snapshot = crate::lock(state).snapshot_since(sequence);
                    if snapshot.generation != generation { return Ok(()); }
                    sequence = snapshot.next_sequence;
                    if !emit(snapshot) { return Ok(()); }
                }
            }
        }
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use crate::{
        lock,
        session::websocket::{SessionSocket, SessionSocketNotice, SessionSocketStatus},
    };
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    #[tokio::test]
    async fn bursts_update_retained_state_before_batched_publication_and_flush_on_finish() {
        let state = Mutex::new(SessionSocket::default());
        let generation = lock(&state).begin().unwrap();
        let dirty = AtomicBool::new(false);
        let updates = Mutex::new(Vec::new());
        let (published, publication) = tokio::sync::oneshot::channel();
        let published = Mutex::new(Some(published));
        let connection = async {
            lock(&state).connected(&generation, vec![], "ws://fixture");
            for i in 0_u32..1500 {
                lock(&state).append(
                    &generation,
                    i.to_le_bytes().to_vec(),
                    true,
                    i % 2 == 0,
                    None,
                );
                dirty.store(true, Ordering::Release);
            }
            assert_eq!(lock(&state).snapshot().messages.len(), 1000);
            tokio::time::timeout(Duration::from_secs(5), publication)
                .await
                .unwrap()
                .unwrap();
            Err("Connection reset".into())
        };
        assert!(
            publish_connection(connection, &state, &generation, &dirty, |snapshot| {
                lock(&updates).push(snapshot);
                if let Some(published) = lock(&published).take() {
                    let _ = published.send(());
                }
                true
            })
            .await
            .is_err()
        );
        let updates = lock(&updates);
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].status, SessionSocketStatus::Connected);
        assert_eq!(updates[0].messages.len(), 1000);
        assert_eq!(updates[0].messages[0].data, 500_u32.to_le_bytes());
        assert_eq!(updates[1].messages.len(), 1);
        assert_eq!(
            updates[1].messages[0].notice,
            Some(SessionSocketNotice::Failed)
        );
        assert_eq!(updates[1].status, SessionSocketStatus::Disconnected);
        assert_eq!(updates[1].error.as_deref(), Some("Connection reset"));
    }
    #[tokio::test]
    async fn disconnected_generations_cannot_publish_late_completion() {
        let state = Mutex::new(SessionSocket::default());
        let generation = lock(&state).begin().unwrap();
        lock(&state).disconnect();
        publish_connection(
            async { Ok(()) },
            &state,
            &generation,
            &AtomicBool::new(true),
            |_| panic!("stale publication"),
        )
        .await
        .unwrap();
    }
}
