//! Durable pending mutations with retry and conflict handling; scrobbler.
//! Owner: core-server.
//!
//! Entry points:
//! - [`Outbox::new`] over a `Db`. [`Outbox::enqueue`] persists a [`Mutation`]
//!   (and applies its optimistic local effect to the mirror so the UI
//!   reflects it at once); coalesces rapid repeats on the same target while
//!   still unsent. Returns the entry id the undo module keeps.
//! - [`Outbox::cancel_if_unsent`] — undo tier 2 for entries not yet sent:
//!   drops the entry and reverts the local effect to the recorded prior.
//! - [`Outbox::flush`] — sends due entries through a [`SubsonicApi`] with
//!   exponential backoff, rebases playlist edits on the server's current
//!   order (by track id), applies last-write-wins for ratings/loves and
//!   reports conflicts. [`OutboxFlushRunner`] runs it as `JobKind::OutboxFlush`.
//! - [`Outbox::execute_cas`] — compare-and-swap for inverses of already-sent
//!   mutations: `Applied` or `Skipped { current }` so undo can say "undid 487
//!   of 500, 13 changed elsewhere".
//! - [`scrobbler::Scrobbler`] — pure, clock-injected Last.fm state machine;
//!   [`scrobbler::ScrobbleRecorder`] turns its actions into play history rows
//!   and outbox entries.

pub mod scrobbler;

use std::sync::Arc;

use futures::future::BoxFuture;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::api::{self, RatingTarget};
use crate::db::{Db, DbError, DbResult};
use crate::jobs::{JobContext, JobError, JobResult, JobRunner, RetryAction};
use crate::subsonic::{PlayQueueSave, PlaylistUpdate, StarTarget, SubsonicApi, SubsonicError};
use crate::util::{new_id, Clock};

pub use scrobbler::{ScrobbleAction, ScrobbleRecorder, Scrobbler};

/// What `star`/`unstar` can target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum LoveTarget {
    Track { id: String },
    Album { id: String },
    Artist { id: String },
}

impl From<RatingTarget> for LoveTarget {
    fn from(t: RatingTarget) -> Self {
        match t {
            RatingTarget::Track { id } => LoveTarget::Track { id },
            RatingTarget::Album { id } => LoveTarget::Album { id },
        }
    }
}

/// A durable, replayable change to the server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum Mutation {
    SetRating { target: RatingTarget, rating: u32 },
    SetLoved { target: LoveTarget, loved: bool },
    PlaylistCreate { name: String, track_ids: Vec<String> },
    PlaylistDelete { playlist_id: String },
    PlaylistRename { playlist_id: String, name: Option<String>, comment: Option<String>, public: Option<bool> },
    /// Append, or insert at `at_index` (rebased on the server's current order).
    PlaylistAdd { playlist_id: String, track_ids: Vec<String>, at_index: Option<u32> },
    /// Removal by track id; `indices` are the positions as seen when queued
    /// and are only a hint — the server's current order is what is edited.
    PlaylistRemove { playlist_id: String, track_ids: Vec<String>, indices: Vec<u32> },
    PlaylistMove { playlist_id: String, track_id: String, from_index: u32, to_index: u32 },
    /// `submission=false` is "now playing"; `history_id` links to `play_history`.
    Scrobble { track_id: String, played_at: f64, submission: bool, history_id: Option<i64> },
    SavePlayQueue { track_ids: Vec<String>, current: Option<String>, position_ms: Option<u32> },
}

impl Mutation {
    /// Coalescing/undo key: the entity this mutation targets.
    pub fn target_key(&self) -> String {
        match self {
            Mutation::SetRating { target: RatingTarget::Track { id }, .. } => format!("rating:track:{id}"),
            Mutation::SetRating { target: RatingTarget::Album { id }, .. } => format!("rating:album:{id}"),
            Mutation::SetLoved { target: LoveTarget::Track { id }, .. } => format!("loved:track:{id}"),
            Mutation::SetLoved { target: LoveTarget::Album { id }, .. } => format!("loved:album:{id}"),
            Mutation::SetLoved { target: LoveTarget::Artist { id }, .. } => format!("loved:artist:{id}"),
            Mutation::PlaylistCreate { name, .. } => format!("playlist:create:{name}"),
            Mutation::PlaylistDelete { playlist_id }
            | Mutation::PlaylistRename { playlist_id, .. }
            | Mutation::PlaylistAdd { playlist_id, .. }
            | Mutation::PlaylistRemove { playlist_id, .. }
            | Mutation::PlaylistMove { playlist_id, .. } => format!("playlist:{playlist_id}"),
            Mutation::Scrobble { track_id, played_at, submission, .. } => format!("scrobble:{track_id}:{played_at}:{submission}"),
            Mutation::SavePlayQueue { .. } => "playQueue".into(),
        }
    }

    /// Whether a newer mutation with the same target key supersedes this one
    /// while both are unsent (a rating dragged 2→3→4 sends one call).
    pub fn coalesces(&self) -> bool {
        matches!(self, Mutation::SetRating { .. } | Mutation::SetLoved { .. } | Mutation::SavePlayQueue { .. })
    }
}

/// The value a target had before the mutation was queued (for undo and
/// conflict detection).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum Prior {
    Rating(u32),
    Loved(bool),
    /// Playlist order before the edit (track ids).
    PlaylistOrder(Vec<String>),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EntryStatus {
    Pending,
    Inflight,
    Done,
    Failed,
    Cancelled,
}

impl EntryStatus {
    pub fn name(self) -> &'static str {
        match self {
            EntryStatus::Pending => "pending",
            EntryStatus::Inflight => "inflight",
            EntryStatus::Done => "done",
            EntryStatus::Failed => "failed",
            EntryStatus::Cancelled => "cancelled",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "pending" => EntryStatus::Pending,
            "inflight" => EntryStatus::Inflight,
            "done" => EntryStatus::Done,
            "failed" => EntryStatus::Failed,
            _ => EntryStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxEntry {
    pub id: String,
    pub server_id: String,
    pub mutation: Mutation,
    pub status: EntryStatus,
    pub created_at: f64,
    pub attempts: u32,
    pub next_attempt_at: f64,
    pub last_error: Option<String>,
    pub expected_prior: Option<Prior>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    /// Dropped before it was sent; local effect reverted.
    Cancelled,
    /// Already sent (or in flight): use `execute_cas` with the inverse instead.
    AlreadySent,
    Unknown,
}

/// A rating/love found changed on the server since the mutation was queued.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub entry_id: String,
    pub target: String,
    pub expected: Prior,
    pub found: Prior,
    /// What ended up on the server (last write wins: ours).
    pub applied: Prior,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlushReport {
    pub applied: usize,
    /// Transient failures, rescheduled with backoff.
    pub deferred: usize,
    /// Permanent failures (entry status `failed`).
    pub failed: Vec<(String, String)>,
    pub conflicts: Vec<Conflict>,
    /// Newly created playlists: (entry id, server playlist id).
    pub created_playlists: Vec<(String, String)>,
}

/// Compare-and-swap target for inverses of sent mutations.
#[derive(Debug, Clone, PartialEq)]
pub enum CasTarget {
    Rating(RatingTarget),
    Loved(LoveTarget),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CasOutcome {
    Applied,
    /// The server value no longer matched `expected`; nothing was changed.
    Skipped { current: Prior },
}

/// Backoff schedule: 5 s, 10 s, 20 s ... capped at 10 min.
pub fn backoff_ms(attempts: u32) -> f64 {
    let base = 5_000.0_f64;
    (base * 2f64.powi(attempts.saturating_sub(1).min(20) as i32)).min(600_000.0)
}

/// Attempts after which a still-transient failure is reported as a problem
/// (the entry stays pending and keeps retrying).
pub const REPORT_AFTER_ATTEMPTS: u32 = 5;

#[derive(Clone)]
pub struct Outbox {
    db: Db,
    clock: Arc<dyn Clock>,
}

fn entry_from_row(r: &rusqlite::Row) -> rusqlite::Result<OutboxEntry> {
    let mutation: String = r.get(2)?;
    let status: String = r.get(3)?;
    let prior: Option<String> = r.get(8)?;
    Ok(OutboxEntry {
        id: r.get(0)?,
        server_id: r.get(1)?,
        mutation: serde_json::from_str(&mutation).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        status: EntryStatus::parse(&status),
        created_at: r.get(4)?,
        attempts: r.get::<_, i64>(5)?.max(0) as u32,
        next_attempt_at: r.get(6)?,
        last_error: r.get(7)?,
        expected_prior: prior.and_then(|p| serde_json::from_str(&p).ok()),
    })
}

const ENTRY_COLUMNS: &str = "id, server_id, mutation, status, created_at, attempts, next_attempt_at, last_error, expected_prior";

impl Outbox {
    pub fn new(db: Db, clock: Arc<dyn Clock>) -> Self {
        Outbox { db, clock }
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Persist a mutation and apply its optimistic local effect. `prior` is
    /// the value before the change (for undo and conflict reporting).
    pub fn enqueue(&self, server_id: &str, mutation: Mutation, prior: Option<Prior>) -> DbResult<String> {
        let id = new_id();
        let now = self.clock.now_ms();
        let target = mutation.target_key();
        let json = serde_json::to_string(&mutation)?;
        let prior_json = match &prior {
            Some(p) => Some(serde_json::to_string(p)?),
            None => None,
        };
        self.db.with_tx(|tx| {
            // Coalesce: an unsent entry for the same target is superseded; keep its original prior.
            let mut inherited_prior: Option<String> = None;
            if mutation.coalesces() {
                let existing: Option<(String, Option<String>)> = tx
                    .query_row(
                        "SELECT id, expected_prior FROM outbox WHERE server_id = ?1 AND target = ?2 AND status = 'pending' ORDER BY created_at DESC LIMIT 1",
                        [server_id, &target],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((old_id, old_prior)) = existing {
                    tx.execute("DELETE FROM outbox WHERE id = ?1", [&old_id])?;
                    inherited_prior = old_prior;
                }
            }
            let prior_json = inherited_prior.or(prior_json);
            tx.execute(
                "INSERT INTO outbox(id, server_id, mutation, status, created_at, attempts, next_attempt_at, target, expected_prior) VALUES (?1, ?2, ?3, 'pending', ?4, 0, ?4, ?5, ?6)",
                params![id, server_id, json, now, target, prior_json],
            )?;
            apply_local(tx, &mutation)?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn entry(&self, id: &str) -> DbResult<Option<OutboxEntry>> {
        self.db.with_conn(|c| {
            Ok(c.query_row(&format!("SELECT {ENTRY_COLUMNS} FROM outbox WHERE id = ?1"), [id], entry_from_row).optional()?)
        })
    }

    /// Unsent entries, oldest first.
    pub fn pending(&self) -> DbResult<Vec<OutboxEntry>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare_cached(&format!("SELECT {ENTRY_COLUMNS} FROM outbox WHERE status IN ('pending','inflight') ORDER BY created_at ASC"))?;
            let rows = st.query_map([], entry_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn failed(&self) -> DbResult<Vec<OutboxEntry>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare_cached(&format!("SELECT {ENTRY_COLUMNS} FROM outbox WHERE status = 'failed' ORDER BY created_at ASC"))?;
            let rows = st.query_map([], entry_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn pending_count(&self) -> DbResult<u32> {
        self.db.with_conn(|c| {
            let n: i64 = c.query_row("SELECT count(*) FROM outbox WHERE status IN ('pending','inflight')", [], |r| r.get(0))?;
            Ok(n as u32)
        })
    }

    /// Undo tier 2: drop an unsent entry and revert its local effect.
    pub fn cancel_if_unsent(&self, id: &str) -> DbResult<CancelOutcome> {
        self.db.with_tx(|tx| {
            let entry = tx.query_row(&format!("SELECT {ENTRY_COLUMNS} FROM outbox WHERE id = ?1"), [id], entry_from_row).optional()?;
            let Some(entry) = entry else { return Ok(CancelOutcome::Unknown) };
            if entry.status != EntryStatus::Pending {
                return Ok(CancelOutcome::AlreadySent);
            }
            tx.execute("UPDATE outbox SET status = 'cancelled', finished_at = ?2 WHERE id = ?1", params![id, self.clock.now_ms()])?;
            if let Some(prior) = &entry.expected_prior {
                revert_local(tx, &entry.mutation, prior)?;
            }
            Ok(CancelOutcome::Cancelled)
        })
    }

    /// Reset permanently failed entries to pending (user pressed retry).
    pub fn retry_failed(&self) -> DbResult<usize> {
        let now = self.clock.now_ms();
        self.db.with_conn(|c| {
            Ok(c.execute("UPDATE outbox SET status = 'pending', next_attempt_at = ?1, last_error = NULL WHERE status = 'failed'", [now])?)
        })
    }

    /// Entries left `inflight` by a crashed process become pending again.
    pub fn recover(&self) -> DbResult<usize> {
        self.db.with_conn(|c| Ok(c.execute("UPDATE outbox SET status = 'pending' WHERE status = 'inflight'", [])?))
    }

    /// Delete finished entries older than `max_age_ms`.
    pub fn prune(&self, max_age_ms: f64) -> DbResult<usize> {
        let cutoff = self.clock.now_ms() - max_age_ms;
        self.db.with_conn(|c| {
            Ok(c.execute("DELETE FROM outbox WHERE status IN ('done','cancelled') AND COALESCE(finished_at, created_at) < ?1", [cutoff])?)
        })
    }

    /// Send every due entry for the api's server. Stops early on the first
    /// network failure (the rest would fail the same way) and reschedules.
    pub async fn flush(&self, api: &dyn SubsonicApi, max: usize) -> DbResult<FlushReport> {
        let now = self.clock.now_ms();
        let server_id = api.server_id().to_string();
        let due: Vec<OutboxEntry> = self.db.with_conn(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {ENTRY_COLUMNS} FROM outbox WHERE server_id = ?1 AND status = 'pending' AND next_attempt_at <= ?2 ORDER BY created_at ASC LIMIT ?3"
            ))?;
            let rows = st.query_map(params![server_id, now, max as i64], entry_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let mut report = FlushReport::default();
        for entry in due {
            let claimed = self.db.with_conn(|c| {
                Ok(c.execute("UPDATE outbox SET status = 'inflight' WHERE id = ?1 AND status = 'pending'", [&entry.id])? > 0)
            })?;
            if !claimed {
                continue; // cancelled meanwhile
            }
            match self.execute(api, &entry, &mut report).await {
                Ok(()) => {
                    report.applied += 1;
                    self.db.with_conn(|c| {
                        c.execute(
                            "UPDATE outbox SET status = 'done', attempts = attempts + 1, finished_at = ?2, last_error = NULL WHERE id = ?1",
                            params![entry.id, self.clock.now_ms()],
                        )?;
                        Ok(())
                    })?;
                }
                Err(e) if e.is_transient() || matches!(e, SubsonicError::Auth(_)) => {
                    let attempts = entry.attempts + 1;
                    let next = self.clock.now_ms() + backoff_ms(attempts);
                    self.db.with_conn(|c| {
                        c.execute(
                            "UPDATE outbox SET status = 'pending', attempts = ?2, next_attempt_at = ?3, last_error = ?4 WHERE id = ?1",
                            params![entry.id, attempts, next, e.to_string()],
                        )?;
                        Ok(())
                    })?;
                    report.deferred += 1;
                    if matches!(e, SubsonicError::Network(_) | SubsonicError::Auth(_)) {
                        // Everything else will fail the same way right now.
                        self.db.with_conn(|c| {
                            c.execute(
                                "UPDATE outbox SET next_attempt_at = MAX(next_attempt_at, ?2) WHERE server_id = ?1 AND status = 'pending'",
                                params![server_id, next],
                            )?;
                            Ok(())
                        })?;
                        break;
                    }
                }
                Err(e) => {
                    self.db.with_conn(|c| {
                        c.execute(
                            "UPDATE outbox SET status = 'failed', attempts = attempts + 1, finished_at = ?2, last_error = ?3 WHERE id = ?1",
                            params![entry.id, self.clock.now_ms(), e.to_string()],
                        )?;
                        Ok(())
                    })?;
                    report.failed.push((entry.id.clone(), e.to_string()));
                }
            }
        }
        Ok(report)
    }

    async fn execute(&self, api: &dyn SubsonicApi, entry: &OutboxEntry, report: &mut FlushReport) -> Result<(), SubsonicError> {
        match &entry.mutation {
            Mutation::SetRating { target, rating } => {
                let id = match target {
                    RatingTarget::Track { id } | RatingTarget::Album { id } => id,
                };
                if let Some(Prior::Rating(expected)) = &entry.expected_prior {
                    if let RatingTarget::Track { id } = target {
                        match api.song(id).await {
                            Ok(song) => {
                                let found = song.user_rating.unwrap_or(0);
                                if found != *expected && found != *rating {
                                    report.conflicts.push(Conflict {
                                        entry_id: entry.id.clone(),
                                        target: entry.mutation.target_key(),
                                        expected: Prior::Rating(*expected),
                                        found: Prior::Rating(found),
                                        applied: Prior::Rating(*rating),
                                    });
                                }
                            }
                            Err(SubsonicError::NotFound(m)) => return Err(SubsonicError::NotFound(m)),
                            Err(e) => tracing::debug!(error = %e, "conflict check skipped"),
                        }
                    }
                }
                api.set_rating(id, *rating).await
            }
            Mutation::SetLoved { target, loved } => {
                let t = match target {
                    LoveTarget::Track { id } => StarTarget::Song(id.clone()),
                    LoveTarget::Album { id } => StarTarget::Album(id.clone()),
                    LoveTarget::Artist { id } => StarTarget::Artist(id.clone()),
                };
                if let (Some(Prior::Loved(expected)), LoveTarget::Track { id }) = (&entry.expected_prior, target) {
                    if let Ok(song) = api.song(id).await {
                        let found = song.starred.is_some();
                        if found != *expected && found != *loved {
                            report.conflicts.push(Conflict {
                                entry_id: entry.id.clone(),
                                target: entry.mutation.target_key(),
                                expected: Prior::Loved(*expected),
                                found: Prior::Loved(found),
                                applied: Prior::Loved(*loved),
                            });
                        }
                    }
                }
                if *loved {
                    api.star(&[t]).await
                } else {
                    api.unstar(&[t]).await
                }
            }
            Mutation::PlaylistCreate { name, track_ids } => {
                let created = api.create_playlist(name, track_ids).await?;
                report.created_playlists.push((entry.id.clone(), created.playlist.id.clone()));
                let sid = api.server_id();
                let p = crate::subsonic::convert::playlist_from_body(sid, api.username().as_deref(), &created.playlist);
                self.db.upsert_playlists(&[p], 0).map_err(db_err)?;
                self.db.set_playlist_tracks(sid, &created.playlist.id, track_ids).map_err(db_err)?;
                Ok(())
            }
            Mutation::PlaylistDelete { playlist_id } => match api.delete_playlist(playlist_id).await {
                Ok(()) | Err(SubsonicError::NotFound(_)) => Ok(()),
                Err(e) => Err(e),
            },
            Mutation::PlaylistRename { playlist_id, name, comment, public } => {
                api.update_playlist(
                    playlist_id,
                    PlaylistUpdate { name: name.clone(), comment: comment.clone(), public: *public, ..Default::default() },
                )
                .await
            }
            Mutation::PlaylistAdd { playlist_id, track_ids, at_index } => {
                match at_index {
                    None => {
                        api.update_playlist(playlist_id, PlaylistUpdate { song_ids_to_add: track_ids.clone(), ..Default::default() }).await?;
                    }
                    Some(idx) => {
                        let current = api.playlist(playlist_id).await?;
                        let mut order: Vec<String> = current.entry.iter().map(|c| c.id.clone()).collect();
                        let at = (*idx as usize).min(order.len());
                        for (i, t) in track_ids.iter().enumerate() {
                            order.insert(at + i, t.clone());
                        }
                        api.replace_playlist(playlist_id, &order).await?;
                    }
                }
                self.refresh_playlist(api, playlist_id).await
            }
            Mutation::PlaylistRemove { playlist_id, track_ids, indices } => {
                // Rebase on the server's order: remove by track id, preferring the queued
                // index when the same track sits there (duplicates), else its first occurrence.
                let current = api.playlist(playlist_id).await?;
                let order: Vec<String> = current.entry.iter().map(|c| c.id.clone()).collect();
                let mut remove: Vec<u32> = vec![];
                for (n, t) in track_ids.iter().enumerate() {
                    let hinted = indices.get(n).copied().filter(|i| order.get(*i as usize) == Some(t) && !remove.contains(i));
                    let found = hinted.or_else(|| {
                        order.iter().enumerate().find(|(i, id)| *id == t && !remove.contains(&(*i as u32))).map(|(i, _)| i as u32)
                    });
                    if let Some(i) = found {
                        remove.push(i);
                    }
                }
                if !remove.is_empty() {
                    api.update_playlist(playlist_id, PlaylistUpdate { song_indices_to_remove: remove, ..Default::default() }).await?;
                }
                self.refresh_playlist(api, playlist_id).await
            }
            Mutation::PlaylistMove { playlist_id, track_id, from_index, to_index } => {
                let current = api.playlist(playlist_id).await?;
                let mut order: Vec<String> = current.entry.iter().map(|c| c.id.clone()).collect();
                let from = if order.get(*from_index as usize) == Some(track_id) {
                    Some(*from_index as usize)
                } else {
                    order.iter().position(|id| id == track_id)
                };
                if let Some(from) = from {
                    let item = order.remove(from);
                    let to = (*to_index as usize).min(order.len());
                    order.insert(to, item);
                    api.replace_playlist(playlist_id, &order).await?;
                }
                self.refresh_playlist(api, playlist_id).await
            }
            Mutation::Scrobble { track_id, played_at, submission, history_id } => {
                api.scrobble(track_id, Some(*played_at), *submission).await?;
                if let (true, Some(h)) = (submission, history_id) {
                    self.db.mark_scrobbled(*h).map_err(db_err)?;
                }
                Ok(())
            }
            Mutation::SavePlayQueue { track_ids, current, position_ms } => {
                api.save_play_queue(PlayQueueSave { song_ids: track_ids.clone(), current: current.clone(), position_ms: *position_ms }).await
            }
        }
    }

    async fn refresh_playlist(&self, api: &dyn SubsonicApi, playlist_id: &str) -> Result<(), SubsonicError> {
        let sid = api.server_id();
        match api.playlist(playlist_id).await {
            Ok(p) => {
                let pl = crate::subsonic::convert::playlist_from_body(sid, api.username().as_deref(), &p.playlist);
                let ids: Vec<String> = p.entry.iter().map(|c| c.id.clone()).collect();
                self.db.upsert_playlists(&[pl], 0).map_err(db_err)?;
                self.db.set_playlist_tracks(sid, playlist_id, &ids).map_err(db_err)?;
                Ok(())
            }
            Err(SubsonicError::NotFound(_)) => {
                self.db.delete_playlist(playlist_id).map_err(db_err)?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Compare-and-swap an already-sent rating/love: only writes when the
    /// server still holds `expected`. Updates the mirror on `Applied`.
    pub async fn execute_cas(
        &self,
        api: &dyn SubsonicApi,
        target: CasTarget,
        expected: Prior,
        new: Prior,
    ) -> Result<CasOutcome, SubsonicError> {
        match (&target, &expected, &new) {
            (CasTarget::Rating(RatingTarget::Track { id }), Prior::Rating(exp), Prior::Rating(new_r)) => {
                let song = api.song(id).await?;
                let current = song.user_rating.unwrap_or(0);
                if current != *exp {
                    return Ok(CasOutcome::Skipped { current: Prior::Rating(current) });
                }
                api.set_rating(id, *new_r).await?;
                self.db.set_track_rating(id, *new_r).map_err(db_err)?;
                Ok(CasOutcome::Applied)
            }
            (CasTarget::Rating(RatingTarget::Album { id }), Prior::Rating(exp), Prior::Rating(new_r)) => {
                let album = api.album(id).await?;
                let current = album.album.user_rating.unwrap_or(0);
                if current != *exp {
                    return Ok(CasOutcome::Skipped { current: Prior::Rating(current) });
                }
                api.set_rating(id, *new_r).await?;
                self.db.set_album_rating(id, *new_r).map_err(db_err)?;
                Ok(CasOutcome::Applied)
            }
            (CasTarget::Loved(t), Prior::Loved(exp), Prior::Loved(new_l)) => {
                let (current, star) = match t {
                    LoveTarget::Track { id } => (api.song(id).await?.starred.is_some(), StarTarget::Song(id.clone())),
                    LoveTarget::Album { id } => (api.album(id).await?.album.starred.is_some(), StarTarget::Album(id.clone())),
                    LoveTarget::Artist { id } => (api.artist(id).await?.artist.starred.is_some(), StarTarget::Artist(id.clone())),
                };
                if current != *exp {
                    return Ok(CasOutcome::Skipped { current: Prior::Loved(current) });
                }
                if *new_l {
                    api.star(&[star]).await?;
                } else {
                    api.unstar(&[star]).await?;
                }
                match t {
                    LoveTarget::Track { id } => self.db.set_track_loved(id, *new_l),
                    LoveTarget::Album { id } => self.db.set_album_loved(id, *new_l),
                    LoveTarget::Artist { id } => self.db.set_artist_loved(id, *new_l),
                }
                .map_err(db_err)?;
                Ok(CasOutcome::Applied)
            }
            _ => Err(SubsonicError::Protocol("mismatched compare-and-swap target and values".into())),
        }
    }
}

fn db_err(e: DbError) -> SubsonicError {
    SubsonicError::Io(e.to_string())
}

/// Optimistic mirror update at enqueue time.
fn apply_local(tx: &rusqlite::Connection, m: &Mutation) -> DbResult<()> {
    match m {
        Mutation::SetRating { target: RatingTarget::Track { id }, rating } => {
            tx.execute("UPDATE tracks SET rating = ?2 WHERE id = ?1", params![id, rating.min(&5)])?;
        }
        Mutation::SetRating { target: RatingTarget::Album { id }, rating } => {
            tx.execute("UPDATE albums SET rating = ?2 WHERE id = ?1", params![id, rating.min(&5)])?;
        }
        Mutation::SetLoved { target: LoveTarget::Track { id }, loved } => {
            tx.execute("UPDATE tracks SET loved = ?2 WHERE id = ?1", params![id, *loved as i64])?;
        }
        Mutation::SetLoved { target: LoveTarget::Album { id }, loved } => {
            tx.execute("UPDATE albums SET loved = ?2 WHERE id = ?1", params![id, *loved as i64])?;
        }
        Mutation::SetLoved { target: LoveTarget::Artist { id }, loved } => {
            tx.execute("UPDATE artists SET loved = ?2 WHERE id = ?1", params![id, *loved as i64])?;
        }
        Mutation::PlaylistRename { playlist_id, name, comment, public } => {
            if let Some(n) = name {
                tx.execute("UPDATE playlists SET name = ?2 WHERE id = ?1", params![playlist_id, n])?;
            }
            if let Some(c) = comment {
                tx.execute("UPDATE playlists SET comment = ?2 WHERE id = ?1", params![playlist_id, c])?;
            }
            if let Some(p) = public {
                tx.execute("UPDATE playlists SET public = ?2 WHERE id = ?1", params![playlist_id, *p as i64])?;
            }
        }
        Mutation::PlaylistDelete { playlist_id } => {
            tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [playlist_id])?;
            tx.execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])?;
        }
        Mutation::PlaylistAdd { playlist_id, track_ids, at_index } => {
            let (sid, mut order) = local_order(tx, playlist_id)?;
            let at = at_index.map(|i| i as usize).unwrap_or(order.len()).min(order.len());
            for (i, t) in track_ids.iter().enumerate() {
                order.insert(at + i, t.clone());
            }
            crate::db::queries::set_playlist_tracks_in(tx, &sid, playlist_id, &order)?;
        }
        Mutation::PlaylistRemove { playlist_id, track_ids, indices } => {
            let (sid, mut order) = local_order(tx, playlist_id)?;
            let mut remove: Vec<usize> = vec![];
            for (n, t) in track_ids.iter().enumerate() {
                let hinted = indices.get(n).map(|i| *i as usize).filter(|i| order.get(*i) == Some(t) && !remove.contains(i));
                if let Some(i) = hinted.or_else(|| order.iter().enumerate().find(|(i, id)| *id == t && !remove.contains(i)).map(|(i, _)| i)) {
                    remove.push(i);
                }
            }
            remove.sort_unstable_by(|a, b| b.cmp(a));
            for i in remove {
                order.remove(i);
            }
            crate::db::queries::set_playlist_tracks_in(tx, &sid, playlist_id, &order)?;
        }
        Mutation::PlaylistMove { playlist_id, track_id, from_index, to_index } => {
            let (sid, mut order) = local_order(tx, playlist_id)?;
            let from = if order.get(*from_index as usize) == Some(track_id) {
                Some(*from_index as usize)
            } else {
                order.iter().position(|id| id == track_id)
            };
            if let Some(from) = from {
                let item = order.remove(from);
                let to = (*to_index as usize).min(order.len());
                order.insert(to, item);
                crate::db::queries::set_playlist_tracks_in(tx, &sid, playlist_id, &order)?;
            }
        }
        Mutation::PlaylistCreate { .. } | Mutation::Scrobble { .. } | Mutation::SavePlayQueue { .. } => {}
    }
    Ok(())
}

fn local_order(tx: &rusqlite::Connection, playlist_id: &str) -> DbResult<(String, Vec<String>)> {
    let sid: String = tx
        .query_row("SELECT server_id FROM playlists WHERE id = ?1", [playlist_id], |r| r.get(0))
        .optional()?
        .unwrap_or_default();
    let mut st = tx.prepare_cached("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position ASC")?;
    let rows = st.query_map([playlist_id], |r| r.get::<_, String>(0))?;
    let order = rows.collect::<Result<Vec<_>, _>>()?;
    Ok((sid, order))
}

fn revert_local(tx: &rusqlite::Connection, m: &Mutation, prior: &Prior) -> DbResult<()> {
    match (m, prior) {
        (Mutation::SetRating { target: RatingTarget::Track { id }, .. }, Prior::Rating(r)) => {
            tx.execute("UPDATE tracks SET rating = ?2 WHERE id = ?1", params![id, r])?;
        }
        (Mutation::SetRating { target: RatingTarget::Album { id }, .. }, Prior::Rating(r)) => {
            tx.execute("UPDATE albums SET rating = ?2 WHERE id = ?1", params![id, r])?;
        }
        (Mutation::SetLoved { target: LoveTarget::Track { id }, .. }, Prior::Loved(l)) => {
            tx.execute("UPDATE tracks SET loved = ?2 WHERE id = ?1", params![id, *l as i64])?;
        }
        (Mutation::SetLoved { target: LoveTarget::Album { id }, .. }, Prior::Loved(l)) => {
            tx.execute("UPDATE albums SET loved = ?2 WHERE id = ?1", params![id, *l as i64])?;
        }
        (Mutation::SetLoved { target: LoveTarget::Artist { id }, .. }, Prior::Loved(l)) => {
            tx.execute("UPDATE artists SET loved = ?2 WHERE id = ?1", params![id, *l as i64])?;
        }
        (
            Mutation::PlaylistAdd { playlist_id, .. } | Mutation::PlaylistRemove { playlist_id, .. } | Mutation::PlaylistMove { playlist_id, .. },
            Prior::PlaylistOrder(order),
        ) => {
            let (sid, _) = local_order(tx, playlist_id)?;
            crate::db::queries::set_playlist_tracks_in(tx, &sid, playlist_id, order)?;
        }
        _ => {}
    }
    Ok(())
}

/// Payload of a `JobKind::OutboxFlush` job.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct FlushJobPayload {
    /// Reset failed entries first (retry from the problems list).
    pub retry_failed: bool,
}

/// Runs the outbox as a job: flushes until nothing is due; files one problem
/// for permanently failed entries and one for entries stuck for many attempts.
pub struct OutboxFlushRunner {
    outbox: Outbox,
    api: Arc<dyn SubsonicApi>,
}

impl OutboxFlushRunner {
    pub fn new(outbox: Outbox, api: Arc<dyn SubsonicApi>) -> Self {
        OutboxFlushRunner { outbox, api }
    }
}

impl JobRunner for OutboxFlushRunner {
    fn run(&self, ctx: JobContext) -> BoxFuture<'static, JobResult<()>> {
        let outbox = self.outbox.clone();
        let api = self.api.clone();
        Box::pin(async move {
            let payload: FlushJobPayload = serde_json::from_str(&ctx.payload).unwrap_or_default();
            if payload.retry_failed {
                outbox.retry_failed()?;
            }
            let total = outbox.pending_count()?;
            ctx.set_progress(0, Some(total));
            let mut done = 0u32;
            let mut all_failed: Vec<(String, String)> = vec![];
            loop {
                ctx.checkpoint().await?;
                let r = outbox.flush(api.as_ref(), 50).await?;
                done += (r.applied + r.failed.len()) as u32;
                ctx.set_progress(done.min(total), Some(total));
                all_failed.extend(r.failed);
                if r.applied == 0 || r.deferred > 0 {
                    break;
                }
            }
            for (entry_id, err) in &all_failed {
                let label = outbox.entry(entry_id)?.map(|e| e.mutation.target_key()).unwrap_or_default();
                ctx.queue().add_problem(
                    Some(&ctx.job_id),
                    &format!("Couldn't sync {label}"),
                    Some(err),
                    Some(RetryAction::Resubmit {
                        kind: api::JobKind::OutboxFlush,
                        label: "Retry pending changes".into(),
                        payload: serde_json::to_string(&FlushJobPayload { retry_failed: true }).unwrap_or_default(),
                        items: vec![],
                    }),
                )?;
            }
            let stuck: Vec<OutboxEntry> = outbox.pending()?.into_iter().filter(|e| e.attempts >= REPORT_AFTER_ATTEMPTS).collect();
            if !stuck.is_empty() {
                ctx.queue().add_problem(
                    Some(&ctx.job_id),
                    &format!("{} change{} waiting for the server", stuck.len(), if stuck.len() == 1 { "" } else { "s" }),
                    stuck.first().and_then(|e| e.last_error.as_deref()),
                    Some(RetryAction::Resubmit { kind: api::JobKind::OutboxFlush, label: "Retry pending changes".into(), payload: "{}".into(), items: vec![] }),
                )?;
            }
            if all_failed.is_empty() {
                Ok(())
            } else {
                Err(JobError::Failed(format!("{} change{} could not be applied", all_failed.len(), if all_failed.len() == 1 { "" } else { "s" })))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{JobQueue, JobSpec};
    use crate::subsonic::fake::FakeServer;
    use crate::util::WallClock;

    struct FixedClock(parking_lot::Mutex<f64>);
    impl Clock for FixedClock {
        fn now_ms(&self) -> f64 {
            *self.0.lock()
        }
    }

    fn setup() -> (Db, FakeServer, Outbox, Arc<FixedClock>) {
        let db = Db::open_in_memory().unwrap();
        let s = FakeServer::new("srv", "alice");
        for i in 0..5 {
            let mut c = FakeServer::song(&format!("t{i}"), &format!("Track {i}"), "al", "ar", 200.0);
            c.user_rating = Some(2);
            s.add_song(c);
        }
        s.add_playlist("pl", "P", "alice", &["t0", "t1", "t2", "t3"], false);
        let tracks: Vec<api::Track> = (0..5)
            .map(|i| api::Track { id: format!("t{i}"), server_id: "srv".into(), title: format!("Track {i}"), rating: 2, ..Default::default() })
            .collect();
        db.upsert_tracks(&tracks, &[], 1).unwrap();
        db.upsert_playlists(&[api::Playlist { id: "pl".into(), server_id: "srv".into(), name: "P".into(), ..Default::default() }], 1).unwrap();
        db.set_playlist_tracks("srv", "pl", &["t0".into(), "t1".into(), "t2".into(), "t3".into()]).unwrap();
        let clock = Arc::new(FixedClock(parking_lot::Mutex::new(1_000_000.0)));
        let outbox = Outbox::new(db.clone(), clock.clone());
        (db, s, outbox, clock)
    }

    #[tokio::test]
    async fn rating_applies_locally_then_flushes_and_coalesces() {
        let (db, s, outbox, _) = setup();
        let id1 = outbox.enqueue("srv", Mutation::SetRating { target: RatingTarget::Track { id: "t0".into() }, rating: 3 }, Some(Prior::Rating(2))).unwrap();
        assert_eq!(db.track("t0").unwrap().unwrap().rating, 3, "optimistic");
        let id2 = outbox.enqueue("srv", Mutation::SetRating { target: RatingTarget::Track { id: "t0".into() }, rating: 5 }, Some(Prior::Rating(3))).unwrap();
        assert!(outbox.entry(&id1).unwrap().is_none(), "superseded");
        let e2 = outbox.entry(&id2).unwrap().unwrap();
        assert_eq!(e2.expected_prior, Some(Prior::Rating(2)), "original prior inherited");
        assert_eq!(outbox.pending_count().unwrap(), 1);
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!(r.applied, 1);
        assert_eq!(s.rating_of("t0"), 5);
        assert_eq!(s.calls_to("setRating"), 1);
        assert_eq!(outbox.entry(&id2).unwrap().unwrap().status, EntryStatus::Done);
        assert!(r.conflicts.is_empty());
    }

    #[tokio::test]
    async fn cancel_if_unsent_reverts_local_and_refuses_after_send() {
        let (db, s, outbox, _) = setup();
        let id = outbox.enqueue("srv", Mutation::SetLoved { target: LoveTarget::Track { id: "t1".into() }, loved: true }, Some(Prior::Loved(false))).unwrap();
        assert!(db.track("t1").unwrap().unwrap().loved);
        assert_eq!(outbox.cancel_if_unsent(&id).unwrap(), CancelOutcome::Cancelled);
        assert!(!db.track("t1").unwrap().unwrap().loved);
        assert_eq!(outbox.cancel_if_unsent(&id).unwrap(), CancelOutcome::AlreadySent, "cancelled is terminal");
        assert_eq!(outbox.cancel_if_unsent("nope").unwrap(), CancelOutcome::Unknown);
        let id = outbox.enqueue("srv", Mutation::SetLoved { target: LoveTarget::Track { id: "t1".into() }, loved: true }, Some(Prior::Loved(false))).unwrap();
        outbox.flush(&s, 10).await.unwrap();
        assert!(s.starred("t1"));
        assert_eq!(outbox.cancel_if_unsent(&id).unwrap(), CancelOutcome::AlreadySent);
    }

    #[tokio::test]
    async fn rating_conflict_is_last_write_wins_but_reported() {
        let (_, s, outbox, _) = setup();
        let id = outbox.enqueue("srv", Mutation::SetRating { target: RatingTarget::Track { id: "t2".into() }, rating: 4 }, Some(Prior::Rating(2))).unwrap();
        s.set_rating("t2", 1).await.unwrap(); // changed elsewhere meanwhile
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!(r.applied, 1);
        assert_eq!(s.rating_of("t2"), 4);
        assert_eq!(r.conflicts.len(), 1);
        assert_eq!(r.conflicts[0].entry_id, id);
        assert_eq!(r.conflicts[0].found, Prior::Rating(1));
        assert_eq!(r.conflicts[0].applied, Prior::Rating(4));
    }

    #[tokio::test]
    async fn transient_failures_back_off_and_permanent_ones_fail() {
        let (_, s, outbox, clock) = setup();
        let id = outbox.enqueue("srv", Mutation::SetRating { target: RatingTarget::Track { id: "t0".into() }, rating: 1 }, None).unwrap();
        let bad = outbox.enqueue("srv", Mutation::SetRating { target: RatingTarget::Track { id: "missing".into() }, rating: 1 }, None).unwrap();
        s.set_offline(true);
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!(r.deferred, 1);
        assert_eq!(r.applied, 0);
        let e = outbox.entry(&id).unwrap().unwrap();
        assert_eq!(e.status, EntryStatus::Pending);
        assert_eq!(e.attempts, 1);
        assert!(e.next_attempt_at > clock.now_ms());
        assert!(e.last_error.is_some());
        s.set_offline(false);
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!((r.applied, r.deferred), (0, 0), "not due yet");
        *clock.0.lock() += backoff_ms(1) + 1.0;
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!(r.applied, 1);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.failed[0].0, bad);
        assert_eq!(outbox.entry(&bad).unwrap().unwrap().status, EntryStatus::Failed);
        assert_eq!(outbox.failed().unwrap().len(), 1);
        assert_eq!(outbox.retry_failed().unwrap(), 1);
        assert_eq!(outbox.pending_count().unwrap(), 1);
        assert_eq!(backoff_ms(1), 5_000.0);
        assert_eq!(backoff_ms(3), 20_000.0);
        assert_eq!(backoff_ms(30), 600_000.0);
    }

    #[tokio::test]
    async fn network_failure_stops_the_batch_and_recover_resets_inflight() {
        let (db, s, outbox, _) = setup();
        for i in 0..3 {
            outbox.enqueue("srv", Mutation::SetRating { target: RatingTarget::Track { id: format!("t{i}") }, rating: 1 }, None).unwrap();
        }
        s.fail_next(SubsonicError::Network("x".into()), 1);
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!(r.deferred, 1);
        assert_eq!(s.calls_to("setRating"), 1, "stopped after the first network failure");
        // crash simulation
        db.with_conn(|c| {
            c.execute("UPDATE outbox SET status = 'inflight' WHERE status = 'pending'", [])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(outbox.recover().unwrap(), 3);
        assert_eq!(outbox.pending_count().unwrap(), 3);
    }

    #[tokio::test]
    async fn playlist_edits_rebase_on_server_order() {
        let (db, s, outbox, _) = setup();
        let prior = Prior::PlaylistOrder(vec!["t0".into(), "t1".into(), "t2".into(), "t3".into()]);
        // Locally remove t1 (index 1); meanwhile the server got t4 inserted at the front.
        let id = outbox
            .enqueue("srv", Mutation::PlaylistRemove { playlist_id: "pl".into(), track_ids: vec!["t1".into()], indices: vec![1] }, Some(prior.clone()))
            .unwrap();
        assert_eq!(db.playlist_track_ids("pl").unwrap(), vec!["t0", "t2", "t3"], "optimistic local edit");
        s.state.lock().playlist_songs.insert("pl".into(), vec!["t4".into(), "t0".into(), "t1".into(), "t2".into(), "t3".into()]);
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!(r.applied, 1);
        assert_eq!(s.playlist_song_ids("pl"), vec!["t4", "t0", "t2", "t3"], "removed by track id, not stale index");
        assert_eq!(db.playlist_track_ids("pl").unwrap(), vec!["t4", "t0", "t2", "t3"], "mirror refreshed from server");
        assert_eq!(outbox.cancel_if_unsent(&id).unwrap(), CancelOutcome::AlreadySent);

        // Move t3 to the front; server order differs from what we queued against.
        outbox.enqueue("srv", Mutation::PlaylistMove { playlist_id: "pl".into(), track_id: "t3".into(), from_index: 3, to_index: 0 }, None).unwrap();
        assert_eq!(db.playlist_track_ids("pl").unwrap(), vec!["t3", "t4", "t0", "t2"]);
        s.state.lock().playlist_songs.insert("pl".into(), vec!["t4", "t2", "t0", "t3"].into_iter().map(String::from).collect());
        outbox.flush(&s, 10).await.unwrap();
        assert_eq!(s.playlist_song_ids("pl"), vec!["t3", "t4", "t2", "t0"]);

        // Insert at index and append.
        outbox.enqueue("srv", Mutation::PlaylistAdd { playlist_id: "pl".into(), track_ids: vec!["t1".into()], at_index: Some(1) }, None).unwrap();
        outbox.enqueue("srv", Mutation::PlaylistAdd { playlist_id: "pl".into(), track_ids: vec!["t1".into()], at_index: None }, None).unwrap();
        outbox.flush(&s, 10).await.unwrap();
        assert_eq!(s.playlist_song_ids("pl"), vec!["t3", "t1", "t4", "t2", "t0", "t1"]);
        assert_eq!(db.playlist_track_ids("pl").unwrap(), s.playlist_song_ids("pl"));

        // Cancelling an unsent playlist edit restores the recorded order.
        let id = outbox
            .enqueue("srv", Mutation::PlaylistRemove { playlist_id: "pl".into(), track_ids: vec!["t4".into()], indices: vec![2] }, Some(Prior::PlaylistOrder(db.playlist_track_ids("pl").unwrap())))
            .unwrap();
        assert_eq!(db.playlist_track_ids("pl").unwrap().len(), 5);
        assert_eq!(outbox.cancel_if_unsent(&id).unwrap(), CancelOutcome::Cancelled);
        assert_eq!(db.playlist_track_ids("pl").unwrap().len(), 6);
    }

    #[tokio::test]
    async fn playlist_create_rename_delete() {
        let (db, s, outbox, _) = setup();
        let id = outbox.enqueue("srv", Mutation::PlaylistCreate { name: "New".into(), track_ids: vec!["t0".into(), "t1".into()] }, None).unwrap();
        outbox.enqueue("srv", Mutation::PlaylistRename { playlist_id: "pl".into(), name: Some("Renamed".into()), comment: None, public: Some(true) }, None).unwrap();
        assert_eq!(db.playlist("pl").unwrap().unwrap().name, "Renamed");
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!(r.applied, 2);
        let (eid, pid) = &r.created_playlists[0];
        assert_eq!(eid, &id);
        assert_eq!(s.playlist_song_ids(pid), vec!["t0", "t1"]);
        assert_eq!(db.playlist_track_ids(pid).unwrap(), vec!["t0", "t1"]);
        assert!(db.playlist(pid).unwrap().unwrap().is_mine);
        assert!(s.state.lock().playlists["pl"].public);
        outbox.enqueue("srv", Mutation::PlaylistDelete { playlist_id: "pl".into() }, None).unwrap();
        assert!(db.playlist("pl").unwrap().is_none());
        outbox.flush(&s, 10).await.unwrap();
        assert!(!s.state.lock().playlists.contains_key("pl"));
        // deleting again (already gone on server) is not an error
        outbox.enqueue("srv", Mutation::PlaylistDelete { playlist_id: "pl".into() }, None).unwrap();
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!(r.applied, 1);
    }

    #[tokio::test]
    async fn scrobble_marks_history_and_save_play_queue() {
        let (db, s, outbox, _) = setup();
        let h = db.record_play("srv", "t0", 123.0, 100_000, false, "dev").unwrap();
        outbox.enqueue("srv", Mutation::Scrobble { track_id: "t0".into(), played_at: 123.0, submission: false, history_id: None }, None).unwrap();
        outbox.enqueue("srv", Mutation::Scrobble { track_id: "t0".into(), played_at: 123.0, submission: true, history_id: Some(h) }, None).unwrap();
        outbox.enqueue("srv", Mutation::SavePlayQueue { track_ids: vec!["t0".into(), "t1".into()], current: Some("t1".into()), position_ms: Some(5000) }, None).unwrap();
        outbox.enqueue("srv", Mutation::SavePlayQueue { track_ids: vec!["t2".into()], current: Some("t2".into()), position_ms: None }, None).unwrap();
        assert_eq!(outbox.pending_count().unwrap(), 3, "play queue saves coalesce");
        let r = outbox.flush(&s, 10).await.unwrap();
        assert_eq!(r.applied, 3);
        let sc = s.scrobbles();
        assert_eq!(sc.len(), 2);
        assert!(!sc[0].submission && sc[1].submission);
        assert_eq!(sc[1].time_ms, Some(123.0));
        assert!(db.recently_played(1).unwrap()[0].scrobbled);
        assert_eq!(s.state.lock().play_queue.as_ref().unwrap().current.as_deref(), Some("t2"));
    }

    #[tokio::test]
    async fn compare_and_swap() {
        let (db, s, outbox, _) = setup();
        let t = CasTarget::Rating(RatingTarget::Track { id: "t0".into() });
        assert_eq!(outbox.execute_cas(&s, t.clone(), Prior::Rating(2), Prior::Rating(5)).await.unwrap(), CasOutcome::Applied);
        assert_eq!(s.rating_of("t0"), 5);
        assert_eq!(db.track("t0").unwrap().unwrap().rating, 5);
        s.set_rating("t0", 1).await.unwrap();
        assert_eq!(
            outbox.execute_cas(&s, t, Prior::Rating(5), Prior::Rating(2)).await.unwrap(),
            CasOutcome::Skipped { current: Prior::Rating(1) }
        );
        assert_eq!(s.rating_of("t0"), 1, "untouched");
        let l = CasTarget::Loved(LoveTarget::Track { id: "t1".into() });
        assert_eq!(outbox.execute_cas(&s, l.clone(), Prior::Loved(false), Prior::Loved(true)).await.unwrap(), CasOutcome::Applied);
        assert!(s.starred("t1") && db.track("t1").unwrap().unwrap().loved);
        assert!(matches!(outbox.execute_cas(&s, l, Prior::Rating(1), Prior::Loved(true)).await, Err(SubsonicError::Protocol(_))));
    }

    #[tokio::test]
    async fn flush_runner_files_problems_for_permanent_failures() {
        let (db, s, outbox, _) = setup();
        outbox.enqueue("srv", Mutation::SetRating { target: RatingTarget::Track { id: "ghost".into() }, rating: 1 }, None).unwrap();
        outbox.enqueue("srv", Mutation::SetRating { target: RatingTarget::Track { id: "t0".into() }, rating: 1 }, None).unwrap();
        let q = JobQueue::new(db.clone(), Arc::new(WallClock));
        q.register(api::JobKind::OutboxFlush, 1, Arc::new(OutboxFlushRunner::new(outbox.clone(), Arc::new(s.clone()))));
        let id = q.submit(JobSpec::new(api::JobKind::OutboxFlush, "Sync changes")).unwrap();
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&id).unwrap().unwrap().state, api::JobState::Failed);
        assert_eq!(s.rating_of("t0"), 1);
        let p = q.problems().unwrap();
        assert_eq!(p.len(), 2, "one per failed entry plus the job's own");
        let mine = p.iter().find(|p| p.summary.contains("rating:track:ghost")).unwrap();
        assert!(mine.retryable);
        // retrying resets the failed entry and runs a fresh flush
        let jid = q.retry_problem(&mine.id).unwrap().unwrap();
        q.run_until_idle().await.unwrap();
        assert_eq!(q.job(&jid).unwrap().unwrap().state, api::JobState::Failed, "still a ghost");
        let failed = outbox.failed().unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].attempts, 2, "the retry reset it to pending and tried again");
    }

    #[test]
    fn mutation_json_is_adjacently_tagged() {
        let m = Mutation::SetRating { target: RatingTarget::Track { id: "x".into() }, rating: 3 };
        let j = serde_json::to_value(&m).unwrap();
        assert_eq!(j["type"], "setRating");
        assert_eq!(j["data"]["target"]["type"], "track");
        let back: Mutation = serde_json::from_value(j).unwrap();
        assert_eq!(back, m);
    }
}
