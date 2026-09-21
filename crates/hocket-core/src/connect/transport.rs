//! The network seam: a [`Transport`] that opens WebSocket connections and a
//! LAN listener that accepts them, both speaking [`WireMessage`] frames.
//!
//! The engine never touches sockets: the actor pumps a [`Connection`]'s
//! channels into `Input::WireIn` / out of `Output::WireOut`. The real
//! implementation is tokio-tungstenite over rustls; the simulation provides
//! an in-memory one (`sim::network`). [`Backoff`] is the pure reconnect
//! schedule the engine uses.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::connect::wire::{WireError, WireMessage};
use crate::connect::PeerId;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("connect {url}: {error}")]
    Connect { url: String, error: String },
    #[error("bind {addr}: {error}")]
    Bind { addr: String, error: String },
    #[error("wire: {0}")]
    Wire(#[from] WireError),
    #[error("connection closed")]
    Closed,
    #[error("no candidate address succeeded")]
    NoCandidate,
}

/// One live connection: send frames on `tx`, receive on `rx`. Dropping `tx`
/// closes it; `rx` ends when the peer goes away.
#[derive(Debug)]
pub struct Connection {
    pub peer: PeerId,
    /// The URL that worked (client side) or the remote address (server side).
    pub url: String,
    pub tx: mpsc::UnboundedSender<WireMessage>,
    pub rx: mpsc::UnboundedReceiver<WireMessage>,
}

pub type ConnectFuture = Pin<Box<dyn Future<Output = Result<Connection, TransportError>> + Send>>;

/// Opens connections. Object-safe so the actor can hold a `Box<dyn Transport>`.
pub trait Transport: Send + Sync {
    /// Open `url` as a new connection with the given peer id.
    fn connect(&self, peer: PeerId, url: String) -> ConnectFuture;
}

/// Try candidate URLs in order (last-known address first) and return the
/// first that connects.
pub async fn connect_first(
    transport: &dyn Transport,
    peer: PeerId,
    candidates: &[String],
) -> Result<Connection, TransportError> {
    let mut last = TransportError::NoCandidate;
    for url in candidates {
        match transport.connect(peer.clone(), url.clone()).await {
            Ok(c) => return Ok(c),
            Err(e) => {
                tracing::debug!(url, error = %e, "candidate failed");
                last = e;
            }
        }
    }
    Err(last)
}

/// Exponential backoff with a cap: 1 s, 2 s, 4 s … 30 s. Pure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Backoff {
    pub base_ms: f64,
    pub max_ms: f64,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            base_ms: 1_000.0,
            max_ms: 30_000.0,
        }
    }
}

impl Backoff {
    /// Delay before attempt `attempt` (0-based).
    pub fn delay_ms(&self, attempt: u32) -> f64 {
        let pow = 2f64.powi(attempt.min(16) as i32);
        (self.base_ms * pow).min(self.max_ms)
    }
}

/// Monotonic peer id source shared by the client and listener sides.
#[derive(Debug, Default)]
pub struct PeerIds {
    next: AtomicU64,
}

impl PeerIds {
    pub fn next(&self, prefix: &str) -> PeerId {
        let n = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{prefix}-{n}")
    }
}

/// tokio-tungstenite over rustls (`wss://`) or plain TCP (`ws://`).
#[derive(Debug, Default)]
pub struct WsTransport;

impl Transport for WsTransport {
    fn connect(&self, peer: PeerId, url: String) -> ConnectFuture {
        Box::pin(async move {
            let (ws, _resp) = tokio_tungstenite::connect_async(url.as_str())
                .await
                .map_err(|e| TransportError::Connect {
                    url: url.clone(),
                    error: e.to_string(),
                })?;
            Ok(pump(peer, url, ws))
        })
    }
}

/// Turn any WebSocket stream into a [`Connection`] by pumping frames both
/// ways on background tasks. Used by the client side and the LAN listener.
pub fn pump<S>(peer: PeerId, url: String, ws: tokio_tungstenite::WebSocketStream<S>) -> Connection
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio_tungstenite::tungstenite::Message;
    let (mut sink, mut stream) = ws.split();
    let (in_tx, in_rx) = mpsc::unbounded_channel::<WireMessage>();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<WireMessage>();
    let p1 = peer.clone();
    tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            match msg.encode() {
                Ok(text) => {
                    if sink.send(Message::text(text)).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::warn!(peer = %p1, error = %e, "unencodable frame dropped"),
            }
        }
        let _ = sink.close().await;
    });
    let p2 = peer.clone();
    tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            match frame {
                Ok(Message::Text(text)) => match WireMessage::decode(&text) {
                    Ok(m) => {
                        if in_tx.send(m).is_err() {
                            break;
                        }
                    }
                    Err(e) => tracing::warn!(peer = %p2, error = %e, "bad frame ignored"),
                },
                Ok(Message::Binary(b)) => match std::str::from_utf8(&b)
                    .ok()
                    .and_then(|t| WireMessage::decode(t).ok())
                {
                    Some(m) => {
                        if in_tx.send(m).is_err() {
                            break;
                        }
                    }
                    None => tracing::warn!(peer = %p2, "bad binary frame ignored"),
                },
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });
    Connection {
        peer,
        url,
        tx: out_tx,
        rx: in_rx,
    }
}

/// A tiny WebSocket server on an ephemeral (or given) port, for peers on the
/// LAN to connect straight to the elected coordinator.
#[derive(Debug)]
pub struct LanListener {
    port: u16,
    accepted: mpsc::UnboundedReceiver<Connection>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl LanListener {
    /// Bind `0.0.0.0:port` (`0` for ephemeral) and start accepting.
    pub async fn bind(port: u16, ids: Arc<PeerIds>) -> Result<LanListener, TransportError> {
        let addr = format!("0.0.0.0:{port}");
        let listener =
            tokio::net::TcpListener::bind(&addr)
                .await
                .map_err(|e| TransportError::Bind {
                    addr: addr.clone(),
                    error: e.to_string(),
                })?;
        let port = listener
            .local_addr()
            .map_err(|e| TransportError::Bind {
                addr,
                error: e.to_string(),
            })?
            .port();
        let (tx, rx) = mpsc::unbounded_channel();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    accepted = listener.accept() => {
                        let Ok((stream, remote)) = accepted else { break };
                        let tx = tx.clone();
                        let peer = ids.next("lan");
                        tokio::spawn(async move {
                            match tokio_tungstenite::accept_async(stream).await {
                                Ok(ws) => {
                                    let conn = pump(peer, remote.to_string(), ws);
                                    let _ = tx.send(conn);
                                }
                                Err(e) => tracing::debug!(%remote, error = %e, "websocket accept failed"),
                            }
                        });
                    }
                }
            }
        });
        Ok(LanListener {
            port,
            accepted: rx,
            shutdown: Some(stop_tx),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Next accepted connection; `None` once shut down.
    pub async fn accept(&mut self) -> Option<Connection> {
        self.accepted.recv().await
    }

    pub fn shutdown(&mut self) {
        if let Some(s) = self.shutdown.take() {
            let _ = s.send(());
        }
    }
}

impl Drop for LanListener {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::wire::Msg;

    #[test]
    fn backoff_grows_and_caps() {
        let b = Backoff::default();
        assert_eq!(b.delay_ms(0), 1000.0);
        assert_eq!(b.delay_ms(1), 2000.0);
        assert_eq!(b.delay_ms(3), 8000.0);
        assert_eq!(b.delay_ms(10), 30_000.0);
        assert_eq!(b.delay_ms(u32::MAX), 30_000.0);
    }

    #[test]
    fn peer_ids_are_unique() {
        let ids = PeerIds::default();
        let a = ids.next("c");
        let b = ids.next("c");
        assert_ne!(a, b);
        assert!(a.starts_with("c-"));
    }

    #[tokio::test]
    async fn listener_and_client_exchange_frames() {
        let ids = Arc::new(PeerIds::default());
        let mut listener = LanListener::bind(0, ids.clone()).await.unwrap();
        let url = format!("ws://127.0.0.1:{}/", listener.port());
        let transport = WsTransport;
        let candidates = vec!["ws://127.0.0.1:1/".to_string(), url];
        let mut client = connect_first(&transport, ids.next("up"), &candidates)
            .await
            .unwrap();
        let mut server_side = listener.accept().await.unwrap();
        client
            .tx
            .send(WireMessage::new(Msg::ClockPing { t0: 1.0 }))
            .unwrap();
        let got = server_side.rx.recv().await.unwrap();
        assert_eq!(got.msg, Msg::ClockPing { t0: 1.0 });
        server_side
            .tx
            .send(WireMessage::new(Msg::ClockPong {
                t0: 1.0,
                t1: 2.0,
                t2: 3.0,
            }))
            .unwrap();
        let got = client.rx.recv().await.unwrap();
        assert!(matches!(got.msg, Msg::ClockPong { .. }));
        // closing the client ends the server's stream
        drop(client.tx);
        drop(client.rx);
        assert!(server_side.rx.recv().await.is_none());
        listener.shutdown();
    }

    #[tokio::test]
    async fn connect_failure_is_an_error() {
        let transport = WsTransport;
        let r = connect_first(&transport, "x".into(), &["ws://127.0.0.1:1/".to_string()]).await;
        assert!(matches!(r, Err(TransportError::Connect { .. })));
        let r = connect_first(&transport, "x".into(), &[]).await;
        assert!(matches!(r, Err(TransportError::NoCandidate)));
    }
}
