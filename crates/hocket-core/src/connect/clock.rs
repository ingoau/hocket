//! Session clock: offset estimation and position extrapolation.
//!
//! Position is never broadcast on a timer. The transport owner sends a
//! [`PositionStamp`] `(position, takenAt, rate, isPlaying)` on change and every
//! receiver extrapolates from it. `takenAt` is on the *session clock* (the
//! room's clock when connected), so each device needs its offset from that
//! clock: estimated NTP-style from `t0..t3` samples, taking the sample with the
//! **minimum round trip** over a sliding window, because the shortest trip is
//! the one with the least asymmetric queueing.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::api::{EpochMs, Ms, PositionStamp};
use crate::util::Clock;

/// How many samples the sliding window keeps.
pub const SAMPLE_WINDOW: usize = 8;
/// Past this gap between a stamp and now, don't extrapolate: snap to the
/// start of the track. "Past about an hour" in the design notes.
pub const MAX_EXTRAPOLATION_GAP_MS: f64 = 3_600_000.0;

/// One NTP-style exchange. `t0` client send, `t1` server receive, `t2` server
/// send, `t3` client receive. `t0`/`t3` are on the local clock, `t1`/`t2` on
/// the session clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockSample {
    pub t0: EpochMs,
    pub t1: EpochMs,
    pub t2: EpochMs,
    pub t3: EpochMs,
}

impl ClockSample {
    /// Session minus local, in ms.
    pub fn offset(&self) -> f64 {
        ((self.t1 - self.t0) + (self.t2 - self.t3)) / 2.0
    }

    /// Network round trip excluding the server's processing time.
    pub fn round_trip(&self) -> f64 {
        ((self.t3 - self.t0) - (self.t2 - self.t1)).max(0.0)
    }
}

/// Sliding-window offset estimator. Pure; no clock inside.
#[derive(Debug, Clone, Default)]
pub struct OffsetEstimator {
    samples: VecDeque<ClockSample>,
}

impl OffsetEstimator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a sample; keeps the last [`SAMPLE_WINDOW`].
    pub fn record(&mut self, sample: ClockSample) {
        if sample.t3 < sample.t0 || !sample.offset().is_finite() {
            return;
        }
        self.samples.push_back(sample);
        while self.samples.len() > SAMPLE_WINDOW {
            self.samples.pop_front();
        }
    }

    /// The sample with the shortest round trip in the window.
    pub fn best(&self) -> Option<&ClockSample> {
        self.samples.iter().min_by(|a, b| a.round_trip().total_cmp(&b.round_trip()))
    }

    /// Estimated offset (session − local) in ms; `0` before any sample.
    pub fn offset_ms(&self) -> f64 {
        self.best().map(|s| s.offset()).unwrap_or(0.0)
    }

    /// Round trip of the best sample.
    pub fn round_trip_ms(&self) -> Option<f64> {
        self.best().map(|s| s.round_trip())
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Forget everything (new room, new clock).
    pub fn reset(&mut self) {
        self.samples.clear();
    }
}

/// A clock that reads session time: local time plus the estimated offset.
/// The lyric renderer and the UI's progress bar read from this.
#[derive(Clone)]
pub struct SessionClock {
    clock: Arc<dyn Clock>,
    estimator: Arc<parking_lot::RwLock<OffsetEstimator>>,
}

impl std::fmt::Debug for SessionClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClock").field("offset_ms", &self.offset_ms()).finish()
    }
}

impl SessionClock {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        SessionClock { clock, estimator: Arc::new(parking_lot::RwLock::new(OffsetEstimator::new())) }
    }

    /// Local wall time from the injected clock.
    pub fn now_local_ms(&self) -> EpochMs {
        self.clock.now_ms()
    }

    /// Session time: local plus offset.
    pub fn now_session_ms(&self) -> EpochMs {
        self.clock.now_ms() + self.offset_ms()
    }

    pub fn offset_ms(&self) -> f64 {
        self.estimator.read().offset_ms()
    }

    pub fn round_trip_ms(&self) -> Option<f64> {
        self.estimator.read().round_trip_ms()
    }

    pub fn record(&self, sample: ClockSample) {
        self.estimator.write().record(sample);
    }

    pub fn reset(&self) {
        self.estimator.write().reset();
    }

    /// Convert a session timestamp to local.
    pub fn to_local(&self, session_ms: EpochMs) -> EpochMs {
        session_ms - self.offset_ms()
    }

    /// Convert a local timestamp to session.
    pub fn to_session(&self, local_ms: EpochMs) -> EpochMs {
        local_ms + self.offset_ms()
    }

    /// Current playback position from the last stamp.
    pub fn position(&self, stamp: &PositionStamp) -> Ms {
        extrapolate(stamp, self.now_session_ms())
    }
}

/// Where playback is now, given the last stamp and the current session time.
/// Paused stamps don't move; playing ones advance at `rate`. Never goes
/// backwards from the stamp when the clock is behind it.
pub fn extrapolate(stamp: &PositionStamp, now_session_ms: EpochMs) -> Ms {
    if !stamp.is_playing {
        return stamp.position_ms;
    }
    let elapsed = (now_session_ms - stamp.taken_at).max(0.0);
    let rate = if stamp.rate.is_finite() && stamp.rate > 0.0 { stamp.rate } else { 1.0 };
    let advanced = elapsed * rate;
    let pos = stamp.position_ms as f64 + advanced;
    if pos >= u32::MAX as f64 {
        u32::MAX
    } else {
        pos as Ms
    }
}

/// Extrapolate, but clamp to the track's duration when known.
pub fn extrapolate_clamped(stamp: &PositionStamp, now_session_ms: EpochMs, duration_ms: Option<Ms>) -> Ms {
    let p = extrapolate(stamp, now_session_ms);
    match duration_ms {
        Some(d) => p.min(d),
        None => p,
    }
}

/// Position to *resume* from after an offline gap. Inside the gap this is
/// plain extrapolation; past [`MAX_EXTRAPOLATION_GAP_MS`] it snaps to the
/// start of the track. Used for resume offers and returning devices.
pub fn resume_position(stamp: &PositionStamp, now_session_ms: EpochMs) -> Ms {
    let gap = now_session_ms - stamp.taken_at;
    if gap > MAX_EXTRAPOLATION_GAP_MS {
        0
    } else if !stamp.is_playing {
        stamp.position_ms
    } else {
        extrapolate(stamp, now_session_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::WallClock;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FixedClock(AtomicU64);
    impl Clock for FixedClock {
        fn now_ms(&self) -> f64 {
            self.0.load(Ordering::SeqCst) as f64
        }
    }

    fn sample(t0: f64, t1: f64, t2: f64, t3: f64) -> ClockSample {
        ClockSample { t0, t1, t2, t3 }
    }

    #[test]
    fn symmetric_sample_gives_exact_offset() {
        // local clock is 1000 behind the session clock; 40 ms each way
        let s = sample(0.0, 1040.0, 1045.0, 85.0);
        assert!((s.offset() - 1000.0).abs() < 1e-9);
        assert!((s.round_trip() - 80.0).abs() < 1e-9);
    }

    #[test]
    fn estimator_prefers_minimum_round_trip() {
        let mut e = OffsetEstimator::new();
        // a noisy long sample with a wrong-looking offset
        e.record(sample(0.0, 1500.0, 1500.0, 1000.0)); // rtt 1000, offset 1000
        e.record(sample(2000.0, 3020.0, 3020.0, 2040.0)); // rtt 40, offset 1000
        e.record(sample(4000.0, 5300.0, 5300.0, 4200.0)); // rtt 200, offset 1200 (asymmetric)
        assert_eq!(e.offset_ms(), 1000.0);
        assert_eq!(e.round_trip_ms(), Some(40.0));
    }

    #[test]
    fn window_slides() {
        let mut e = OffsetEstimator::new();
        e.record(sample(0.0, 10.0, 10.0, 2.0)); // best: rtt 2
        for i in 1..=SAMPLE_WINDOW {
            let t = i as f64 * 100.0;
            e.record(sample(t, t + 100.0, t + 100.0, t + 50.0)); // rtt 50, offset 75
        }
        assert_eq!(e.sample_count(), SAMPLE_WINDOW);
        assert_eq!(e.offset_ms(), 75.0);
    }

    #[test]
    fn estimator_ignores_garbage() {
        let mut e = OffsetEstimator::new();
        e.record(sample(10.0, 0.0, 0.0, 5.0)); // t3 < t0
        e.record(sample(0.0, f64::NAN, 0.0, 5.0));
        assert_eq!(e.sample_count(), 0);
        assert_eq!(e.offset_ms(), 0.0);
        assert_eq!(e.round_trip_ms(), None);
    }

    #[test]
    fn session_clock_applies_offset() {
        let c = Arc::new(FixedClock(AtomicU64::new(5000)));
        let sc = SessionClock::new(c.clone());
        assert_eq!(sc.now_session_ms(), 5000.0);
        sc.record(sample(4000.0, 4530.0, 4530.0, 4060.0)); // offset 500
        assert_eq!(sc.now_session_ms(), 5500.0);
        assert_eq!(sc.to_local(5500.0), 5000.0);
        assert_eq!(sc.to_session(5000.0), 5500.0);
        c.0.store(6000, Ordering::SeqCst);
        assert_eq!(sc.now_session_ms(), 6500.0);
        sc.reset();
        assert_eq!(sc.now_session_ms(), 6000.0);
    }

    #[test]
    fn extrapolation_honours_rate_and_playing() {
        let playing = PositionStamp { position_ms: 10_000, taken_at: 1000.0, rate: 1.0, is_playing: true };
        assert_eq!(extrapolate(&playing, 3500.0), 12_500);
        let fast = PositionStamp { rate: 2.0, ..playing.clone() };
        assert_eq!(extrapolate(&fast, 3500.0), 15_000);
        let paused = PositionStamp { is_playing: false, ..playing.clone() };
        assert_eq!(extrapolate(&paused, 99_999.0), 10_000);
        // clock behind the stamp: never before the stamp
        assert_eq!(extrapolate(&playing, 0.0), 10_000);
        // bad rate falls back to 1
        let bad = PositionStamp { rate: f64::NAN, ..playing.clone() };
        assert_eq!(extrapolate(&bad, 2000.0), 11_000);
        assert_eq!(extrapolate_clamped(&playing, 3500.0, Some(11_000)), 11_000);
    }

    #[test]
    fn resume_snaps_past_an_hour() {
        let stamp = PositionStamp { position_ms: 90_000, taken_at: 0.0, rate: 1.0, is_playing: true };
        assert_eq!(resume_position(&stamp, 60_000.0), 150_000);
        assert_eq!(resume_position(&stamp, MAX_EXTRAPOLATION_GAP_MS + 1.0), 0);
        let paused = PositionStamp { is_playing: false, ..stamp.clone() };
        assert_eq!(resume_position(&paused, 1000.0), 90_000);
        assert_eq!(resume_position(&paused, MAX_EXTRAPOLATION_GAP_MS * 2.0), 0);
    }

    #[test]
    fn wall_clock_session_clock_is_monotone_enough() {
        let sc = SessionClock::new(Arc::new(WallClock));
        let a = sc.now_session_ms();
        let b = sc.now_session_ms();
        assert!(b >= a);
        assert!(sc.position(&PositionStamp::default()) == 0);
    }
}
