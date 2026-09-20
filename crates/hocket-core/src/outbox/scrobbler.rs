//! Scrobbler: a pure, clock-injected state machine applying Last.fm's rules,
//! plus a recorder that turns its decisions into play history and outbox
//! entries.
//!
//! Rules (design notes, "One outbox"):
//! - "now playing" is announced when a track starts;
//! - a play is submitted once played time reaches 50% of the track or
//!   4 minutes, whichever comes first;
//! - tracks shorter than 30 s never scrobble;
//! - seek-skipped time does not count: played time accumulates only from
//!   actual playback progress (bounded by elapsed wall time), so a jump in
//!   position is a seek, not listening;
//! - every repeat-one pass scrobbles like any other play;
//! - a failed (unplayable) track never scrobbles and is not a play;
//! - accumulated `played_ms` carries across a handoff so a takeover 90 s in
//!   doesn't eat the scrobble.
//!
//! Where the notes are silent (what counts as "played" for local history
//! when the threshold isn't reached), we follow Symfonium: a track only
//! enters the local play history and bumps the local play count when it
//! reaches the scrobble threshold, so stats and server counts agree.

use std::sync::Arc;

use crate::api::{Ms, TrackId};
use crate::db::{Db, DbResult};
use crate::util::Clock;

use super::{Mutation, Outbox};

/// Minimum track length to be eligible (Last.fm: 30 s).
pub const MIN_TRACK_MS: Ms = 30_000;
/// Absolute cap on the play threshold (Last.fm: 4 minutes).
pub const MAX_THRESHOLD_MS: Ms = 240_000;
/// Progress reports further apart than this (after allowing for elapsed wall
/// time) are treated as seeks.
pub const SEEK_TOLERANCE_MS: f64 = 1_500.0;

/// Threshold at which a track of `duration_ms` counts as played.
pub fn threshold_ms(duration_ms: Ms) -> Ms {
    (duration_ms / 2).min(MAX_THRESHOLD_MS)
}

/// What the state machine wants done.
#[derive(Debug, Clone, PartialEq)]
pub enum ScrobbleAction {
    NowPlaying {
        track_id: TrackId,
    },
    /// Threshold reached: record and submit. `played_at` is when the play started.
    Submit {
        track_id: TrackId,
        started_at: f64,
        played_ms: Ms,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct Current {
    track_id: TrackId,
    duration_ms: Ms,
    started_at: f64,
    played_ms: f64,
    last_position_ms: Option<f64>,
    last_progress_at: f64,
    playing: bool,
    submitted: bool,
    failed: bool,
    /// Which pass of a repeat-one loop this is (for logging only).
    pass: u32,
}

/// The pure state machine. Feed it transport facts; collect actions.
pub struct Scrobbler {
    clock: Arc<dyn Clock>,
    current: Option<Current>,
}

impl Scrobbler {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Scrobbler {
            clock,
            current: None,
        }
    }

    /// A new item started. `carried_played_ms` is the accumulated time from a
    /// handoff (0 for a fresh start). Returns the now-playing action for
    /// eligible tracks, and implicitly abandons the previous item.
    pub fn track_started(
        &mut self,
        track_id: &str,
        duration_ms: Ms,
        position_ms: Ms,
        carried_played_ms: Ms,
        playing: bool,
    ) -> Vec<ScrobbleAction> {
        let now = self.clock.now_ms();
        self.current = Some(Current {
            track_id: track_id.to_string(),
            duration_ms,
            started_at: now - f64::from(carried_played_ms),
            played_ms: f64::from(carried_played_ms),
            last_position_ms: Some(f64::from(position_ms)),
            last_progress_at: now,
            playing,
            submitted: false,
            failed: false,
            pass: 0,
        });
        let mut actions = vec![];
        if duration_ms >= MIN_TRACK_MS && playing {
            actions.push(ScrobbleAction::NowPlaying {
                track_id: track_id.to_string(),
            });
        }
        actions.extend(self.check());
        actions
    }

    /// Repeat-one wrapped around: same track, a fresh play.
    pub fn track_repeated(&mut self, track_id: &str, duration_ms: Ms) -> Vec<ScrobbleAction> {
        let pass = self.current.as_ref().map(|c| c.pass + 1).unwrap_or(0);
        let mut a = self.track_started(track_id, duration_ms, 0, 0, true);
        if let Some(c) = &mut self.current {
            c.pass = pass;
        }
        // now-playing is repeated per pass, as any player would
        if a.is_empty() && duration_ms >= MIN_TRACK_MS {
            a.push(ScrobbleAction::NowPlaying {
                track_id: track_id.to_string(),
            });
        }
        a
    }

    /// A position report from the backend. Only forward motion consistent
    /// with elapsed wall time counts as played.
    pub fn progress(&mut self, track_id: &str, position_ms: Ms) -> Vec<ScrobbleAction> {
        let now = self.clock.now_ms();
        let Some(c) = self.current.as_mut() else {
            return vec![];
        };
        if c.track_id != track_id || c.failed {
            return vec![];
        }
        let pos = f64::from(position_ms);
        if let Some(last) = c.last_position_ms {
            let delta = pos - last;
            let elapsed = (now - c.last_progress_at).max(0.0);
            if c.playing && delta > 0.0 && delta <= elapsed + SEEK_TOLERANCE_MS {
                c.played_ms += delta;
            }
            // else: a seek (either direction), a stall, or paused — nothing counted
        }
        c.last_position_ms = Some(pos);
        c.last_progress_at = now;
        self.check()
    }

    /// Explicit seek: resets the reference position so the jump is not counted.
    pub fn seeked(&mut self, track_id: &str, position_ms: Ms) {
        let now = self.clock.now_ms();
        if let Some(c) = self.current.as_mut() {
            if c.track_id == track_id {
                c.last_position_ms = Some(f64::from(position_ms));
                c.last_progress_at = now;
            }
        }
    }

    pub fn set_playing(&mut self, playing: bool) -> Vec<ScrobbleAction> {
        let now = self.clock.now_ms();
        let mut actions = vec![];
        if let Some(c) = self.current.as_mut() {
            let was = c.playing;
            c.playing = playing;
            c.last_progress_at = now;
            if playing
                && !was
                && !c.submitted
                && !c.failed
                && c.duration_ms >= MIN_TRACK_MS
                && c.played_ms == 0.0
            {
                actions.push(ScrobbleAction::NowPlaying {
                    track_id: c.track_id.clone(),
                });
            }
        }
        actions
    }

    /// The item could not be played: it never scrobbles and is not a play.
    pub fn track_failed(&mut self, track_id: &str) {
        if let Some(c) = self.current.as_mut() {
            if c.track_id == track_id {
                c.failed = true;
                c.playing = false;
            }
        }
    }

    /// Item ended naturally or was skipped away from. Returns a submit if
    /// the threshold was crossed but not yet reported (e.g. the final
    /// progress event arrived late).
    pub fn track_ended(
        &mut self,
        track_id: &str,
        final_position_ms: Option<Ms>,
    ) -> Vec<ScrobbleAction> {
        let mut actions = vec![];
        if let Some(p) = final_position_ms {
            actions.extend(self.progress(track_id, p));
        }
        if self
            .current
            .as_ref()
            .is_some_and(|c| c.track_id == track_id)
        {
            actions.extend(self.check());
            self.current = None;
        }
        actions
    }

    /// Accumulated played time of the current item (travels with a handoff).
    pub fn played_ms(&self) -> Ms {
        self.current
            .as_ref()
            .map(|c| c.played_ms.round().max(0.0) as Ms)
            .unwrap_or(0)
    }

    pub fn current_track(&self) -> Option<&str> {
        self.current.as_ref().map(|c| c.track_id.as_str())
    }

    pub fn is_submitted(&self) -> bool {
        self.current.as_ref().is_some_and(|c| c.submitted)
    }

    fn check(&mut self) -> Vec<ScrobbleAction> {
        let Some(c) = self.current.as_mut() else {
            return vec![];
        };
        if c.submitted || c.failed || c.duration_ms < MIN_TRACK_MS {
            return vec![];
        }
        if c.played_ms >= f64::from(threshold_ms(c.duration_ms)) {
            c.submitted = true;
            return vec![ScrobbleAction::Submit {
                track_id: c.track_id.clone(),
                started_at: c.started_at,
                played_ms: c.played_ms.round() as Ms,
            }];
        }
        vec![]
    }
}

/// Applies [`ScrobbleAction`]s: play history + local counts in the mirror,
/// and `Scrobble` mutations in the outbox.
pub struct ScrobbleRecorder {
    db: Db,
    outbox: Outbox,
    device_id: String,
}

impl ScrobbleRecorder {
    pub fn new(db: Db, outbox: Outbox, device_id: impl Into<String>) -> Self {
        ScrobbleRecorder {
            db,
            outbox,
            device_id: device_id.into(),
        }
    }

    pub fn apply(&self, server_id: &str, action: &ScrobbleAction) -> DbResult<()> {
        match action {
            ScrobbleAction::NowPlaying { track_id } => {
                self.outbox.enqueue(
                    server_id,
                    Mutation::Scrobble {
                        track_id: track_id.clone(),
                        played_at: self.outbox_now(),
                        submission: false,
                        history_id: None,
                    },
                    None,
                )?;
            }
            ScrobbleAction::Submit {
                track_id,
                started_at,
                played_ms,
            } => {
                let history_id = self.db.record_play(
                    server_id,
                    track_id,
                    *started_at,
                    *played_ms,
                    false,
                    &self.device_id,
                )?;
                self.outbox.enqueue(
                    server_id,
                    Mutation::Scrobble {
                        track_id: track_id.clone(),
                        played_at: *started_at,
                        submission: true,
                        history_id: Some(history_id),
                    },
                    None,
                )?;
            }
        }
        Ok(())
    }

    pub fn apply_all(&self, server_id: &str, actions: &[ScrobbleAction]) -> DbResult<()> {
        for a in actions {
            self.apply(server_id, a)?;
        }
        Ok(())
    }

    fn outbox_now(&self) -> f64 {
        self.outbox.clock_now()
    }
}

impl Outbox {
    pub(crate) fn clock_now(&self) -> f64 {
        self.clock.now_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    struct TestClock(Mutex<f64>);
    impl Clock for TestClock {
        fn now_ms(&self) -> f64 {
            *self.0.lock()
        }
    }
    impl TestClock {
        fn advance(&self, ms: f64) {
            *self.0.lock() += ms;
        }
    }

    fn scrobbler() -> (Scrobbler, Arc<TestClock>) {
        let clock = Arc::new(TestClock(Mutex::new(1_000_000.0)));
        (Scrobbler::new(clock.clone()), clock)
    }

    /// Simulate real playback: 1 s ticks with matching position.
    fn play_for(
        s: &mut Scrobbler,
        clock: &TestClock,
        id: &str,
        from_ms: Ms,
        seconds: u32,
    ) -> Vec<ScrobbleAction> {
        let mut out = vec![];
        for i in 1..=seconds {
            clock.advance(1000.0);
            out.extend(s.progress(id, from_ms + i * 1000));
        }
        out
    }

    #[test]
    fn thresholds() {
        assert_eq!(threshold_ms(200_000), 100_000);
        assert_eq!(threshold_ms(600_000), 240_000);
        assert_eq!(threshold_ms(30_000), 15_000);
    }

    #[test]
    fn now_playing_then_submit_at_half() {
        let (mut s, clock) = scrobbler();
        let a = s.track_started("t", 200_000, 0, 0, true);
        assert_eq!(
            a,
            vec![ScrobbleAction::NowPlaying {
                track_id: "t".into()
            }]
        );
        let a = play_for(&mut s, &clock, "t", 0, 99);
        assert!(a.is_empty());
        assert_eq!(s.played_ms(), 99_000);
        let a = play_for(&mut s, &clock, "t", 99_000, 1);
        assert_eq!(
            a,
            vec![ScrobbleAction::Submit {
                track_id: "t".into(),
                started_at: 1_000_000.0,
                played_ms: 100_000
            }]
        );
        assert!(s.is_submitted());
        // no double submit
        assert!(play_for(&mut s, &clock, "t", 100_000, 50).is_empty());
        assert!(s.track_ended("t", Some(200_000)).is_empty());
        assert!(s.current_track().is_none());
    }

    #[test]
    fn four_minute_cap_on_long_tracks() {
        let (mut s, clock) = scrobbler();
        s.track_started("long", 3_600_000, 0, 0, true);
        let a = play_for(&mut s, &clock, "long", 0, 240);
        assert!(matches!(
            a.last(),
            Some(ScrobbleAction::Submit {
                played_ms: 240_000,
                ..
            })
        ));
    }

    #[test]
    fn short_tracks_never_scrobble() {
        let (mut s, clock) = scrobbler();
        assert!(s.track_started("short", 29_999, 0, 0, true).is_empty());
        assert!(play_for(&mut s, &clock, "short", 0, 29).is_empty());
        assert!(s.track_ended("short", Some(29_999)).is_empty());
        let (mut s, clock) = scrobbler();
        assert_eq!(s.track_started("edge", 30_000, 0, 0, true).len(), 1);
        assert!(play_for(&mut s, &clock, "edge", 0, 15)
            .iter()
            .any(|a| matches!(a, ScrobbleAction::Submit { .. })));
    }

    #[test]
    fn seeks_do_not_count() {
        let (mut s, clock) = scrobbler();
        s.track_started("t", 200_000, 0, 0, true);
        play_for(&mut s, &clock, "t", 0, 10);
        // jump forward 150 s in one tick: a seek
        clock.advance(1000.0);
        let a = s.progress("t", 160_000);
        assert!(a.is_empty());
        assert_eq!(s.played_ms(), 10_000);
        // continue playing from there: only real time accrues
        play_for(&mut s, &clock, "t", 160_000, 30);
        assert_eq!(s.played_ms(), 40_000);
        // backwards seek
        s.seeked("t", 5_000);
        clock.advance(1000.0);
        s.progress("t", 6_000);
        assert_eq!(s.played_ms(), 41_000);
        // a stall (position unchanged) counts nothing
        clock.advance(5000.0);
        s.progress("t", 6_000);
        assert_eq!(s.played_ms(), 41_000);
        // explicit seek then a progress right after: the jump is ignored
        s.seeked("t", 100_000);
        clock.advance(500.0);
        s.progress("t", 100_500);
        assert_eq!(s.played_ms(), 41_500);
    }

    #[test]
    fn scrubbing_to_the_end_never_scrobbles() {
        let (mut s, clock) = scrobbler();
        s.track_started("t", 200_000, 0, 0, true);
        play_for(&mut s, &clock, "t", 0, 5);
        clock.advance(200.0);
        let a = s.progress("t", 199_000);
        assert!(a.is_empty());
        assert!(s.track_ended("t", Some(200_000)).is_empty());
    }

    #[test]
    fn pause_does_not_accrue() {
        let (mut s, clock) = scrobbler();
        s.track_started("t", 200_000, 0, 0, true);
        play_for(&mut s, &clock, "t", 0, 20);
        assert!(s.set_playing(false).is_empty());
        clock.advance(60_000.0);
        s.progress("t", 20_000);
        assert_eq!(s.played_ms(), 20_000);
        let a = s.set_playing(true);
        assert!(
            a.is_empty(),
            "resume of a partly-played track isn't a new now-playing"
        );
        play_for(&mut s, &clock, "t", 20_000, 80);
        assert!(s.is_submitted());
    }

    #[test]
    fn started_paused_announces_on_play() {
        let (mut s, _) = scrobbler();
        assert!(s.track_started("t", 200_000, 0, 0, false).is_empty());
        assert_eq!(
            s.set_playing(true),
            vec![ScrobbleAction::NowPlaying {
                track_id: "t".into()
            }]
        );
    }

    #[test]
    fn failed_tracks_never_scrobble() {
        let (mut s, clock) = scrobbler();
        s.track_started("bad", 200_000, 0, 0, true);
        play_for(&mut s, &clock, "bad", 0, 10);
        s.track_failed("bad");
        assert!(play_for(&mut s, &clock, "bad", 10_000, 200).is_empty());
        assert!(s.track_ended("bad", Some(200_000)).is_empty());
    }

    #[test]
    fn every_repeat_one_pass_scrobbles() {
        let (mut s, clock) = scrobbler();
        s.track_started("t", 60_000, 0, 0, true);
        let a = play_for(&mut s, &clock, "t", 0, 60);
        assert_eq!(
            a.iter()
                .filter(|a| matches!(a, ScrobbleAction::Submit { .. }))
                .count(),
            1
        );
        let a = s.track_repeated("t", 60_000);
        assert_eq!(
            a,
            vec![ScrobbleAction::NowPlaying {
                track_id: "t".into()
            }]
        );
        let a = play_for(&mut s, &clock, "t", 0, 60);
        let subs: Vec<_> = a
            .iter()
            .filter(|a| matches!(a, ScrobbleAction::Submit { .. }))
            .collect();
        assert_eq!(subs.len(), 1);
        if let ScrobbleAction::Submit { started_at, .. } = subs[0] {
            assert_eq!(
                *started_at,
                1_000_000.0 + 60_000.0,
                "second pass has its own start time"
            );
        }
    }

    #[test]
    fn played_ms_carries_across_handoff() {
        let (mut a, clock) = scrobbler();
        a.track_started("t", 200_000, 0, 0, true);
        play_for(&mut a, &clock, "t", 0, 90);
        let carried = a.played_ms();
        assert_eq!(carried, 90_000);
        // device B takes over 90 s in, with the carried played time
        let (mut b, clock_b) = scrobbler();
        let started = b.track_started("t", 200_000, 90_000, carried, true);
        assert_eq!(
            started,
            vec![ScrobbleAction::NowPlaying {
                track_id: "t".into()
            }]
        );
        let acts = play_for(&mut b, &clock_b, "t", 90_000, 10);
        assert!(
            matches!(
                acts.last(),
                Some(ScrobbleAction::Submit {
                    played_ms: 100_000,
                    ..
                })
            ),
            "{acts:?}"
        );
        // without the carry it would not have scrobbled
        let (mut c, clock_c) = scrobbler();
        c.track_started("t", 200_000, 90_000, 0, true);
        assert!(play_for(&mut c, &clock_c, "t", 90_000, 10)
            .iter()
            .all(|a| !matches!(a, ScrobbleAction::Submit { .. })));
    }

    #[test]
    fn late_final_progress_on_end_still_submits() {
        let (mut s, clock) = scrobbler();
        s.track_started("t", 100_000, 0, 0, true);
        play_for(&mut s, &clock, "t", 0, 49);
        clock.advance(1000.0);
        let a = s.track_ended("t", Some(50_000));
        assert!(matches!(a.as_slice(), [ScrobbleAction::Submit { .. }]));
    }

    #[test]
    fn switching_tracks_abandons_the_old_one() {
        let (mut s, clock) = scrobbler();
        s.track_started("a", 100_000, 0, 0, true);
        play_for(&mut s, &clock, "a", 0, 10);
        s.track_started("b", 100_000, 0, 0, true);
        assert!(
            s.progress("a", 60_000).is_empty(),
            "stale reports for a are ignored"
        );
        assert_eq!(s.played_ms(), 0);
        assert_eq!(s.current_track(), Some("b"));
    }

    #[test]
    fn recorder_writes_history_and_outbox() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_tracks(
            &[crate::api::Track {
                id: "t".into(),
                server_id: "srv".into(),
                title: "T".into(),
                ..Default::default()
            }],
            &[],
            1,
        )
        .unwrap();
        let clock = Arc::new(TestClock(Mutex::new(5_000.0)));
        let outbox = Outbox::new(db.clone(), clock.clone());
        let rec = ScrobbleRecorder::new(db.clone(), outbox.clone(), "dev");
        rec.apply_all(
            "srv",
            &[
                ScrobbleAction::NowPlaying {
                    track_id: "t".into(),
                },
                ScrobbleAction::Submit {
                    track_id: "t".into(),
                    started_at: 4_000.0,
                    played_ms: 120_000,
                },
            ],
        )
        .unwrap();
        let pending = outbox.pending().unwrap();
        assert_eq!(pending.len(), 2);
        assert!(matches!(
            &pending[0].mutation,
            Mutation::Scrobble {
                submission: false,
                ..
            }
        ));
        assert!(
            matches!(&pending[1].mutation, Mutation::Scrobble { submission: true, history_id: Some(_), played_at, .. } if *played_at == 4_000.0)
        );
        let h = db.recently_played(5).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].played_ms, 120_000);
        assert!(!h[0].scrobbled);
        assert_eq!(h[0].device_id, "dev");
        let (lpc, llp): (i64, Option<f64>) = db
            .with_conn(|c| {
                Ok(c.query_row(
                    "SELECT local_play_count, local_last_played FROM tracks WHERE id='t'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((lpc, llp), (1, Some(4_000.0)));
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        #[derive(Debug, Clone)]
        enum Ev {
            Tick(u32),
            Seek(u32),
            Pause,
            Play,
        }

        fn ev() -> impl Strategy<Value = Ev> {
            prop_oneof![
                6 => (1u32..=3000).prop_map(Ev::Tick),
                1 => (0u32..=600_000).prop_map(Ev::Seek),
                1 => Just(Ev::Pause),
                1 => Just(Ev::Play),
            ]
        }

        proptest! {
            /// Played time never exceeds wall time spent playing, never
            /// exceeds the track, and a submit happens iff the threshold is
            /// reached (given eligibility).
            #[test]
            fn played_time_is_bounded_and_threshold_is_exact(duration in 1_000u32..=600_000, events in proptest::collection::vec(ev(), 1..200)) {
                let clock = Arc::new(TestClock(Mutex::new(0.0)));
                let mut s = Scrobbler::new(clock.clone());
                s.track_started("t", duration, 0, 0, true);
                let mut pos: u32 = 0;
                let mut playing = true;
                let mut wall_playing = 0.0;
                // model: real listening = playback progress, which stops at the end of the track
                let mut expected_played = 0.0;
                let mut submits = 0;
                for e in events {
                    match e {
                        Ev::Tick(ms) => {
                            clock.advance(f64::from(ms));
                            if playing {
                                wall_playing += f64::from(ms);
                                let before = pos;
                                pos = (pos + ms).min(duration);
                                expected_played += f64::from(pos - before);
                            }
                            for a in s.progress("t", pos) {
                                if matches!(a, ScrobbleAction::Submit { .. }) { submits += 1; }
                            }
                        }
                        Ev::Seek(p) => { pos = p.min(duration); s.seeked("t", pos); }
                        Ev::Pause => { playing = false; s.set_playing(false); }
                        Ev::Play => { playing = true; s.set_playing(true); }
                    }
                    prop_assert!(f64::from(s.played_ms()) <= wall_playing + 1.0);
                    prop_assert!(s.played_ms() <= duration);
                }
                for a in s.track_ended("t", Some(pos)) {
                    if matches!(a, ScrobbleAction::Submit { .. }) { submits += 1; }
                }
                prop_assert!(submits <= 1);
                let eligible = duration >= MIN_TRACK_MS;
                prop_assert!((f64::from(s.played_ms()) - expected_played).abs() <= 1.0 || s.current_track().is_none());
                prop_assert_eq!(submits == 1, eligible && expected_played >= f64::from(threshold_ms(duration)), "submits={} eligible={} played={} thr={}", submits, eligible, expected_played, threshold_ms(duration));
            }
        }
    }
}
