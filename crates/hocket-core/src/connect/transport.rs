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

/// Largest frame (and message) accepted on any Connect socket. A session
/// document is usually a few tens of KiB, but one at every protocol cap (the
/// list at `session::MAX_QUEUE_LEN` entries, history at
/// `session::HISTORY_CAP`, saved queues at their byte budget, every entry
/// marked offline) is about 4 MiB; this leaves twice that. Asserted by
/// `a_document_at_every_cap_fits_a_frame`. Shared with the coordinator.
pub const MAX_FRAME_BYTES: usize = 8 << 20;
/// A socket that delivers nothing for this long is dead, whatever the
/// engine or room think (they time out at 30 s; this is the backstop).
pub const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// tungstenite configuration with the frame caps applied.
pub fn ws_config() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
}

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
            let (ws, _resp) = tokio_tungstenite::connect_async_with_config(
                url.as_str(),
                Some(ws_config()),
                false,
            )
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
///
/// Frames over [`MAX_FRAME_BYTES`] end the connection (tungstenite refuses
/// them), a silent peer is dropped after [`READ_TIMEOUT`], and once the
/// sender is dropped (the engine said `Disconnect`) the reader is aborted
/// rather than left waiting for a peer that may never answer the close.
pub fn pump<S>(peer: PeerId, url: String, ws: tokio_tungstenite::WebSocketStream<S>) -> Connection
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio_tungstenite::tungstenite::Message;
    let (mut sink, mut stream) = ws.split();
    let (in_tx, in_rx) = mpsc::unbounded_channel::<WireMessage>();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<WireMessage>();
    let p2 = peer.clone();
    let reader = tokio::spawn(async move {
        loop {
            let frame = match tokio::time::timeout(READ_TIMEOUT, stream.next()).await {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(_) => {
                    tracing::debug!(peer = %p2, "read timeout");
                    break;
                }
            };
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
        // Don't wait for the peer to acknowledge: the socket is done.
        reader.abort();
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
                            match tokio_tungstenite::accept_async_with_config(
                                stream,
                                Some(ws_config()),
                            )
                            .await
                            {
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
    async fn oversized_frames_end_the_connection() {
        let ids = Arc::new(PeerIds::default());
        let mut listener = LanListener::bind(0, ids.clone()).await.unwrap();
        let url = format!("ws://127.0.0.1:{}/", listener.port());
        // a raw client that ignores our caps
        let (mut raw, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .unwrap();
        let mut server_side = listener.accept().await.unwrap();
        let big = format!(
            r#"{{"protocolVersion":2,"msg":{{"type":"bye","data":{{"reason":"{}"}}}}}}"#,
            "x".repeat(MAX_FRAME_BYTES + 1024)
        );
        // Sent from a task: a frame this size outgrows the socket buffers, so
        // the write can stall once the listener stops reading.
        let sender = tokio::spawn(async move {
            let _ = raw
                .send(tokio_tungstenite::tungstenite::Message::text(big))
                .await;
            raw
        });
        // the listener side never delivers it and the connection ends
        assert!(server_side.rx.recv().await.is_none());
        sender.abort();
        listener.shutdown();
    }

    /// The largest session document the protocol's caps allow, with
    /// realistic key and id lengths, fits a frame whichever message carries
    /// it, with room to spare.
    #[test]
    fn a_document_at_every_cap_fits_a_frame() {
        use crate::api::{
            AutoplayProvider, ContextKind, PositionStamp, QueueContext, QueueItem, QueueSource,
            SavedQueue, SortOrder, TransportLease,
        };
        use crate::connect::wire::SessionOp;
        use crate::session::saved::{
            self, SavedQueuePolicy, DEFAULT_BYTE_CAP, SNAPSHOT_HISTORY, SNAPSHOT_MAX_ENTRIES,
        };
        use crate::session::{HISTORY_CAP, MAX_QUEUE_LEN, OFFLINE_SKIPPED_FIELD};

        // `<device uuid>-<op nonce>-<op counter>-<n>`, as `OpEntropy` makes them.
        let device = "0f8e5a2b9c7d4e1fa3b6c8d0e2f4a6b8";
        let key = |op: usize, n: usize| format!("{device}-18f3a7c2b1d-{op}-{n}");
        // Navidrome ids are 22-character base62; md5 hex (older servers) is longer.
        let track = |i: usize| format!("{i:032x}");
        let item = |i: usize| QueueItem {
            key: key(10_000 + i / 50, i % 50),
            track_id: track(i),
            source: match i % 4 {
                0 | 1 => QueueSource::Context { index: i as u32 },
                2 => QueueSource::Inserted,
                _ => QueueSource::Autoplay {
                    provider: AutoplayProvider::SonicSimilarity,
                    reason: "Because you played Some Fairly Long Artist Name".into(),
                    score: Some(0.123_456_789_012_345),
                },
            },
            unavailable: true,
            added: i % 4 == 2,
            queued: false,
            ordinal: i as u32,
        };
        let list: Vec<QueueItem> = (0..MAX_QUEUE_LEN).map(item).collect();
        let context = |label: &str, tracks: Vec<String>| QueueContext {
            server_id: "4b9a1c2d3e4f5a6b7c8d9e0f1a2b3c4d".into(),
            kind: ContextKind::AdHoc {
                label: label.into(),
            },
            label: label.into(),
            sort: SortOrder::Default,
            tracks,
        };
        let label = "A selection with a long name, the kind people give playlists".to_string();

        // Saved queues filling their byte budget: the biggest snapshots a
        // queue makes (history and upcoming at their caps, ad-hoc so the
        // track list stays), as many as fit.
        let mut queues: Vec<SavedQueue> = vec![];
        for n in 0..50 {
            let entries: Vec<QueueItem> = (0..SNAPSHOT_MAX_ENTRIES).map(item).collect();
            let mut tracks: Vec<String> = (0..SNAPSHOT_MAX_ENTRIES).map(track).collect();
            tracks[0] = format!("{n}");
            queues.push(SavedQueue {
                id: key(n, 0),
                context: context(&label, tracks),
                label: label.clone(),
                history: entries[..SNAPSHOT_HISTORY].to_vec(),
                current: Some(entries[SNAPSHOT_HISTORY].clone()),
                upcoming: entries[SNAPSHOT_HISTORY + 1..].to_vec(),
                shuffle: true,
                repeat: Default::default(),
                position_ms: 123_456,
                pinned: n % 2 == 0,
                created_at: 1_790_215_057_538.123,
                last_interacted_at: 1_790_215_057_538.123 + n as f64,
                updated_at: 1_790_215_057_538.123 + n as f64,
                track_count: SNAPSHOT_MAX_ENTRIES as u32,
                cover_art: Some("al-4b9a1c2d3e4f5a6b7c8d9e0f_6512bd43".into()),
            });
        }
        saved::enforce(
            &mut queues,
            &SavedQueuePolicy::default(),
            1_790_215_057_538.123 + 60.0,
        );
        let saved_bytes = saved::total_bytes(&queues);
        assert!(saved_bytes <= DEFAULT_BYTE_CAP);
        assert!(
            saved_bytes > DEFAULT_BYTE_CAP / 2,
            "budget filled: {saved_bytes}"
        );

        let mut doc = crate::session::new_document(
            "4b9a1c2d3e4f5a6b7c8d9e0f1a2b3c4d:someone@example.com",
            key(0, 0),
            1_790_215_057_538.123,
        );
        doc.revision = u32::MAX;
        doc.context = Some(context(&label, (0..MAX_QUEUE_LEN).map(track).collect()));
        doc.history = list[..HISTORY_CAP].to_vec();
        doc.current = Some(list[HISTORY_CAP].clone());
        doc.upcoming = list[HISTORY_CAP + 1..].to_vec();
        doc.shuffle = true;
        doc.transport = crate::api::TransportState {
            lease: TransportLease {
                owner: Some(device.into()),
                epoch: u32::MAX,
                expires_at: 1_790_215_057_538.123,
            },
            position: PositionStamp {
                position_ms: 123_456,
                taken_at: 1_790_215_057_538.123,
                rate: 1.0,
                is_playing: true,
            },
            buffering: false,
            played_ms: 123_456,
            volume: 0.123_456_789,
            muted: false,
            volume_device: Some(device.into()),
        };
        doc.saved_queues = queues;
        // Every entry marked unavailable while offline.
        let offline: Vec<&str> = list.iter().map(|i| i.key.as_str()).collect();
        doc.extra.insert(
            OFFLINE_SKIPPED_FIELD.into(),
            serde_json::to_string(&offline).unwrap(),
        );

        let frames = [
            WireMessage::new(Msg::Document {
                document: doc.clone(),
            }),
            WireMessage::new(Msg::Op {
                base_revision: u32::MAX,
                op: SessionOp::Replace {
                    document: doc.clone(),
                },
                device_id: device.into(),
                op_id: key(u32::MAX as usize, 0),
                epoch: Some(u32::MAX),
                at: 1_790_215_057_538.123,
                position_ms: 123_456,
            }),
            WireMessage::new(Msg::OpReject {
                op_id: key(u32::MAX as usize, 0),
                current_revision: u32::MAX,
                reason: crate::connect::wire::RejectReason::Stale,
                document: doc,
            }),
        ];
        for frame in frames {
            // Counted as if the saved queues filled their budget exactly.
            let bytes = frame.encode().unwrap().len() + (DEFAULT_BYTE_CAP - saved_bytes);
            assert!(
                bytes * 3 / 2 < MAX_FRAME_BYTES,
                "{bytes} bytes leave too little room under {MAX_FRAME_BYTES}"
            );
        }
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
