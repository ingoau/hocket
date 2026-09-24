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
    /// `None` only if the client could not be built; verification then fails
    /// closed rather than falling back to a client that follows redirects.
    http: Option<reqwest::Client>,
}

impl RealIo {
    pub fn new(config: &CoreConfig) -> RealIo {
        RealIo {
            transport: WsTransport,
            device_id: config.device_id.clone(),
            lan: config.coordinator_listen.is_none(),
            http: verify_client().ok(),
        }
    }
}

/// Largest ping response body read (a Subsonic ping is a few hundred bytes).
pub const VERIFY_BODY_CAP: usize = 64 * 1024;

/// The client for credential pings: 10 s budget, never follows a redirect
/// (that would carry the token and salt to wherever the server points).
pub fn verify_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// The `ping` endpoint for a Subsonic base URL a peer presented: plain
/// `http(s)` with a host and no userinfo. The path is set structurally, so
/// a `#` or `?` in the base cannot swallow it.
pub fn ping_url(base: &str) -> Result<url::Url, String> {
    let mut url = url::Url::parse(base.trim()).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("server URL must be http or https".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("server URL must not carry credentials".into());
    }
    if url.host_str().map(str::is_empty).unwrap_or(true) {
        return Err("server URL needs a host".into());
    }
    url.set_fragment(None);
    url.set_query(None);
    let path = format!("{}/rest/ping.view", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

/// Proxy one Subsonic `ping` with `credential`; `true` when the server says
/// `ok`. Nothing here ever logs the request URL (its query is the credential).
pub async fn verify_ping(http: &reqwest::Client, credential: &Credential) -> bool {
    let url = match ping_url(&credential.server_url) {
        Ok(u) => u,
        Err(e) => {
            tracing::debug!(error = %e, "peer named an unusable server");
            return false;
        }
    };
    let host = url.host_str().unwrap_or("?").to_string();
    let params = credential.ping_params();
    let mut resp = match http.get(url).query(&params).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(%host, error = %e.without_url(), "credential ping failed");
            return false;
        }
    };
    if !resp.status().is_success() {
        tracing::debug!(%host, status = %resp.status(), "credential ping refused");
        return false;
    }
    if resp.content_length().unwrap_or(0) > VERIFY_BODY_CAP as u64 {
        return false;
    }
    let mut body = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(c)) => {
                if body.len() + c.len() > VERIFY_BODY_CAP {
                    tracing::debug!(%host, "credential ping response too large");
                    return false;
                }
                body.extend_from_slice(&c);
            }
            Ok(None) => break,
            Err(e) => {
                tracing::debug!(%host, error = %e.without_url(), "credential ping failed");
                return false;
            }
        }
    }
    serde_json::from_slice::<serde_json::Value>(&body)
        .map(|v| v["subsonic-response"]["status"].as_str() == Some("ok"))
        .unwrap_or(false)
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
            match http {
                Some(http) => verify_ping(&http, &credential).await,
                None => false,
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
        /// Links go through relays that [`MemoryNet::set_partitioned`] can
        /// cut (see [`MemoryNet::new_partitionable`]).
        relayed: bool,
        /// Every device is cut off from every other: no connection opens.
        partitioned: bool,
        /// Cut switches of the live relayed links.
        cuts: Vec<tokio::sync::watch::Sender<bool>>,
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

        /// A network whose links [`MemoryNet::set_partitioned`] can cut while
        /// they are open (each link runs through a relay task, so plain
        /// networks keep their direct channels).
        pub fn new_partitionable() -> Arc<MemoryNet> {
            let net = MemoryNet::default();
            net.hub.lock().relayed = true;
            Arc::new(net)
        }

        /// Partition every device from every other (open links are cut and
        /// no new connection opens; discovery adverts stay, as on a LAN
        /// whose peers still see each other's mDNS but can't reach them),
        /// or heal. Only on a [`MemoryNet::new_partitionable`] network.
        pub fn set_partitioned(&self, partitioned: bool) {
            let mut h = self.hub.lock();
            assert!(h.relayed, "set_partitioned needs new_partitionable()");
            h.partitioned = partitioned;
            if partitioned {
                for cut in h.cuts.drain(..) {
                    let _ = cut.send(true);
                }
            }
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

    /// A sender whose messages reach `to` until the link is cut; then `to`
    /// sees the link close.
    fn relay(
        to: mpsc::UnboundedSender<WireMessage>,
        mut cut: tokio::sync::watch::Receiver<bool>,
    ) -> mpsc::UnboundedSender<WireMessage> {
        let (tx, mut rx) = mpsc::unbounded_channel::<WireMessage>();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    m = rx.recv() => match m {
                        Some(m) if !*cut.borrow() => {
                            if to.send(m).is_err() {
                                break;
                            }
                        }
                        _ => break,
                    },
                    _ = cut.changed() => break,
                }
            }
        });
        tx
    }

    /// A receiver fed from `from` until the link is cut.
    fn relay_rx(
        mut from: mpsc::UnboundedReceiver<WireMessage>,
        mut cut: tokio::sync::watch::Receiver<bool>,
    ) -> mpsc::UnboundedReceiver<WireMessage> {
        let (tx, rx) = mpsc::unbounded_channel::<WireMessage>();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    m = from.recv() => match m {
                        Some(m) if !*cut.borrow() => {
                            if tx.send(m).is_err() {
                                break;
                            }
                        }
                        _ => break,
                    },
                    _ = cut.changed() => break,
                }
            }
        });
        rx
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
                    if h.partitioned {
                        last = TransportError::Connect {
                            url,
                            error: "partitioned".into(),
                        };
                        continue;
                    }
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
                    let cut = h.relayed.then(|| {
                        let (cut_tx, cut_rx) = tokio::sync::watch::channel(false);
                        h.cuts.retain(|c| !c.is_closed());
                        h.cuts.push(cut_tx);
                        cut_rx
                    });
                    drop(h);
                    let (a_tx, a_rx) = mpsc::unbounded_channel::<WireMessage>();
                    let (b_tx, b_rx) = mpsc::unbounded_channel::<WireMessage>();
                    let (a_tx, b_rx) = match cut {
                        Some(cut) => (relay(a_tx, cut.clone()), relay_rx(b_rx, cut)),
                        None => (a_tx, b_rx),
                    };
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

#[cfg(test)]
mod verify_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn credential(server_url: &str) -> Credential {
        Credential {
            server_url: server_url.into(),
            username: "u".into(),
            token: Some("tok".into()),
            salt: Some("salt".into()),
            api_key: None,
            client: "hocket".into(),
            api_version: "1.16.1".into(),
        }
    }

    /// A one-route HTTP server: every request whose path starts with
    /// `/rest/ping.view` gets `ping`, anything else gets `other`.
    async fn serve(ping: String, other: String) -> std::net::SocketAddr {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    break;
                };
                let (ping, other) = (ping.clone(), other.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let resp = if req.starts_with("GET /rest/ping.view") {
                        ping
                    } else {
                        other
                    };
                    let _ = s.write_all(resp.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        addr
    }

    fn ok_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    const OK: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#;

    #[test]
    fn ping_url_accepts_only_plain_http_without_userinfo() {
        for bad in [
            "ftp://music.example/",
            "file:///etc/passwd",
            "http://user:pw@music.example/",
            "https://user@music.example/",
            "not a url",
        ] {
            assert!(ping_url(bad).is_err(), "{bad}");
        }
        assert_eq!(
            ping_url("http://10.0.0.2:4533/nav/#x?y=1")
                .unwrap()
                .as_str(),
            "http://10.0.0.2:4533/nav/rest/ping.view"
        );
        assert_eq!(
            ping_url("https://music.example#").unwrap().path(),
            "/rest/ping.view"
        );
    }

    #[tokio::test]
    async fn verify_accepts_ok_and_follows_no_redirect() {
        let http = verify_client().unwrap();
        let good = serve(ok_response(OK), ok_response(OK)).await;
        assert!(verify_ping(&http, &credential(&format!("http://{good}/"))).await);
        // a redirect that would lead to an "ok" is not followed
        let target = format!("http://{good}/rest/ping.view");
        let redirect = format!(
            "HTTP/1.1 302 Found\r\nlocation: {target}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        );
        let bouncer = serve(redirect.clone(), redirect).await;
        assert!(!verify_ping(&http, &credential(&format!("http://{bouncer}/"))).await);
    }

    #[tokio::test]
    async fn verify_caps_the_response_body() {
        let http = verify_client().unwrap();
        let padded = format!(
            r#"{{"subsonic-response":{{"status":"ok","pad":"{}"}}}}"#,
            "x".repeat(VERIFY_BODY_CAP)
        );
        let big = serve(ok_response(&padded), ok_response(&padded)).await;
        assert!(!verify_ping(&http, &credential(&format!("http://{big}/"))).await);
        // the same without a content-length (read until close)
        let chunked = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{padded}"
        );
        let big = serve(chunked.clone(), chunked).await;
        assert!(!verify_ping(&http, &credential(&format!("http://{big}/"))).await);
    }
}
