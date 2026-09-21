//! LAN discovery: advertise and browse `_hocket._tcp` over mDNS.
//!
//! Each device advertises one service instance named by its device id with
//! TXT records `id`, `name`, `platform`, `scope`, `port`, `proto`, `rev`,
//! `serving`. Peers of the same scope hash feed the election
//! ([`crate::connect::election`]); the winner's advertised address and port
//! form the room URL. The last-known address is tried first
//! ([`PeerAdvert::urls`]).
//!
//! Behind the [`Discovery`] trait so the simulation can fake it. The real
//! implementation ([`MdnsDiscovery`], feature `mdns`) wraps `mdns-sd` and is
//! off on the headless coordinator.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::api::{DeviceId, Platform};

/// The DNS-SD service type.
pub const SERVICE_TYPE: &str = "_hocket._tcp.local.";

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("mdns: {0}")]
    Mdns(String),
    #[error("discovery not started")]
    NotStarted,
}

/// What a peer advertises. Everything the election and the connect step need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerAdvert {
    pub device_id: DeviceId,
    pub device_name: String,
    pub platform: Platform,
    /// Hash of the session scope; peers of other scopes are ignored.
    pub scope_hash: String,
    /// LAN listener port.
    pub port: u16,
    pub protocol: u32,
    pub session_revision: u32,
    /// Serving a room with members right now.
    pub serving: bool,
    /// Resolved addresses, most recently seen first.
    pub addresses: Vec<String>,
}

/// Stable, non-reversible scope hash for the TXT record (the scope contains
/// the server URL and username, which don't belong in a multicast packet).
pub fn scope_hash(scope: &str) -> String {
    use md5::{Digest, Md5};
    let d = Md5::digest(scope.as_bytes());
    hex::encode(&d[..8])
}

impl PeerAdvert {
    /// TXT records.
    pub fn txt(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("id".into(), self.device_id.clone());
        m.insert("name".into(), self.device_name.clone());
        m.insert("platform".into(), platform_str(self.platform).into());
        m.insert("scope".into(), self.scope_hash.clone());
        m.insert("port".into(), self.port.to_string());
        m.insert("proto".into(), self.protocol.to_string());
        m.insert("rev".into(), self.session_revision.to_string());
        m.insert(
            "serving".into(),
            if self.serving { "1" } else { "0" }.into(),
        );
        m
    }

    /// Parse TXT records plus the resolved addresses/port. `None` when a
    /// required key is missing.
    pub fn from_txt(
        txt: &HashMap<String, String>,
        addresses: Vec<String>,
        port: u16,
    ) -> Option<PeerAdvert> {
        Some(PeerAdvert {
            device_id: txt.get("id")?.clone(),
            device_name: txt.get("name").cloned().unwrap_or_default(),
            platform: txt
                .get("platform")
                .and_then(|p| parse_platform(p))
                .unwrap_or(Platform::Linux),
            scope_hash: txt.get("scope")?.clone(),
            port: txt.get("port").and_then(|p| p.parse().ok()).unwrap_or(port),
            protocol: txt.get("proto").and_then(|p| p.parse().ok()).unwrap_or(1),
            session_revision: txt.get("rev").and_then(|p| p.parse().ok()).unwrap_or(0),
            serving: txt.get("serving").map(|s| s == "1").unwrap_or(false),
            addresses,
        })
    }

    /// WebSocket URLs to try, in order. A last-known good address goes first.
    pub fn urls(&self, last_known: Option<&str>) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(lk) = last_known {
            out.push(lk.to_string());
        }
        for a in &self.addresses {
            let url = if a.contains(':') && !a.starts_with('[') {
                format!("ws://[{}]:{}/", a, self.port)
            } else {
                format!("ws://{}:{}/", a, self.port)
            };
            if !out.contains(&url) {
                out.push(url);
            }
        }
        out
    }
}

pub fn platform_str(p: Platform) -> &'static str {
    match p {
        Platform::Android => "android",
        Platform::Linux => "linux",
        Platform::MacOs => "macos",
        Platform::Windows => "windows",
        Platform::Coordinator => "coordinator",
    }
}

pub fn parse_platform(s: &str) -> Option<Platform> {
    Some(match s {
        "android" => Platform::Android,
        "linux" => Platform::Linux,
        "macos" => Platform::MacOs,
        "windows" => Platform::Windows,
        "coordinator" => Platform::Coordinator,
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    /// A peer appeared or its advert changed.
    Found(PeerAdvert),
    Lost {
        device_id: DeviceId,
    },
}

/// The discovery seam. `start` begins browsing and delivers events on the
/// given channel; `advertise` publishes (or, with `None`, withdraws) our own
/// record; `stop` tears everything down.
pub trait Discovery: Send {
    fn start(
        &mut self,
        events: mpsc::UnboundedSender<DiscoveryEvent>,
    ) -> Result<(), DiscoveryError>;
    fn advertise(&mut self, advert: Option<PeerAdvert>) -> Result<(), DiscoveryError>;
    fn stop(&mut self);
}

/// A discovery that never finds anyone: the headless coordinator, or LAN
/// discovery switched off.
#[derive(Debug, Default)]
pub struct NoDiscovery;

impl Discovery for NoDiscovery {
    fn start(
        &mut self,
        _events: mpsc::UnboundedSender<DiscoveryEvent>,
    ) -> Result<(), DiscoveryError> {
        Ok(())
    }
    fn advertise(&mut self, _advert: Option<PeerAdvert>) -> Result<(), DiscoveryError> {
        Ok(())
    }
    fn stop(&mut self) {}
}

#[cfg(feature = "mdns")]
pub use mdns_impl::MdnsDiscovery;

#[cfg(feature = "mdns")]
mod mdns_impl {
    use super::*;
    use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

    /// mDNS over `mdns-sd`. Browses on a background thread that forwards
    /// resolved/removed services onto the tokio channel.
    pub struct MdnsDiscovery {
        self_id: DeviceId,
        daemon: Option<ServiceDaemon>,
        registered: Option<String>,
        stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    }

    impl MdnsDiscovery {
        pub fn new(self_id: DeviceId) -> Self {
            MdnsDiscovery {
                self_id,
                daemon: None,
                registered: None,
                stop: None,
            }
        }

        fn daemon(&mut self) -> Result<&ServiceDaemon, DiscoveryError> {
            if self.daemon.is_none() {
                let d = ServiceDaemon::new().map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
                self.daemon = Some(d);
            }
            Ok(self.daemon.as_ref().expect("just set"))
        }
    }

    impl Discovery for MdnsDiscovery {
        fn start(
            &mut self,
            events: mpsc::UnboundedSender<DiscoveryEvent>,
        ) -> Result<(), DiscoveryError> {
            let self_id = self.self_id.clone();
            let rx = self
                .daemon()?
                .browse(SERVICE_TYPE)
                .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            self.stop = Some(stop.clone());
            std::thread::Builder::new()
                .name("hocket-mdns".into())
                .spawn(move || {
                    let mut names: HashMap<String, DeviceId> = HashMap::new();
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let ev = match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                            Ok(ev) => ev,
                            Err(_) if rx.is_disconnected() => break,
                            Err(_) => continue,
                        };
                        match ev {
                            ServiceEvent::ServiceResolved(info) => {
                                if let Some(advert) = advert_from(&info) {
                                    if advert.device_id == self_id {
                                        continue;
                                    }
                                    names.insert(
                                        info.get_fullname().to_string(),
                                        advert.device_id.clone(),
                                    );
                                    if events.send(DiscoveryEvent::Found(advert)).is_err() {
                                        break;
                                    }
                                }
                            }
                            ServiceEvent::ServiceRemoved(_, fullname) => {
                                if let Some(id) = names.remove(&fullname) {
                                    if events.send(DiscoveryEvent::Lost { device_id: id }).is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                })
                .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
            Ok(())
        }

        fn advertise(&mut self, advert: Option<PeerAdvert>) -> Result<(), DiscoveryError> {
            if let Some(name) = self.registered.take() {
                if let Some(d) = &self.daemon {
                    let _ = d.unregister(&name);
                }
            }
            let Some(advert) = advert else { return Ok(()) };
            let host = format!("{}.local.", advert.device_id);
            let info = ServiceInfo::new(
                SERVICE_TYPE,
                &advert.device_id,
                &host,
                "",
                advert.port,
                advert.txt(),
            )
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))?
            .enable_addr_auto();
            let fullname = info.get_fullname().to_string();
            self.daemon()?
                .register(info)
                .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
            self.registered = Some(fullname);
            Ok(())
        }

        fn stop(&mut self) {
            if let Some(s) = self.stop.take() {
                s.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(name) = self.registered.take() {
                if let Some(d) = &self.daemon {
                    let _ = d.unregister(&name);
                }
            }
            if let Some(d) = self.daemon.take() {
                let _ = d.stop_browse(SERVICE_TYPE);
                let _ = d.shutdown();
            }
        }
    }

    fn advert_from(info: &ServiceInfo) -> Option<PeerAdvert> {
        let mut txt = HashMap::new();
        for p in info.get_properties().iter() {
            txt.insert(p.key().to_string(), p.val_str().to_string());
        }
        let mut addrs: Vec<String> = info.get_addresses().iter().map(|a| a.to_string()).collect();
        addrs.sort();
        // Prefer IPv4 first: link-local v6 needs a zone id in URLs.
        addrs.sort_by_key(|a| a.contains(':'));
        PeerAdvert::from_txt(&txt, addrs, info.get_port())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advert() -> PeerAdvert {
        PeerAdvert {
            device_id: "dev-1".into(),
            device_name: "Pixel".into(),
            platform: Platform::Android,
            scope_hash: scope_hash("https://x|u"),
            port: 4321,
            protocol: 1,
            session_revision: 12,
            serving: true,
            addresses: vec!["192.168.1.5".into(), "fe80::1".into()],
        }
    }

    #[test]
    fn txt_round_trips() {
        let a = advert();
        let txt = a.txt();
        let back = PeerAdvert::from_txt(&txt, a.addresses.clone(), 9).unwrap();
        assert_eq!(back, a);
        let mut missing = txt.clone();
        missing.remove("scope");
        assert!(PeerAdvert::from_txt(&missing, vec![], 1).is_none());
    }

    #[test]
    fn urls_try_last_known_first_and_bracket_v6() {
        let a = advert();
        let urls = a.urls(Some("ws://10.0.0.9:4321/"));
        assert_eq!(urls[0], "ws://10.0.0.9:4321/");
        assert_eq!(urls[1], "ws://192.168.1.5:4321/");
        assert_eq!(urls[2], "ws://[fe80::1]:4321/");
        let urls = a.urls(Some("ws://192.168.1.5:4321/"));
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn scope_hash_is_short_and_stable() {
        assert_eq!(scope_hash("a"), scope_hash("a"));
        assert_ne!(scope_hash("a"), scope_hash("b"));
        assert_eq!(scope_hash("a").len(), 16);
    }

    #[test]
    fn platform_strings_round_trip() {
        for p in [
            Platform::Android,
            Platform::Linux,
            Platform::MacOs,
            Platform::Windows,
            Platform::Coordinator,
        ] {
            assert_eq!(parse_platform(platform_str(p)), Some(p));
        }
        assert_eq!(parse_platform("amiga"), None);
    }

    #[test]
    fn no_discovery_is_inert() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut d = NoDiscovery;
        d.start(tx).unwrap();
        d.advertise(Some(advert())).unwrap();
        d.stop();
        assert!(rx.try_recv().is_err());
    }
}
