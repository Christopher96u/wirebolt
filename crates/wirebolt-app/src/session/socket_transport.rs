//! WebSocket execution and cancellation, independent of native callbacks.
use super::transport::{RunInput, prepare_protocol_run};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;
use wirebolt_core::{RequestBody, WebSocketConnection, WebSocketFrame};
/// # Errors
/// Returns the existing connection, preparation or receive error.
pub async fn run_websocket<R>(
    input: Vec<u8>,
    mut messages: Receiver<WebSocketFrame>,
    cancellation: CancellationToken,
    emit: impl Fn(u8, &[u8]) -> bool + Send + Sync,
    secrets: R,
) -> Result<(), String>
where
    R: wirebolt_core::SecretResolver + Send + 'static,
{
    let mut input: RunInput =
        serde_json::from_slice(&input).map_err(|_| "The connection settings are invalid.")?;
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
        .map(|header| serde_json::json!({"name": header.name, "value": header.value}))
        .collect::<Vec<_>>();
    let headers =
        serde_json::to_vec(&headers).map_err(|_| "The handshake headers could not be read.")?;
    if !emit(0, &headers) {
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
                if !emit( kind, &payload) { return Err("Messages arrived faster than they could be displayed.".into()); }
            }
            result = socket.next() => match result.map_err(|error| error.to_string())? {
                Some(WebSocketFrame::Text(value)) => { if !emit( 1, value.as_bytes()) { return Err("Messages arrived faster than they could be displayed.".into()); } }
                Some(WebSocketFrame::Binary(value)) => { if !emit( 2, &value) { return Err("Messages arrived faster than they could be displayed.".into()); } }
                Some(WebSocketFrame::Ping(value)) => {
                    if !emit( 7, &value) || !emit( 9, &value) { return Ok(()); }
                }
                Some(WebSocketFrame::Pong(value)) => { if !emit( 8, &value) { return Ok(()); } }
                Some(WebSocketFrame::Closed { .. }) | None => return Ok(()),
            }
        }
    }
}
