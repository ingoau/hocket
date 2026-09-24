//! The album primer: when an album page has been open a moment, or the
//! pointer rests on play, the first seconds of its first track are fetched
//! into the stream cache on the device that will play it, so pressing play
//! starts from disk.
//!
//! - Routing: `Command::PrimeAlbum`/`PrimeTrack` go to the Connect engine,
//!   which hands the track to whoever holds a live transport lease
//!   (`Msg::PrimeRequest`, relayed by the room; peers that do not know it
//!   ignore it). With no remote owner this device primes.
//! - Gating, with the priming device's own state: never on the coordinator,
//!   only on an unmetered, non-cellular network (no mobile-data override;
//!   an unknown network counts as unmetered, as for prefetch), not in
//!   battery saver, not offline.
//! - Size: about [`PRIME_SECONDS`] of audio, estimated from the bit rate the
//!   stream will have (the transcoding profile, else size/duration, else the
//!   track's bit rate), clamped to [`PRIME_MIN_BYTES`]..[`PRIME_MAX_BYTES`].
//!   The bytes land as an ordinary partial cache entry through the stream
//!   reader (a bounded `Range` request), marked primed so eviction takes it
//!   first until the track is played.
//! - Never in the way: one prime at a time, at background priority (reads
//!   wait while a playback stream pulls from the server), never while a
//!   queue prefetch runs (a prefetch that starts cancels it and it is
//!   retried after); a new prime replaces an older one that has not
//!   started; tracks already pinned, cached or primed are skipped; at most
//!   [`PRIME_MAX_PER_WINDOW`] primes start per [`PRIME_WINDOW_MS`].

use std::collections::VecDeque;

use tokio_util::sync::CancellationToken;

use crate::api::*;
use crate::connect::engine::Input;
use crate::core::actor::Actor;
use crate::core::stream_reader::{StreamReader, MAX_READ, STREAM_URL_PREFIX};
use crate::core::{ActorMsg, Internal};
use crate::downloads::{CacheSignal, StreamMinter};

/// Seconds of audio a prime fetches.
pub const PRIME_SECONDS: f64 = 8.0;
pub const PRIME_MIN_BYTES: u64 = 256 * 1024;
pub const PRIME_MAX_BYTES: u64 = 1536 * 1024;
/// Rate limit: at most this many primes start ...
pub const PRIME_MAX_PER_WINDOW: usize = 20;
/// ... within this window.
pub const PRIME_WINDOW_MS: f64 = 10.0 * 60_000.0;

pub(crate) struct PrimeRunning {
    pub track_id: TrackId,
    pub generation: u64,
    pub cancel: CancellationToken,
}

#[derive(Default)]
pub(crate) struct PrimeState {
    /// Waiting for the running prime (or a queue prefetch) to finish; a
    /// newer request replaces it.
    pub pending: Option<TrackId>,
    pub running: Option<PrimeRunning>,
    /// When recent primes started (the rate limit's window).
    pub started: VecDeque<f64>,
    pub generation: u64,
}

impl Actor {
    /// `Command::PrimeAlbum`: prime the album's first track.
    pub(crate) fn prime_album(&mut self, album_id: AlbumId) {
        match self.db.album_tracks(&album_id) {
            Ok(tracks) => {
                if let Some(first) = tracks.into_iter().next() {
                    self.request_prime(first.id);
                }
            }
            Err(e) => self.log("debug", format!("prime album {album_id}: {e}")),
        }
    }

    /// `Command::PrimeTrack` (and the album's first track): ask the device
    /// that owns playback to prime it.
    pub(crate) fn request_prime(&mut self, track_id: TrackId) {
        if self.cfg.platform == Platform::Coordinator {
            return;
        }
        if self.engine.is_some() {
            self.engine_input(Input::PrimeRequest { track_id });
        } else {
            self.prime_here(track_id);
        }
    }

    /// Prime a track on this device (the engine decided it is the one).
    pub(crate) fn prime_here(&mut self, track_id: TrackId) {
        if !self.prime_allowed() {
            self.log("debug", format!("prime of {track_id} not allowed here"));
            return;
        }
        let Some(track) = self.track_or_bare(&track_id) else {
            return;
        };
        let running = self.prime.running.as_ref().map(|r| r.track_id.clone());
        if running.as_deref() == Some(track_id.as_str())
            || self.prime.pending.as_deref() == Some(track_id.as_str())
            || !self.prime_needed(&track)
        {
            return;
        }
        if running.is_some() || self.prefetch.running.is_some() {
            // Replaces an older request that has not started.
            self.prime.pending = Some(track_id);
            return;
        }
        self.start_prime(&track);
    }

    /// Whether this device may prime right now, by its own state.
    fn prime_allowed(&self) -> bool {
        if self.cfg.platform == Platform::Coordinator
            || self.stream_reader.is_none()
            || self.api().is_none()
            || self.shutting_down
            || self.battery_saver
        {
            return false;
        }
        match &self.network {
            None => true,
            Some(n) => {
                n.kind != NetworkKind::Offline && n.kind != NetworkKind::Cellular && !n.metered
            }
        }
    }

    /// Bytes a prime of this track fetches.
    pub(crate) fn prime_bytes(&self, track: &Track) -> u64 {
        let seconds = f64::from(track.duration_ms) / 1000.0;
        let profile = self.downloads.effective_profile(track.suffix.as_deref());
        let rate = if let Some(kbps) = profile.as_ref().and_then(|p| p.max_bit_rate) {
            f64::from(kbps) * 1000.0 / 8.0
        } else if let (Some(size), true) = (track.size_bytes.filter(|b| *b > 0.0), seconds > 0.0) {
            size / seconds
        } else if let Some(kbps) = track.bit_rate.filter(|k| *k > 0) {
            f64::from(kbps) * 1000.0 / 8.0
        } else {
            // Unknown: a lossless-ish rate.
            1000.0 * 1000.0 / 8.0
        };
        ((rate * PRIME_SECONDS) as u64).clamp(PRIME_MIN_BYTES, PRIME_MAX_BYTES)
    }

    /// Not pinned, not fully cached, not already primed (a partial entry
    /// holding the start), not about to be prefetched.
    fn prime_needed(&self, track: &Track) -> bool {
        let key = (track.server_id.clone(), track.id.clone());
        if self.prefetch.targets.contains(&key) {
            return false;
        }
        if matches!(
            self.downloads.downloaded_path(&track.server_id, &track.id),
            Ok(Some(_))
        ) {
            return false;
        }
        let profile = self.downloads.effective_profile(track.suffix.as_deref());
        if matches!(
            self.downloads
                .cache_lookup(&track.server_id, &track.id, profile.as_ref(), false),
            Ok(Some(_))
        ) {
            return false;
        }
        let want = self.prime_bytes(track);
        match self
            .downloads
            .cache_partial(&track.server_id, &track.id, profile.as_ref())
        {
            Ok((Some(row), _)) => {
                let held = row.spans.prefix();
                held < want && row.total.is_none_or(|t| held < t)
            }
            _ => true,
        }
    }

    fn start_prime(&mut self, track: &Track) {
        let now = self.now();
        while self
            .prime
            .started
            .front()
            .is_some_and(|t| now - t >= PRIME_WINDOW_MS)
        {
            self.prime.started.pop_front();
        }
        if self.prime.started.len() >= PRIME_MAX_PER_WINDOW {
            self.log("debug", format!("prime of {} rate-limited", track.id));
            return;
        }
        let (Some(reader), Some(api)) = (self.stream_reader.clone(), self.api()) else {
            return;
        };
        let source = match self.downloads.resolve_with(
            api.as_ref(),
            "prime",
            track,
            Some(&reader as &dyn StreamMinter),
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(target: "hocket_core", error = %e, "prime resolve");
                return;
            }
        };
        if !source.url.starts_with(STREAM_URL_PREFIX) {
            return;
        }
        let bytes = self.prime_bytes(track);
        if let Err(e) =
            self.downloads
                .cache_signal(&track.server_id, &track.id, CacheSignal::Primed)
        {
            tracing::debug!(target: "hocket_core", error = %e, "prime signal");
        }
        self.prime.started.push_back(now);
        self.prime.generation += 1;
        let generation = self.prime.generation;
        let cancel = CancellationToken::new();
        self.prime.running = Some(PrimeRunning {
            track_id: track.id.clone(),
            generation,
            cancel: cancel.clone(),
        });
        let tx = self.tx.clone();
        let url = source.url;
        self.rt.spawn(async move {
            prime_read(&reader, &url, bytes, &cancel).await;
            let _ = tx.send(ActorMsg::Internal(Internal::PrimeDone { generation }));
        });
    }

    pub(crate) fn on_prime_done(&mut self, generation: u64) {
        if self
            .prime
            .running
            .as_ref()
            .is_some_and(|r| r.generation == generation)
        {
            self.prime.running = None;
        }
        self.prime_next();
    }

    /// Start the waiting prime when nothing else is fetching.
    pub(crate) fn prime_next(&mut self) {
        if self.prime.running.is_some() || self.prefetch.running.is_some() {
            return;
        }
        if let Some(id) = self.prime.pending.take() {
            self.prime_here(id);
        }
    }

    /// A queue prefetch starts: a running prime steps aside (retried after).
    pub(crate) fn yield_prime(&mut self) {
        if let Some(r) = self.prime.running.take() {
            r.cancel.cancel();
            if self.prime.pending.is_none() {
                self.prime.pending = Some(r.track_id);
            }
        }
    }

    /// Stop priming (shutdown).
    pub(crate) fn cancel_prime(&mut self) {
        self.prime.pending = None;
        if let Some(r) = self.prime.running.take() {
            r.cancel.cancel();
        }
    }
}

/// Read the first `bytes` of the stream at background priority.
async fn prime_read(reader: &StreamReader, url: &str, bytes: u64, cancel: &CancellationToken) {
    let info = tokio::select! {
        _ = cancel.cancelled() => return,
        r = reader.open_background_range(url, 0, Some(bytes)) => match r {
            Ok(i) => i,
            Err(e) => {
                tracing::debug!(target: "hocket_core", error = %e, "prime open");
                return;
            }
        },
    };
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => break,
            r = reader.read(info.handle, MAX_READ) => r,
        };
        match chunk {
            Ok(b) if b.is_empty() => break,
            Ok(_) => {}
            Err(e) => {
                tracing::debug!(target: "hocket_core", error = %e, "prime read");
                break;
            }
        }
    }
    reader.close(info.handle);
}
