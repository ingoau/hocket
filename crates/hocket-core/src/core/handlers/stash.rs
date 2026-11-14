//! The song stash (see `crate::stash`): the actor keeps the account's list, persists it under
//! `stash:{scope}` and hands every local change to the engine, which syncs it to the room and the
//! coordinator. Remote copies come back as `Output::StashMerged`.

use crate::api::{Command, Event, QueueKey, StashEntry, StashItem, Track, TrackRef};
use crate::connect::engine::Input;
use crate::core::actor::Actor;
use crate::stash;

fn songs(n: usize) -> String {
    if n == 1 {
        "1 song".to_string()
    } else {
        format!("{n} songs")
    }
}

impl Actor {
    /// Load the signed-in account's stash. Called once its scope is known.
    pub(crate) fn load_stash(&mut self) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        self.stash = match self
            .db
            .saved_state_get::<Vec<StashEntry>>(&stash::state_key(&scope))
        {
            Ok(list) => list.unwrap_or_default(),
            Err(e) => {
                self.log("warn", format!("stash: unreadable, starting empty: {e}"));
                vec![]
            }
        };
        if !self.stash.is_empty() {
            self.engine_input(Input::StashChanged(self.stash.clone()));
        }
        self.emit_stash();
    }

    /// Forget the stash of an account that's no longer signed in (it stays on disk).
    pub(crate) fn drop_stash(&mut self) {
        if !self.stash.is_empty() {
            self.stash.clear();
            self.emit_stash();
        }
    }

    pub(crate) fn stash_items(&self) -> Vec<StashItem> {
        stash::items(&self.stash, |refs| {
            self.db.tracks_by_refs(refs).unwrap_or_default()
        })
    }

    pub(crate) fn emit_stash(&self) {
        self.emit(Event::StashChanged {
            items: self.stash_items(),
        });
    }

    fn save_stash(&mut self) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let now = self.now();
        stash::prune(&mut self.stash, now);
        if let Err(e) =
            self.db
                .saved_state_set(&stash::state_key(&scope), &self.stash, self.clock.as_ref())
        {
            self.log("warn", format!("stash: couldn't save: {e}"));
        }
    }

    /// After a local edit: persist, tell the UI, sync, and refresh the media
    /// session so a notification's stash icon follows the playing track.
    fn stash_edited(&mut self) {
        self.save_stash();
        self.emit_stash();
        self.engine_input(Input::StashChanged(self.stash.clone()));
        self.emit_media_session();
    }

    pub(crate) fn on_stash_merged(&mut self, list: Vec<StashEntry>) {
        let (mut merged, _) = stash::merge(&self.stash, &list);
        stash::prune(&mut merged, self.now());
        if merged != self.stash {
            self.stash = merged;
            self.save_stash();
            self.emit_stash();
            self.emit_media_session();
        }
    }

    /// Stash the "Next in queue" group: its songs go to the stash and leave the queue. A song
    /// the stash had no room for stays queued.
    pub(crate) fn stash_queue_group(&mut self) {
        let Some(doc) = self.doc() else { return };
        if doc.current.is_none() {
            return;
        }
        let group: Vec<(QueueKey, TrackRef)> = doc
            .upcoming
            .iter()
            .take_while(|i| i.queued)
            .map(|i| (i.key.clone(), i.track.clone()))
            .collect();
        if group.is_empty() {
            return;
        }
        let mut tracks: Vec<TrackRef> = vec![];
        for (_, t) in &group {
            if !tracks.contains(t) {
                tracks.push(t.clone());
            }
        }
        if self.scope.is_none() {
            self.toast("Sign in to a server to use the stash", None);
            return;
        }
        let now = self.now();
        self.stash_add(tracks, now);
        let stashed: std::collections::HashSet<&TrackRef> = self
            .stash
            .iter()
            .filter(|e| !e.removed)
            .map(|e| &e.track)
            .collect();
        let keys: Vec<QueueKey> = group
            .iter()
            .filter(|(_, t)| stashed.contains(t))
            .map(|(k, _)| k.clone())
            .collect();
        if !keys.is_empty() {
            self.queue_command(Command::RemoveQueueItems { keys });
        }
    }

    pub(crate) fn stash_command(&mut self, cmd: Command) {
        if self.scope.is_none() {
            self.toast("Sign in to a server to use the stash", None);
            return;
        }
        let now = self.now();
        match cmd {
            Command::StashAdd { track_ids } => {
                let tracks = track_ids.iter().map(|id| self.track_ref_of(id)).collect();
                self.stash_add(tracks, now)
            }
            Command::StashRemove { entry_ids } => {
                let removed = stash::set_removed(&mut self.stash, &entry_ids, true, now);
                if !removed.is_empty() {
                    self.stash_edited();
                    self.toast(
                        format!("Removed {} from the stash", songs(removed.len())),
                        Some(("Undo".into(), Command::StashRestore { entry_ids: removed })),
                    );
                }
            }
            Command::StashMove { entry_id, to_index } => {
                if stash::move_to(&mut self.stash, &entry_id, to_index as usize, now) {
                    self.stash_edited();
                }
            }
            Command::StashClear => {
                let removed = stash::clear(&mut self.stash, now);
                if !removed.is_empty() {
                    self.stash_edited();
                    self.toast(
                        "Stash cleared",
                        Some(("Undo".into(), Command::StashRestore { entry_ids: removed })),
                    );
                }
            }
            Command::StashRestore { entry_ids }
                if !stash::set_removed(&mut self.stash, &entry_ids, false, now).is_empty() =>
            {
                self.stash_edited();
            }
            _ => {}
        }
    }

    fn stash_add(&mut self, refs: Vec<TrackRef>, now: f64) {
        if refs.is_empty() {
            return;
        }
        let found = self.db.tracks_by_refs(&refs).unwrap_or_default();
        let tracks: Vec<Track> = refs
            .iter()
            .map(|r| {
                found
                    .iter()
                    .find(|t| t.id == r.id && t.server_id == r.source)
                    .cloned()
                    .unwrap_or_else(|| Track {
                        id: r.id.clone(),
                        server_id: r.source.clone(),
                        ..Track::default()
                    })
            })
            .collect();
        let device = self.cfg.device_id.clone();
        let (added, dropped) = stash::add(&mut self.stash, &tracks, &device, now);
        if added > 0 {
            self.stash_edited();
        }
        let message = match (added, dropped) {
            (0, 0) => "Already in the stash".to_string(),
            (_, 0) => format!("Stashed {}", songs(added)),
            (0, _) => format!("The stash is full ({} songs)", stash::MAX_ENTRIES),
            (_, d) => format!("Stashed {}; {} didn't fit", songs(added), d),
        };
        self.toast(message, None);
    }
}
