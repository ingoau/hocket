//! The song stash (see `crate::stash`): the actor keeps the account's list, persists it under
//! `stash:{scope}` and hands every local change to the engine, which syncs it to the room and the
//! coordinator. Remote copies come back as `Output::StashMerged`.

use crate::api::{Command, Event, StashEntry, StashItem, Track, TrackId};
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
        stash::items(&self.stash, |ids| {
            self.db.tracks_by_ids(ids).unwrap_or_default()
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

    pub(crate) fn stash_command(&mut self, cmd: Command) {
        if self.scope.is_none() {
            self.toast("Sign in to a server to use the stash", None);
            return;
        }
        let now = self.now();
        match cmd {
            Command::StashAdd { track_ids } => self.stash_add(track_ids, now),
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

    fn stash_add(&mut self, track_ids: Vec<TrackId>, now: f64) {
        if track_ids.is_empty() {
            return;
        }
        let server_id = self.server_id().unwrap_or_default();
        let found = self.db.tracks_by_ids(&track_ids).unwrap_or_default();
        let tracks: Vec<Track> = track_ids
            .iter()
            .map(|id| {
                found
                    .iter()
                    .find(|t| &t.id == id)
                    .cloned()
                    .unwrap_or_else(|| Track {
                        id: id.clone(),
                        server_id: server_id.clone(),
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
