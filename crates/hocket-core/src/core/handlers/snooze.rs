//! Snoozed songs (see `crate::snooze`): the actor keeps the account's set, persists it under
//! `snooze:{scope}` and hands every local change to the engine, which syncs it. Remote copies come
//! back as `Output::SnoozesMerged`. Snooze and unsnooze push an inverse onto the global undo stack.

use std::collections::HashSet;

use crate::api::{Command, Event, Snooze, SnoozeFor, SnoozeItem, TrackId};
use crate::connect::engine::Input;
use crate::core::actor::Actor;
use crate::snooze;
use crate::undo::SnoozeUndo;

fn songs(n: usize) -> String {
    if n == 1 {
        "1 song".to_string()
    } else {
        format!("{n} songs")
    }
}

fn duration_label(duration: SnoozeFor) -> &'static str {
    match duration {
        SnoozeFor::Day => "1 day",
        SnoozeFor::ThreeDays => "3 days",
        SnoozeFor::Week => "1 week",
        SnoozeFor::Month => "1 month",
        SnoozeFor::Indefinite => "indefinitely",
    }
}

impl Actor {
    /// Load the signed-in account's snoozes. Called once its scope is known.
    pub(crate) fn load_snoozes(&mut self) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        self.snoozes = match self
            .db
            .saved_state_get::<Vec<Snooze>>(&snooze::state_key(&scope))
        {
            Ok(list) => list.unwrap_or_default(),
            Err(e) => {
                self.log("warn", format!("snooze: unreadable, starting empty: {e}"));
                vec![]
            }
        };
        let now = self.now();
        snooze::prune(&mut self.snoozes, now);
        if !self.snoozes.is_empty() {
            self.engine_input(Input::SnoozesChanged(self.snoozes.clone()));
        }
        self.emit_snoozes();
    }

    /// Forget the snoozes of an account that's no longer signed in (they stay on disk).
    pub(crate) fn drop_snoozes(&mut self) {
        if !self.snoozes.is_empty() {
            self.snoozes.clear();
            self.emit_snoozes();
        }
    }

    pub(crate) fn snooze_items(&self) -> Vec<SnoozeItem> {
        snooze::items(&self.snoozes, self.now(), |ids| {
            self.db.tracks_by_ids(ids).unwrap_or_default()
        })
    }

    pub(crate) fn snoozed_ids(&self) -> Vec<TrackId> {
        snooze::active_ids(&self.snoozes, self.now())
    }

    pub(crate) fn emit_snoozes(&self) {
        self.emit(Event::SnoozesChanged {
            items: self.snooze_items(),
        });
    }

    fn save_snoozes(&mut self) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let now = self.now();
        snooze::prune(&mut self.snoozes, now);
        if let Err(e) = self.db.saved_state_set(
            &snooze::state_key(&scope),
            &self.snoozes,
            self.clock.as_ref(),
        ) {
            self.log("warn", format!("snooze: couldn't save: {e}"));
        }
    }

    fn snoozes_edited(&mut self) {
        self.save_snoozes();
        self.emit_snoozes();
        self.engine_input(Input::SnoozesChanged(self.snoozes.clone()));
    }

    pub(crate) fn on_snoozes_merged(&mut self, list: Vec<Snooze>) {
        let (mut merged, _) = snooze::merge(&self.snoozes, &list);
        snooze::prune(&mut merged, self.now());
        if merged != self.snoozes {
            self.snoozes = merged;
            self.save_snoozes();
            self.emit_snoozes();
        }
    }

    /// Drop snoozes whose end has passed. Called from housekeeping and before a play, so a snooze
    /// ends when it says it does even if nothing else has looked.
    pub(crate) fn prune_snoozes(&mut self) {
        let now = self.now();
        let before = self.snoozes.clone();
        snooze::prune(&mut self.snoozes, now);
        if self.snoozes != before {
            self.snoozes_edited();
        }
    }

    pub(crate) fn snooze_command(&mut self, cmd: Command) {
        if self.scope.is_none() {
            self.toast("Sign in to a server to snooze a song", None);
            return;
        }
        match cmd {
            Command::Snooze {
                track_ids,
                duration,
            } => {
                let until = snooze::until_for(duration, self.now());
                let n = track_ids.len();
                let message = match (n == 1, duration) {
                    (true, SnoozeFor::Indefinite) => "Snoozed indefinitely".to_string(),
                    (true, d) => format!("Snoozed for {}", duration_label(d)),
                    (false, SnoozeFor::Indefinite) => format!("Snoozed {} indefinitely", songs(n)),
                    (false, d) => format!("Snoozed {} for {}", songs(n), duration_label(d)),
                };
                self.write_snoozes(track_ids, Some(until), "snooze", "Snooze", Some(message));
            }
            Command::Unsnooze { track_ids } => {
                let n = track_ids.len();
                let message = if n == 1 {
                    "Unsnoozed".to_string()
                } else {
                    format!("Unsnoozed {}", songs(n))
                };
                self.write_snoozes(track_ids, None, "unsnooze", "Unsnooze", Some(message));
            }
            _ => {}
        }
    }

    /// `until == None` unsnoozes. `Some(end)` snoozes until `end` (`None` inside doesn't end).
    fn write_snoozes(
        &mut self,
        track_ids: Vec<TrackId>,
        until: Option<Option<f64>>,
        kind: &str,
        label: &str,
        toast: Option<String>,
    ) {
        if track_ids.is_empty() {
            return;
        }
        let now = self.now();
        let mut before = vec![];
        let mut after = vec![];
        for id in track_ids {
            let prior = snooze::state_of(&self.snoozes, &id, now);
            let changed = match until {
                None => snooze::clear(&mut self.snoozes, &id, now),
                Some(end) => snooze::set(&mut self.snoozes, &id, end, now),
            };
            if changed {
                before.push((id.clone(), prior));
                after.push((id, until));
            }
        }
        if before.is_empty() {
            return;
        }
        self.snoozes_edited();
        self.undo.push(
            Box::new(SnoozeUndo::new(kind, label, before, after)),
            self.selection.clone(),
            now,
        );
        self.emit(Event::UndoChanged {
            state: self.undo.state(),
        });
        if let Some(message) = toast {
            self.toast(message, Some(("Undo".into(), Command::Undo)));
        }
    }

    /// The inverse of a snooze, from the undo stack. Not itself undoable.
    pub(crate) fn apply_snooze_states(&mut self, states: Vec<(String, Option<Option<f64>>)>) {
        if states.is_empty() {
            return;
        }
        let now = self.now();
        let mut changed = false;
        for (id, state) in states {
            changed |= match state {
                None => snooze::clear(&mut self.snoozes, &id, now),
                Some(end) => snooze::set(&mut self.snoozes, &id, end, now),
            };
        }
        if changed {
            self.snoozes_edited();
        }
    }

    /// Leave snoozed songs out of a context as it becomes a queue. See [`snooze::filter_context`].
    pub(crate) fn filter_snoozed_context(
        &self,
        tracks: &[TrackId],
        kind: &crate::api::ContextKind,
        start: Option<u32>,
    ) -> (Vec<TrackId>, Option<u32>) {
        let snoozed: HashSet<TrackId> = self.snoozed_ids().into_iter().collect();
        snooze::filter_context(tracks, kind, start, &snoozed)
    }
}
