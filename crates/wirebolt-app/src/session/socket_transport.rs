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
/// Returns the existing connection, preparation or receive error.
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
