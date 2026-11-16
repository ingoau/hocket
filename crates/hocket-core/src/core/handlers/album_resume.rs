//! Album resume (see `crate::album_resume`): the actor keeps the account's map, persists it
//! under `album-resume:{scope}` and hands every local change to the engine, which syncs it.
//! Remote copies come back as `Output::AlbumResumeMerged`.

use crate::album_resume::{self, Snapshot};
use crate::api::*;
use crate::connect::engine::Input;
use crate::connect::wire::TransportCommand;
use crate::core::actor::Actor;
use crate::settings::keys;

impl Actor {
    pub(crate) fn load_album_resumes(&mut self) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        self.album_resumes = match self
            .db
            .saved_state_get::<Vec<AlbumResumeEntry>>(&album_resume::state_key(&scope))
        {
            Ok(list) => list.unwrap_or_default(),
            Err(e) => {
                self.log(
                    "warn",
                    format!("album resume: unreadable, starting empty: {e}"),
                );
                vec![]
            }
        };
        let now = self.now();
        album_resume::prune(&mut self.album_resumes, now);
        if !self.album_resumes.is_empty() {
            self.engine_input(Input::AlbumResumeChanged(self.album_resumes.clone()));
        }
        self.emit_album_resumes();
    }

    pub(crate) fn drop_album_resumes(&mut self) {
        self.pending_resume_at = None;
        if !self.album_resumes.is_empty() {
            self.album_resumes.clear();
            self.emit_album_resumes();
        }
    }

    fn save_album_resumes(&mut self) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let now = self.now();
        album_resume::prune(&mut self.album_resumes, now);
        if let Err(e) = self.db.saved_state_set(
            &album_resume::state_key(&scope),
            &self.album_resumes,
            self.clock.as_ref(),
        ) {
            self.log("warn", format!("album resume: couldn't save: {e}"));
        }
    }

    pub(crate) fn emit_album_resumes(&mut self) {
        let points = self.resume_points();
        // When the oldest offered point runs out, the tick announces the shorter list.
        self.album_resume_expires_at = points
            .iter()
            .map(|p| p.updated_at + album_resume::TTL_MS)
            .min_by(|a, b| a.total_cmp(b));
        self.emit(Event::AlbumResumesChanged { points });
    }

    /// Called from the tick: re-announce once a point has passed its expiry.
    pub(crate) fn album_resume_tick(&mut self, now: EpochMs) {
        if self.album_resume_expires_at.is_some_and(|at| now >= at) {
            self.emit_album_resumes();
        }
    }

    pub(crate) fn resume_points(&self) -> Vec<AlbumResumePoint> {
        if !self.settings.get_bool(keys::PLAYBACK_RESUME_ALBUMS) {
            return vec![];
        }
        album_resume::points(&self.album_resumes, self.now())
    }

    pub(crate) fn album_resume_point(&self, album_id: &str) -> Option<AlbumResumePoint> {
        if !self.settings.get_bool(keys::PLAYBACK_RESUME_ALBUMS) {
            return None;
        }
        album_resume::point(&self.album_resumes, album_id, self.now())
    }

    /// `sync` tells other devices. Position samples while an album plays stay on this
    /// device; pause, a track change, stop and a clear are what travel.
    fn album_resume_changed(&mut self, sync: bool) {
        self.save_album_resumes();
        if sync {
            self.emit_album_resumes();
            self.engine_input(Input::AlbumResumeChanged(self.album_resumes.clone()));
        }
    }

    pub(crate) fn on_album_resume_merged(&mut self, list: Vec<AlbumResumeEntry>) {
        let (mut merged, _) = album_resume::merge(&self.album_resumes, &list);
        album_resume::prune(&mut merged, self.now());
        if merged != self.album_resumes {
            self.album_resumes = merged;
            self.save_album_resumes();
            self.emit_album_resumes();
        }
    }

    /// Clear the album's point and tell other devices. `tombstone` (Start over, an album
    /// transition, finishing the album, a pause or seek on its first track) records the clear
    /// even when this device has no live point, so it beats an older save another device made
    /// offline. A periodic sample passes false so it does not rewrite the set every tick.
    pub(crate) fn clear_album_resume(&mut self, album_id: &str, tombstone: bool) {
        let now = self.now();
        if album_resume::clear(&mut self.album_resumes, album_id, now, tombstone) {
            self.album_resume_changed(true);
        }
    }

    /// Continue `album_id` at its saved track and position, or from the start when there is
    /// nothing to resume. Already being in that album just continues when some device still
    /// holds the track: playback is already at the live position, and rebuilding the queue
    /// would jump back to the last sample. Stop rewinds and gives the track up, so that case
    /// falls through and loads the saved point instead of the rewound 0.
    pub(crate) fn resume_album(&mut self, album_id: AlbumId) {
        let in_album = self.doc().is_some_and(|d| {
            matches!(&d.context, Some(c) if matches!(&c.kind, ContextKind::Album { id } if id == &album_id))
        });
        let held_here = self.owns_transport() && self.playback.loaded;
        let held_elsewhere = self
            .engine
            .as_ref()
            .is_some_and(|e| e.remote_owner().is_some());
        if in_album && (held_here || held_elsewhere) {
            if held_here {
                self.capture_album_resume(true);
            }
            self.playback.want_playing = true;
            self.transport_command(TransportCommand::Play);
            return;
        }
        let point = self.album_resume_point(&album_id);
        let kind = ContextKind::Album {
            id: album_id.clone(),
        };
        let source = point
            .as_ref()
            .map(|p| p.source.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| self.source_of_context(&kind))
            .or_else(|| self.scope_account())
            .unwrap_or_default();
        let mut args = PlayContextArgs {
            context: QueueContext {
                source,
                kind,
                label: String::new(),
                sort: SortOrder::Default,
                tracks: vec![],
            },
            start_index: None,
            shuffle: false,
            save_outgoing: true,
            start_position_ms: None,
        };
        if let Some(p) = point {
            if args.context.tracks.is_empty() {
                args.context.tracks =
                    self.resolve_context_tracks(&args.context.source, &args.context.kind);
            }
            let n = args.context.tracks.len() as u32;
            let index = args
                .context
                .tracks
                .iter()
                .position(|t| t.id == p.track_id && t.source == args.context.source)
                .map(|i| i as u32)
                .filter(|i| *i < n)
                .or((p.track_index < n).then_some(p.track_index));
            if let Some(index) = index {
                args.start_index = Some(index);
                args.start_position_ms = Some(p.position_ms);
                self.pending_resume_at = Some((
                    TrackRef::new(args.context.source.clone(), p.track_id),
                    p.position_ms,
                ));
            }
        }
        self.play_context(args);
    }

    /// This device is the one playing (or is about to be: a local play claims an idle lease).
    fn records_album_resume(&self) -> bool {
        if self.owns_transport() {
            return true;
        }
        let nobody = self
            .engine
            .as_ref()
            .map(|e| e.remote_owner().is_none())
            .unwrap_or(true);
        nobody && (self.playback.playing || self.playback.want_playing)
    }

    /// The album's queue ran out (stopped, or autoplay was asked for). Drop its resume point.
    pub(crate) fn finish_album_resume(&mut self) {
        if !self.records_album_resume() {
            return;
        }
        let Some(snap) = self.album_snapshot(true) else {
            return;
        };
        self.consider_snapshot(snap, true);
    }

    /// Sample the album that is playing. `precise` writes the position even when it has barely
    /// moved (a pause, a seek, a track change).
    pub(crate) fn capture_album_resume(&mut self, precise: bool) {
        if !self.records_album_resume() {
            return;
        }
        let Some(snap) = self.album_snapshot(false) else {
            return;
        };
        self.consider_snapshot(snap, precise);
    }

    /// The playing album, without cloning the session document.
    fn album_snapshot(&self, finished: bool) -> Option<Snapshot> {
        let doc = self.doc()?;
        let same = doc.current.as_ref().map(|c| c.track.clone()) == self.playback.track_ref();
        let position = self.playback.position_now(self.now());
        let duration = if same { self.playback.duration_ms() } else { 0 };
        Some(snapshot_of(doc, position, duration, finished))
    }

    /// A queue change on the device that plays (a local one, or a remote one while this
    /// device holds the transport). The outgoing album is sampled at the position still loaded, and
    /// the album now playing at `resume_at` (a resume) or at the start of its track.
    pub(crate) fn note_album_transition(
        &mut self,
        old: Option<&SessionDocument>,
        new: &SessionDocument,
        key_changed: bool,
        ended: bool,
        resume_at: Option<Ms>,
    ) {
        if !self.records_album_resume() {
            return;
        }
        if key_changed {
            if let Some(old) = old {
                let same =
                    old.current.as_ref().map(|c| c.track.clone()) == self.playback.track_ref();
                let duration = if same { self.playback.duration_ms() } else { 0 };
                let position = self.playback.position_now(self.now());
                self.consider_album(old, position, duration, false, true);
            }
            self.consider_album(new, resume_at.unwrap_or(0), 0, false, true);
        } else {
            let same = new.current.as_ref().map(|c| c.track.clone()) == self.playback.track_ref();
            let duration = if same { self.playback.duration_ms() } else { 0 };
            let position = self.playback.position_now(self.now());
            self.consider_album(new, position, duration, ended, true);
        }
    }

    /// Take a pending resume position when `track_id` is the track it was for.
    pub(crate) fn take_pending_resume(&mut self, track: Option<&TrackRef>) -> Option<Ms> {
        let (id, pos) = self.pending_resume_at.take()?;
        if Some(&id) == track {
            Some(pos)
        } else {
            None
        }
    }

    fn consider_album(
        &mut self,
        doc: &SessionDocument,
        position_ms: Ms,
        duration_ms: Ms,
        finished: bool,
        precise: bool,
    ) {
        self.consider_snapshot(
            snapshot_of(doc, position_ms, duration_ms, finished),
            precise,
        );
    }

    fn consider_snapshot(&mut self, snap: Snapshot, precise: bool) {
        let enabled = self.settings.get_bool(keys::PLAYBACK_RESUME_ALBUMS);
        match album_resume::decide(&snap, self.now(), enabled) {
            album_resume::Decision::Ignore => {}
            album_resume::Decision::Clear { album_id } => {
                self.clear_album_resume(&album_id, precise)
            }
            album_resume::Decision::Save(entry) => {
                if !precise {
                    if let Some(cur) = self
                        .album_resumes
                        .iter()
                        .find(|e| e.album_id == entry.album_id && !e.removed)
                    {
                        if cur.track_id == entry.track_id
                            && entry.position_ms.abs_diff(cur.position_ms)
                                < album_resume::POSITION_QUANTUM_MS
                        {
                            return;
                        }
                    }
                }
                if album_resume::upsert(&mut self.album_resumes, entry) {
                    // `precise` is a pause, seek, track change or stop: that is what other
                    // devices need. A sample while the album plays stays on this device.
                    self.album_resume_changed(precise);
                }
            }
        }
    }
}

fn snapshot_of(
    doc: &SessionDocument,
    position_ms: Ms,
    duration_ms: Ms,
    finished: bool,
) -> Snapshot {
    let (album_id, track_count) = match &doc.context {
        Some(c) => {
            let id = match &c.kind {
                ContextKind::Album { id } => Some(id.clone()),
                _ => None,
            };
            (id, c.tracks.len() as u32)
        }
        None => (None, 0),
    };
    let (from_context, index, autoplay, track_id) = match &doc.current {
        Some(cur) => match &cur.source {
            QueueSource::Context { index } => (true, Some(*index), false, cur.track.clone()),
            QueueSource::Autoplay { .. } => (false, None, true, cur.track.clone()),
            _ => (false, None, false, cur.track.clone()),
        },
        None => (false, None, false, TrackRef::default()),
    };
    Snapshot {
        album_id,
        shuffled: doc.shuffle,
        track_count,
        from_context,
        index,
        autoplay,
        track_id: track_id.id,
        source: track_id.source,
        position_ms,
        duration_ms,
        finished,
    }
}
