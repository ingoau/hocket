//! Background audio prefetch: the device that owns playback reads the next
//! two upcoming queue items through the in-process stream reader, from
//! byte 0 to the end, so the reader's normal write-through path completes
//! their stream-cache entries before they come up (no separate download
//! code, and they play from disk with zero server requests; bytes an
//! earlier read or the album primer left are not fetched again).
//!
//! - Which: the first two playable items after the current one in derived
//!   play order (playing-next insertions, then upcoming; shuffle and repeat
//!   as the reducer derives them), resolved with the transcoding profile
//!   playback would use on the current network. Items already pinned or
//!   fully cached count toward the two but need no fetch.
//! - Who: only the transport owner, with its own battery/network state.
//!   Losing ownership cancels at once; gaining it starts after the debounce.
//! - Gating: off on the coordinator; paused offline, with battery saver on
//!   and `battery.pausePrefetch`, and on metered/cellular networks unless
//!   `storage.prefetchOnMobileData`.
//! - Budget: the targets are protected from eviction like the loaded
//!   tracks; if fetching them would take more than a quarter of
//!   `storage.cacheMaxBytes`, the ones that don't fit are skipped.
//! - One fetch at a time, reading at background priority (it waits while
//!   a playback stream is pulling from the server). A queue change is
//!   debounced ([`PREFETCH_DEBOUNCE_MS`]); a fetch no longer among the next
//!   two is cancelled (its temp file removed) and the new ones start.
//!
//! [`Actor::start_prefetch`] is the reusable entry point for one track.

use std::collections::HashSet;

use tokio_util::sync::CancellationToken;

use crate::api::*;
use crate::core::actor::Actor;
use crate::core::stream_reader::{StreamReader, MAX_READ, STREAM_URL_PREFIX};
use crate::core::{ActorMsg, Internal};
use crate::downloads::{StreamMinter, TrackKey};
use crate::session::reducer::derive;
use crate::settings::keys;

/// How many upcoming items are prefetched.
pub const PREFETCH_AHEAD: usize = 2;
/// Quiet time after a queue change before prefetch follows it.
pub const PREFETCH_DEBOUNCE_MS: f64 = 2_000.0;
/// Share of the cache budget prefetch may fill.
pub const PREFETCH_BUDGET_SHARE: f64 = 0.25;

/// How one prefetch ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchOutcome {
    Done,
    Cancelled,
    Failed,
}

pub(crate) struct PrefetchRunning {
    pub key: TrackKey,
    pub generation: u64,
    pub cancel: CancellationToken,
}

#[derive(Default)]
pub(crate) struct PrefetchState {
    /// Something that may change the wanted set happened; re-evaluated on
    /// the next tick.
    pub check: bool,
    /// The wanted set as last computed.
    pub wanted: Vec<TrackKey>,
    /// When `wanted` last changed and has not been acted on (debounce).
    pub changed_at: Option<f64>,
    /// What the prefetcher is working towards (protected from eviction).
    pub targets: Vec<TrackKey>,
    pub running: Option<PrefetchRunning>,
    pub generation: u64,
    /// Tracks whose fetch failed while they stayed wanted (not retried
    /// until the wanted set changes).
    pub failed: HashSet<String>,
}

impl Actor {
    /// Something that can change what to prefetch happened (queue, network,
    /// battery, settings, ownership, server).
    pub(crate) fn mark_prefetch_check(&mut self) {
        self.prefetch.check = true;
    }

    pub(crate) fn prefetch_tick(&mut self, now: f64) {
        if self.prefetch.check {
            self.prefetch.check = false;
            let wanted = self.prefetch_wanted();
            if wanted != self.prefetch.wanted {
                self.prefetch.wanted = wanted;
                self.prefetch.failed.clear();
                if self.prefetch.wanted.is_empty() {
                    // Paused or no longer the owner: stop now.
                    self.prefetch.changed_at = None;
                    self.apply_prefetch();
                } else {
                    self.prefetch.changed_at = Some(now);
                }
            }
        }
        if let Some(at) = self.prefetch.changed_at {
            if now - at >= PREFETCH_DEBOUNCE_MS {
                self.prefetch.changed_at = None;
                self.apply_prefetch();
            }
        }
    }

    /// Whether this device prefetches audio right now.
    fn prefetch_allowed(&self) -> bool {
        if self.cfg.platform == Platform::Coordinator
            || self.stream_reader.is_none()
            || self.api().is_none()
            || !self.owns_transport()
            || self.shutting_down
        {
            return false;
        }
        if self.battery_saver && self.settings.get_bool(keys::BATTERY_PAUSE_PREFETCH) {
            return false;
        }
        match &self.network {
            None => true,
            Some(n) if n.kind == NetworkKind::Offline => false,
            Some(n) if n.metered || n.kind == NetworkKind::Cellular => self
                .settings
                .get_bool(keys::STORAGE_PREFETCH_ON_MOBILE_DATA),
            Some(_) => true,
        }
    }

    /// The next [`PREFETCH_AHEAD`] playable items, trimmed to the budget
    /// share; empty when prefetch is not allowed.
    fn prefetch_wanted(&self) -> Vec<TrackKey> {
        if !self.prefetch_allowed() {
            return vec![];
        }
        let Some(doc) = self.doc() else {
            return vec![];
        };
        if doc.repeat == RepeatMode::One {
            return vec![];
        }
        let current = doc.current.as_ref().map(|i| i.track_id.clone());
        let d = derive(doc);
        let items: Vec<TrackId> = d
            .playing_next
            .into_iter()
            .chain(d.upcoming)
            .filter(|i| !i.unavailable && Some(&i.track_id) != current.as_ref())
            .map(|i| i.track_id)
            .take(PREFETCH_AHEAD)
            .collect();
        let share = self.downloads.cache_budget() * PREFETCH_BUDGET_SHARE;
        let mut used = 0.0;
        let mut out = vec![];
        for id in items {
            let Some(track) = self.track_or_bare(&id) else {
                continue;
            };
            let key = (track.server_id.clone(), track.id.clone());
            if out.contains(&key) {
                continue;
            }
            // Already-cached targets count with their real size (pins are
            // outside the cache budget).
            used += match self.prefetch_cached_bytes(&track) {
                Some(bytes) => bytes,
                None => self.prefetch_estimate(&track),
            };
            if used > share {
                break;
            }
            out.push(key);
        }
        out
    }

    /// Bytes the track already takes in the stream cache (0 when pinned),
    /// or `None` when it still needs fetching.
    fn prefetch_cached_bytes(&self, track: &Track) -> Option<f64> {
        if matches!(
            self.downloads.downloaded_path(&track.server_id, &track.id),
            Ok(Some(_))
        ) {
            return Some(0.0);
        }
        let profile = self.downloads.effective_profile(track.suffix.as_deref());
        match self
            .downloads
            .cache_lookup(&track.server_id, &track.id, profile.as_ref(), false)
        {
            Ok(Some(e)) => Some(e.bytes as f64),
            _ => None,
        }
    }

    /// Not pinned and not fully cached under the profile playback would use.
    fn prefetch_needed(&self, track: &Track) -> bool {
        if matches!(
            self.downloads.downloaded_path(&track.server_id, &track.id),
            Ok(Some(_))
        ) {
            return false;
        }
        let profile = self.downloads.effective_profile(track.suffix.as_deref());
        !matches!(
            self.downloads
                .cache_lookup(&track.server_id, &track.id, profile.as_ref(), false),
            Ok(Some(_))
        )
    }

    /// Rough bytes a fetch of the track will write.
    fn prefetch_estimate(&self, track: &Track) -> f64 {
        let seconds = f64::from(track.duration_ms) / 1000.0;
        let profile = self.downloads.effective_profile(track.suffix.as_deref());
        if let Some(kbps) = profile.as_ref().and_then(|p| p.max_bit_rate) {
            return f64::from(kbps) * 1000.0 / 8.0 * seconds;
        }
        if let Some(b) = track.size_bytes.filter(|b| *b > 0.0) {
            return b;
        }
        if let Some(kbps) = track.bit_rate.filter(|k| *k > 0) {
            return f64::from(kbps) * 1000.0 / 8.0 * seconds;
        }
        // Unknown: assume a lossless-ish rate.
        1000.0 * 1000.0 / 8.0 * seconds
    }

    /// Act on the wanted set: protect it, cancel a fetch that left it,
    /// start the next one needed.
    pub(crate) fn apply_prefetch(&mut self) {
        let targets = self.prefetch.wanted.clone();
        self.prefetch.targets = targets.clone();
        self.protect_loaded_tracks();
        if let Some(r) = &self.prefetch.running {
            if !targets.contains(&r.key) {
                r.cancel.cancel();
                self.prefetch.running = None;
            }
        }
        if self.prefetch.running.is_some() {
            return;
        }
        for (_, id) in targets {
            if self.prefetch.failed.contains(&id) {
                continue;
            }
            let Some(track) = self.track_or_bare(&id) else {
                continue;
            };
            if !self.prefetch_needed(&track) {
                continue;
            }
            if self.start_prefetch(&track, None) {
                return;
            }
        }
    }

    /// Read one track through the stream reader in the background, from
    /// byte 0: the whole track (`byte_limit` `None`, which completes its
    /// cache entry) or only its first bytes (kept as a partial span).
    /// Whatever an earlier read or the primer left is served from disk and
    /// only the gaps are fetched. Replaces any running prefetch and makes a
    /// running prime step aside. `false` when there is nothing to fetch
    /// (downloaded, no server, no reader).
    pub(crate) fn start_prefetch(&mut self, track: &Track, byte_limit: Option<u64>) -> bool {
        let (Some(reader), Some(api)) = (self.stream_reader.clone(), self.api()) else {
            return false;
        };
        let source = match self.downloads.resolve_with(
            api.as_ref(),
            "prefetch",
            track,
            Some(&reader as &dyn StreamMinter),
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(target: "hocket_core", error = %e, "prefetch resolve");
                return false;
            }
        };
        if !source.url.starts_with(STREAM_URL_PREFIX) {
            return false;
        }
        if let Some(r) = self.prefetch.running.take() {
            r.cancel.cancel();
        }
        // The primer never holds up the queue.
        self.yield_prime();
        self.prefetch.generation += 1;
        let generation = self.prefetch.generation;
        let cancel = CancellationToken::new();
        let key = (track.server_id.clone(), track.id.clone());
        self.prefetch.running = Some(PrefetchRunning {
            key: key.clone(),
            generation,
            cancel: cancel.clone(),
        });
        let tx = self.tx.clone();
        let url = source.url;
        self.rt.spawn(async move {
            let outcome = prefetch_read(&reader, &url, byte_limit, &cancel).await;
            let _ = tx.send(ActorMsg::Internal(Internal::PrefetchDone {
                track_id: key.1,
                generation,
                outcome,
            }));
        });
        true
    }

    pub(crate) fn on_prefetch_done(
        &mut self,
        track_id: TrackId,
        generation: u64,
        outcome: PrefetchOutcome,
    ) {
        if self
            .prefetch
            .running
            .as_ref()
            .is_none_or(|r| r.generation != generation)
        {
            return;
        }
        self.prefetch.running = None;
        if outcome == PrefetchOutcome::Failed {
            self.log("debug", format!("prefetch of {track_id} failed"));
            self.prefetch.failed.insert(track_id);
        }
        self.apply_prefetch();
        self.prime_next();
    }

    /// Stop any prefetch (shutdown).
    pub(crate) fn cancel_prefetch(&mut self) {
        if let Some(r) = self.prefetch.running.take() {
            r.cancel.cancel();
        }
    }
}

async fn prefetch_read(
    reader: &StreamReader,
    url: &str,
    byte_limit: Option<u64>,
    cancel: &CancellationToken,
) -> PrefetchOutcome {
    let info = tokio::select! {
        _ = cancel.cancelled() => return PrefetchOutcome::Cancelled,
        r = reader.open_background(url) => match r {
            Ok(i) => i,
            Err(e) => {
                tracing::debug!(target: "hocket_core", error = %e, "prefetch open");
                return PrefetchOutcome::Failed;
            }
        },
    };
    let mut read: u64 = 0;
    let outcome = loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => break PrefetchOutcome::Cancelled,
            r = reader.read(info.handle, MAX_READ) => r,
        };
        match chunk {
            Ok(b) if b.is_empty() => break PrefetchOutcome::Done,
            Ok(b) => {
                read += b.len() as u64;
                if byte_limit.is_some_and(|n| read >= n) {
                    break PrefetchOutcome::Done;
                }
            }
            Err(e) => {
                tracing::debug!(target: "hocket_core", error = %e, "prefetch read");
                break PrefetchOutcome::Failed;
            }
        }
    };
    reader.close(info.handle);
    outcome
}
