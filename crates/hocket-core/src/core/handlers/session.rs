//! Session: the Connect engine owns the document; every queue mutation is a
//! `SessionOp` submitted through it. The actor computes the reducer's
//! effects for its own ops (same reducer, same document), acts on them once
//! the engine reports `DocumentChanged`, and diffs documents for changes that
//! arrive from peers.

use std::collections::HashMap;

use crate::actions::{Resolver, StateView};
use crate::api::*;
use crate::connect::engine::{DocChange, Engine, EngineConfig, Input, Output, PersistedConnectState};
use crate::connect::session_adapter::TunableReducer;
use crate::connect::wire::{scope_key, SessionOp, TransportCommand};
use crate::core::actor::{Actor, AUTOPLAY_BATCH};
use crate::core::state::SavedPosition;
use crate::session::reducer::{derive, reduce, QueueOp, ReduceCtx};
use crate::session::{saved, Effect};
use crate::undo::{SessionUndo, UndoAction, UndoTier};

/// Resolver over the mirror for the action registry.
pub(crate) struct DbResolver<'a> {
    pub actor: &'a Actor,
}

impl Resolver for DbResolver<'_> {
    fn track_for_key(&self, key: &QueueKey) -> Option<TrackId> {
        let doc = self.actor.engine.as_ref()?.document();
        if let Some(c) = &doc.current {
            if &c.key == key {
                return Some(c.track_id.clone());
            }
        }
        for i in doc.history.iter().chain(doc.insertions.iter()) {
            if &i.key == key {
                return Some(i.track_id.clone());
            }
        }
        derive(doc)
            .upcoming
            .into_iter()
            .find(|i| &i.key == key)
            .map(|i| i.track_id)
    }

    fn context_tracks(&self, server_id: &str, kind: &ContextKind) -> Vec<TrackId> {
        self.actor.resolve_context_tracks(server_id, kind)
    }

    fn context_label(&self, server_id: &str, kind: &ContextKind) -> String {
        self.actor.context_label(server_id, kind)
    }
}

impl Actor {
    // -- session setup ----------------------------------------------------------

    /// Open (or create) the session for the attached server's scope and
    /// build the Connect engine around it.
    pub(crate) fn open_session(&mut self) {
        let Some(server) = &self.server else { return };
        let scope = scope_key(&server.info.url, &server.info.username);
        let credential = server.credential.clone();
        let doc = match self
            .db
            .saved_state_get_raw(&format!("session:{scope}"))
        {
            Ok(Some(json)) => match crate::session::load(&json) {
                Ok(d) if crate::session::validate(&d).is_ok() && d.scope == scope => d,
                Ok(_) => {
                    self.log("warn", "stored session document invalid; starting fresh");
                    crate::session::new_document(&scope, crate::util::new_id(), self.now())
                }
                Err(e) => {
                    self.log("warn", format!("session document unreadable: {e}"));
                    crate::session::new_document(&scope, crate::util::new_id(), self.now())
                }
            },
            _ => crate::session::new_document(&scope, crate::util::new_id(), self.now()),
        };
        let sync_base = self
            .db
            .saved_state_get::<PersistedConnectState>(&format!("connect:{scope}"))
            .ok()
            .flatten()
            .and_then(|s| s.sync_base);
        let device = DeviceInfo {
            id: self.cfg.device_id.clone(),
            name: self.cfg.device_name.clone(),
            platform: self.cfg.platform,
            app_version: self.cfg.app_version.clone(),
            playing: false,
            ready: false,
            last_seen: self.now(),
            is_self: true,
        };
        let mut cfg = EngineConfig::new(device, scope.clone());
        cfg.credential = credential;
        cfg.coordinator_url = self
            .settings
            .get_string(crate::settings::keys::CONNECT_COORDINATOR_URL);
        cfg.lan_enabled = self
            .settings
            .get_bool(crate::settings::keys::CONNECT_LAN_DISCOVERY)
            && self.cfg.coordinator_listen.is_none();
        let reducer = TunableReducer::shared(self.reducer_policy.clone());
        let engine = Engine::new(cfg, self.clock.clone(), reducer, doc.clone(), sync_base);
        self.last_saved_queues = doc.saved_queues.clone();
        self.last_doc = Some(doc.clone());
        self.engine = Some(engine);
        self.scope = Some(scope.clone());
        // Saved queues and synced settings ride the engine's LWW sets.
        if !doc.saved_queues.is_empty() {
            self.engine_input(Input::SavedQueuesChanged(doc.saved_queues.clone()));
        }
        for s in self.settings.synced_settings() {
            self.engine_input(Input::SettingChanged(s));
        }
        // Restore the resume point: the last known item and position, dormant
        // (never auto-resume on start).
        if let Ok(Some(pos)) = self
            .db
            .saved_state_get::<SavedPosition>(&format!("position:{scope}"))
        {
            if let (Some(key), Some(cur)) = (pos.key, doc.current.as_ref()) {
                if key == cur.key {
                    self.playback.restore_position = Some((key, pos.position_ms));
                    self.playback.position_ms = pos.position_ms;
                }
            }
        }
        if let Some(cur) = doc.current.as_ref() {
            self.playback.doc_key = Some(cur.key.clone());
            self.playback.track = self.track_or_bare(&cur.track_id);
        }
    }

    // -- engine plumbing ----------------------------------------------------------

    /// Feed the engine and act on its outputs, including inputs the
    /// reactions queue.
    pub(crate) fn engine_input(&mut self, input: Input) {
        self.queued_inputs.push_back(input);
        let mut guard = 0;
        while let Some(input) = self.queued_inputs.pop_front() {
            let outs = match &mut self.engine {
                Some(e) => e.handle(input),
                None => return,
            };
            for o in outs {
                self.react(o);
            }
            guard += 1;
            if guard > 10_000 {
                self.log("error", "engine reaction loop did not converge");
                self.queued_inputs.clear();
                break;
            }
        }
    }

    pub(crate) fn queue_input(&mut self, input: Input) {
        self.queued_inputs.push_back(input);
    }

    pub(crate) fn doc(&self) -> Option<&SessionDocument> {
        self.engine.as_ref().map(|e| e.document())
    }

    pub(crate) fn owns_transport(&self) -> bool {
        self.engine.as_ref().is_some_and(|e| e.owns_transport())
    }

    /// Submit a local op. Effects the reducer reports are acted on when the
    /// engine hands back the changed document.
    pub(crate) fn local_op(&mut self, op: SessionOp) -> bool {
        let Some(engine) = &self.engine else {
            self.toast("Add a server first", None);
            return false;
        };
        let before = engine.document().clone();
        // Same reducer, same document: the effects are exactly what the
        // engine's apply will produce (keys differ, the document tells us those).
        let effects = crate::connect::session_adapter::to_queue_op(&op).and_then(|qop| {
            let (history_cap, saved) = self.reducer_policy.get();
            let ctx = ReduceCtx {
                now: engine.now_session_ms(),
                position_ms: self.playback.position_now(self.now()),
                history_cap,
                saved,
                entropy: self.entropy.as_ref(),
            };
            match reduce(&before, qop, &ctx) {
                Ok((_, fx)) => Some(fx),
                Err(e) => {
                    tracing::debug!(target: "hocket_core", "op not applicable: {e}");
                    None
                }
            }
        });
        if effects.is_none() && !matches!(op, SessionOp::Replace { .. }) {
            self.toast("That doesn't apply right now", None);
            return false;
        }
        self.pending_effects = effects;
        self.engine_input(Input::LocalOp { op });
        self.pending_effects = None;
        true
    }

    /// A public queue command: undo snapshot around the op.
    pub(crate) fn queue_command(&mut self, cmd: Command) {
        let Some(op) = SessionOp::from_command(&cmd) else { return };
        let label = command_label(&cmd);
        self.undoable_op(op, &label, command_kind(&cmd));
    }

    pub(crate) fn undoable_op(&mut self, op: SessionOp, label: &str, kind: &str) {
        let Some(before) = self.doc().cloned() else {
            self.toast("Add a server first", None);
            return;
        };
        let mut before_snapshot = before.clone();
        // Undo of an accidental replacement lands on the outgoing track at
        // the position it was at.
        before_snapshot.transport.position.position_ms = self.playback.position_now(self.now());
        if matches!(
            op,
            SessionOp::PlayContext { .. }
                | SessionOp::PlayTracks { .. }
                | SessionOp::Next
                | SessionOp::Previous
                | SessionOp::JumpToQueueItem { .. }
                | SessionOp::RestoreSavedQueue { .. }
        ) {
            self.playback.want_playing = true;
        }
        if !self.local_op(op) {
            return;
        }
        let after = self.doc().cloned().unwrap_or(before.clone());
        if crate::connect::same_session_state(&before, &after) {
            return;
        }
        let now = self.now();
        let id = self.undo.push(
            Box::new(SessionUndo::new(kind, label, before_snapshot.clone(), after)),
            self.selection.clone(),
            now,
        );
        if let Some(entry) = self.undo.entries().get(&id).cloned() {
            self.queue_input(Input::UndoEntryCreated {
                entry,
                before_revision: before.revision,
                before: Some(before_snapshot),
            });
            self.engine_input(Input::Tick);
        }
        self.emit(Event::UndoChanged {
            state: self.undo.state(),
        });
    }

    // -- engine outputs ---------------------------------------------------------

    pub(crate) fn react(&mut self, o: Output) {
        match o {
            Output::WireOut { peer, msg } => {
                if let Some(tx) = self.conns.get(&peer) {
                    let _ = tx.send(msg);
                }
            }
            Output::Connect { candidates } => self.io_connect(candidates),
            Output::Disconnect { peer } => {
                self.conns.remove(&peer);
            }
            Output::StartListener => self.io_start_listener(),
            Output::StopListener => self.io_stop_listener(),
            Output::Advertise(a) => self.io_advertise(a),
            Output::VerifyCredential { peer, credential } => self.io_verify(peer, credential),
            Output::DocumentChanged { document, cause } => self.on_document_changed(document, cause),
            Output::TransportChanged { transport } => {
                self.last_transport = transport;
                self.emit_transport();
            }
            Output::LeaseChanged {
                owns,
                detached,
                lease,
            } => self.on_lease_changed(owns, detached, lease),
            Output::ConnectionChanged(state) => self.emit(Event::ConnectionChanged { state }),
            Output::DevicesChanged(devices) => self.emit(Event::DevicesChanged { devices }),
            Output::PickerChanged { open, targets } => {
                self.picker_open = open;
                self.picker_targets = targets.clone();
                self.emit(Event::HandoffPickerChanged { open, targets });
            }
            Output::ResumeOffer(draft) => {
                self.resume_offer = draft.map(|d| ResumeOffer {
                    device_name: d.device_name,
                    track: self.summary_or_bare(&d.track_id),
                    position_ms: d.position_ms,
                    last_seen: d.last_seen,
                });
                self.emit(Event::ResumeOfferChanged {
                    offer: self.resume_offer.clone(),
                });
            }
            Output::FilePreviousStateAsSavedQueue { document, reason } => {
                self.log("info", format!("filing diverged state as a saved queue: {reason}"));
                let now = self.now();
                let position = document.transport.position.position_ms;
                if let Some(mut sq) = saved::snapshot(&document, position, now, crate::util::new_id()) {
                    sq.updated_at = self.session_now();
                    sq.label = format!("{} (from {})", sq.label, self.cfg.device_name);
                    sq.cover_art = self.context_cover(&sq.context);
                    self.local_op(SessionOp::MergeSavedQueues { remote: vec![sq] });
                }
            }
            Output::PreBuffer {
                key,
                track_id,
                position_ms,
            } => self.pre_buffer(key, track_id, position_ms),
            Output::DiscardPreBuffer => {
                if let Err(e) = self.backend.discard_pre_buffer() {
                    self.log("debug", format!("discard_pre_buffer: {e}"));
                }
            }
            Output::TakeTransport {
                key,
                track_id,
                position_ms,
                played_ms,
                started_at,
                scrobbled,
                play,
            } => self.take_transport(key, track_id, position_ms, played_ms, started_at, scrobbled, play),
            Output::ReleaseTransport => self.release_transport(),
            Output::TransportCommand(cmd) => self.apply_transport_command(cmd),
            Output::Scrobble {
                track_id,
                started_at,
                allowed,
            } => self.on_scrobble_verdict(track_id, started_at, allowed),
            Output::SavedQueuesMerged(list) => {
                let ours = self.doc().map(|d| d.saved_queues.clone()).unwrap_or_default();
                if !same_saved_set(&ours, &list) {
                    self.local_op(SessionOp::MergeSavedQueues { remote: list });
                }
            }
            Output::SettingsMerged(list) => self.on_settings_merged(list),
            Output::UndoEntryReceived {
                entry,
                before_revision: _,
                before,
            } => {
                let after = self.doc().cloned();
                if let (Some(before), Some(after)) = (before, after) {
                    let now = self.now();
                    self.undo.push_from(
                        Box::new(SessionUndo::new("peer", entry.label.clone(), before, after)),
                        ActionTarget::None,
                        now,
                        entry.device_id,
                    );
                    self.emit(Event::UndoChanged {
                        state: self.undo.state(),
                    });
                }
            }
            Output::ReplicaChanged(_) => {}
            Output::Log { level, message } => self.log(level, message),
        }
    }

    // -- document changes -------------------------------------------------------

    fn on_document_changed(&mut self, document: SessionDocument, cause: DocChange) {
        let old = self.last_doc.replace(document.clone());
        let effects = match cause {
            DocChange::Local => self.pending_effects.take().unwrap_or_default(),
            _ => vec![],
        };
        let old_key = old.as_ref().and_then(|d| d.current.as_ref().map(|c| c.key.clone()));
        let new_key = document.current.as_ref().map(|c| c.key.clone());
        let owns = self.owns_transport();
        let nobody_owns = self
            .engine
            .as_ref()
            .map(|e| e.lease().owner.is_none())
            .unwrap_or(true);

        let mut restart = false;
        let mut stopped = false;
        let mut needs_autoplay = false;
        let mut resolve: Option<QueueContext> = None;
        let mut restore_position: Option<Ms> = None;
        let mut skipped: Vec<QueueKey> = vec![];
        for fx in &effects {
            match fx {
                Effect::RestartCurrent => restart = true,
                Effect::Stopped => stopped = true,
                Effect::NeedsAutoplay => needs_autoplay = true,
                Effect::ResolveContext { context } => resolve = Some(context.clone()),
                Effect::PositionRestore { position_ms } => restore_position = Some(*position_ms),
                Effect::Skipped { key } => skipped.push(key.clone()),
                Effect::CurrentChanged { position_ms, .. } if *position_ms > 0 => {
                    restore_position = Some(*position_ms)
                }
                _ => {}
            }
        }
        for key in skipped {
            let name = self.track_title_for_key(&document, &key);
            self.emit(Event::PlayerNotice {
                message: Some(format!("Couldn't play {name}, skipped")),
            });
        }

        if new_key != old_key {
            match document.current.clone() {
                Some(item) => {
                    let position = self
                        .playback
                        .restore_position
                        .take()
                        .filter(|(k, _)| *k == item.key)
                        .map(|(_, p)| p)
                        .or(restore_position)
                        .unwrap_or(0);
                    if owns {
                        let keep_playing = match cause {
                            DocChange::Local => self.playback.want_playing || self.playback.playing,
                            _ => self.playback.playing || self.playback.awaiting_transition,
                        };
                        self.load_item(&item, position, keep_playing, None);
                    } else if nobody_owns && cause == DocChange::Local && self.playback.want_playing {
                        // Nobody is playing and the user pressed play here.
                        self.playback.doc_key = Some(item.key.clone());
                        self.playback.track = self.track_or_bare(&item.track_id);
                        self.playback.restore_position = Some((item.key.clone(), position));
                        self.queue_input(Input::ClaimTransport { takeover: false });
                    } else {
                        // Another device plays; we only render.
                        self.playback.doc_key = Some(item.key.clone());
                        self.playback.track = self.track_or_bare(&item.track_id);
                        self.playback.loaded = false;
                    }
                }
                None => {
                    self.unload();
                }
            }
        } else if restart && owns {
            self.restart_current();
        } else if stopped && owns {
            self.playback.want_playing = false;
            self.set_playing(false);
        }
        if document.current.is_some() && (new_key == old_key) && owns && self.playback.loaded {
            // Queue edits around the current item: keep the gapless preload right.
            self.refresh_next();
        }
        if let Some(context) = resolve {
            let tracks = self.resolve_context_tracks(&context.server_id, &context.kind);
            if !tracks.is_empty() {
                self.local_op(SessionOp::SetContextTracks { tracks });
                return;
            }
        }
        if needs_autoplay && owns {
            self.request_autoplay();
        }
        // Saved queues ride the engine's LWW set too.
        if !same_saved_set(&document.saved_queues, &self.last_saved_queues) {
            self.last_saved_queues = document.saved_queues.clone();
            self.emit(Event::SavedQueuesChanged {
                queues: document.saved_queues.clone(),
            });
            let engine_has = self
                .engine
                .as_ref()
                .map(|e| e.saved_queues().to_vec())
                .unwrap_or_default();
            if !same_saved_set(&engine_has, &document.saved_queues) {
                self.queue_input(Input::SavedQueuesChanged(document.saved_queues.clone()));
            }
        }
        self.mark_doc_dirty();
        self.emit(Event::SessionChanged { document });
        self.emit_queue();
    }

    pub(crate) fn emit_queue(&mut self) {
        let queue = self.queue_view();
        self.emit(Event::NowPlayingChanged {
            entry: queue.current.clone(),
        });
        self.emit(Event::QueueChanged { queue });
        self.emit_media_session();
    }

    /// Ask the autoplay chain for a batch; `AppendAutoplay` + `Next` follow.
    pub(crate) fn request_autoplay(&mut self) {
        let Some(engine) = self.autoplay.take() else {
            self.autoplay_wanted = true;
            return;
        };
        let Some(api) = self.api() else {
            self.autoplay = Some(engine);
            return;
        };
        let Some(doc) = self.doc().cloned() else {
            self.autoplay = Some(engine);
            return;
        };
        self.autoplay_wanted = false;
        self.autoplay_generation += 1;
        let generation = self.autoplay_generation;
        let mut recent_ids: Vec<TrackId> = vec![];
        if let Some(c) = &doc.current {
            recent_ids.push(c.track_id.clone());
        }
        recent_ids.extend(doc.history.iter().rev().map(|i| i.track_id.clone()));
        let recent: Vec<TrackSummary> = recent_ids
            .iter()
            .take(20)
            .map(|id| self.summary_or_bare(id))
            .collect();
        let mut exclude: Vec<TrackId> = doc
            .context
            .as_ref()
            .map(|c| c.tracks.clone())
            .unwrap_or_default();
        exclude.extend(doc.insertions.iter().map(|i| i.track_id.clone()));
        exclude.extend(recent_ids);
        let seeds = crate::autoplay::SeedInput {
            recent,
            context: doc.context.as_ref().map(|c| crate::autoplay::ContextClass::of(&c.kind)),
            exclude,
        };
        let source = crate::core::handlers::library::DbAutoplaySource {
            api,
            db: self.db.clone(),
            server_id: self.server_id().unwrap_or_default(),
            now: self.now(),
            filters: self.filters(),
        };
        let tx = self.tx.clone();
        self.spawn(async move {
            let mut engine = engine;
            let picks = engine.next_batch(&source, &seeds, AUTOPLAY_BATCH).await;
            let _ = tx.send(crate::core::ActorMsg::Internal(crate::core::Internal::Autoplay {
                engine,
                picks,
                generation,
            }));
        });
    }

    pub(crate) fn on_autoplay_picks(
        &mut self,
        engine: Box<crate::autoplay::AutoplayEngine>,
        picks: Vec<crate::autoplay::AutoplayPick>,
        generation: u32,
    ) {
        self.autoplay = Some(engine);
        if let Some(a) = &self.autoplay {
            let _ = self
                .db
                .saved_state_set("autoplay:exclusion", &a.exclusion_ids(), self.clock.as_ref());
        }
        if generation != self.autoplay_generation {
            return;
        }
        if picks.is_empty() {
            self.emit(Event::PlayerNotice {
                message: Some("Autoplay found nothing to add".into()),
            });
            return;
        }
        let items = picks
            .iter()
            .map(|p| crate::connect::wire::AutoplayOpItem {
                track_id: p.track.id.clone(),
                provider: p.provider,
                reason: p.reason.clone(),
                score: p.score,
            })
            .collect();
        // Cache the summaries so the queue view can show what the server sent.
        let tracks: Vec<Track> = picks
            .iter()
            .filter(|p| self.db.track(&p.track.id).ok().flatten().is_none())
            .map(|p| track_from_summary(&p.track))
            .collect();
        if !tracks.is_empty() {
            let changed = vec![None; tracks.len()];
            let _ = self.db.upsert_tracks(&tracks, &changed, 0);
        }
        let queue_ran_out = self
            .doc()
            .map(|d| derive(d).total_upcoming == 0)
            .unwrap_or(false);
        self.local_op(SessionOp::AppendAutoplay { items });
        if queue_ran_out && self.owns_transport() {
            self.playback.want_playing = true;
            self.local_op(SessionOp::Next);
        }
        if self.autoplay_wanted {
            self.autoplay_wanted = false;
        }
    }

    // -- public queue commands ----------------------------------------------------

    pub(crate) fn play_context(&mut self, mut args: PlayContextArgs) {
        if args.context.tracks.is_empty() && saved::is_id_referenced(&args.context.kind) {
            args.context.tracks =
                self.resolve_context_tracks(&args.context.server_id, &args.context.kind);
        }
        if args.context.label.is_empty() {
            args.context.label = self.context_label(&args.context.server_id, &args.context.kind);
        }
        if args.context.tracks.is_empty() {
            self.toast("Nothing to play", None);
            return;
        }
        let label = format!("Play {}", args.context.label);
        self.undoable_op(SessionOp::PlayContext { args }, &label, "playContext");
    }

    pub(crate) fn restore_saved_queue(&mut self, id: String) {
        let Some(doc) = self.doc() else { return };
        let Some(sq) = doc.saved_queues.iter().find(|q| q.id == id).cloned() else {
            self.toast("That saved queue is gone", None);
            return;
        };
        let tracks = if saved::needs_resolution(&sq) {
            let t = self.resolve_context_tracks(&sq.context.server_id, &sq.context.kind);
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        } else {
            None
        };
        let label = format!("Restore {}", sq.label);
        self.undoable_op(SessionOp::RestoreSavedQueue { id, tracks }, &label, "restoreSavedQueue");
    }

    pub(crate) fn previous(&mut self) {
        let position = self.playback.position_now(self.now());
        if self.playback.loaded && crate::session::previous_should_restart(position) {
            // Previous after a few seconds restarts the track (Symfonium's behaviour).
            if self.owns_transport() {
                self.seek_to(0);
            } else {
                self.transport_command(TransportCommand::SeekTo { position_ms: 0 });
            }
            return;
        }
        self.queue_command(Command::Previous);
    }

    pub(crate) fn touch(&mut self, target: ActionTarget) {
        if let ActionTarget::SavedQueue { id } = target {
            self.local_op(SessionOp::TouchSavedQueue { id });
        }
    }

    pub(crate) fn save_queue_as_playlist(&mut self, saved_queue_id: Option<String>, name: String) {
        let Some(doc) = self.doc().cloned() else { return };
        let Some(server_id) = self.server_id() else { return };
        let ids: Vec<TrackId> = match saved_queue_id {
            Some(id) => {
                let Some(sq) = doc.saved_queues.iter().find(|q| q.id == id) else {
                    self.toast("That saved queue is gone", None);
                    return;
                };
                let mut tracks = sq.context.tracks.clone();
                if tracks.is_empty() {
                    tracks = self.resolve_context_tracks(&sq.context.server_id, &sq.context.kind);
                }
                let mut ids: Vec<TrackId> = sq.history.iter().map(|i| i.track_id.clone()).collect();
                ids.extend(sq.current.iter().map(|i| i.track_id.clone()));
                ids.extend(sq.insertions.iter().map(|i| i.track_id.clone()));
                let perm = crate::session::shuffle::Permutation::from_state(
                    sq.shuffle.as_ref(),
                    tracks.len(),
                );
                for pos in sq.cursor..perm.len() as u32 {
                    if let Some(i) = perm.to_context(pos) {
                        if let Some(t) = tracks.get(i as usize) {
                            ids.push(t.clone());
                        }
                    }
                }
                ids
            }
            None => {
                let d = derive(&doc);
                d.history
                    .iter()
                    .chain(d.current.iter())
                    .chain(d.playing_next.iter())
                    .chain(d.upcoming.iter())
                    .map(|i| i.track_id.clone())
                    .collect()
            }
        };
        if ids.is_empty() {
            self.toast("Nothing to save", None);
            return;
        }
        self.create_playlist(server_id, name, ids);
    }

    // -- undo -------------------------------------------------------------------

    pub(crate) fn undo(&mut self) {
        let Some(result) = self.undo.undo() else {
            return;
        };
        self.perform_undo_action(result, false);
    }

    pub(crate) fn redo(&mut self) {
        let Some(result) = self.undo.redo() else {
            return;
        };
        self.perform_undo_action(result, true);
    }

    pub(crate) fn undo_to(&mut self, id: String) {
        let results = self.undo.undo_to(&id);
        for r in results {
            self.perform_undo_action(r, false);
        }
    }

    fn perform_undo_action(&mut self, result: crate::undo::UndoResult, redo: bool) {
        let label = result.label.clone();
        match result.action {
            UndoAction::RestoreDocument(doc) => {
                let mut doc = *doc;
                if let Some(cur) = &doc.current {
                    self.playback.restore_position =
                        Some((cur.key.clone(), doc.transport.position.position_ms));
                }
                if let Some(live) = self.doc() {
                    doc.transport = live.transport.clone();
                }
                self.playback.want_playing = self.playback.playing;
                self.local_op(SessionOp::Replace { document: doc });
            }
            UndoAction::Cas(mutations) => self.run_cas(&result.entry_id, &label, mutations, redo),
            UndoAction::Nothing => {}
        }
        self.selection = result.target;
        let verb = if redo { "Redid" } else { "Undid" };
        let action = if redo {
            ("Undo".to_string(), Command::Undo)
        } else {
            ("Redo".to_string(), Command::Redo)
        };
        if result.tier == UndoTier::SessionState {
            self.toast(format!("{verb} {label}"), Some(action));
        }
        self.emit(Event::UndoChanged {
            state: self.undo.state(),
        });
    }

    pub(crate) fn restore_selection(&mut self) {
        if let Some(t) = self.undo.restore_selection() {
            self.selection = t;
            self.emit(Event::UndoChanged {
                state: self.undo.state(),
            });
        }
    }

    // -- views ------------------------------------------------------------------

    pub(crate) fn queue_view(&self) -> QueueView {
        let Some(doc) = self.doc() else {
            return QueueView::default();
        };
        let derived = derive(doc);
        let mut ids: Vec<TrackId> = vec![];
        for i in derived
            .history
            .iter()
            .chain(derived.current.iter())
            .chain(derived.playing_next.iter())
            .chain(derived.upcoming.iter())
        {
            if !ids.contains(&i.track_id) {
                ids.push(i.track_id.clone());
            }
        }
        let map = self.summaries(&ids);
        derived.into_view(|id| {
            map.get(id).cloned().unwrap_or_else(|| bare_summary(id))
        })
    }

    pub(crate) fn summaries(&self, ids: &[TrackId]) -> HashMap<TrackId, TrackSummary> {
        let mut map = HashMap::new();
        if ids.is_empty() {
            return map;
        }
        if let Ok(list) = self.db.summaries_by_ids(ids) {
            for s in list {
                map.insert(s.id.clone(), s);
            }
        }
        map
    }

    pub(crate) fn summary_or_bare(&self, id: &str) -> TrackSummary {
        self.db
            .summaries_by_ids(std::slice::from_ref(&id.to_string()))
            .ok()
            .and_then(|v| v.into_iter().next())
            .unwrap_or_else(|| bare_summary(id))
    }

    pub(crate) fn track_or_bare(&self, id: &str) -> Option<Track> {
        match self.db.track(id) {
            Ok(Some(t)) => Some(t),
            _ => Some(Track {
                id: id.to_string(),
                server_id: self.server_id().unwrap_or_default(),
                title: id.to_string(),
                ..Default::default()
            }),
        }
    }

    fn track_title_for_key(&self, doc: &SessionDocument, key: &QueueKey) -> String {
        let id = doc
            .history
            .iter()
            .chain(doc.current.iter())
            .chain(doc.insertions.iter())
            .find(|i| &i.key == key)
            .map(|i| i.track_id.clone());
        match id {
            Some(id) => self.summary_or_bare(&id).title,
            None => "a track".into(),
        }
    }

    pub(crate) fn transport_state(&self) -> TransportState {
        let mut t = match &self.engine {
            Some(e) => e.transport(),
            None => TransportState::default(),
        };
        if self.owns_transport() && self.playback.loaded {
            let now = self.now();
            t.position = PositionStamp {
                position_ms: self.playback.position_now(now),
                taken_at: self.session_now(),
                rate: 1.0,
                is_playing: self.playback.playing,
            };
            t.buffering = self.playback.buffering;
            t.played_ms = self.scrobbler.played_ms();
        } else if let Some(e) = &self.engine {
            // Extrapolate the remote stamp to now (never past the track end).
            let pos = e.position_ms();
            let duration = self.playback.duration_ms();
            t.position.position_ms = if duration > 0 { pos.min(duration) } else { pos };
            t.position.taken_at = e.now_session_ms();
        }
        t.volume = self.playback.volume;
        t
    }

    pub(crate) fn emit_transport(&mut self) {
        let transport = self.transport_state();
        self.emit(Event::TransportChanged { transport });
        self.emit_media_session();
    }

    pub(crate) fn state_view(&self) -> StateView {
        let doc = self.doc();
        let current = doc.and_then(|d| d.current.as_ref());
        let current_track = current.map(|c| self.summary_or_bare(&c.track_id));
        let album_id = current_track.as_ref().and_then(|t| t.album_id.clone());
        let artist_id = current_track.as_ref().and_then(|t| t.artist_id.clone());
        let (peer_count, has_resume) = match &self.engine {
            Some(e) => (
                e.devices().iter().filter(|d| !d.is_self).count() as u32,
                e.resume_offer().is_some(),
            ),
            None => (0, false),
        };
        StateView {
            server_id: self.server_id(),
            has_session: doc.is_some_and(|d| d.context.is_some()),
            has_current: current.is_some(),
            is_playing: self.playback.playing || self.last_transport.position.is_playing && !self.owns_transport(),
            can_undo: self.undo.can_undo(),
            can_redo: self.undo.can_redo(),
            shuffle: doc.is_some_and(|d| d.shuffle.is_some()),
            repeat: doc.map(|d| d.repeat).unwrap_or_default(),
            autoplay: doc.is_some_and(|d| d.autoplay),
            current_track_id: current.map(|c| c.track_id.clone()),
            current_album_id: album_id,
            current_artist_id: artist_id,
            current_loved: current_track.as_ref().map(|t| t.loved).unwrap_or(false),
            current_rating: current_track.as_ref().map(|t| t.rating).unwrap_or(0),
            viewing_playlist: None,
            viewing_playlist_is_smart: false,
            selection_indices: vec![],
            queue_panel_open: false,
            fullscreen: false,
            mini_player: false,
            sleep_timer_active: self.sleep.is_active(),
            peer_count,
            has_resume_offer: has_resume,
            has_cleared_selection: self.undo.has_cleared_selection(),
            text_field_focused: false,
            saved_queue_count: doc.map(|d| d.saved_queues.len() as u32).unwrap_or(0),
            volume: self.playback.volume,
        }
    }

    // -- context resolution -----------------------------------------------------

    pub(crate) fn resolve_context_tracks(&self, server_id: &str, kind: &ContextKind) -> Vec<TrackId> {
        let ids = |r: Result<Vec<Track>, crate::db::DbError>| -> Vec<TrackId> {
            r.map(|v| v.into_iter().map(|t| t.id).collect())
                .unwrap_or_default()
        };
        match kind {
            ContextKind::Album { id } => ids(self.db.album_tracks(id)),
            ContextKind::Artist { id } => ids(self.db.artist_tracks(id)),
            ContextKind::Playlist { id } => self.db.playlist_track_ids(id).unwrap_or_default(),
            ContextKind::Genre { name } => ids(self.db.genre_tracks(server_id, name)),
            ContextKind::Filter { filter } => self.filter_track_ids(filter, server_id),
            ContextKind::AdHoc { .. } | ContextKind::Autoplay => vec![],
        }
    }

    pub(crate) fn context_label(&self, _server_id: &str, kind: &ContextKind) -> String {
        match kind {
            ContextKind::Album { id } => self
                .db
                .album(id)
                .ok()
                .flatten()
                .map(|a| a.name)
                .unwrap_or_else(|| id.clone()),
            ContextKind::Artist { id } => self
                .db
                .artist(id)
                .ok()
                .flatten()
                .map(|a| a.name)
                .unwrap_or_else(|| id.clone()),
            ContextKind::Playlist { id } => self
                .db
                .playlist(id)
                .ok()
                .flatten()
                .map(|p| p.name)
                .unwrap_or_else(|| id.clone()),
            ContextKind::Genre { name } => name.clone(),
            ContextKind::Filter { filter } => filter.name.clone(),
            ContextKind::AdHoc { label } => label.clone(),
            ContextKind::Autoplay => "Autoplay".into(),
        }
    }

    pub(crate) fn context_cover(&self, ctx: &QueueContext) -> Option<String> {
        match &ctx.kind {
            ContextKind::Album { id } => self.db.album(id).ok().flatten().and_then(|a| a.cover_art),
            ContextKind::Playlist { id } => {
                self.db.playlist(id).ok().flatten().and_then(|p| p.cover_art)
            }
            _ => ctx
                .tracks
                .first()
                .and_then(|t| self.db.track(t).ok().flatten())
                .and_then(|t| t.cover_art),
        }
    }

    pub(crate) fn server_infos(&self) -> Vec<ServerInfo> {
        match &self.server {
            Some(s) => {
                let mut info = s.info.clone();
                info.reachable = s.api.is_some() && s.info.reachable;
                vec![info]
            }
            None => vec![],
        }
    }
}

pub(crate) fn bare_summary(id: &str) -> TrackSummary {
    TrackSummary {
        id: id.to_string(),
        title: id.to_string(),
        ..Default::default()
    }
}

pub(crate) fn track_from_summary(s: &TrackSummary) -> Track {
    Track {
        id: s.id.clone(),
        server_id: s.server_id.clone(),
        title: s.title.clone(),
        album_id: s.album_id.clone(),
        album: s.album.clone(),
        artist_id: s.artist_id.clone(),
        artist: s.artist.clone(),
        duration_ms: s.duration_ms,
        cover_art: s.cover_art.clone(),
        rating: s.rating,
        loved: s.loved,
        offline: s.offline,
        ..Default::default()
    }
}

/// Same set (ignoring order): ids and LWW clocks.
pub(crate) fn same_saved_set(a: &[SavedQueue], b: &[SavedQueue]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|x| {
        b.iter()
            .any(|y| y.id == x.id && y.updated_at == x.updated_at && y.pinned == x.pinned)
    })
}

fn command_kind(cmd: &Command) -> &'static str {
    match cmd {
        Command::PlayContext { .. } => "playContext",
        Command::PlayTracks { .. } => "playTracks",
        Command::PlayNext { .. } => "playNext",
        Command::PlayLater { .. } => "playLater",
        Command::JumpToQueueItem { .. } => "jump",
        Command::RemoveQueueItems { .. } => "removeFromQueue",
        Command::MoveQueueItem { .. } => "moveQueueItem",
        Command::ClearQueue => "clearQueue",
        Command::ClearInsertions => "clearInsertions",
        Command::SetShuffle { .. } => "shuffle",
        Command::SetRepeat { .. } => "repeat",
        Command::SetAutoplay { .. } => "autoplay",
        Command::SetQueueMode { .. } => "queueMode",
        Command::Next => "next",
        Command::Previous => "previous",
        Command::SkipUnavailable { .. } => "skipUnavailable",
        Command::RestoreSavedQueue { .. } => "restoreSavedQueue",
        Command::PinSavedQueue { .. } => "pinSavedQueue",
        Command::DeleteSavedQueue { .. } => "deleteSavedQueue",
        _ => "session",
    }
}

fn command_label(cmd: &Command) -> String {
    match cmd {
        Command::PlayTracks { label, .. } => format!("Play {label}"),
        Command::PlayNext { track_ids, .. } => plural("Play next", track_ids.len()),
        Command::PlayLater { track_ids, .. } => plural("Play later", track_ids.len()),
        Command::JumpToQueueItem { .. } => "Jump to track".into(),
        Command::RemoveQueueItems { keys } => plural("Remove from queue", keys.len()),
        Command::MoveQueueItem { .. } => "Move track".into(),
        Command::ClearQueue => "Clear queue".into(),
        Command::ClearInsertions => "Clear playing next".into(),
        Command::SetShuffle { enabled } => {
            if *enabled {
                "Shuffle on".into()
            } else {
                "Shuffle off".into()
            }
        }
        Command::SetRepeat { mode } => format!("Repeat {mode:?}").to_lowercase(),
        Command::SetAutoplay { enabled } => {
            if *enabled {
                "Autoplay on".into()
            } else {
                "Autoplay off".into()
            }
        }
        Command::SetQueueMode { mode } => format!("Queue mode {mode:?}"),
        Command::Next => "Next track".into(),
        Command::Previous => "Previous track".into(),
        Command::SkipUnavailable { .. } => "Skip unavailable".into(),
        Command::RestoreSavedQueue { .. } => "Restore queue".into(),
        Command::PinSavedQueue { pinned, .. } => {
            if *pinned {
                "Pin queue".into()
            } else {
                "Unpin queue".into()
            }
        }
        Command::DeleteSavedQueue { .. } => "Delete saved queue".into(),
        _ => "Queue change".into(),
    }
}

fn plural(base: &str, n: usize) -> String {
    if n == 1 {
        base.to_string()
    } else {
        format!("{base} ({n} tracks)")
    }
}

impl Actor {
    /// `QueueOp` for the effects preview (kept in one place so both paths agree).
    #[allow(dead_code)]
    pub(crate) fn queue_op_of(&self, op: &SessionOp) -> Option<QueueOp> {
        crate::connect::session_adapter::to_queue_op(op)
    }
}
