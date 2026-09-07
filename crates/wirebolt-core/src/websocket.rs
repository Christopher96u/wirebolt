use std::{error::Error, fmt};

use futures_util::{SinkExt, StreamExt};
use http::{HeaderValue, StatusCode, Version, header};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        handshake::{client::generate_key, derive_accept_key},
        protocol::{Role, WebSocketConfig},
    },
};

use crate::{HttpEngine, PreparedRequest};

/// A bounded message, exposed only to the active connection's viewer.
#[derive(Clone, Eq, PartialEq)]
pub enum WebSocketFrame {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Closed { code: Option<u16>, reason: String },
}

impl fmt::Debug for WebSocketFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => f.debug_tuple("Text").field(&text.len()).finish(),
            Self::Binary(bytes) => f.debug_tuple("Binary").field(&bytes.len()).finish(),
            Self::Ping(bytes) => f.debug_tuple("Ping").field(&bytes.len()).finish(),
            Self::Pong(bytes) => f.debug_tuple("Pong").field(&bytes.len()).finish(),
            Self::Closed { code, .. } => f.debug_tuple("Closed").field(code).finish(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketError {
    Handshake,
    Connection,
    MessageTooLarge,
}

impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Handshake => "The server did not accept the WebSocket handshake.",
            Self::Connection => "The WebSocket connection failed.",
            Self::MessageTooLarge => "The message exceeds the 16 MB limit.",
        })
    }
}

impl Error for WebSocketError {}

pub struct WebSocketConnection {
    socket: WebSocketStream<reqwest::Upgraded>,
    pub handshake_headers: Vec<crate::HeaderField>,
}

impl fmt::Debug for WebSocketConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSocketConnection")
            .finish_non_exhaustive()
    }
}

impl WebSocketConnection {
    pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

    /// Upgrades the prepared HTTP equivalent of a ws/wss URL using the same
    /// client, certificate policy, proxy and resolved auth as HTTP requests.
    ///
    /// # Errors
    /// Returns a value-free failure when transport or handshake validation fails.
    pub async fn connect(
        engine: &HttpEngine,
        request: &PreparedRequest,
    ) -> Result<Self, WebSocketError> {
        let key = generate_key();
        let mut headers = request.headers().clone();
        headers.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
        headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
        headers.insert(
            header::SEC_WEBSOCKET_VERSION,
            HeaderValue::from_static("13"),
        );
        headers.insert(
            header::SEC_WEBSOCKET_KEY,
            HeaderValue::from_str(&key).map_err(|_| WebSocketError::Handshake)?,
        );
        headers.remove(header::CONTENT_LENGTH);
        headers.remove(header::CONTENT_TYPE);
        let response = engine
            .client()
            .get(request.url().clone())
            .version(Version::HTTP_11)
            .headers(headers)
            .send()
            .await
            .map_err(|_| WebSocketError::Connection)?;
        let contains_token = |name, token: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|value| {
                    value
                        .split(',')
                        .any(|part| part.trim().eq_ignore_ascii_case(token))
                })
        };
        let accept = derive_accept_key(key.as_bytes());
        if response.status() != StatusCode::SWITCHING_PROTOCOLS
            || !contains_token(header::CONNECTION, "upgrade")
            || !contains_token(header::UPGRADE, "websocket")
            || response
                .headers()
                .get(header::SEC_WEBSOCKET_ACCEPT)
                .and_then(|v| v.to_str().ok())
                != Some(accept.as_str())
            || response
                .headers()
                .contains_key(header::SEC_WEBSOCKET_EXTENSIONS)
        {
            return Err(WebSocketError::Handshake);
        }
        if let Some(selected) = response.headers().get(header::SEC_WEBSOCKET_PROTOCOL) {
            let offered = request
                .headers()
                .get(header::SEC_WEBSOCKET_PROTOCOL)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            let selected = selected.to_str().map_err(|_| WebSocketError::Handshake)?;
            if !offered.split(',').any(|value| value.trim() == selected) {
                return Err(WebSocketError::Handshake);
            }
        }
        let handshake_headers = response
            .headers()
            .iter()
            .map(|(name, value)| crate::HeaderField {
                name: name.to_string(),
                value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
            })
            .collect();
        let upgraded = response
            .upgrade()
            .await
            .map_err(|_| WebSocketError::Connection)?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(Self::MAX_MESSAGE_BYTES))
            .max_frame_size(Some(Self::MAX_MESSAGE_BYTES));
        Ok(Self {
            handshake_headers,
            socket: WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config)).await,
        })
    }

    /// # Errors
    /// Returns a value-free failure for oversize messages or broken connections.
    pub async fn send(&mut self, message: WebSocketFrame) -> Result<(), WebSocketError> {
        let message = match message {
            WebSocketFrame::Text(value) if value.len() <= Self::MAX_MESSAGE_BYTES => {
                Message::Text(value.into())
            }
            WebSocketFrame::Binary(value) if value.len() <= Self::MAX_MESSAGE_BYTES => {
                Message::Binary(value.into())
            }
            WebSocketFrame::Ping(value) if value.len() <= 125 => Message::Ping(value.into()),
            WebSocketFrame::Pong(value) if value.len() <= 125 => Message::Pong(value.into()),
            WebSocketFrame::Closed { .. } => Message::Close(None),
            _ => return Err(WebSocketError::MessageTooLarge),
        };
        self.socket
            .send(message)
            .await
            .map_err(|_| WebSocketError::Connection)
    }

    /// Polling is cancellation-safe; control frames are handled by the protocol.
    ///
    /// # Errors
    /// Returns a value-free failure if the peer sends an invalid frame.
    pub async fn next(&mut self) -> Result<Option<WebSocketFrame>, WebSocketError> {
        loop {
            match self.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    return Ok(Some(WebSocketFrame::Text(text.to_string())));
                }
                Some(Ok(Message::Binary(bytes))) => {
                    return Ok(Some(WebSocketFrame::Binary(bytes.to_vec())));
                }
                Some(Ok(Message::Close(close))) => {
                    self.socket
                        .flush()
                        .await
                        .map_err(|_| WebSocketError::Connection)?;
                    return Ok(Some(WebSocketFrame::Closed {
                        code: close.as_ref().map(|frame| u16::from(frame.code)),
                        reason: close.map_or_else(String::new, |frame| frame.reason.to_string()),
                    }));
                }
                Some(Ok(Message::Pong(bytes))) => {
                    return Ok(Some(WebSocketFrame::Pong(bytes.to_vec())));
                }
                Some(Ok(Message::Ping(bytes))) => {
                    self.socket
                        .flush()
                        .await
                        .map_err(|_| WebSocketError::Connection)?;
                    return Ok(Some(WebSocketFrame::Ping(bytes.to_vec())));
                }
                Some(Ok(_)) => {}
                Some(Err(_)) => return Err(WebSocketError::Connection),
                None => return Ok(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HttpEngineConfig, RequestDraft, prepare_request};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn exchanges_text_binary_and_close_with_a_real_peer() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            for _ in 0..2 {
                let frame = socket.next().await.unwrap().unwrap();
                socket.send(frame).await.unwrap();
            }
            socket.send(Message::Ping(vec![42].into())).await.unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Pong(vec![42].into())
            );
            socket.close(None).await.unwrap();
            assert!(matches!(socket.next().await, Some(Ok(Message::Close(_)))));
        });
        let request = prepare_request(RequestDraft {
            method: "GET".into(),
            url: format!("http://{address}/echo"),
            headers: vec![],
            body: vec![],
        })
        .unwrap();
        let engine = HttpEngine::new(&HttpEngineConfig::default()).unwrap();
        let mut socket = WebSocketConnection::connect(&engine, &request)
            .await
            .unwrap();
        for frame in [
            WebSocketFrame::Text("Unicode café 東京".into()),
            WebSocketFrame::Binary(vec![0, 1, 255]),
        ] {
            socket.send(frame.clone()).await.unwrap();
            assert_eq!(socket.next().await.unwrap(), Some(frame));
        }
        assert_eq!(
            socket.next().await.unwrap(),
            Some(WebSocketFrame::Ping(vec![42]))
        );
        assert!(matches!(
            socket.next().await.unwrap(),
            Some(WebSocketFrame::Closed { .. })
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_accept_and_does_not_expose_response_values() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            stream.write_all(b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: secret-peer-value\r\n\r\n").await.unwrap();
        });
        let request = prepare_request(RequestDraft {
            method: "GET".into(),
            url: format!("http://{address}"),
            headers: vec![],
            body: vec![],
        })
        .unwrap();
        let engine = HttpEngine::new(&HttpEngineConfig::default()).unwrap();
        let error = WebSocketConnection::connect(&engine, &request)
            .await
            .unwrap_err();
        assert_eq!(error, WebSocketError::Handshake);
        assert!(!error.to_string().contains("secret-peer-value"));
        server.await.unwrap();
    }
}
