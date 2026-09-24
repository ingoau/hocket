//! The actor's side of the stream cache: the signals eviction weighs
//! (autoplay picks, early skips, plays), what is playable offline (a
//! download or a complete cache entry: offline playback skips everything
//! else and autoplay picks from it), the auto-sized budget and the traffic
//! counters behind the "data saved" figure.

use crate::api::*;
use crate::core::actor::Actor;
use crate::core::{ActorMsg, Internal};
use crate::downloads::CacheSignal;
use crate::session::{Effect, QueueOp};
use crate::settings::keys;

/// Leaving a track before this (or before half of it, when shorter) is an
/// early skip.
pub const EARLY_SKIP_MS: Ms = 30_000;
/// Items skipped in a row as unavailable offline before playback stops.
pub const MAX_OFFLINE_SKIPS: u32 = 500;
/// How often the auto-sized budget is re-evaluated and the traffic
/// counters are saved.
pub const CACHE_CHECK_MS: f64 = 10.0 * 60_000.0;

#[derive(Default)]
pub(crate) struct CacheState {
    /// Items skipped in a row because they cannot play offline.
    pub offline_skips: u32,
    /// Whether the "skipping what isn't offline" notice was shown for the
    /// current run of skips.
    pub offline_notice: bool,
    /// The offline marks a clear submitted to a remote room, until its
    /// verdict: refused as stale (a peer's op landed first), the clear runs
    /// once more on the room's document. See `clear_offline_skips`.
    pub offline_clear_sent: Option<std::collections::BTreeSet<QueueKey>>,
    pub last_check: f64,
}

impl Actor {
    pub(crate) fn is_offline(&self) -> bool {
        self.network
            .as_ref()
            .is_some_and(|n| n.kind == NetworkKind::Offline)
    }

    /// Playable with no network: downloaded, or complete in the stream
    /// cache (in the profile playback asks for, the original, or another
    /// complete variant).
    pub(crate) fn available_offline(&self, track: &Track) -> bool {
        if matches!(
            self.downloads.downloaded_path(&track.server_id, &track.id),
            Ok(Some(_))
        ) {
            return true;
        }
        let profile = self.downloads.effective_profile(track.suffix.as_deref());
        matches!(
            self.downloads
                .cache_lookup(&track.server_id, &track.id, profile.as_ref(), false),
            Ok(Some(_))
        )
    }

    /// Record what eviction weighs when `item` is loaded: the track being
    /// left early is a skip; an autoplay pick is marked; a track about to
    /// play is no longer a primed-but-never-played entry.
    pub(crate) fn note_cache_signals(
        &mut self,
        item: &QueueItem,
        track: &Track,
        transitioned: bool,
        play: bool,
    ) {
        let now = self.now();
        let leaving = self
            .playback
            .track
            .clone()
            .filter(|_| self.playback.loaded && !transitioned)
            .filter(|_| self.playback.doc_key.as_deref() != Some(item.key.as_str()));
        let dl = self.downloads.clone();
        let record = |sid: &str, tid: &str, s: CacheSignal| {
            if let Err(e) = dl.cache_signal(sid, tid, s) {
                tracing::debug!(target: "hocket_core", error = %e, "cache signal");
            }
        };
        if let Some(prev) = leaving {
            let pos = self.playback.position_now(now);
            let limit = EARLY_SKIP_MS.min(prev.duration_ms / 2);
            if pos < limit {
                record(&prev.server_id, &prev.id, CacheSignal::Skipped);
            }
        }
        if matches!(item.source, QueueSource::Autoplay { .. }) {
            record(&track.server_id, &track.id, CacheSignal::Autoplay);
        }
        if play {
            record(&track.server_id, &track.id, CacheSignal::Played);
        }
    }

    /// Offline, an item that is neither downloaded nor fully cached is
    /// skipped (as unavailable on this device) instead of failing to load.
    /// `true` when it was skipped and must not be loaded.
    pub(crate) fn skip_if_unavailable_offline(&mut self, item: &QueueItem, track: &Track) -> bool {
        if !self.is_offline() || !self.owns_transport() || self.available_offline(track) {
            self.cache.offline_skips = 0;
            self.cache.offline_notice = false;
            return false;
        }
        self.cache.offline_skips += 1;
        if self.cache.offline_skips > MAX_OFFLINE_SKIPS {
            self.cache.offline_skips = 0;
            self.playback.want_playing = false;
            self.player_notice(
                PlayerNoticeCode::NothingAvailableOffline,
                "Nothing in the queue is available offline",
                None,
            );
            return true;
        }
        if !self.cache.offline_notice {
            self.cache.offline_notice = true;
            self.player_notice(
                PlayerNoticeCode::OfflineSkipping,
                "Offline: skipping tracks that aren't downloaded or cached",
                None,
            );
        }
        // Skipped on the next turn of the loop (never from inside the
        // reaction that is loading it).
        let _ = self.tx.send(ActorMsg::Internal(Internal::SkipOffline {
            key: item.key.clone(),
        }));
        true
    }

    pub(crate) fn on_skip_offline(&mut self, key: QueueKey) {
        let current = self
            .doc()
            .and_then(|d| d.current.as_ref())
            .map(|c| c.key.clone());
        if current.as_deref() == Some(key.as_str()) && self.owns_transport() {
            self.playback.want_playing = true;
            // Marked as an offline skip (cleared when the network returns),
            // not as a failure; the offline notice already said why.
            self.local_reduced_op(QueueOp::SkipOffline { key }, |e| {
                !matches!(e, Effect::Skipped { .. })
            });
        }
    }

    /// Autoplay while offline: tracks that play without a network, nearest
    /// the listening first (same artist, then genre), least recently played
    /// first, never what the queue already holds.
    pub(crate) fn offline_autoplay_picks(
        &self,
        seeds: &crate::autoplay::SeedInput,
        recently_autoplayed: &[TrackId],
        want: usize,
    ) -> Vec<crate::autoplay::AutoplayPick> {
        let Some(sid) = self.server_id() else {
            return vec![];
        };
        let seed = seeds
            .recent
            .first()
            .and_then(|s| self.db.track(&s.id).ok().flatten());
        let artist = seed.as_ref().and_then(|t| t.artist_id.clone());
        let genre = seed.as_ref().and_then(|t| t.genre.clone());
        let ids: Vec<String> = self
            .db
            .with_conn(|c| {
                let mut st = c.prepare_cached(
                    "SELECT id FROM tracks WHERE server_id = ?1 AND offline IN (1, 2)
                     ORDER BY (artist_id IS ?2 AND ?2 IS NOT NULL) DESC, (genre IS ?3 AND ?3 IS NOT NULL) DESC,
                              COALESCE(local_last_played, 0) ASC, id",
                )?;
                let rows = st.query_map(rusqlite::params![sid, artist, genre], |r| r.get(0))?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap_or_default();
        let taken: std::collections::HashSet<&TrackId> = seeds
            .exclude
            .iter()
            .chain(seeds.recent.iter().map(|t| &t.id))
            .collect();
        let fresh: Vec<&String> = ids
            .iter()
            .filter(|id| !taken.contains(id) && !recently_autoplayed.contains(id))
            .collect();
        let pool: Vec<&String> = if fresh.is_empty() {
            ids.iter().filter(|id| !taken.contains(id)).collect()
        } else {
            fresh
        };
        pool.into_iter()
            .take(want)
            .filter_map(|id| {
                let track = self.db.track(id).ok().flatten()?;
                Some(crate::autoplay::AutoplayPick {
                    track: crate::subsonic::convert::summary_of(&track),
                    provider: AutoplayProvider::Random,
                    reason: "Available offline".into(),
                    score: None,
                })
            })
            .collect()
    }

    /// The stream cache's budget: `storage.cacheMaxBytes` when the user
    /// chose a size (any size, the old 2 GiB default included), otherwise
    /// (`null`, the default) sized from the cache volume. Enforced at once
    /// when it shrank.
    pub(crate) fn apply_cache_budget(&mut self) {
        let before = self.downloads.cache_budget();
        let user = self
            .settings
            .get(keys::STORAGE_CACHE_MAX_BYTES)
            .as_f64()
            .filter(|b| *b > 0.0);
        match user {
            Some(b) => {
                self.downloads.set_cache_budget(b);
                self.downloads.set_cache_budget_auto(false);
            }
            None => {
                let b = self.downloads.auto_cache_budget();
                self.downloads.set_cache_budget(b);
                self.downloads.set_cache_budget_auto(true);
            }
        }
        if self.downloads.cache_budget() < before {
            match self.downloads.evict_over_budget() {
                Ok(evicted) if !evicted.is_empty() => self.on_stream_cache_changed(evicted),
                Ok(_) => {}
                Err(e) => self.log("warn", format!("stream cache budget: {e}")),
            }
        }
    }

    /// Every [`CACHE_CHECK_MS`]: re-size an automatic budget (free space
    /// moves) and save the traffic counters.
    pub(crate) fn cache_tick(&mut self, now: f64) {
        if now - self.cache.last_check < CACHE_CHECK_MS {
            return;
        }
        self.cache.last_check = now;
        self.apply_cache_budget();
        self.downloads.save_traffic();
    }
}
