//! Playback: only the lease owner drives the backend; everyone else forwards
//! transport commands over Connect and renders from stamps.

use crate::api::*;
use crate::audio::dsp::gain_for_track;
use crate::audio::sleep::SleepAction;
use crate::connect::engine::Input;
use crate::connect::wire::{SessionOp, TransportCommand};
use crate::core::actor::{
    Actor, LOAD_RETRIES, MAX_CONSECUTIVE_SKIPS, MEDIA_SESSION_ART, MEDIA_SESSION_ART_SMALL,
};
use crate::core::state::{pending_scrobbles_key, PendingScrobble};
use crate::core::Internal;
use crate::db::DbResult;
use crate::media_session::{command_for, derive_media_session_state};
use crate::outbox::{ScrobbleAction, Verdict};
use crate::session::reducer::derive;

impl Actor {
    // -- loading ----------------------------------------------------------------

    /// Resolve a queue item to a media source: a downloaded file, or (for
    /// a backend that reads through the core) a `hocket-stream://` URL
    /// served from the stream cache or the server with no credentials in
    /// it; otherwise the legacy cached file or server stream URL. With
    /// ReplayGain folded into `gain_db`.
    pub(crate) fn media_source_for(&self, key: &str, track: &Track) -> Option<MediaSource> {
        let api = self.api()?;
        let reader = self
            .stream_reader
            .as_ref()
            .filter(|_| self.core_stream)
            .map(|r| r as &dyn crate::downloads::StreamMinter);
        let mut source = match self
            .downloads
            .resolve_with(api.as_ref(), key, track, reader)
        {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(target: "hocket_core", error = %e, "resolve media source");
                return None;
            }
        };
        let is_album = self
            .doc()
            .and_then(|d| d.context.as_ref())
            .is_some_and(|c| matches!(c.kind, ContextKind::Album { .. }));
        let download_gain = self
            .downloads
            .gain(&track.server_id, &track.id)
            .ok()
            .flatten();
        let rg = track.replay_gain.clone().or_else(|| {
            download_gain.map(|g| ReplayGain {
                track_gain_db: Some(g),
                ..Default::default()
            })
        });
        source.gain_db = gain_for_track(
            rg.as_ref(),
            self.audio.replay_gain,
            is_album,
            self.audio.replay_gain_preamp_db,
        );
        Some(source)
    }

    /// The item that follows the current one, for gapless preload
    /// (offline: the next one that can play offline).
    fn next_item(&self) -> Option<QueueItem> {
        let doc = self.doc()?;
        if doc.repeat == RepeatMode::One {
            return None;
        }
        let d = derive(doc);
        let offline = self.is_offline();
        d.playing_next
            .into_iter()
            .chain(d.upcoming)
            .filter(|i| !i.unavailable)
            .find(|i| {
                !offline
                    || self
                        .track_or_bare(&i.track_id)
                        .is_some_and(|t| self.available_offline(&t))
            })
    }

    pub(crate) fn refresh_next(&mut self) {
        if !self.audio.gapless {
            self.playback.next = None;
            self.playback.next_doc_key = None;
            let _ = self.backend.set_next(None);
            return;
        }
        let next = self.next_item();
        let next_key = next.as_ref().map(|i| i.key.clone());
        if next_key == self.playback.next_doc_key && self.playback.next.is_some() {
            return;
        }
        let source = next.and_then(|i| {
            let track = self.track_or_bare(&i.track_id)?;
            self.media_source_for(&i.key, &track)
        });
        self.playback.next_doc_key = next_key;
        self.playback.next = source.clone();
        if let Err(e) = self.backend.set_next(source) {
            self.log("debug", format!("set_next: {e}"));
        }
        self.protect_loaded_tracks();
    }

    /// Keep the stream-cache files of the loaded and preloaded tracks (a
    /// seek re-reads them) and of the prefetch targets until they leave the
    /// player or the next-two window.
    pub(crate) fn protect_loaded_tracks(&mut self) {
        let mut keys = vec![];
        if self.playback.loaded {
            if let Some(t) = &self.playback.track {
                keys.push((t.server_id.clone(), t.id.clone()));
            }
        }
        if let Some(n) = &self.playback.next {
            keys.push((n.track.server_id.clone(), n.track.id.clone()));
        }
        keys.extend(self.prefetch.targets.iter().cloned());
        let changed = self.downloads.set_protected(keys);
        if !changed.is_empty() {
            self.on_stream_cache_changed(changed);
        }
    }

    /// Stream-cache entries appeared or went: tell the UIs which tracks'
    /// offline state changed, and the new storage totals.
    pub(crate) fn on_stream_cache_changed(&mut self, tracks: Vec<crate::downloads::TrackKey>) {
        let mut by_server: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for (sid, tid) in tracks {
            let ids = by_server.entry(sid).or_default();
            if !ids.contains(&tid) {
                ids.push(tid);
            }
        }
        let mut all: Vec<String> = vec![];
        for (server_id, ids) in by_server {
            all.extend(ids.iter().cloned());
            self.emit(Event::LibraryChanged {
                server_id,
                tables: vec!["tracks".into()],
                ids,
            });
        }
        let storage = self.storage_summary();
        self.emit(Event::StorageChanged { storage });
        // Queue rows carry the offline state: refresh them when one of
        // these tracks is in the queue (a prefetched item shows Cached).
        let in_queue = self.doc().is_some_and(|d| {
            d.current
                .iter()
                .chain(d.insertions.iter())
                .any(|i| all.contains(&i.track_id))
                || d.context
                    .as_ref()
                    .is_some_and(|c| c.tracks.iter().any(|t| all.contains(t)))
        });
        if in_queue {
            let queue = self.queue_view();
            self.emit(Event::QueueChanged { queue });
        }
    }

    /// Load a document item into the backend. `carried` is the handoff
    /// state (played time, scrobble identity) when taking over.
    pub(crate) fn load_item(
        &mut self,
        item: &QueueItem,
        position_ms: Ms,
        play: bool,
        carried: Option<(Ms, EpochMs, bool)>,
    ) {
        let Some(track) = self.track_or_bare(&item.track_id) else {
            return;
        };
        if self.skip_if_unavailable_offline(item, &track) {
            self.playback.doc_key = Some(item.key.clone());
            self.playback.track = Some(track);
            self.playback.loaded = false;
            return;
        }
        // Never hand the backend a position past the end (a merged saved
        // queue or an early seek may carry one): it would seek to EOF and
        // skip the track.
        let position_ms = if track.duration_ms > 0 {
            position_ms.min(track.duration_ms)
        } else {
            position_ms
        };
        let now = self.now();
        // Gapless: the backend already moved on to this track.
        let transitioned = carried.is_none()
            && position_ms == 0
            && self
                .playback
                .next
                .as_ref()
                .is_some_and(|n| n.track.id == item.track_id)
            && self.playback.awaiting_transition;
        self.note_cache_signals(item, &track, transitioned, play);
        let (played_ms, started_at, scrobbled) = match carried {
            Some((p, s, sc)) => (p, s, sc),
            None => (0, self.session_now(), false),
        };
        if transitioned {
            let backend_key = self.playback.next.as_ref().map(|n| n.key.clone());
            self.playback.doc_key = Some(item.key.clone());
            self.playback.backend_key = backend_key;
            self.playback.track = Some(track.clone());
            self.playback.position_ms = 0;
            self.playback.position_at = now;
            self.playback.next = None;
            self.playback.next_doc_key = None;
            self.playback.awaiting_transition = false;
            self.playback.loaded = true;
        } else {
            let Some(source) = self.media_source_for(&item.key, &track) else {
                self.player_notice(
                    PlayerNoticeCode::NoServer,
                    "No server connection: add or reconnect your server",
                    None,
                );
                self.playback.doc_key = Some(item.key.clone());
                self.playback.track = Some(track);
                self.playback.loaded = false;
                return;
            };
            self.playback.doc_key = Some(item.key.clone());
            self.playback.backend_key = Some(item.key.clone());
            self.playback.track = Some(track.clone());
            self.playback.position_ms = position_ms;
            self.playback.position_at = now;
            self.playback.awaiting_transition = false;
            self.playback.loaded = true;
            self.playback.next = None;
            self.playback.next_doc_key = None;
            let next = self.audio.gapless.then(|| self.next_item()).flatten();
            let next_source = next.as_ref().and_then(|i| {
                let t = self.track_or_bare(&i.track_id)?;
                self.media_source_for(&i.key, &t)
            });
            self.playback.next_doc_key = next.map(|i| i.key);
            self.playback.next = next_source.clone();
            if let Err(e) = self.backend.load(source, next_source, position_ms, play) {
                self.error(ErrorKind::Playback, "load", Some(e.to_string()));
            }
        }
        self.playback.playing = play;
        self.playback.focus_suspended = false;
        self.playback.buffering = !transitioned;
        self.playback.started_at = started_at;
        self.playback.started_local = now - f64::from(played_ms);
        self.playback.scrobbled = scrobbled;
        self.playback.load_failures = 0;
        if play {
            self.playback.want_playing = true;
        }
        let actions = self.scrobbler.track_started(
            &track.id,
            track.duration_ms,
            position_ms,
            played_ms,
            play,
        );
        self.apply_scrobble_actions(actions);
        self.stamp();
        self.emit_transport();
        self.save_position();
        self.prefetch_artwork_for_current();
    }

    pub(crate) fn unload(&mut self) {
        if self.playback.loaded {
            let _ = self.backend.stop();
        }
        if let Some(id) = self.playback.track_id().map(str::to_string) {
            let pos = self.playback.position_now(self.now());
            let actions = self.scrobbler.track_ended(&id, Some(pos));
            self.apply_scrobble_actions(actions);
        }
        let volume = self.playback.volume;
        let restore = self.playback.restore_position.take();
        self.playback = crate::core::state::Playback {
            volume,
            restore_position: restore,
            ..Default::default()
        };
        self.stamp();
        self.emit_transport();
    }

    pub(crate) fn restart_current(&mut self) {
        let Some(track) = self.playback.track.clone() else {
            return;
        };
        if !self.playback.loaded {
            if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                self.load_item(&item, 0, true, None);
            }
            return;
        }
        let actions = self.scrobbler.track_repeated(&track.id, track.duration_ms);
        self.apply_scrobble_actions(actions);
        self.playback.position_ms = 0;
        self.playback.position_at = self.now();
        self.playback.started_at = self.session_now();
        self.playback.started_local = self.now();
        self.playback.scrobbled = false;
        if let Err(e) = self.backend.seek(0) {
            self.log("debug", format!("seek: {e}"));
        }
        if !self.playback.playing {
            self.playback.playing = true;
            let _ = self.backend.play();
        }
        self.stamp();
        self.emit_transport();
    }

    // -- transport ownership ------------------------------------------------------

    pub(crate) fn on_lease_changed(&mut self, owns: bool, detached: bool, lease: TransportLease) {
        // The owner prefetches what comes next, everyone else the current
        // item: a fetch the new role no longer wants stops at once.
        self.mark_prefetch_check();
        self.prefetch_tick(self.now());
        self.drop_unwanted_prefetch();
        if owns {
            self.playback.consecutive_skips = 0;
            // Taking over from a device that skipped tracks while offline:
            // here they may play.
            if self
                .doc()
                .is_some_and(|d| !crate::session::offline_skipped(d).is_empty())
            {
                let _ = self.tx.send(crate::core::ActorMsg::Internal(
                    crate::core::Internal::ClearOfflineSkips,
                ));
            }
            if !self.playback.loaded {
                if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                    let position = self.resume_position_for(&item);
                    let play = self.playback.want_playing;
                    self.load_item(&item, position, play, None);
                }
            } else if self.playback.want_playing && !self.playback.playing {
                self.set_playing(true);
            }
        } else {
            // Another device owns transport now: this backend must go quiet
            // and stop accepting late reports for what it had loaded.
            self.render_only();
        }
        if detached {
            self.log("info", "owning transport while cut off from the room");
        }
        let _ = lease;
        self.emit_transport();
        if let Some(e) = &self.engine {
            let devices = e.devices();
            self.emit(Event::DevicesChanged { devices });
        }
    }

    pub(crate) fn release_transport(&mut self) {
        self.playback.want_playing = false;
        if self.playback.loaded && self.playback.playing {
            self.set_playing(false);
        }
        self.emit_transport();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn take_transport(
        &mut self,
        key: QueueKey,
        track_id: TrackId,
        position_ms: Ms,
        played_ms: Ms,
        started_at: EpochMs,
        scrobbled: bool,
        play: bool,
    ) {
        let item = self
            .doc()
            .and_then(|d| {
                d.current
                    .iter()
                    .chain(d.history.iter())
                    .chain(d.insertions.iter())
                    .find(|i| i.key == key)
                    .cloned()
            })
            .unwrap_or(QueueItem {
                key: key.clone(),
                track_id: track_id.clone(),
                source: QueueSource::Inserted,
                unavailable: false,
            });
        self.playback.want_playing = play;
        self.load_item(
            &item,
            position_ms,
            play,
            Some((played_ms, started_at, scrobbled)),
        );
        self.clear_player_notice();
    }

    pub(crate) fn pre_buffer(&mut self, key: QueueKey, track_id: TrackId, position_ms: Ms) {
        let Some(track) = self.track_or_bare(&track_id) else {
            return;
        };
        match self.media_source_for(&key, &track) {
            Some(source) => {
                if let Err(e) = self.backend.pre_buffer(source, position_ms) {
                    self.log("debug", format!("pre_buffer: {e}"));
                    self.engine_input(Input::PreBufferFailed { key });
                }
            }
            None => self.engine_input(Input::PreBufferFailed { key }),
        }
    }

    /// A transport command from this device's UI.
    pub(crate) fn transport_command(&mut self, cmd: TransportCommand) {
        let Some(engine) = &self.engine else {
            return;
        };
        if engine.owns_transport() {
            self.apply_transport_command(cmd);
            return;
        }
        let nobody = engine.lease().owner.is_none();
        match (&cmd, nobody) {
            (TransportCommand::Play | TransportCommand::TogglePlay, true) => {
                if self.doc().is_some_and(|d| d.current.is_some()) {
                    self.playback.want_playing = true;
                    self.engine_input(Input::ClaimTransport { takeover: false });
                }
            }
            (TransportCommand::SetVolume { volume }, _) => self.set_volume(*volume, false),
            (_, true) => {}
            (_, false) => self.engine_input(Input::TransportRequest(cmd)),
        }
    }

    /// Apply a transport command on this device (we own transport).
    pub(crate) fn apply_transport_command(&mut self, cmd: TransportCommand) {
        match cmd {
            TransportCommand::Play => {
                self.playback.want_playing = true;
                self.ensure_loaded_then_play();
            }
            TransportCommand::Pause => {
                self.playback.want_playing = false;
                self.set_playing(false);
            }
            TransportCommand::TogglePlay => {
                if self.playback.playing {
                    self.playback.want_playing = false;
                    self.set_playing(false);
                } else {
                    self.playback.want_playing = true;
                    self.ensure_loaded_then_play();
                }
            }
            TransportCommand::Stop => {
                self.playback.want_playing = false;
                if self.playback.loaded {
                    self.set_playing(false);
                    self.seek_to(0);
                }
                self.engine_input(Input::ReleaseTransport);
            }
            TransportCommand::SeekTo { position_ms } => self.seek_to(position_ms),
            TransportCommand::SeekBy { delta_ms } => {
                let pos = self.playback.position_now(self.now()) as i64 + delta_ms as i64;
                let cap = self.playback.duration_ms() as i64;
                let pos = if cap > 0 {
                    pos.clamp(0, cap)
                } else {
                    pos.max(0)
                };
                self.seek_to(pos as Ms);
            }
            TransportCommand::SetVolume { volume } => self.set_volume(volume, false),
        }
    }

    fn ensure_loaded_then_play(&mut self) {
        if self.playback.loaded {
            self.set_playing(true);
        } else if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
            let position = self.resume_position_for(&item);
            self.load_item(&item, position, true, None);
        }
    }

    /// Where to start `item` when loading it fresh: a pending restore for
    /// this key, else the last known position if it was taken for this key,
    /// else the start. A position taken for another item (the queue moved on
    /// while another device played) is never reused.
    fn resume_position_for(&mut self, item: &QueueItem) -> Ms {
        if let Some((_, p)) = self
            .playback
            .restore_position
            .take_if(|(k, _)| *k == item.key)
        {
            return p;
        }
        if self.playback.doc_key.as_ref() == Some(&item.key) {
            self.playback.position_ms
        } else {
            0
        }
    }

    /// Stop driving the backend but keep rendering the current item: used
    /// when transport moves to another device. Clears the backend key so
    /// late reports for the old item no longer pass as ours.
    pub(crate) fn render_only(&mut self) {
        if self.playback.playing {
            self.set_playing(false);
        }
        if self.playback.loaded {
            if let Err(e) = self.backend.stop() {
                self.log("debug", format!("stop: {e}"));
            }
        }
        self.playback.loaded = false;
        self.playback.playing = false;
        self.playback.buffering = false;
        self.playback.backend_key = None;
        self.playback.next = None;
        self.playback.next_doc_key = None;
        self.playback.awaiting_transition = false;
    }

    pub(crate) fn set_playing(&mut self, playing: bool) {
        if !playing && self.playback.loaded && self.playback.focus_suspended {
            // Paused while focus is away: the backend must not resume later.
            self.playback.focus_suspended = false;
            if let Err(e) = self.backend.pause() {
                self.log("debug", format!("pause: {e}"));
            }
        }
        if !self.playback.loaded || self.playback.playing == playing {
            return;
        }
        self.playback.focus_suspended = false;
        let now = self.now();
        self.playback.position_ms = self.playback.position_now(now);
        self.playback.position_at = now;
        self.playback.playing = playing;
        let r = if playing {
            self.backend.play()
        } else {
            self.backend.pause()
        };
        if let Err(e) = r {
            self.error(ErrorKind::Playback, "play/pause", Some(e.to_string()));
        }
        let actions = self.scrobbler.set_playing(playing);
        self.apply_scrobble_actions(actions);
        if !playing {
            self.save_position();
        }
        self.stamp();
        self.emit_transport();
    }

    pub(crate) fn seek_to(&mut self, position_ms: Ms) {
        let cap = self.playback.duration_ms();
        let position_ms = if cap > 0 {
            position_ms.min(cap)
        } else {
            position_ms
        };
        if !self.playback.loaded {
            self.playback.position_ms = position_ms;
            return;
        }
        self.playback.position_ms = position_ms;
        self.playback.position_at = self.now();
        if let Err(e) = self.backend.seek(position_ms) {
            self.error(ErrorKind::Playback, "seek", Some(e.to_string()));
        }
        if let Some(id) = self.playback.track_id().map(str::to_string) {
            self.scrobbler.seeked(&id, position_ms);
        }
        self.stamp();
        self.emit_transport();
    }

    pub(crate) fn set_volume(&mut self, volume: f64, persist: bool) {
        let volume = crate::audio::backend::clamp_volume(volume);
        self.playback.volume = volume;
        if let Err(e) = self.backend.set_volume(self.effective_volume()) {
            self.log("debug", format!("set_volume: {e}"));
        }
        if persist {
            let _ = self
                .db
                .saved_state_set("volume", &volume, self.clock.as_ref());
        }
        self.emit_transport();
    }

    /// Send a transport stamp: only ever from here, only on change.
    pub(crate) fn stamp(&mut self) {
        if !self.owns_transport() {
            return;
        }
        let now = self.now();
        let p = &self.playback;
        let input = Input::LocalStamp {
            key: p.doc_key.clone(),
            track_id: p.track_id().map(str::to_string),
            position: PositionStamp {
                position_ms: p.position_now(now),
                taken_at: now,
                rate: 1.0,
                is_playing: p.playing && p.loaded,
            },
            played_ms: self.scrobbler.played_ms(),
            started_at: p.started_at,
            scrobbled: p.scrobbled,
        };
        self.engine_input(input);
    }

    // -- backend reports -----------------------------------------------------------

    pub(crate) fn on_backend_report(&mut self, report: BackendReport) {
        let current = |k: &QueueKey, me: &Actor| me.playback.backend_key.as_ref() == Some(k);
        if let BackendReport::TransitionedToNext { key } = &report {
            // A backend that moved on to the preloaded item without reporting
            // `Ended` for the one that finished (Media3's auto-advance): the
            // end is implied. Without it the document never advances, the
            // new item's reports are dropped as foreign, and playback stops
            // for good when it ends.
            let implied_end = !self.playback.awaiting_transition
                && !current(key, self)
                && self.playback.next.as_ref().is_some_and(|n| &n.key == key);
            if implied_end {
                if let Some(ended) = self.playback.backend_key.clone() {
                    self.on_backend_report(BackendReport::Ended { key: ended });
                }
            }
        }
        match report {
            BackendReport::Ready { key, duration_ms } => {
                if !current(&key, self) {
                    return;
                }
                if let (Some(d), Some(t)) = (duration_ms, self.playback.track.as_mut()) {
                    if t.duration_ms == 0 && d > 0 {
                        t.duration_ms = d;
                    }
                }
                self.playback.buffering = false;
                self.playback.load_failures = 0;
                self.playback.consecutive_skips = 0;
                self.clear_player_notice();
                self.emit_transport();
            }
            BackendReport::Playing { key, position_ms } => {
                if !current(&key, self) {
                    return;
                }
                let now = self.now();
                self.playback.position_ms = position_ms;
                self.playback.position_at = now;
                self.playback.buffering = false;
                self.playback.focus_suspended = false;
                if !self.playback.playing {
                    self.playback.playing = true;
                    let actions = self.scrobbler.set_playing(true);
                    self.apply_scrobble_actions(actions);
                }
                self.stamp();
                self.emit_transport();
            }
            BackendReport::Paused { key, position_ms } => {
                if !current(&key, self) {
                    return;
                }
                self.playback.position_ms = position_ms;
                self.playback.position_at = self.now();
                if self.playback.playing {
                    self.playback.playing = false;
                    let actions = self.scrobbler.set_playing(false);
                    self.apply_scrobble_actions(actions);
                    self.save_position();
                }
                self.stamp();
                self.emit_transport();
            }
            BackendReport::Buffering { key, buffering } => {
                if !current(&key, self) {
                    return;
                }
                self.playback.buffering = buffering;
                self.emit_transport();
            }
            BackendReport::Position { key, position_ms } => {
                if !current(&key, self) {
                    return;
                }
                self.playback.position_ms = position_ms;
                self.playback.position_at = self.now();
                if let Some(id) = self.playback.track_id().map(str::to_string) {
                    let actions = self.scrobbler.progress(&id, position_ms);
                    self.apply_scrobble_actions(actions);
                }
                let transport = self.transport_state();
                self.emit(Event::TransportChanged { transport });
                self.emit_media_session();
            }
            BackendReport::Ended { key } => {
                if !current(&key, self) {
                    return;
                }
                let duration = self.playback.duration_ms();
                if let Some(id) = self.playback.track_id().map(str::to_string) {
                    let actions = self.scrobbler.track_ended(&id, Some(duration));
                    self.apply_scrobble_actions(actions);
                }
                self.playback.position_ms = duration;
                self.playback.position_at = self.now();
                let sleep = self.sleep.track_ended();
                let stop_now = matches!(sleep, SleepAction::Stop);
                self.apply_sleep_action(sleep);
                if stop_now {
                    self.playback.want_playing = false;
                    self.playback.playing = false;
                    self.playback.awaiting_transition = false;
                    let _ = self.backend.pause();
                }
                // The backend has (or will have) moved to the preloaded item.
                self.playback.awaiting_transition = self.playback.next.is_some() && !stop_now;
                if self.owns_transport() {
                    self.local_op(SessionOp::TrackEnded);
                }
                if !self.playback.awaiting_transition {
                    // Nothing followed: stay on the last item, stopped.
                    if self
                        .doc()
                        .and_then(|d| d.current.as_ref())
                        .is_some_and(|c| c.key == key)
                    {
                        self.playback.playing = false;
                        self.stamp();
                        self.emit_transport();
                    }
                }
            }
            BackendReport::TransitionedToNext { key } => {
                // The document moved first (TrackEnded); we adopted the item
                // in `load_item`. Late arrival means the doc did not follow
                // (repeat one, queue edited): reload what the document says.
                if self.playback.backend_key.as_ref() == Some(&key) {
                    self.playback.playing = true;
                    self.playback.position_ms = 0;
                    self.playback.position_at = self.now();
                    self.refresh_next();
                    self.stamp();
                    self.emit_transport();
                } else if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                    // A stale transition (we already reloaded what the document
                    // says) is ignored; otherwise follow the document.
                    if !(self.playback.loaded && self.playback.doc_key.as_ref() == Some(&item.key))
                    {
                        self.playback.awaiting_transition = false;
                        let play = self.playback.want_playing;
                        self.load_item(&item, 0, play, None);
                    }
                }
            }
            BackendReport::Error {
                key,
                message,
                fatal,
            } => {
                let is_next = self.playback.next.as_ref().is_some_and(|n| n.key == key);
                if !current(&key, self) && !is_next {
                    return;
                }
                self.log("warn", format!("playback error on {key}: {message}"));
                if !fatal {
                    self.player_notice(
                        PlayerNoticeCode::PlaybackProblem,
                        format!("Playback problem: {message}"),
                        Some(message),
                    );
                    return;
                }
                if is_next && !current(&key, self) {
                    // The preloaded item failed to start after the current one ended.
                    self.playback.awaiting_transition = false;
                    self.playback.next = None;
                    if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                        if self.playback.doc_key.as_ref() == Some(&item.key) {
                            self.playback.load_failures += 1;
                            self.load_item(&item, 0, true, None);
                        }
                    }
                    return;
                }
                if let Some(id) = self.playback.track_id().map(str::to_string) {
                    self.scrobbler.track_failed(&id);
                }
                self.playback.load_failures += 1;
                let title = self
                    .playback
                    .track
                    .as_ref()
                    .map(|t| t.title.clone())
                    .unwrap_or_else(|| "track".into());
                if self.playback.load_failures <= LOAD_RETRIES {
                    // One retry: transient stream errors are common.
                    if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                        let failures = self.playback.load_failures;
                        let pos = self.playback.position_ms;
                        let play = self.playback.want_playing;
                        self.load_item(&item, pos, play, None);
                        self.playback.load_failures = failures;
                    }
                    return;
                }
                self.playback.consecutive_skips += 1;
                self.playback.loaded = false;
                self.playback.playing = false;
                let _ = self.jobs.add_problem(
                    None,
                    &format!("Couldn't play {title}"),
                    Some(&message),
                    None,
                );
                if self.playback.consecutive_skips >= MAX_CONSECUTIVE_SKIPS {
                    self.player_notice(
                        PlayerNoticeCode::CouldNotPlayStopped,
                        format!(
                            "Couldn't play {title}; stopped after {MAX_CONSECUTIVE_SKIPS} unplayable tracks"
                        ),
                        Some(title),
                    );
                    self.playback.want_playing = false;
                    self.playback.consecutive_skips = 0;
                    self.stamp();
                    self.emit_transport();
                    return;
                }
                self.player_notice(
                    PlayerNoticeCode::CouldNotPlaySkipped,
                    format!("Couldn't play {title}, skipped"),
                    Some(title),
                );
                if let Some(doc_key) = self.playback.doc_key.clone() {
                    if self.owns_transport() {
                        self.playback.want_playing = true;
                        self.local_op(SessionOp::SkipUnavailable { key: doc_key });
                    }
                }
            }
            BackendReport::PreBufferReady { key } => {
                self.engine_input(Input::PreBufferReady { key });
            }
            BackendReport::AudioFocusLost { transient } => {
                if !self.playback.playing {
                    return;
                }
                if !transient {
                    self.playback.want_playing = false;
                    self.set_playing(false);
                    return;
                }
                // A call, a navigation prompt: the platform player keeps its
                // intent to play and resumes by itself when focus returns.
                // Pausing it here would leave it paused for good, so only
                // record that nothing is audible.
                let now = self.now();
                self.playback.position_ms = self.playback.position_now(now);
                self.playback.position_at = now;
                self.playback.playing = false;
                self.playback.focus_suspended = true;
                let actions = self.scrobbler.set_playing(false);
                self.apply_scrobble_actions(actions);
                self.save_position();
                self.stamp();
                self.emit_transport();
            }
            BackendReport::OutputDevicesChanged { devices } => {
                self.output_devices = devices.clone();
                self.emit(Event::OutputDevicesChanged { devices });
            }
        }
    }

    // -- scrobbling -------------------------------------------------------------

    pub(crate) fn apply_scrobble_actions(&mut self, actions: Vec<ScrobbleAction>) {
        for a in actions {
            match a {
                ScrobbleAction::NowPlaying { track_id } => {
                    if !self
                        .settings
                        .get_bool(crate::settings::keys::SCROBBLE_ENABLED)
                        || !self
                            .settings
                            .get_bool(crate::settings::keys::SCROBBLE_NOW_PLAYING)
                    {
                        continue;
                    }
                    let Some(server_id) = self.server_id() else {
                        continue;
                    };
                    if let Err(e) = self
                        .recorder()
                        .apply(&server_id, &ScrobbleAction::NowPlaying { track_id })
                    {
                        self.log("warn", format!("now playing: {e}"));
                    }
                    self.schedule_flush();
                }
                ScrobbleAction::Submit {
                    track_id,
                    played_ms,
                    ..
                } => {
                    // The play reached its threshold: ask the session (dedupe
                    // across a handoff) before submitting.
                    self.playback.scrobbled = true;
                    let started_at = self.playback.started_at;
                    let Some(server_id) = self.server_id() else {
                        continue;
                    };
                    // The play's ORIGINAL start (its identity, on the session
                    // clock), not when this device resumed it: every device
                    // that ever submits this play sends the same Subsonic
                    // `time`, so a duplicate that slips past the session's
                    // dedupe (a device judging alone after the grace)
                    // collapses into one downstream.
                    let played_at = if started_at > 0.0 {
                        started_at
                    } else {
                        self.playback.started_local
                    };
                    let pending = PendingScrobble {
                        server_id,
                        track_id: track_id.clone(),
                        started_at,
                        played_at,
                        played_ms,
                    };
                    self.pending_submits
                        .insert((track_id.clone(), started_at.to_bits()), pending.clone());
                    if self.engine.is_none() {
                        // No session to ask (between servers): nobody else
                        // could have scrobbled this play.
                        self.on_scrobble_verdict(track_id, started_at, true);
                        continue;
                    }
                    // Durable before the engine is asked (its verdict may come
                    // back at once): a restart before the verdict asks again.
                    self.store_pending_scrobble(&pending);
                    self.stamp();
                    self.engine_input(Input::ScrobbleReached {
                        track_id,
                        started_at,
                    });
                    // An unanswered ask is a wait the engine persists with its
                    // connect state (awaitingSince): save it now, not only
                    // when the session document next changes.
                    self.mark_doc_dirty();
                }
            }
        }
    }

    /// Add a play to the open scope's durable "awaiting a verdict" list.
    fn store_pending_scrobble(&mut self, pending: &PendingScrobble) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let key = pending_scrobbles_key(&scope);
        let now = self.now();
        let r = self.db.with_tx(|tx| {
            let mut list = load_pending_scrobbles(tx, &key)?;
            list.retain(|p| !p.is(&pending.track_id, pending.started_at));
            list.push(pending.clone());
            save_pending_scrobbles(tx, &key, &list, now)
        });
        if let Err(e) = r {
            self.error(
                ErrorKind::Storage,
                "save pending scrobble",
                Some(e.to_string()),
            );
        }
    }

    /// After the engine is (re)built for `scope`: ask again about every play
    /// that was still waiting for its verdict when the app last stopped, with
    /// its original identity, so the engine resumes the wait (its restored
    /// `awaitingSince` keeps the grace honest) instead of the play being lost.
    pub(crate) fn resume_pending_scrobbles(&mut self, scope: &str) {
        let key = pending_scrobbles_key(scope);
        let list = match self.db.with_tx(|tx| load_pending_scrobbles(tx, &key)) {
            Ok(list) => list,
            Err(e) => {
                self.log("warn", format!("pending scrobbles unreadable: {e}"));
                return;
            }
        };
        for p in list {
            let (track_id, started_at) = (p.track_id.clone(), p.started_at);
            self.pending_submits
                .insert((track_id.clone(), started_at.to_bits()), p);
            self.engine_input(Input::ScrobbleReached {
                track_id,
                started_at,
            });
        }
        self.mark_doc_dirty();
    }

    pub(crate) fn recorder(&self) -> crate::outbox::ScrobbleRecorder {
        crate::outbox::ScrobbleRecorder::new(
            self.db.clone(),
            self.outbox.clone(),
            self.cfg.device_id.clone(),
        )
    }

    pub(crate) fn on_scrobble_verdict(
        &mut self,
        track_id: TrackId,
        started_at: EpochMs,
        allowed: bool,
    ) {
        // The engine's awaiting list changed: persist it with the document.
        self.mark_doc_dirty();
        let pending = self
            .pending_submits
            .remove(&(track_id.clone(), started_at.to_bits()));
        let (server_id, played_at, played_ms) = match pending {
            Some(p) => (p.server_id, p.played_at, p.played_ms),
            None => {
                let Some(server_id) = self.server_id() else {
                    return;
                };
                let played_ms = self.scrobbler.played_ms();
                let played_at = if started_at > 0.0 {
                    started_at
                } else if self.playback.started_at == started_at {
                    self.playback.started_local
                } else {
                    self.now() - f64::from(played_ms)
                };
                (server_id, played_at, played_ms)
            }
        };
        let submit = allowed
            && self
                .settings
                .get_bool(crate::settings::keys::SCROBBLE_ENABLED);
        // Already scrobbled by the device that handed over (`!allowed`), or
        // scrobbling is off here: it still counts as a local play, and is
        // only marked scrobbled when a device actually submitted it.
        let verdict = if submit {
            Verdict::Submit
        } else if !allowed {
            Verdict::ScrobbledElsewhere
        } else {
            Verdict::LocalOnly
        };
        // The play is recorded and its pending record cleared together.
        let pending_key = self.scope.as_deref().map(pending_scrobbles_key);
        let now = self.now();
        let r = self.recorder().record_verdict(
            &server_id,
            &track_id,
            played_at,
            played_ms,
            verdict,
            |tx| {
                let Some(key) = &pending_key else {
                    return Ok(());
                };
                let mut list = load_pending_scrobbles(tx, key)?;
                let before = list.len();
                list.retain(|p| !p.is(&track_id, started_at));
                if list.len() != before {
                    save_pending_scrobbles(tx, key, &list, now)?;
                }
                Ok(())
            },
        );
        if let Err(e) = r {
            self.log("warn", format!("record play: {e}"));
        }
        if submit {
            self.schedule_flush();
        }
        self.emit(Event::LibraryChanged {
            server_id,
            tables: vec!["tracks".into(), "play_history".into()],
            ids: vec![track_id],
        });
    }

    #[cfg(feature = "sim")]
    pub(crate) fn scrobble_command(
        &mut self,
        track_id: TrackId,
        played_at: EpochMs,
        submission: bool,
    ) {
        let Some(server_id) = self.server_id() else {
            return;
        };
        let r = self.outbox.enqueue(
            &server_id,
            crate::outbox::Mutation::Scrobble {
                track_id,
                played_at,
                submission,
                history_id: None,
            },
            None,
        );
        if let Err(e) = r {
            self.error(ErrorKind::Storage, "scrobble", Some(e.to_string()));
        }
        self.schedule_flush();
    }

    // -- sleep timer --------------------------------------------------------------

    pub(crate) fn set_sleep_timer(&mut self, timer: Option<SleepTimer>) {
        let action = self.sleep.set(timer);
        self.apply_sleep_action(action);
        self.emit(Event::SleepTimerChanged {
            timer: self.sleep.state(),
        });
    }

    pub(crate) fn apply_sleep_action(&mut self, action: SleepAction) {
        match action {
            SleepAction::None => {}
            SleepAction::Fade { gain } => {
                self.playback.fade_gain = if gain >= 1.0 { None } else { Some(gain) };
                let _ = self.backend.set_volume(self.effective_volume());
            }
            SleepAction::Stop => {
                self.playback.want_playing = false;
                self.set_playing(false);
                self.playback.fade_gain = None;
                let _ = self.backend.set_volume(self.effective_volume());
                self.emit(Event::SleepTimerChanged {
                    timer: self.sleep.state(),
                });
            }
        }
    }

    // -- media session -------------------------------------------------------------

    pub(crate) fn media_session_state(
        &mut self,
        queue: &QueueView,
        transport: &TransportState,
    ) -> MediaSessionState {
        let art = self.media_session_artwork(queue);
        let actions = self.registry.media_session_actions(&self.state_view());
        derive_media_session_state(
            queue,
            transport,
            self.session_now(),
            art,
            self.owns_transport(),
            Some(actions),
        )
    }

    pub(crate) fn emit_media_session(&mut self) {
        let queue = self.queue_view();
        let transport = self.transport_state();
        let state = self.media_session_state(&queue, &transport);
        self.emit(Event::MediaSession { state });
    }

    fn media_art_size(&self) -> u32 {
        if self.battery_saver
            && self
                .settings
                .get_bool(crate::settings::keys::BATTERY_SMALL_ARTWORK)
        {
            MEDIA_SESSION_ART_SMALL
        } else {
            MEDIA_SESSION_ART
        }
    }

    /// Artwork for the current track as a plain local filesystem path (no
    /// scheme; platforms add their own), fetching in the background when
    /// the cache misses. Same convention as `Query::Artwork`.
    fn media_session_artwork(&mut self, queue: &QueueView) -> Option<String> {
        let cover = queue.current.as_ref()?.track.cover_art.clone()?;
        let size = self.media_art_size();
        if let Some((id, s, path)) = &self.media_art {
            if id == &cover && *s == size {
                return path.clone();
            }
        }
        let server_id = self.server_id()?;
        match self.caches.artwork_cached(&server_id, &cover, size) {
            Ok(Some(p)) => {
                let path = p.to_string_lossy().into_owned();
                self.media_art = Some((cover, size, Some(path.clone())));
                Some(path)
            }
            _ => {
                self.fetch_artwork(cover, size);
                None
            }
        }
    }

    pub(crate) fn fetch_artwork(&mut self, id: String, size: u32) {
        if self.media_art_pending.as_ref() == Some(&(id.clone(), size)) {
            return;
        }
        let Some(api) = self.api() else { return };
        self.media_art_pending = Some((id.clone(), size));
        let caches = self.caches.clone();
        let tx = self.tx.clone();
        self.spawn(async move {
            let path = caches
                .artwork_path(api.as_ref(), &id, size)
                .await
                .ok()
                .flatten()
                .map(|p| crate::downloads::file_url(&p));
            let _ = tx.send(crate::core::ActorMsg::Internal(Internal::Artwork {
                id,
                size,
                path,
            }));
        });
    }

    pub(crate) fn on_artwork(&mut self, id: String, size: u32, path: Option<String>) {
        if self.media_art_pending.as_ref() == Some(&(id.clone(), size)) {
            self.media_art_pending = None;
        }
        self.media_art = Some((id, size, path));
        self.emit_media_session();
    }

    fn prefetch_artwork_for_current(&mut self) {
        let Some(cover) = self
            .playback
            .track
            .as_ref()
            .and_then(|t| t.cover_art.clone())
        else {
            return;
        };
        let size = self.media_art_size();
        let Some(server_id) = self.server_id() else {
            return;
        };
        if matches!(
            self.caches.artwork_cached(&server_id, &cover, size),
            Ok(Some(_))
        ) {
            return;
        }
        if self.battery_saver
            && self
                .settings
                .get_bool(crate::settings::keys::BATTERY_PAUSE_PREFETCH)
        {
            return;
        }
        self.fetch_artwork(cover, size);
    }

    pub(crate) fn media_session_command(&mut self, action: MediaSessionAction, value: Option<f64>) {
        let queue = self.queue_view();
        let transport = self.transport_state();
        let state = self.media_session_state(&queue, &transport);
        if let Some(cmd) = command_for(action, value, &state) {
            self.handle_command(cmd);
        }
    }

    pub(crate) fn set_battery_saver(&mut self, enabled: bool) {
        if self.battery_saver != enabled {
            self.battery_saver = enabled;
            self.mark_prefetch_check();
            self.media_art = None;
            self.emit_media_session();
        }
    }
}

fn load_pending_scrobbles(tx: &rusqlite::Transaction, key: &str) -> DbResult<Vec<PendingScrobble>> {
    use rusqlite::OptionalExtension;
    let json: Option<String> = tx
        .query_row("SELECT json FROM saved_state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(match json {
        Some(j) => serde_json::from_str(&j)?,
        None => vec![],
    })
}

fn save_pending_scrobbles(
    tx: &rusqlite::Transaction,
    key: &str,
    list: &[PendingScrobble],
    now: EpochMs,
) -> DbResult<()> {
    if list.is_empty() {
        tx.execute("DELETE FROM saved_state WHERE key = ?1", [key])?;
        return Ok(());
    }
    let json = serde_json::to_string(list)?;
    tx.execute(
        "INSERT INTO saved_state(key, json, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
        rusqlite::params![key, json, now],
    )?;
    Ok(())
}
