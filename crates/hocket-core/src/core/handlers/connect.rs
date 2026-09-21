//! Connect I/O on the engine's behalf: sockets, the LAN listener, discovery
//! and credential verification. Every result comes back as an `Internal`.

use tokio::sync::mpsc;

use crate::api::*;
use crate::connect::discovery::PeerAdvert;
use crate::connect::transport::Connection;
use crate::connect::wire::Credential;
use crate::connect::PeerId;
use crate::core::actor::Actor;
use crate::core::{ActorMsg, Internal};

impl Actor {
    /// Pump a connection's receive side into the actor until it closes.
    fn pump_connection(&self, conn: Connection) -> mpsc::UnboundedSender<crate::connect::wire::WireMessage> {
        let Connection { peer, tx, mut rx, .. } = conn;
        let actor_tx = self.tx.clone();
        self.rt.spawn(async move {
            while let Some(msg) = rx.recv().await {
                if actor_tx
                    .send(ActorMsg::Internal(Internal::WireIn {
                        peer: peer.clone(),
                        msg,
                    }))
                    .is_err()
                {
                    return;
                }
            }
            let _ = actor_tx.send(ActorMsg::Internal(Internal::Disconnected { peer }));
        });
        tx
    }

    pub(crate) fn io_connect(&mut self, candidates: Vec<String>) {
        let peer = self.peer_ids.next("up");
        let io = self.io.clone();
        let tx = self.tx.clone();
        let rt = self.rt.clone();
        self.spawn(async move {
            match io.connect(peer, candidates).await {
                Ok(conn) => {
                    let Connection {
                        peer,
                        url,
                        tx: send,
                        mut rx,
                    } = conn;
                    let pump_tx = tx.clone();
                    let pump_peer = peer.clone();
                    rt.spawn(async move {
                        while let Some(msg) = rx.recv().await {
                            if pump_tx
                                .send(ActorMsg::Internal(Internal::WireIn {
                                    peer: pump_peer.clone(),
                                    msg,
                                }))
                                .is_err()
                            {
                                return;
                            }
                        }
                        let _ = pump_tx.send(ActorMsg::Internal(Internal::Disconnected {
                            peer: pump_peer,
                        }));
                    });
                    let _ = tx.send(ActorMsg::Internal(Internal::Connected {
                        peer,
                        url,
                        tx: send,
                    }));
                }
                Err(e) => {
                    let _ = tx.send(ActorMsg::Internal(Internal::ConnectFailed {
                        error: e.to_string(),
                    }));
                }
            }
        });
    }

    pub(crate) fn io_start_listener(&mut self) {
        if self.listener_stop.is_some() {
            if let Some(port) = self.listener_port {
                self.queue_input(crate::connect::engine::Input::ListenerStarted { port });
            }
            return;
        }
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
        self.listener_stop = Some(stop_tx);
        let io = self.io.clone();
        let ids = self.peer_ids.clone();
        let tx = self.tx.clone();
        let rt = self.rt.clone();
        self.spawn(async move {
            match io.listen(ids).await {
                Ok(mut listener) => {
                    let port = listener.port();
                    let _ = tx.send(ActorMsg::Internal(Internal::ListenerStarted { port }));
                    // Accept loop lives until the actor stops the listener.
                    let accept_tx = tx.clone();
                    rt.spawn(async move {
                        loop {
                            let conn = tokio::select! {
                                _ = &mut stop_rx => None,
                                accepted = listener.accept() => accepted,
                            };
                            let Some(conn) = conn else { break };
                            let Connection {
                                peer,
                                tx: send,
                                mut rx,
                                ..
                            } = conn;
                            let pump_tx = accept_tx.clone();
                            let pump_peer = peer.clone();
                            tokio::spawn(async move {
                                while let Some(msg) = rx.recv().await {
                                    if pump_tx
                                        .send(ActorMsg::Internal(Internal::WireIn {
                                            peer: pump_peer.clone(),
                                            msg,
                                        }))
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                                let _ = pump_tx.send(ActorMsg::Internal(Internal::Disconnected {
                                    peer: pump_peer,
                                }));
                            });
                            if accept_tx
                                .send(ActorMsg::Internal(Internal::PeerAccepted { peer, tx: send }))
                                .is_err()
                            {
                                break;
                            }
                        }
                        listener.shutdown();
                    });
                }
                Err(e) => {
                    let _ = tx.send(ActorMsg::Internal(Internal::ListenerFailed {
                        error: e.to_string(),
                    }));
                }
            }
        });
        if self.discovery.is_none() {
            let mut d = self.io.discovery();
            let (dtx, mut drx) = mpsc::unbounded_channel();
            if let Err(e) = d.start(dtx) {
                self.log("warn", format!("discovery: {e}"));
            }
            let tx = self.tx.clone();
            self.rt.spawn(async move {
                while let Some(ev) = drx.recv().await {
                    if tx
                        .send(ActorMsg::Internal(Internal::Discovery(ev)))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            self.discovery = Some(d);
        }
    }

    pub(crate) fn io_stop_listener(&mut self) {
        if let Some(stop) = self.listener_stop.take() {
            let _ = stop.send(());
        }
        self.listener_port = None;
        if let Some(d) = &mut self.discovery {
            let _ = d.advertise(None);
        }
        self.queue_input(crate::connect::engine::Input::ListenerStopped);
    }

    pub(crate) fn io_advertise(&mut self, advert: Option<PeerAdvert>) {
        if let Some(d) = &mut self.discovery {
            if let Err(e) = d.advertise(advert) {
                self.log("debug", format!("advertise: {e}"));
            }
        }
    }

    pub(crate) fn io_verify(&mut self, peer: PeerId, credential: Credential) {
        let io = self.io.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            let ok = io.verify(credential).await;
            let _ = tx.send(ActorMsg::Internal(Internal::CredentialVerified { peer, ok }));
        });
    }

    /// Connect a preset (test) server's fake socket: unused in production.
    #[allow(dead_code)]
    pub(crate) fn adopt_connection(&mut self, conn: Connection) {
        let peer = conn.peer.clone();
        let tx = self.pump_connection(conn);
        self.conns.insert(peer, tx);
    }

    pub(crate) fn on_settings_merged(&mut self, list: Vec<Setting>) {
        let changed = self.settings.merge_remote(&list);
        if changed.is_empty() {
            return;
        }
        self.save_settings();
        for key in changed {
            if let Some(s) = self.settings.setting(&key) {
                self.emit(Event::SettingChanged { setting: s });
            }
            self.apply_setting_side_effects(&key, false);
        }
    }

    pub(crate) fn connection_state(&self) -> ConnectionState {
        self.engine
            .as_ref()
            .map(|e| e.connection_state())
            .unwrap_or_default()
    }
}
