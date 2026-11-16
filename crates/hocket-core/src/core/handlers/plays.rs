//! Shared listening history: plays travel between devices through the
//! room's play log (the coordinator, or a LAN leader), so a play on one
//! device shows in every device's history and stats.
//!
//! - **Out.** Every play in `play_history` (this device's own, and any it
//!   learnt in another room) is pushed to the room until the room
//!   acknowledges it (`PlaysPushed`); `play_relays` notes which logs hold
//!   which plays, so nothing is pushed twice to the same log. Plays made
//!   offline wait in `play_history` like any other and go out the next time
//!   a room is reachable. Pushes go one page at a time.
//! - **In.** On reaching a room the device asks for the log past its mark
//!   for that log (`PlaysRequest`), and the room relays new plays as they
//!   arrive. Each is recorded once (a play's identity is `(track,
//!   playedAt)`, the same for every device that held it) as a `remote` row
//!   that counts toward local stats but never toward this device's
//!   scrobbles.

use serde::{Deserialize, Serialize};

use crate::api::Event;
use crate::connect::engine::Input;
use crate::connect::replica::{PLAY_LOG_TTL_MS, PLAY_PAGE};
use crate::connect::wire::{PlayLogMark, PlayRecord};
use crate::core::actor::Actor;
use crate::db::queries;

/// Per-actor bookkeeping of the play-log exchange with the current room.
#[derive(Debug, Default)]
pub(crate) struct PlaysSync {
    /// The log of the room we are syncing with, once it told us.
    log_id: Option<String>,
    /// The push awaiting its `PlaysPushed`: batch number and row ids.
    in_flight: Option<(u32, Vec<i64>)>,
    next_batch: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Marks(Vec<PlayLogMark>);

fn marks_key(scope: &str) -> String {
    format!("playLogMarks:{scope}")
}

impl Actor {
    /// A shared room is reachable (again): start over with it.
    pub(crate) fn on_plays_sync_ready(&mut self) {
        self.plays_sync.log_id = None;
        self.plays_sync.in_flight = None;
        let since = self.now() - PLAY_LOG_TTL_MS;
        if let Err(e) = self
            .db
            .with_tx(|tx| queries::prune_play_relays_in(tx, since))
        {
            self.log("debug", format!("prune play relays: {e}"));
        }
        let mut marks = self.load_play_marks();
        if !marks.0.is_empty() && !self.has_play_history() {
            // Marks left from before the server was removed (or its history
            // lost): read every log from the start again.
            marks.0.clear();
            self.save_play_marks(&marks);
        }
        self.engine_input(Input::RequestPlays { marks: marks.0 });
    }

    pub(crate) fn on_plays_pushed(&mut self, batch: u32, log_id: String) {
        let Some((b, ids)) = self.plays_sync.in_flight.take() else {
            return;
        };
        if b != batch {
            self.plays_sync.in_flight = Some((b, ids));
            return;
        }
        if let Err(e) = self
            .db
            .with_tx(|tx| queries::mark_relayed_in(tx, &ids, &log_id))
        {
            self.log("warn", format!("mark plays relayed: {e}"));
            return;
        }
        self.plays_sync.log_id = Some(log_id);
        self.pump_plays();
    }

    pub(crate) fn on_plays_received(&mut self, log_id: String, after: u32, plays: Vec<PlayRecord>) {
        let Some(server_id) = self.server_id() else {
            return;
        };
        let added = match self
            .db
            .with_tx(|tx| queries::import_plays_in(tx, &server_id, &plays, &log_id))
        {
            Ok(n) => n,
            Err(e) => {
                self.log("warn", format!("import shared plays: {e}"));
                return;
            }
        };
        // The mark moves only over a contiguous stretch of the log, so a
        // relay that overtakes a catch-up page never skips that page.
        let last = plays.iter().map(|p| p.seq).max().unwrap_or(after);
        let mut marks = self.load_play_marks();
        let mark = marks.0.iter_mut().find(|m| m.log_id == log_id);
        let current = mark.as_ref().map(|m| m.seq).unwrap_or(0);
        if after <= current && last > current {
            match mark {
                Some(m) => m.seq = last,
                None => marks.0.push(PlayLogMark {
                    log_id: log_id.clone(),
                    seq: last,
                }),
            }
            self.save_play_marks(&marks);
        }
        if added > 0 {
            self.log(
                "info",
                format!("{added} plays from other devices added to history"),
            );
            let ids = plays.iter().map(|p| p.track_id.clone()).collect();
            self.emit(Event::LibraryChanged {
                server_id,
                tables: vec!["tracks".into(), "play_history".into()],
                ids,
            });
        }
        if self.plays_sync.log_id.as_deref() != Some(log_id.as_str()) {
            self.plays_sync.log_id = Some(log_id);
            self.plays_sync.in_flight = None;
        }
        self.pump_plays();
    }

    /// Push the next page of plays the current room's log is not known to
    /// hold, unless a push is already waiting for its answer. Called when
    /// the room's log is known, on every acknowledgement, and after this
    /// device records a play.
    pub(crate) fn pump_plays(&mut self) {
        if self.plays_sync.in_flight.is_some() || self.engine.is_none() {
            return;
        }
        let (Some(log_id), Some(server_id)) = (self.plays_sync.log_id.clone(), self.server_id())
        else {
            return;
        };
        let since = self.now() - PLAY_LOG_TTL_MS;
        let rows = match self
            .db
            .with_tx(|tx| queries::unrelayed_plays_in(tx, &server_id, &log_id, since, PLAY_PAGE))
        {
            Ok(r) => r,
            Err(e) => {
                self.log("warn", format!("read plays to share: {e}"));
                return;
            }
        };
        if rows.is_empty() {
            return;
        }
        self.plays_sync.next_batch = self.plays_sync.next_batch.wrapping_add(1);
        let batch = self.plays_sync.next_batch;
        let (ids, plays): (Vec<i64>, Vec<PlayRecord>) = rows.into_iter().unzip();
        self.plays_sync.in_flight = Some((batch, ids));
        self.engine_input(Input::PushPlays { batch, plays });
    }

    fn has_play_history(&self) -> bool {
        let Some(server_id) = self.server_id() else {
            return false;
        };
        self.db
            .with_conn(|c| {
                Ok(c.query_row(
                    "SELECT EXISTS(SELECT 1 FROM play_history WHERE server_id = ?1)",
                    [&server_id],
                    |r| r.get::<_, bool>(0),
                )?)
            })
            .unwrap_or(true)
    }

    fn load_play_marks(&self) -> Marks {
        let Some(scope) = self.scope.as_deref() else {
            return Marks::default();
        };
        self.db
            .saved_state_get::<Marks>(&marks_key(scope))
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    fn save_play_marks(&mut self, marks: &Marks) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        if let Err(e) = self
            .db
            .saved_state_set(&marks_key(&scope), marks, self.clock.as_ref())
        {
            self.log("warn", format!("save play log marks: {e}"));
        }
    }
}
