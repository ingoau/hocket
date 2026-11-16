//! Streams a remote renderer fetches itself from a provider that resolves
//! them over the network (YouTube Music on a Cast device: `docs/providers.md`,
//! Phase 5).
//!
//! The receiver gets the direct googlevideo URL. Fetching one takes a
//! network round trip, so the actor never blocks on it: a load or a
//! follow-up whose URL isn't at hand asks for it here and carries on when it
//! arrives (`Internal::RemoteStreamResolved`).
//!
//! - The current item waits (shown buffering) for its URL, then loads.
//! - The follow-up's URL is fetched as soon as it becomes the follow-up
//!   (the receiver preloads it minutes later, well inside its 6 h life).
//! - A cached URL with less than [`MIN_LIFETIME_MS`] left is fetched anew.
//! - Resuming after a pause of [`LONG_PAUSE_MS`] or more fetches both URLs
//!   anew first: the phone may be on another network by now, and the URLs
//!   are bound to the address that fetched them.
//! - A receiver failure evicts the URL, so the one retry the player makes
//!   fetches a fresh one; a second failure skips the track (the usual path).

use std::collections::HashSet;

use crate::api::*;
use crate::core::actor::Actor;
use crate::core::{ActorMsg, Internal};
use crate::provider::StreamRequest;

/// A URL handed to a receiver should outlive the track by a wide margin.
pub(crate) const MIN_LIFETIME_MS: f64 = 45.0 * 60_000.0;
/// Resuming after this long paused fetches the URLs anew.
pub(crate) const LONG_PAUSE_MS: f64 = 10.0 * 60_000.0;

/// A load waiting for its URL.
#[derive(Debug, Clone)]
pub(crate) struct RemoteWait {
    pub key: QueueKey,
    pub position_ms: Ms,
    pub play: bool,
    pub carried: Option<(Ms, EpochMs, bool)>,
}

#[derive(Default)]
pub(crate) struct RemoteStreams {
    /// `(source, track id)` being resolved.
    resolving: HashSet<(String, String)>,
    pub wait: Option<RemoteWait>,
    /// When playback last paused (a renderer resuming checks it).
    pub paused_at: Option<EpochMs>,
    /// The track whose URL just arrived: taken as it is, however short its
    /// life, so a provider handing out short-lived URLs can't loop the wait.
    just_resolved: Option<(String, String)>,
}

impl Actor {
    /// Whether `track` plays on the renderer from a URL that has to be
    /// fetched first: a renderer is set and the track's provider resolves
    /// over the network.
    pub(crate) fn remote_live(&self, track: &Track) -> bool {
        self.renderer.is_some()
            && self
                .provider_for(&track.server_id)
                .is_some_and(|p| p.resolves_live())
    }

    /// The URL to hand the receiver for `track`, when one with enough life
    /// left is at hand.
    pub(crate) fn remote_fresh(&self, track: &Track) -> Option<crate::provider::ResolvedStream> {
        let provider = self.provider_for(&track.server_id)?;
        let s = provider.cached_stream(&track.id)?;
        let just = self.remote.just_resolved.as_ref();
        if just.is_some_and(|(src, id)| *src == track.server_id && *id == track.id) {
            return Some(s);
        }
        let left = s.expires_at.map_or(f64::INFINITY, |e| e - self.now());
        (left >= MIN_LIFETIME_MS).then_some(s)
    }

    /// Fetch `track`'s URL unless a fresh one is at hand or already on its
    /// way. True when it is at hand.
    pub(crate) fn want_remote_url(&mut self, track: &Track) -> bool {
        if !self.remote_live(track) {
            return true;
        }
        if self.remote_fresh(track).is_some() {
            return true;
        }
        let Some(provider) = self.provider_for(&track.server_id) else {
            return true;
        };
        let id = (track.server_id.clone(), track.id.clone());
        if !self.remote.resolving.insert(id.clone()) {
            return false;
        }
        // One cached but too close to expiry: drop it, so the resolve fetches anew.
        provider.stream_failed(&track.id, 0);
        tracing::debug!(target: "hocket::player", track = %track.id, "fetching a stream URL for the renderer");
        let tx = self.tx.clone();
        self.spawn(async move {
            let req = StreamRequest {
                profile: None,
                offset_s: None,
                suffix: None,
            };
            let error = provider
                .resolve_stream(&id.1, &req)
                .await
                .err()
                .map(|e| e.to_string());
            let _ = tx.send(ActorMsg::Internal(Internal::RemoteStreamResolved {
                source: id.0,
                track_id: id.1,
                error,
            }));
        });
        false
    }

    /// Forget `track`'s URL, so the next load fetches another.
    pub(crate) fn evict_remote_url(&self, track: &Track) {
        if let Some(p) = self.provider_for(&track.server_id) {
            p.stream_failed(&track.id, 0);
        }
    }

    /// `load_item` for an item whose URL isn't at hand: show it loading,
    /// and load it when the URL arrives.
    pub(crate) fn wait_for_remote_url(
        &mut self,
        item: &QueueItem,
        track: Track,
        position_ms: Ms,
        play: bool,
        carried: Option<(Ms, EpochMs, bool)>,
    ) {
        tracing::debug!(target: "hocket::player", key = %item.key, "waiting for the renderer's stream URL");
        // The follow-up's too, so both arrive together.
        if let Some(next) = self
            .upcoming_playable(1)
            .first()
            .and_then(|i| self.track_for_item(i))
        {
            self.want_remote_url(&next);
        }
        if self.playback.loaded && self.playback.playing {
            // What the receiver still plays is no longer current.
            if let Err(e) = self.backend.pause() {
                self.log("debug", format!("pause: {e}"));
            }
        }
        self.remote.wait = Some(RemoteWait {
            key: item.key.clone(),
            position_ms,
            play,
            carried,
        });
        let now = self.now();
        let p = &mut self.playback;
        p.doc_key = Some(item.key.clone());
        p.backend_key = None;
        p.track = Some(track);
        p.position_ms = position_ms;
        p.position_at = now;
        p.loaded = false;
        p.playing = false;
        p.buffering = true;
        p.next = None;
        p.next_doc_key = None;
        p.load_failures = 0;
        if play {
            p.want_playing = true;
        }
        self.stamp();
        self.emit_transport();
    }

    /// A URL for the renderer arrived (or couldn't be fetched).
    pub(crate) fn on_remote_stream_resolved(
        &mut self,
        source: String,
        track_id: String,
        error: Option<String>,
    ) {
        self.remote
            .resolving
            .remove(&(source.clone(), track_id.clone()));
        if self.renderer.is_none() {
            self.remote.wait = None;
            return;
        }
        let is_track = |t: &Track| t.server_id == source && t.id == track_id;
        let current = self.doc().and_then(|d| d.current.clone());
        let waiting = match (&self.remote.wait, &current) {
            (Some(w), Some(item)) if w.key == item.key => {
                self.track_for_item(item).is_some_and(|t| is_track(&t))
            }
            _ => false,
        };
        if waiting {
            let wait = self.remote.wait.take().expect("waiting");
            let item = current.expect("current");
            if let Some(message) = error {
                tracing::info!(target: "hocket::player", key = %item.key, %message, "no stream URL for the renderer");
                // As if the receiver had failed on it: one retry, then a skip.
                self.playback.backend_key = Some(item.key.clone());
                self.on_backend_report(BackendReport::Error {
                    key: item.key,
                    message,
                    fatal: true,
                });
                return;
            }
            let failures = self.playback.load_failures;
            let play = wait.play && self.playback.want_playing;
            self.remote.just_resolved = Some((source, track_id));
            self.load_item(&item, wait.position_ms, play, wait.carried);
            self.remote.just_resolved = None;
            self.playback.load_failures = failures;
            return;
        }
        let next = self
            .upcoming_playable(1)
            .first()
            .and_then(|i| self.track_for_item(i));
        if error.is_none() && next.as_ref().is_some_and(is_track) {
            self.remote.just_resolved = Some((source, track_id));
            self.refresh_next();
            self.remote.just_resolved = None;
        }
    }

    /// Resuming on the renderer after a long pause, or with the URL it holds
    /// running out: fetch the current and next URLs anew and reload at the
    /// position. True when it did.
    pub(crate) fn refresh_remote_on_resume(&mut self) -> bool {
        if self.playback.playing || !self.playback.loaded {
            return false;
        }
        let Some(track) = self.playback.track.clone() else {
            return false;
        };
        if !self.remote_live(&track) {
            return false;
        }
        let now = self.now();
        let long_pause = self
            .remote
            .paused_at
            .is_some_and(|at| now - at >= LONG_PAUSE_MS);
        if !long_pause && self.remote_fresh(&track).is_some() {
            return false;
        }
        let Some(item) = self.doc().and_then(|d| d.current.clone()) else {
            return false;
        };
        tracing::info!(target: "hocket::player", key = %item.key, long_pause, "resuming on the renderer: fetching its URL anew");
        self.evict_remote_url(&track);
        if let Some(next) = self
            .upcoming_playable(1)
            .first()
            .and_then(|i| self.track_for_item(i))
        {
            self.evict_remote_url(&next);
        }
        self.remote.paused_at = None;
        let position = self.playback.position_now(now);
        let carried = (
            self.scrobbler.played_ms(),
            self.playback.started_at,
            self.playback.scrobbled,
        );
        self.load_item(&item, position, true, Some(carried));
        true
    }
}
