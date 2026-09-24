//! The network seam the actor drives the Connect engine through.
//!
//! [`ConnectIo`] bundles the three I/O jobs the engine asks for: open an
//! upstream WebSocket ([`Output::Connect`](crate::connect::Output::Connect)),
//! run the LAN listener, and publish/browse mDNS, plus the credential ping
//! the LAN coordinator role proxies. [`RealIo`] is tokio-tungstenite + the
//! `mdns` feature; [`memory::MemoryNet`] (feature `sim`) is an in-process
//! network so several cores can talk in one test without sockets.

use std::sync::Arc;

use futures::future::BoxFuture;

use crate::api::CoreConfig;
use crate::connect::discovery::{Discovery, NoDiscovery};
use crate::connect::transport::{
    connect_first, Connection, LanListener, PeerIds, TransportError, WsTransport,
};
use crate::connect::wire::Credential;
use crate::connect::PeerId;

/// A running LAN listener.
pub trait Listener: Send {
    fn port(&self) -> u16;
    /// Next accepted connection; `None` once shut down.
    fn accept(&mut self) -> BoxFuture<'_, Option<Connection>>;
    fn shutdown(&mut self);
}

/// The I/O the actor performs on the engine's behalf.
pub trait ConnectIo: Send + Sync {
    /// Try the candidate URLs in order and return the first connection.
    fn connect(
        &self,
        peer: PeerId,
        candidates: Vec<String>,
    ) -> BoxFuture<'static, Result<Connection, TransportError>>;
    /// Bind the LAN listener.
    fn listen(
        &self,
        ids: Arc<PeerIds>,
    ) -> BoxFuture<'static, Result<Box<dyn Listener>, TransportError>>;
    /// A fresh discovery instance (started by the actor).
    fn discovery(&self) -> Box<dyn Discovery>;
    /// Proxy a Subsonic `ping` with the credential; `true` when the server accepts it.
    fn verify(&self, credential: Credential) -> BoxFuture<'static, bool>;
}

/// Production I/O.
pub struct RealIo {
    transport: WsTransport,
    device_id: String,
    lan: bool,
    http: reqwest::Client,
}

impl RealIo {
    pub fn new(config: &CoreConfig) -> RealIo {
        RealIo {
            transport: WsTransport,
            device_id: config.device_id.clone(),
            lan: config.coordinator_listen.is_none(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Listener for LanListener {
    fn port(&self) -> u16 {
        LanListener::port(self)
    }
    fn accept(&mut self) -> BoxFuture<'_, Option<Connection>> {
        Box::pin(LanListener::accept(self))
    }
    fn shutdown(&mut self) {
        LanListener::shutdown(self)
    }
}

impl ConnectIo for RealIo {
    fn connect(
        &self,
        peer: PeerId,
        candidates: Vec<String>,
    ) -> BoxFuture<'static, Result<Connection, TransportError>> {
        let _ = &self.transport;
        Box::pin(async move { connect_first(&WsTransport, peer, &candidates).await })
    }

    fn listen(
        &self,
        ids: Arc<PeerIds>,
    ) -> BoxFuture<'static, Result<Box<dyn Listener>, TransportError>> {
        Box::pin(async move {
            let l = LanListener::bind(0, ids).await?;
            Ok(Box::new(l) as Box<dyn Listener>)
        })
    }

    fn discovery(&self) -> Box<dyn Discovery> {
        #[cfg(feature = "mdns")]
        {
            if self.lan {
                return Box::new(crate::connect::discovery::MdnsDiscovery::new(
                    self.device_id.clone(),
                ));
            }
        }
        let _ = (&self.device_id, self.lan);
        Box::new(NoDiscovery)
    }

    fn verify(&self, credential: Credential) -> BoxFuture<'static, bool> {
        let http = self.http.clone();
        Box::pin(async move {
            let base = credential.server_url.trim_end_matches('/').to_string();
            let url = format!("{base}/rest/ping.view");
            let params = credential.ping_params();
            match http.get(&url).query(&params).send().await {
                Ok(resp) => match resp.json::<serde_json::Value>().await {
                    Ok(v) => v["subsonic-response"]["status"].as_str() == Some("ok"),
                    Err(_) => false,
                },
                Err(e) => {
                    // `without_url`: the request URL carries token and salt.
                    tracing::debug!(%url, error = %e.without_url(), "credential ping failed");
                    false
                }
            }
        })
    }
}

/// In-process network for tests: listeners are ports on a shared hub,
/// `ws://mem:<port>/` connects to one, and adverts are broadcast to every
/// other subscriber immediately.
#[cfg(any(feature = "sim", test))]
pub mod memory {
    use super::*;
    use crate::api::DeviceId;
    use crate::connect::discovery::{DiscoveryError, DiscoveryEvent, PeerAdvert};
    use crate::connect::wire::WireMessage;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    #[derive(Default)]
    struct Hub {
        next_port: u16,
        listeners: HashMap<u16, mpsc::UnboundedSender<Connection>>,
        adverts: HashMap<DeviceId, PeerAdvert>,
        subscribers: Vec<(DeviceId, mpsc::UnboundedSender<DiscoveryEvent>)>,
        next_peer: u64,
        /// Ports whose links are cut (both directions).
        down: Vec<u16>,
    }

    /// The shared hub. Clone the `Arc` into every core's [`MemoryIo`].
    #[derive(Default)]
    pub struct MemoryNet {
        hub: Mutex<Hub>,
    }

    impl MemoryNet {
        pub fn new() -> Arc<MemoryNet> {
            Arc::new(MemoryNet::default())
        }

        /// I/O for one device on this network.
        pub fn io(self: &Arc<Self>, device_id: &str, verify_ok: bool) -> Arc<MemoryIo> {
            Arc::new(MemoryIo {
                net: self.clone(),
                device_id: device_id.to_string(),
                verify_ok,
            })
        }

        /// Refuse new connections to a listener port (existing links stay).
        pub fn set_port_down(&self, port: u16, down: bool) {
            let mut h = self.hub.lock();
            h.down.retain(|p| *p != port);
            if down {
                h.down.push(port);
            }
        }

        pub fn url_for(port: u16) -> String {
            format!("ws://mem:{port}/")
        }

        fn port_of(url: &str) -> Option<u16> {
            url.strip_prefix("ws://mem:")?
                .trim_end_matches('/')
                .parse()
                .ok()
        }
    }

    pub struct MemoryIo {
        net: Arc<MemoryNet>,
        device_id: DeviceId,
        verify_ok: bool,
    }

    struct MemoryListener {
        net: Arc<MemoryNet>,
        port: u16,
        rx: mpsc::UnboundedReceiver<Connection>,
    }

    impl Listener for MemoryListener {
        fn port(&self) -> u16 {
            self.port
        }
        fn accept(&mut self) -> BoxFuture<'_, Option<Connection>> {
            Box::pin(self.rx.recv())
        }
        fn shutdown(&mut self) {
            self.net.hub.lock().listeners.remove(&self.port);
        }
    }

    impl Drop for MemoryListener {
        fn drop(&mut self) {
            self.shutdown();
        }
    }

    struct MemoryDiscovery {
        net: Arc<MemoryNet>,
        device_id: DeviceId,
        started: bool,
    }

    impl Discovery for MemoryDiscovery {
        fn start(
            &mut self,
            events: mpsc::UnboundedSender<DiscoveryEvent>,
        ) -> Result<(), DiscoveryError> {
            let mut h = self.net.hub.lock();
            h.subscribers.retain(|(id, _)| id != &self.device_id);
            for (id, a) in &h.adverts {
                if id != &self.device_id {
                    let _ = events.send(DiscoveryEvent::Found(a.clone()));
                }
            }
            h.subscribers.push((self.device_id.clone(), events));
            self.started = true;
            Ok(())
        }

        fn advertise(&mut self, advert: Option<PeerAdvert>) -> Result<(), DiscoveryError> {
            let mut h = self.net.hub.lock();
            let event = match advert {
                Some(mut a) => {
                    a.addresses = vec!["mem".into()];
                    h.adverts.insert(self.device_id.clone(), a.clone());
                    DiscoveryEvent::Found(a)
                }
                None => {
                    h.adverts.remove(&self.device_id);
                    DiscoveryEvent::Lost {
                        device_id: self.device_id.clone(),
                    }
                }
            };
            for (id, tx) in &h.subscribers {
                if id != &self.device_id {
                    let _ = tx.send(event.clone());
                }
            }
            Ok(())
        }

        fn stop(&mut self) {
            let _ = self.advertise(None);
            self.net
                .hub
                .lock()
                .subscribers
                .retain(|(id, _)| id != &self.device_id);
            self.started = false;
        }
    }

    impl ConnectIo for MemoryIo {
        fn connect(
            &self,
            peer: PeerId,
            candidates: Vec<String>,
        ) -> BoxFuture<'static, Result<Connection, TransportError>> {
            let net = self.net.clone();
            Box::pin(async move {
                // A real connect takes a network round trip; yield so the
                // actor's ordering matches production (never re-entrant).
                tokio::task::yield_now().await;
                let mut last = TransportError::NoCandidate;
                for url in candidates {
                    let Some(port) = MemoryNet::port_of(&url) else {
                        last = TransportError::Connect {
                            url,
                            error: "not a mem:// url".into(),
                        };
                        continue;
                    };
                    let mut h = net.hub.lock();
                    if h.down.contains(&port) {
                        last = TransportError::Connect {
                            url,
                            error: "port down".into(),
                        };
                        continue;
                    }
                    let Some(acceptor) = h.listeners.get(&port).cloned() else {
                        last = TransportError::Connect {
                            url,
                            error: "nothing listening".into(),
                        };
                        continue;
                    };
                    h.next_peer += 1;
                    let server_peer = format!("mem-{}", h.next_peer);
                    drop(h);
                    let (a_tx, a_rx) = mpsc::unbounded_channel::<WireMessage>();
                    let (b_tx, b_rx) = mpsc::unbounded_channel::<WireMessage>();
                    let server_side = Connection {
                        peer: server_peer,
                        url: format!("mem://{}", peer),
                        tx: b_tx,
                        rx: a_rx,
                    };
                    if acceptor.send(server_side).is_err() {
                        last = TransportError::Connect {
                            url,
                            error: "listener gone".into(),
                        };
                        continue;
                    }
                    return Ok(Connection {
                        peer,
                        url,
                        tx: a_tx,
                        rx: b_rx,
                    });
                }
                Err(last)
            })
        }

        fn listen(
            &self,
            _ids: Arc<PeerIds>,
        ) -> BoxFuture<'static, Result<Box<dyn Listener>, TransportError>> {
            let net = self.net.clone();
            Box::pin(async move {
                tokio::task::yield_now().await;
                let (tx, rx) = mpsc::unbounded_channel();
                let port = {
                    let mut h = net.hub.lock();
                    h.next_port += 1;
                    let port = 40_000 + h.next_port;
                    h.listeners.insert(port, tx);
                    port
                };
                Ok(Box::new(MemoryListener { net, port, rx }) as Box<dyn Listener>)
            })
        }

        fn discovery(&self) -> Box<dyn Discovery> {
            Box::new(MemoryDiscovery {
                net: self.net.clone(),
                device_id: self.device_id.clone(),
                started: false,
            })
        }

        fn verify(&self, _credential: Credential) -> BoxFuture<'static, bool> {
            let ok = self.verify_ok;
            Box::pin(async move { ok })
        }
    }
}
