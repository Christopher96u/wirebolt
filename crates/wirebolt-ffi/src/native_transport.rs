//! Typed adapters for isolated HTTP probes and WebSocket streams.
use crate::app_session::runtime_secrets;
use std::{sync::Arc, time::Duration};
use wirebolt_app::session::run::{
    SessionCompletion, SessionFailure, SessionHead, SessionPrepared, SessionRunEvent,
};
use wirebolt_app::{
    domain::DomainRunInput,
    response::ResponseHeaderValue,
    session::{
        runtime,
        socket_transport::SocketEvent,
        transport::{
            self, PreparedRunSnapshotDocument, RequestIssueDocument, RunFailureDocument,
            TransportObserver,
        },
    },
};
use wirebolt_core::RunCancellation;

#[uniffi::export(with_foreign)]
pub trait SessionRequestObserver: Send + Sync {
    fn event(&self, event: SessionRunEvent);
    fn bytes(&self, data: Vec<u8>) -> bool;
    fn cookies(&self, url: String, headers: Vec<ResponseHeaderValue>);
    fn finish(&self, failure: Option<SessionFailure>);
}
struct RequestObserver(Arc<dyn SessionRequestObserver>);
impl TransportObserver for RequestObserver {
    fn prepared(&self, snapshot: &PreparedRunSnapshotDocument) {
        match SessionPrepared::try_from(snapshot) {
            Ok(snapshot) => self.0.event(SessionRunEvent::Prepared { snapshot }),
            Err(_) => self.0.finish(Some(SessionFailure::new("bridge"))),
        }
    }
    fn head(&self, head: &wirebolt_core::RunHead, _: &[RequestIssueDocument]) {
        self.0.event(SessionRunEvent::Head {
            head: SessionHead::from(head),
        });
    }
    fn chunk(&self, bytes: &[u8]) -> bool {
        self.0.bytes(bytes.to_vec())
    }
    fn cookies(&self, url: &str, headers: Vec<(String, String)>) {
        self.0.cookies(
            url.into(),
            headers
                .into_iter()
                .map(|(name, value)| ResponseHeaderValue { name, value })
                .collect(),
        );
    }
    fn complete(&self, bytes_received: u64, _: u64, total_time: Duration) {
        self.0.event(SessionRunEvent::Complete {
            completion: SessionCompletion::from_transport(bytes_received, total_time),
        });
        self.0.finish(None);
    }
    fn failure(&self, failure: &RunFailureDocument) {
        self.0.finish(Some(SessionFailure::from(failure)));
    }
}
#[derive(Debug, uniffi::Object)]
pub struct AppRequestRun {
    cancellation: RunCancellation,
}
#[uniffi::export]
impl AppRequestRun {
    #[uniffi::constructor]
    pub fn start(input: DomainRunInput, observer: Arc<dyn SessionRequestObserver>) -> Arc<Self> {
        let cancellation = RunCancellation::new();
        let worker = cancellation.clone();
        if let Some(runtime) = crate::SHARED_RUNTIME.as_ref() {
            runtime.spawn(async move {
                let terminal = observer.clone();
                let task = tokio::spawn(async move {
                    match runtime::transport_input(input) {
                        Ok(input) => {
                            transport::execute(
                                input,
                                worker,
                                Arc::new(RequestObserver(observer)),
                                runtime_secrets(),
                            )
                            .await;
                        }
                        Err(failure) => observer.finish(Some(failure)),
                    }
                });
                if task.await.is_err() {
                    terminal.finish(Some(SessionFailure::new("internal")));
                }
            });
        } else {
            observer.finish(Some(SessionFailure::new("bridge")));
        }
        Arc::new(Self { cancellation })
    }
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}
impl Drop for AppRequestRun {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[uniffi::remote(Enum)]
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
#[uniffi::export(with_foreign)]
pub trait SessionSocketObserver: Send + Sync {
    fn event(&self, event: SocketEvent) -> bool;
    fn finish(&self, reason: Option<String>);
}
#[derive(Debug, uniffi::Object)]
pub struct AppSocketConnection {
    sender: tokio::sync::mpsc::Sender<wirebolt_core::WebSocketFrame>,
    cancellation: tokio_util::sync::CancellationToken,
}
#[uniffi::export]
impl AppSocketConnection {
    #[uniffi::constructor]
    pub fn start(input: DomainRunInput, observer: Arc<dyn SessionSocketObserver>) -> Arc<Self> {
        let cancellation = tokio_util::sync::CancellationToken::new();
        let worker = cancellation.clone();
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        if let Some(runtime) = crate::SHARED_RUNTIME.as_ref() {
            runtime.spawn(async move {
                let terminal = observer.clone();
                let task = tokio::spawn(wirebolt_app::session::socket_transport::run_websocket(
                    input,
                    receiver,
                    worker,
                    move |event| observer.event(event),
                    runtime_secrets(),
                ));
                let reason = match task.await {
                    Ok(result) => result.err(),
                    Err(_) => Some("The WebSocket connection failed.".into()),
                };
                terminal.finish(reason);
            });
        } else {
            observer.finish(Some("The connection could not be started.".into()));
        }
        Arc::new(Self {
            sender,
            cancellation,
        })
    }
    pub fn send(&self, data: Vec<u8>, binary: bool) -> bool {
        if self.cancellation.is_cancelled()
            || data.len() > wirebolt_core::WebSocketConnection::MAX_MESSAGE_BYTES
        {
            return false;
        }
        let frame = if binary {
            wirebolt_core::WebSocketFrame::Binary(data)
        } else {
            let Ok(text) = String::from_utf8(data) else {
                return false;
            };
            wirebolt_core::WebSocketFrame::Text(text)
        };
        self.sender.try_send(frame).is_ok()
    }
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}
impl Drop for AppSocketConnection {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use std::{io::Read, net::TcpListener, thread};

    use std::sync::mpsc;
    struct Collector(mpsc::Sender<Option<String>>);
    impl SessionSocketObserver for Collector {
        fn event(&self, _: SocketEvent) -> bool {
            true
        }
        fn finish(&self, reason: Option<String>) {
            let _ = self.0.send(reason);
        }
    }
    fn input(url: String) -> DomainRunInput {
        wirebolt_app::domain::request::run_input(
            wirebolt_app::domain::DomainRequestDraft {
                url,
                ..Default::default()
            },
            std::collections::HashMap::default(),
            Some(wirebolt_app::domain::DomainProxy::Direct),
            None,
        )
    }
    #[test]
    fn websocket_handshake_uses_the_app_manual_proxy() {
        use std::io::Write;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let peer = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "proxy handshake never arrived"
                );
                thread::sleep(Duration::from_millis(5));
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut buffer = [0; 4096];
            let count = stream.read(&mut buffer).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            String::from_utf8_lossy(&buffer[..count]).into_owned()
        });
        let mut input = input("ws://127.0.0.1:9/socket".into());
        input.workspace_proxy = None;
        input.app_proxy = Some(wirebolt_app::domain::DomainProxy::Manual {
            routes: vec![wirebolt_app::domain::DomainProxyRoute {
                destination: "all".into(),
                endpoint: format!("http://{address}"),
                credentials: None,
            }],
        });
        let (sender, receiver) = mpsc::channel();
        let session = AppSocketConnection::start(input, Arc::new(Collector(sender)));
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .is_some()
        );
        drop(session);
        let request = peer.join().unwrap();
        assert!(request.starts_with("GET http://127.0.0.1:9/socket HTTP/1.1"));
        assert!(request.to_lowercase().contains("upgrade: websocket"));
    }

    #[test]
    fn invalid_connection_has_one_terminal_callback() {
        let (sender, receiver) = mpsc::channel();
        let session =
            AppSocketConnection::start(input("not a URL".into()), Arc::new(Collector(sender)));
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .is_some()
        );
        drop(session);
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
        let mut input = input(format!("ws://{address}/echo"));
        input.total_timeout_ms = 0;
        let (sender, receiver) = mpsc::channel();
        let session = AppSocketConnection::start(input, Arc::new(Collector(sender)));
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        session.cancel();
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .is_none()
        );
        drop(session);
        assert!(receiver.try_recv().is_err());
        release.send(()).unwrap();
        peer.join().unwrap();
    }

    #[test]
    fn typed_socket_rejects_invalid_text_and_oversized_or_closed_messages() {
        let (sender, _receiver) = tokio::sync::mpsc::channel(4);
        let session = AppSocketConnection {
            sender,
            cancellation: tokio_util::sync::CancellationToken::new(),
        };
        assert!(!session.send(vec![0xff], false));
        assert!(!session.send(
            vec![0; wirebolt_core::WebSocketConnection::MAX_MESSAGE_BYTES + 1],
            true
        ));
        session.cancel();
        assert!(!session.send(vec![], true));
    }
}
