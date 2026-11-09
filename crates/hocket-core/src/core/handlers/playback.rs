//! Playback: only the lease owner drives the backend; everyone else forwards
//! transport commands over Connect and renders from stamps.

use crate::api::*;
use crate::audio::dsp::gain_for_track;
use crate::audio::sleep::SleepAction;
use crate::connect::engine::Input;
use crate::connect::wire::{SessionOp, TransportCommand};
use crate::core::actor::{
    Actor, ADOPTION_TIMEOUT_MS, LOAD_RETRIES, MAX_CONSECUTIVE_SKIPS, MEDIA_SESSION_ART,
    MEDIA_SESSION_ART_SMALL, SKIP_SETTLE_MS,
};
use crate::core::state::{pending_scrobbles_key, PendingScrobble};

/// How long a volume sent to another device wins over its stamps.
const REMOTE_VOLUME_HOLD_MS: f64 = 1500.0;
/// A position report this far from where the track should be is logged.
const POSITION_JUMP_LOG_MS: i64 = 2_000;
use crate::core::Internal;
use crate::db::DbResult;
use crate::media_session::{command_for, derive_media_session_state};
use crate::outbox::{ScrobbleAction, Verdict};

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
        let resolved = if let Some(r) = &self.renderer {
            let cannot_play =
                crate::downloads::receivers::cannot_play(r, &self.renderer_formats, track);
            self.downloads
                .resolve_remote(api.as_ref(), key, track, cannot_play)
        } else {
            self.downloads
                .resolve_with(api.as_ref(), key, track, reader)
        };
        let mut source = match resolved {
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

    /// The first `n` items that will play after the current one: the first
    /// is the gapless preload (offline: the next ones that can play offline),
    /// and AutoMix plans the follow-up's own mix-out from the second.
    pub(crate) fn upcoming_playable(&self, n: usize) -> Vec<QueueItem> {
        let Some(doc) = self.doc() else {
            return vec![];
        };
        if doc.repeat == RepeatMode::One {
            return vec![];
        }
        let offline = self.is_offline();
        // Repeat All wraps to the top: those entries are next too (#13).
        let wrapped = (doc.repeat == RepeatMode::All)
            .then(|| doc.history.iter().chain(doc.current.iter()))
            .into_iter()
            .flatten();
        doc.upcoming
            .iter()
            .chain(wrapped)
            .filter(|i| !i.unavailable)
            .filter(|i| {
                !offline
                    || self
                        .track_or_bare(&i.track_id)
                        .is_some_and(|t| self.playable_offline(&t))
            })
            .take(n)
            .cloned()
            .collect()
    }

    /// Whether the backend gets the follow-up ahead of time: with gapless
    /// on, with transitions on (a mix needs it), and always on a remote
    /// renderer, whose own queue moves on to it while this device sleeps.
    pub(crate) fn preloads_next(&self) -> bool {
        self.audio.gapless || self.renderer.is_some() || self.transition_settings().mode.is_on()
    }

    /// The follow-up's source, with its AutoMix fields (`locked`: see
    /// [`Self::with_mix`]).
    fn next_source(&self, upcoming: &[QueueItem], locked: Option<Ms>) -> Option<MediaSource> {
        let item = upcoming.first()?;
        let track = self.track_or_bare(&item.track_id)?;
        let source = self.media_source_for(&item.key, &track)?;
        let following = upcoming
            .get(1)
            .and_then(|i| self.track_or_bare(&i.track_id));
        Some(self.with_mix(source, &track, following.as_ref(), locked))
    }

    /// Playback moves to a remote renderer (a Cast device), to another one,
    /// or back here (`Command::SetRenderer`). Sources resolve differently from now on, so
    /// the current item reloads at its position, keeping its played time:
    /// on the renderer it keeps playing (or not); back here it pauses,
    /// rather than bursting out of the phone's speaker.
    pub(crate) fn set_renderer(&mut self, renderer: Option<crate::api::Renderer>) {
        // Some again (another session, maybe a receiver of the same name)
        // still reloads: the new receiver has nothing loaded.
        if self.renderer.is_none() && renderer.is_none() {
            return;
        }
        tracing::info!(target: "hocket::player", renderer = ?renderer, "renderer");
        let leaving = renderer.is_none();
        self.renderer = renderer;
        if !leaving {
            // Nothing here reads the stream cache while the renderer plays.
            self.cancel_prefetch();
        }
        let reload =
            self.owns_transport() && self.playback.loaded && self.playback.pending_load.is_none();
        if reload {
            if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                let now = self.now();
                let position = self.playback.position_now(now);
                let play = self.playback.playing && !leaving;
                let carried = (
                    self.scrobbler.played_ms(),
                    self.playback.started_at,
                    self.playback.scrobbled,
                );
                self.load_item(&item, position, play, Some(carried));
                if !play {
                    self.playback.want_playing = false;
                }
                return;
            }
        }
        // Nothing to reload: the follow-up (if any) still resolves anew.
        self.playback.next = None;
        self.playback.next_doc_key = None;
        self.refresh_next();
        self.stamp();
        self.emit_transport();
    }

    /// The renderer's receiver died right after starting `track_id`
    /// untranscoded (`Command::RendererFailed`): note its format as one
    /// this receiver can't play, so from now on it is transcoded there.
    pub(crate) fn renderer_failed(&mut self, track_id: &str) {
        let Some(renderer) = self.renderer.clone() else {
            return;
        };
        let track = match self.db.track(track_id) {
            Ok(Some(t)) => t,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(target: "hocket::player", error = %e, "renderer failed: track");
                return;
            }
        };
        if !crate::downloads::receivers::learn(&mut self.renderer_formats, &renderer, &track) {
            return;
        }
        tracing::info!(
            target: "hocket::player",
            receiver = %renderer.name,
            suffix = ?track.suffix,
            hi_res = crate::downloads::receivers::hi_res(&track),
            "receiver can't play this format: transcoding it there from now on"
        );
        if let Err(e) = self.db.saved_state_set(
            crate::downloads::receivers::STATE_KEY,
            &self.renderer_formats,
            self.clock.as_ref(),
        ) {
            tracing::warn!(target: "hocket::player", error = %e, "save renderer formats");
        }
    }

    pub(crate) fn refresh_next(&mut self) {
        self.refresh_next_with(false);
    }

    /// `replan`: rebuild the follow-up's AutoMix plan even when the
    /// follow-up itself is unchanged (see [`Self::replan_next`]).
    pub(crate) fn refresh_next_with(&mut self, replan: bool) {
        if self.playback.pending_load.is_some() {
            // The backend holds the item a burst skipped away from; the
            // deferred load hands over the follow-up with the item.
            return;
        }
        if self.playback.transition.is_some() {
            // The backend is mixing into the follow-up and ignores a new one
            // until the handover, which rebuilds it for the item after.
            return;
        }
        if !self.preloads_next() {
            self.playback.next = None;
            self.playback.next_doc_key = None;
            let _ = self.backend.set_next(None);
            return;
        }
        let upcoming = self.upcoming_playable(2);
        let next = upcoming.first();
        let next_key = next.map(|i| i.key.clone());
        // A document an older peer edited keys upcoming context items by
        // index (`ctx-N`), and an edit ahead of one hands its key to another
        // track: the preload stays only if the track is the same too (#62).
        let same_track = match (next, &self.playback.next) {
            (Some(i), Some(n)) => n.track.id == i.track_id,
            _ => false,
        };
        let locked = self.locked_tail_start();
        if next_key == self.playback.next_doc_key && same_track {
            // A committed plan stays, unless a context rule (Off, sleep
            // end-of-track, Cast, exclusive output) now refuses mixing.
            let keep = locked.is_some() && self.mix_context_allows();
            if !replan || keep {
                return;
            }
        }
        let source = self.next_source(&upcoming, locked);
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
                .chain(d.upcoming.iter())
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
        self.playback.pending_load = None;
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
        let is_next = carried.is_none()
            && position_ms == 0
            && self
                .playback
                .next
                .as_ref()
                .is_some_and(|n| n.track.id == item.track_id);
        // Gapless (or at an AutoMix handover): the backend already moved on
        // to this track.
        let transitioned = is_next && self.playback.awaiting_transition;
        // Next pressed while the backend mixes into this track: it completes
        // the transition instead of loading it afresh.
        let completes = is_next && !transitioned && self.playback.transition.is_some();
        let transitioned = transitioned || completes;
        self.note_cache_signals(item, &track, transitioned, play);
        let (played_ms, started_at, scrobbled) = match carried {
            Some((p, s, sc)) => (p, s, sc),
            None => (0, self.session_now(), false),
        };
        let mut position_ms = position_ms;
        if transitioned {
            let next = self.playback.next.take();
            let backend_key = next.as_ref().map(|n| n.key.clone());
            // Where the incoming track is: 0 after a gapless join; after a mix,
            // already some way in (the backend's `Position` follows). A plan
            // alone is no mix: the backend skips one it reached too late (a
            // seek past the tail start, a late plan, a failed render) and
            // joins from 0, so only a reported start counts.
            let mixed = next.as_ref().is_some_and(|n| {
                self.playback
                    .transition
                    .as_ref()
                    .is_some_and(|t| t.to == n.key)
            });
            position_ms = match next.as_ref().and_then(|n| n.mix_in.as_ref()) {
                Some(plan) if mixed && completes => {
                    let a_pos = self.playback.position_now(now);
                    plan.incoming_position_at(a_pos.saturating_sub(plan.tail_start_ms))
                }
                Some(plan) if mixed => plan.handover_position_ms(),
                _ => 0,
            };
            if completes {
                tracing::debug!(target: "hocket::player", key = %item.key, "next during a transition: completing it");
                if let Err(e) = self.backend.complete_transition() {
                    self.log("debug", format!("complete_transition: {e}"));
                }
            }
            self.playback.doc_key = Some(item.key.clone());
            self.playback.backend_key = backend_key;
            self.playback.track = Some(track.clone());
            self.playback.position_ms = position_ms;
            self.playback.position_at = now;
            self.playback.adopted_position_ms = position_ms;
            self.playback.mix_out_ms = next.as_ref().and_then(|n| n.mix_out_ms);
            self.playback.transition = None;
            self.playback.next_doc_key = None;
            self.playback.awaiting_transition = false;
            self.playback.adopted_at = Some(now);
            self.playback.loaded = true;
        } else {
            let Some(source) = self.media_source_for(&item.key, &track) else {
                tracing::info!(target: "hocket::player", key = %item.key, track = %track.id, "no source to play this from");
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
            let upcoming = if self.preloads_next() {
                self.upcoming_playable(2)
            } else {
                vec![]
            };
            // Loaded as current, a source never mixes in (a handoff starts
            // the incoming track unmixed); its mix-out comes from what follows.
            let mut source = source;
            source.mix_in = None;
            source.mix_out_ms = upcoming
                .first()
                .and_then(|i| self.track_or_bare(&i.track_id))
                .and_then(|next| self.mix_out_for(&track, Some(&next)));
            self.playback.doc_key = Some(item.key.clone());
            self.playback.backend_key = Some(item.key.clone());
            self.playback.track = Some(track.clone());
            self.playback.position_ms = position_ms;
            self.playback.position_at = now;
            self.playback.awaiting_transition = false;
            self.playback.adopted_at = None;
            self.playback.loaded = true;
            self.playback.mix_out_ms = source.mix_out_ms;
            self.playback.transition = None;
            self.playback.adopted_position_ms = 0;
            self.playback.next = None;
            self.playback.next_doc_key = None;
            let next_source = self.next_source(&upcoming, None);
            self.playback.next_doc_key = upcoming.first().map(|i| i.key.clone());
            self.playback.next = next_source.clone();
            if let Err(e) = self.backend.load(source, next_source, position_ms, play) {
                self.error(ErrorKind::Playback, "load", Some(e.to_string()));
            }
        }
        self.playback.playing = play;
        if play {
            // A paused load says nothing of focus coming back.
            self.playback.focus_suspended = false;
        }
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

    /// The current item changed by a skip in a burst (see
    /// [`SKIP_SETTLE_MS`]): show it as loading at once, but give the backend
    /// nothing until the burst settles, so a run of Next presses does not
    /// open a stream for every track it passes. The old item stops (paused,
    /// its reports ignored) and its play ends here; the new one starts
    /// counting as a play, with its now-playing, only once it loads.
    pub(crate) fn defer_load(&mut self, item: &QueueItem, position_ms: Ms, play: bool) {
        let now = self.now();
        if self.playback.loaded && self.playback.backend_key.is_some() {
            // Leaving what the backend plays, as any skip would.
            self.note_left_early();
            if let Some(id) = self.playback.track_id().map(str::to_string) {
                let pos = self.playback.position_now(now);
                let actions = self.scrobbler.track_ended(&id, Some(pos));
                self.apply_scrobble_actions(actions);
            }
            if self.playback.playing {
                if let Err(e) = self.backend.pause() {
                    self.log("debug", format!("pause: {e}"));
                }
            }
        }
        let track = self.track_or_bare(&item.track_id);
        let position_ms = match &track {
            Some(t) if t.duration_ms > 0 => position_ms.min(t.duration_ms),
            _ => position_ms,
        };
        let p = &mut self.playback;
        p.doc_key = Some(item.key.clone());
        p.backend_key = None;
        p.track = track;
        p.position_ms = position_ms;
        p.position_at = now;
        p.playing = play;
        p.buffering = true;
        p.loaded = true;
        p.awaiting_transition = false;
        p.adopted_at = None;
        p.next = None;
        p.next_doc_key = None;
        p.transition = None;
        if play {
            p.focus_suspended = false;
        }
        p.load_failures = 0;
        if play {
            p.want_playing = true;
        }
        p.pending_load = Some(now + SKIP_SETTLE_MS);
        self.arm_pending_load_timer();
        self.stamp();
        self.emit_transport();
    }

    /// Wake the actor when the settle window should have passed. The
    /// periodic tick checks too (virtual time in tests moves only with it);
    /// this only makes the load follow the last skip closely.
    fn arm_pending_load_timer(&mut self) {
        let Some(deadline) = self.playback.pending_load else {
            return;
        };
        if self.pending_load_timer.is_some() {
            // One timer at a time: when it fires it re-arms for a deadline
            // that moved meanwhile.
            return;
        }
        self.pending_load_timer = Some(deadline);
        let delay = (deadline - self.now()).clamp(1.0, SKIP_SETTLE_MS);
        let tx = self.tx.clone();
        self.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
            let _ = tx.send(crate::core::ActorMsg::Internal(Internal::PendingLoadDue));
        });
    }

    pub(crate) fn on_pending_load_due(&mut self) {
        let armed = self.pending_load_timer.take();
        self.flush_pending_load(false);
        if let (Some(deadline), Some(armed)) = (self.playback.pending_load, armed) {
            // Skips came after the timer was set: wait for the new deadline.
            // An unmoved deadline not yet reached (a virtual clock) is left
            // to the tick.
            if deadline > armed {
                self.arm_pending_load_timer();
            }
        }
    }

    /// Load the item a skip burst landed on, once the settle window passed
    /// (`force`: now, e.g. on a restart). It loads where the session stands
    /// then: paused or seeked meanwhile is honoured.
    pub(crate) fn flush_pending_load(&mut self, force: bool) {
        let Some(deadline) = self.playback.pending_load else {
            return;
        };
        if !force && self.now() < deadline {
            return;
        }
        self.playback.pending_load = None;
        if !self.owns_transport() {
            return;
        }
        let Some(item) = self.doc().and_then(|d| d.current.clone()) else {
            return;
        };
        if self.playback.doc_key.as_ref() != Some(&item.key) {
            return;
        }
        let play = self.playback.playing;
        let position = self.playback.position_ms;
        self.load_item(&item, position, play, None);
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
        let p = &self.playback;
        let (volume, muted, system_volume, fade_gain) =
            (p.volume, p.muted, p.system_volume, p.fade_gain);
        let restore = self.playback.restore_position.take();
        // The device's volume outlives what was loaded.
        self.playback = crate::core::state::Playback {
            volume,
            muted,
            system_volume,
            fade_gain,
            restore_position: restore,
            ..Default::default()
        };
        self.stamp();
        self.emit_transport();
    }

    pub(crate) fn restart_current(&mut self) {
        if self.playback.pending_load.is_some() {
            self.playback.position_ms = 0;
            self.playback.playing = true;
            self.flush_pending_load(true);
            return;
        }
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
        // Only the owner prefetches audio; a lost lease cancels at once.
        self.mark_prefetch_check();
        if !owns {
            self.prefetch_tick(self.now());
        }
        if owns {
            // A preview started while someone else held the transport must
            // not keep playing once this device takes it over.
            if self.preview.as_ref().is_some_and(|p| p.hold.is_none()) {
                self.stop_preview(false);
            }
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
            // A lease change (every renewal is one) is no cue to start
            // playing while a call holds audio focus: the platform player
            // resumes by itself when focus returns.
            let resume = self.playback.want_playing && !self.playback.focus_suspended;
            if !self.playback.loaded {
                if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                    tracing::debug!(target: "hocket::player", key = %item.key, play = resume, "holding the transport: loading");
                    let position = self.resume_position_for(&item);
                    self.load_item(&item, position, resume, None);
                }
            } else if resume && !self.playback.playing {
                tracing::debug!(target: "hocket::player", "holding the transport: resuming");
                self.set_playing(true);
            }
        } else {
            if self.playback.loaded {
                tracing::debug!(target: "hocket::player", "another device holds the transport: stopping here");
            }
            // Another device owns transport now: this backend must go quiet
            // and stop accepting late reports for what it had loaded.
            self.render_only();
        }
        if detached {
            self.log("info", "owning transport while cut off from the room");
        }
        let _ = lease;
        self.emit_transport();
        if let Some(devices) = self.engine.as_ref().map(|e| e.devices()) {
            self.emit_devices(devices);
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
                    .chain(d.upcoming.iter())
                    .find(|i| i.key == key)
                    .cloned()
            })
            .unwrap_or(QueueItem {
                key: key.clone(),
                track_id: track_id.clone(),
                source: QueueSource::Inserted,
                unavailable: false,
                added: false,
                queued: false,
                ordinal: 0,
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
        // Where each press went, so a bundle shows one that did nothing.
        // Volume is left out: a slider drag sends dozens.
        let quiet = matches!(cmd, TransportCommand::SetVolume { .. });
        let Some(engine) = &self.engine else {
            if !quiet {
                tracing::debug!(target: "hocket::player", ?cmd, "no session: dropped");
            }
            return;
        };
        if engine.owns_transport() {
            if !quiet {
                tracing::debug!(target: "hocket::player", ?cmd, "applied here");
            }
            self.apply_transport_command(cmd);
            return;
        }
        // Not "is the lease unowned" but "is there another device to ask":
        // a lease still naming this device while the engine holds nothing is
        // no owner at all, and a request addressed to it would come back here
        // and be dropped. Play claims it instead of vanishing.
        let owner = engine.remote_owner();
        let nobody = owner.is_none();
        match (&cmd, nobody) {
            (TransportCommand::Play | TransportCommand::TogglePlay, true) => {
                if self.doc().is_some_and(|d| d.current.is_some()) {
                    tracing::debug!(target: "hocket::player", ?cmd, "nobody plays: claiming the transport");
                    self.playback.want_playing = true;
                    self.engine_input(Input::ClaimTransport { takeover: false });
                } else {
                    tracing::debug!(target: "hocket::player", ?cmd, "nobody plays and nothing is queued: dropped");
                }
            }
            (TransportCommand::SetVolume { volume, muted }, true) => {
                self.set_local_volume(*volume, muted.unwrap_or(false))
            }
            (_, true) => tracing::debug!(target: "hocket::player", ?cmd, "nobody plays: dropped"),
            (_, false) => {
                if !quiet {
                    tracing::debug!(target: "hocket::player", ?cmd, owner = ?owner, "sent to the device that plays");
                }
                self.engine_input(Input::TransportRequest(cmd))
            }
        }
    }

    /// Apply a transport command on this device (we own transport).
    pub(crate) fn apply_transport_command(&mut self, cmd: TransportCommand) {
        // A Connect peer's command lands here without passing
        // `handle_command`, so a preview has to give way here too. Play,
        // stop and seek take the output back. A pause leaves the preview
        // playing but drops the hold, so closing it does not undo the pause.
        match cmd {
            TransportCommand::Pause => self.drop_preview_hold(),
            TransportCommand::SetVolume { .. } => {}
            _ => self.stop_preview(false),
        }
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
                    // Pause first so the album resume point keeps this position. The rewind
                    // below is only the transport; it is not where the album was left.
                    self.set_playing(false);
                    self.seek_to_inner(0, false);
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
            TransportCommand::SetVolume { volume, muted } => {
                self.set_local_volume(volume, muted.unwrap_or(false))
            }
        }
    }

    fn ensure_loaded_then_play(&mut self) {
        if self.playback.loaded {
            self.set_playing(true);
        } else if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
            tracing::debug!(target: "hocket::player", key = %item.key, "play with nothing loaded: loading");
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
            tracing::debug!(target: "hocket::player", key = %item.key, position_ms = p, "resuming at the position to restore");
            return p;
        }
        if self.playback.doc_key.as_ref() == Some(&item.key) {
            let p = self.playback.position_ms;
            tracing::debug!(target: "hocket::player", key = %item.key, position_ms = p, "resuming where it was");
            p
        } else {
            tracing::debug!(target: "hocket::player", key = %item.key, last = ?self.playback.doc_key, "starting from the top: the last position was for another queue entry");
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
        // Nothing of a call's suspension carries over: there is no longer a
        // player here to resume.
        self.playback.focus_suspended = false;
        self.playback.backend_key = None;
        self.playback.next = None;
        self.playback.next_doc_key = None;
        self.playback.awaiting_transition = false;
        self.playback.adopted_at = None;
        self.playback.pending_load = None;
        self.playback.transition = None;
        self.playback.mix_out_ms = None;
    }

    /// Every tick: an item adopted at a gapless boundary that the backend
    /// never confirmed is loaded explicitly (see `Playback::adopted_at`).
    pub(crate) fn playback_tick(&mut self, now: f64) {
        let Some(at) = self.playback.adopted_at else {
            return;
        };
        if now - at < ADOPTION_TIMEOUT_MS {
            return;
        }
        self.playback.adopted_at = None;
        if !self.playback.loaded || !self.owns_transport() {
            return;
        }
        let Some(item) = self.doc().and_then(|d| d.current.clone()) else {
            return;
        };
        if self.playback.doc_key.as_ref() != Some(&item.key) {
            return;
        }
        self.log(
            "info",
            "the backend did not move on to the preloaded item; loading it",
        );
        let play = self.playback.want_playing || self.playback.playing;
        self.playback.next = None;
        self.playback.next_doc_key = None;
        self.load_item(&item, 0, play, None);
    }

    pub(crate) fn set_playing(&mut self, playing: bool) {
        self.set_playing_inner(playing, true);
    }

    /// `save_position` persists the resume point. A preview pause passes
    /// false: the preview must not rewrite resume state.
    pub(crate) fn set_playing_inner(&mut self, playing: bool, save_position: bool) {
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
        let r = if self.playback.pending_load.is_some() {
            // Nothing of this item is in the backend yet: the deferred load
            // starts it playing or paused.
            Ok(())
        } else if playing {
            self.backend.play()
        } else {
            self.backend.pause()
        };
        if let Err(e) = r {
            self.error(ErrorKind::Playback, "play/pause", Some(e.to_string()));
        }
        let actions = self.scrobbler.set_playing(playing);
        self.apply_scrobble_actions(actions);
        if !playing && save_position {
            self.save_position();
            self.capture_album_resume(true);
        }
        self.stamp();
        self.emit_transport();
    }

    pub(crate) fn seek_to(&mut self, position_ms: Ms) {
        self.seek_to_inner(position_ms, true);
    }

    /// `record` is false for the rewind Stop does after the resume point was saved.
    fn seek_to_inner(&mut self, position_ms: Ms, record: bool) {
        let cap = self.playback.duration_ms();
        let position_ms = if cap > 0 {
            position_ms.min(cap)
        } else {
            position_ms
        };
        if !self.playback.loaded {
            self.playback.position_ms = position_ms;
            if record {
                self.capture_album_resume(true);
            }
            return;
        }
        self.playback.position_ms = position_ms;
        self.playback.position_at = self.now();
        // A seek ends a stall as far as we know (the native engine clears it
        // silently); a backend still short of data reports buffering again.
        if self.playback.pending_load.is_some() {
            // The deferred load starts there.
            if record {
                self.capture_album_resume(true);
            }
            self.stamp();
            self.emit_transport();
            return;
        }
        self.playback.buffering = false;
        // A seek cancels a transition still before its handover: the backend
        // drops the incoming side and the follow-up stays preloaded.
        self.playback.transition = None;
        if let Err(e) = self.backend.seek(position_ms) {
            self.error(ErrorKind::Playback, "seek", Some(e.to_string()));
        }
        if let Some(id) = self.playback.track_id().map(str::to_string) {
            self.scrobbler.seeked(&id, position_ms);
        }
        if record {
            self.capture_album_resume(true);
        }
        self.stamp();
        self.emit_transport();
    }

    /// The device the volume control drives when it isn't this one: the
    /// device that holds transport. `None` when this device plays or nobody
    /// does (the control then sets this device's own volume).
    pub(crate) fn volume_target(&self) -> Option<DeviceId> {
        let engine = self.engine.as_ref()?;
        if engine.owns_transport() {
            return None;
        }
        engine
            .lease()
            .owner
            .clone()
            .filter(|o| *o != self.cfg.device_id)
    }

    /// What the volume control shows: the device it drives and its volume.
    /// The device is `None` while one that doesn't report its volume (an
    /// older version) plays.
    pub(crate) fn volume_control(&self) -> (Option<DeviceId>, DeviceVolume) {
        let Some(owner) = self.volume_target() else {
            let own = DeviceVolume {
                volume: self.playback.volume,
                muted: self.playback.muted,
            };
            return (Some(self.cfg.device_id.clone()), own);
        };
        if let Some((device, v, at)) = &self.playback.remote_volume_hold {
            if *device == owner && self.now() - at < REMOTE_VOLUME_HOLD_MS {
                return (Some(owner), *v);
            }
        }
        match self.engine.as_ref().map(|e| e.transport()) {
            Some(t) if t.volume_device.as_ref() == Some(&owner) => (
                Some(owner),
                DeviceVolume {
                    volume: t.volume,
                    muted: t.muted,
                },
            ),
            _ => (None, DeviceVolume::default()),
        }
    }

    /// A held remote volume that the device's stamps never confirmed (a
    /// dropped request) goes back to what the device last reported.
    pub(crate) fn remote_volume_tick(&mut self, now: f64) {
        let expired = self
            .playback
            .remote_volume_hold
            .as_ref()
            .is_some_and(|(_, _, at)| now - at >= REMOTE_VOLUME_HOLD_MS);
        if expired {
            self.playback.remote_volume_hold = None;
            self.emit_transport();
        }
    }

    /// The volume control moved (UI, keys, media session, a watch): set the
    /// volume of the device playing, or this one when nobody is. `volume`
    /// `None` keeps the level (a mute toggle); setting a level unmutes.
    pub(crate) fn volume_command(&mut self, volume: Option<f64>, muted: bool) {
        let Some(owner) = self.volume_target() else {
            let level = volume.unwrap_or(self.playback.volume);
            self.set_local_volume(level, muted);
            return;
        };
        let (device, known) = self.volume_control();
        let Some(level) = volume.or(device.is_some().then_some(known.volume)) else {
            // A mute toggle for a device whose level we don't know: sending
            // a made-up level would change it.
            return;
        };
        let level = crate::audio::backend::clamp_volume(level);
        let now = self.now();
        self.playback.remote_volume_hold = Some((
            owner,
            DeviceVolume {
                volume: level,
                muted,
            },
            now,
        ));
        self.engine_input(Input::TransportRequest(TransportCommand::SetVolume {
            volume: level,
            muted: Some(muted),
        }));
        self.emit_transport();
    }

    /// Set this device's own volume (its UI while nobody else plays, or a
    /// remote control while it plays), persist it and tell the others.
    pub(crate) fn set_local_volume(&mut self, volume: f64, muted: bool) {
        let volume = crate::audio::backend::clamp_volume(volume);
        if self.playback.system_volume {
            // The platform keeps it, in its own steps: its SystemVolume report
            // is what this device's volume becomes (and what the others see).
            if let Err(e) = self.backend.set_system_volume(volume, muted) {
                self.log("debug", format!("set_system_volume: {e}"));
            }
            return;
        }
        self.playback.volume = volume;
        self.playback.muted = muted;
        {
            if let Err(e) = self.backend.set_volume(self.effective_volume()) {
                self.log("debug", format!("set_volume: {e}"));
            }
            let clock = self.clock.clone();
            let _ =
                self.db
                    .saved_state_set(crate::core::actor::VOLUME_KEY, &volume, clock.as_ref());
            let _ = self
                .db
                .saved_state_set(crate::core::actor::MUTED_KEY, &muted, clock.as_ref());
        }
        self.stamp();
        self.emit_transport();
    }

    /// The platform's own volume changed (Android's music stream): mirror it
    /// and tell the others. From the first report on, the platform owns this
    /// device's volume and the in-player gain carries only the sleep fade.
    fn on_system_volume(&mut self, volume: f64, muted: bool) {
        let mut volume = crate::audio::backend::clamp_volume(volume);
        // Android reports a muted stream at 0; mute keeps the level it had.
        if muted && volume == 0.0 && self.playback.system_volume && self.playback.volume > 0.0 {
            volume = self.playback.volume;
        }
        let first = !self.playback.system_volume;
        let changed = first || volume != self.playback.volume || muted != self.playback.muted;
        self.playback.system_volume = true;
        self.playback.volume = volume;
        self.playback.muted = muted;
        if first {
            if let Err(e) = self.backend.set_volume(self.effective_volume()) {
                self.log("debug", format!("set_volume: {e}"));
            }
        }
        if changed {
            self.stamp();
            self.emit_transport();
        }
    }

    /// Enter or leave a stall. The position freezes where playback stopped
    /// and counts on from the moment it resumes.
    fn set_buffering(&mut self, buffering: bool) {
        let now = self.now();
        self.playback.position_ms = self.playback.position_now(now);
        self.playback.position_at = now;
        self.playback.buffering = buffering;
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
            volume: Some(DeviceVolume {
                volume: p.volume,
                muted: p.muted,
            }),
            renderer: self.renderer.as_ref().map(|r| r.name.clone()),
        };
        self.engine_input(input);
    }

    // -- backend reports -----------------------------------------------------------

    pub(crate) fn on_backend_report(&mut self, report: BackendReport) {
        if self.preview.is_some() {
            match &report {
                // The preview player taking the output is not another app,
                // and a Playing report must not start the main item under it.
                BackendReport::AudioFocusLost { .. } => return,
                BackendReport::Playing { .. } => {
                    let _ = self.backend.pause();
                    return;
                }
                _ => {}
            }
        }
        match &report {
            BackendReport::PreviewReady { duration_ms } => {
                self.on_preview_ready(*duration_ms);
                return;
            }
            BackendReport::PreviewProgress {
                position_ms,
                playing,
            } => {
                self.on_preview_progress(*position_ms, *playing);
                return;
            }
            BackendReport::PreviewEnded => {
                self.stop_preview(true);
                return;
            }
            BackendReport::PreviewInterrupted => {
                self.stop_preview(false);
                return;
            }
            BackendReport::PreviewFailed => {
                if self.preview.is_some() {
                    self.toast("Couldn't preview this song", None);
                }
                self.stop_preview(true);
                return;
            }
            _ => {}
        }
        if !matches!(
            report,
            BackendReport::Position { .. }
                | BackendReport::SystemVolume { .. }
                | BackendReport::OutputDevicesChanged { .. }
        ) {
            tracing::debug!(target: "hocket::player", ?report, "player reported");
        }
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
            BackendReport::SystemVolume { volume, muted } => self.on_system_volume(volume, muted),
            BackendReport::Ready { key, duration_ms } => {
                if !current(&key, self) {
                    return;
                }
                if let (Some(d), Some(t)) = (duration_ms, self.playback.track.as_mut()) {
                    if t.duration_ms == 0 && d > 0 {
                        t.duration_ms = d;
                    }
                }
                self.set_buffering(false);
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
                if self.playback.buffering == buffering {
                    return;
                }
                self.set_buffering(buffering);
                if !buffering {
                    // Tell the room, which otherwise keeps counting through
                    // the stall and ends up ahead.
                    self.stamp();
                }
                self.emit_transport();
            }
            BackendReport::Position { key, position_ms } => {
                if !current(&key, self) {
                    return;
                }
                // Ticks are left out, but not one far from where the track
                // should be: a backend that restarted or stalled by itself.
                let expected = self.playback.position_now(self.now());
                if (i64::from(position_ms) - i64::from(expected)).abs() > POSITION_JUMP_LOG_MS {
                    tracing::debug!(target: "hocket::player", %key, position_ms, expected, "player position is not where it should be");
                }
                self.playback.position_ms = position_ms;
                self.playback.position_at = self.now();
                if let Some(id) = self.playback.track_id().map(str::to_string) {
                    let actions = self.scrobbler.progress(&id, position_ms);
                    self.apply_scrobble_actions(actions);
                }
                // UIs and the OS media session extrapolate from the stamp;
                // a MediaSession event here would rebuild notifications,
                // binder playlists and the Pebble now-playing push ~1 Hz.
                // The stamp goes out only when the position strayed from
                // the extrapolation: a stall the backend did not report, or
                // output latency (the player reports playing before the sound
                // is heard, ~200 ms later over Bluetooth). Lyrics follow this
                // stamp, so this report is held to a much tighter tolerance
                // than the one that keeps steady ticks quiet. Not the native
                // engine's: its position steps by the device period and
                // already starts when the sound is queued.
                let tolerance = if self.cfg.audio == AudioMode::Native {
                    crate::media_session::STAMP_TOLERANCE_MS
                } else {
                    crate::media_session::RESYNC_TOLERANCE_MS
                };
                let transport = self.transport_state();
                if self.emit_transport_state_within(transport, tolerance) {
                    // Devices following in the room run from the same stamp.
                    self.stamp();
                }
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
                    // Nothing followed: stay on the last item, stopped. The
                    // document names it by `doc_key`, which a room that
                    // re-keyed it has moved off the backend's `key`.
                    let doc_key = self.playback.doc_key.clone();
                    if self
                        .doc()
                        .and_then(|d| d.current.as_ref())
                        .is_some_and(|c| Some(&c.key) == doc_key.as_ref())
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
                    self.playback.adopted_at = None;
                    self.playback.playing = true;
                    self.playback.position_ms = self.playback.adopted_position_ms;
                    self.playback.position_at = self.now();
                    self.refresh_next();
                    self.stamp();
                    self.emit_transport();
                } else if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                    // A stale transition (we already reloaded what the document
                    // says) is ignored; otherwise follow the document.
                    if !(self.playback.loaded && self.playback.doc_key.as_ref() == Some(&item.key))
                    {
                        tracing::debug!(target: "hocket::player", key = %item.key, "the backend's transition never came: loading the document's item");
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
                    // The preloaded follow-up failed. Forget it: it is resolved
                    // afresh (with the usual retry and skip) when it comes up.
                    self.playback.next = None;
                    self.playback.next_doc_key = None;
                    if !self.playback.awaiting_transition {
                        // The current item is still playing: leave it alone.
                        return;
                    }
                    // The current one ended and the follow-up never started:
                    // move on explicitly.
                    self.playback.awaiting_transition = false;
                    if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                        if self.playback.doc_key.as_ref() == Some(&item.key) {
                            tracing::debug!(target: "hocket::player", key = %item.key, "the follow-up never started: loading it");
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
                    // One retry: transient stream errors are common. Not
                    // playing over a call, though, should one hold focus.
                    if let Some(item) = self.doc().and_then(|d| d.current.clone()) {
                        let failures = self.playback.load_failures;
                        let pos = self.playback.position_ms;
                        tracing::debug!(target: "hocket::player", key = %item.key, position_ms = pos, failures, "retrying after a playback error");
                        let play = self.playback.want_playing && !self.playback.focus_suspended;
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
            BackendReport::TransitionStarted {
                from,
                to,
                tail_len_ms,
                handover_ms,
                ..
            } => {
                let ours = current(&from, self)
                    && self.playback.next.as_ref().is_some_and(|n| n.key == to);
                if !ours {
                    return;
                }
                tracing::debug!(target: "hocket::player", %from, %to, tail_len_ms, handover_ms, "transition started");
                self.playback.transition = Some(crate::core::state::ActiveTransition { from, to });
            }
            BackendReport::PreBufferReady { key } => {
                self.engine_input(Input::PreBufferReady { key });
            }
            BackendReport::AudioFocusLost { transient } => {
                if !transient && (self.playback.playing || self.playback.focus_suspended) {
                    // Another app took focus for good, perhaps while a call
                    // already had us silent: stay stopped after it too.
                    self.playback.want_playing = false;
                    self.set_playing(false);
                    return;
                }
                if !self.playback.playing {
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
            BackendReport::PreviewReady { .. }
            | BackendReport::PreviewProgress { .. }
            | BackendReport::PreviewEnded
            | BackendReport::PreviewFailed
            | BackendReport::PreviewInterrupted => {}
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
        // The end-of-track mode never mixes out of the track it stops after.
        self.replan_next();
    }

    pub(crate) fn apply_sleep_action(&mut self, action: SleepAction) {
        match action {
            SleepAction::None => {}
            SleepAction::Fade { gain } => {
                self.playback.fade_gain = if gain >= 1.0 { None } else { Some(gain) };
                let _ = self.backend.set_volume(self.effective_volume());
            }
            SleepAction::Stop => {
                // The timer stops all audio here, and closing a preview
                // afterwards must not start the main player again.
                self.stop_preview(false);
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
        let mut state = derive_media_session_state(
            queue,
            transport,
            self.session_now(),
            art,
            self.owns_transport(),
            Some(actions),
        );
        if let Some(meta) = state.metadata.as_mut() {
            if let Some(id) = meta.track_id.as_deref() {
                meta.stashed = crate::stash::live(&self.stash)
                    .iter()
                    .any(|entry| entry.track_id == id);
            }
        }
        state
    }

    /// The OS media session's state, unless it would only repeat the last
    /// one sent (see [`Actor::emit_transport_state`]): each one rebuilds
    /// notifications, binder playlists and the watch's now-playing screen.
    pub(crate) fn emit_media_session(&mut self) {
        let queue = self.queue_head_view();
        let transport = self.transport_state();
        let state = self.media_session_state(&queue, &transport);
        if self
            .emitted_media_session
            .as_ref()
            .is_some_and(|last| crate::media_session::media_session_continues(last, &state))
        {
            return;
        }
        self.emitted_media_session = Some(state.clone());
        self.emit(Event::MediaSession { state });
    }

    fn media_art_size(&self) -> u32 {
        if self
            .battery_saver
            .has(crate::api::SaverFeature::SmallArtwork)
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
        self.fetch_artwork(cover, size);
    }

    pub(crate) fn media_session_command(&mut self, action: MediaSessionAction, value: Option<f64>) {
        let queue = self.queue_head_view();
        let transport = self.transport_state();
        let state = self.media_session_state(&queue, &transport);
        // Stash toggles: a song already in the stash comes out. `command_for`
        // only knows the track, not the entries listing it, so this lives here.
        if action == MediaSessionAction::Stash {
            if let Some(meta) = state.metadata.as_ref().filter(|m| m.stashed) {
                let Some(id) = meta.track_id.as_deref() else {
                    return;
                };
                let entry_ids: Vec<String> = self
                    .stash
                    .iter()
                    .filter(|e| !e.removed && e.track_id == id)
                    .map(|e| e.entry_id.clone())
                    .collect();
                if !entry_ids.is_empty() {
                    self.handle_command(Command::StashRemove { entry_ids });
                }
                return;
            }
        }
        if let Some(cmd) = command_for(action, value, &state) {
            self.handle_command(cmd);
        }
    }

    pub(crate) fn set_power_save(&mut self, enabled: bool) {
        if self.power_save != enabled {
            self.power_save = enabled;
            self.refresh_battery_saver();
        }
    }

    /// Battery saver from the power-save state and the `battery.*` settings;
    /// announced (and acted on) when it changes.
    pub(crate) fn refresh_battery_saver(&mut self) {
        use crate::settings::keys;
        let level = |key| crate::saver::parse_level(&self.settings.get(key));
        let saver = crate::saver::resolve(
            level(keys::BATTERY_LEVEL).unwrap_or(crate::api::SaverLevel::Off),
            level(keys::BATTERY_AUTO_LEVEL).unwrap_or(crate::api::SaverLevel::Medium),
            self.settings.get_bool(keys::BATTERY_AUTO_ENGAGE),
            self.power_save,
            &crate::saver::parse_custom(&self.settings.get(keys::BATTERY_CUSTOM)),
        );
        if saver == self.battery_saver {
            return;
        }
        let was = std::mem::replace(&mut self.battery_saver, saver.clone());
        self.emit(Event::BatterySaverChanged { saver });
        if was.has(crate::api::SaverFeature::SmallArtwork)
            != self
                .battery_saver
                .has(crate::api::SaverFeature::SmallArtwork)
        {
            self.media_art = None;
            self.emit_media_session();
        }
        let slow_sync = self.battery_saver.has(crate::api::SaverFeature::SlowSync);
        if slow_sync != was.has(crate::api::SaverFeature::SlowSync) && self.engine.is_some() {
            self.engine_input(Input::SetSlowSync(slow_sync));
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
