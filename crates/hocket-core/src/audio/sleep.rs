//! Sleep timer state machine.
//!
//! Pure and clock-injected: the actor calls [`SleepTimerMachine::tick`] on its
//! timer and [`SleepTimerMachine::track_ended`] at item boundaries and
//! executes the returned [`SleepAction`]. Nothing here touches the backend.
//!
//! Semantics (matching Symfonium's sleep timer, which offers a duration, a
//! "wait for the end of the track" option and a fade-out — see
//! <https://support.symfonium.app/>):
//!
//! | `ends_at` | `stop_at_end_of_track` | behaviour |
//! |-----------|------------------------|-----------|
//! | set       | false                  | stop when the clock reaches `ends_at` |
//! | none      | true                   | stop when the current track ends |
//! | set       | true                   | when `ends_at` is reached, keep playing until the track that is playing *then* ends, then stop |
//!
//! With [`SleepTimerMachine::set_fade`] enabled the machine asks for a
//! linear volume fade over the last [`FADE_MS`] before a timed stop, and
//! restores the volume when it fires or is cancelled (so a cancelled timer
//! never leaves the volume half way down). End-of-track stops don't fade:
//! the track's own ending is the fade.

use std::sync::Arc;

use crate::api::SleepTimer;
use crate::util::Clock;

/// Length of the optional fade-out before a timed stop.
pub const FADE_MS: f64 = 10_000.0;

/// What the actor should do after a tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SleepAction {
    /// Nothing due.
    None,
    /// Multiply the user volume by `gain` (0.0–1.0) for the fade-out. The
    /// actor applies it without changing the stored user volume.
    Fade { gain: f64 },
    /// Pause playback, restore the user volume if a fade was in progress, and
    /// clear the timer (the machine has already cleared itself).
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    /// Counting down to `ends_at`.
    Timed {
        ends_at: f64,
        then_end_of_track: bool,
    },
    /// Waiting for the current item to finish.
    EndOfTrack,
}

/// See the module docs.
pub struct SleepTimerMachine {
    clock: Arc<dyn Clock>,
    phase: Phase,
    fade: bool,
    fading: bool,
    /// `stop_at_end_of_track` as configured, echoed back in [`Self::state`].
    stop_at_end_of_track: bool,
}

impl SleepTimerMachine {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            phase: Phase::Idle,
            fade: true,
            fading: false,
            stop_at_end_of_track: false,
        }
    }

    /// Whether timed stops fade the volume over the last [`FADE_MS`].
    pub fn set_fade(&mut self, enabled: bool) {
        self.fade = enabled;
    }

    pub fn fade_enabled(&self) -> bool {
        self.fade
    }

    /// Install or clear the timer. Returns the action that applies right
    /// away: `Stop` if an already-elapsed timer was set, or `Fade{1.0}` when
    /// cancelling mid-fade so the volume comes back.
    pub fn set(&mut self, timer: Option<SleepTimer>) -> SleepAction {
        let restore = if self.fading {
            self.fading = false;
            SleepAction::Fade { gain: 1.0 }
        } else {
            SleepAction::None
        };
        match timer {
            None => {
                self.phase = Phase::Idle;
                self.stop_at_end_of_track = false;
                restore
            }
            Some(SleepTimer {
                ends_at: None,
                stop_at_end_of_track: false,
            }) => {
                // A timer with nothing to wait for is a cleared timer.
                self.phase = Phase::Idle;
                self.stop_at_end_of_track = false;
                restore
            }
            Some(SleepTimer {
                ends_at: None,
                stop_at_end_of_track: true,
            }) => {
                self.phase = Phase::EndOfTrack;
                self.stop_at_end_of_track = true;
                restore
            }
            Some(SleepTimer {
                ends_at: Some(ends_at),
                stop_at_end_of_track,
            }) => {
                self.stop_at_end_of_track = stop_at_end_of_track;
                self.phase = Phase::Timed {
                    ends_at,
                    then_end_of_track: stop_at_end_of_track,
                };
                match self.tick() {
                    SleepAction::None => restore,
                    due => due,
                }
            }
        }
    }

    /// The timer as the UI should show it (`None` when idle).
    pub fn state(&self) -> Option<SleepTimer> {
        match &self.phase {
            Phase::Idle => None,
            Phase::Timed {
                ends_at,
                then_end_of_track,
            } => Some(SleepTimer {
                ends_at: Some(*ends_at),
                stop_at_end_of_track: *then_end_of_track,
            }),
            Phase::EndOfTrack => Some(SleepTimer {
                ends_at: None,
                stop_at_end_of_track: true,
            }),
        }
    }

    pub fn is_active(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Milliseconds until the timed stop, `None` when not counting down.
    pub fn remaining_ms(&self) -> Option<f64> {
        match &self.phase {
            Phase::Timed { ends_at, .. } => Some((ends_at - self.clock.now_ms()).max(0.0)),
            _ => None,
        }
    }

    /// Evaluate the timer against the clock. Call at ~1 Hz while playing and
    /// whenever the timer changes. Cheap when idle.
    pub fn tick(&mut self) -> SleepAction {
        let now = self.clock.now_ms();
        match self.phase.clone() {
            Phase::Idle | Phase::EndOfTrack => SleepAction::None,
            Phase::Timed {
                ends_at,
                then_end_of_track,
            } => {
                if now >= ends_at {
                    if then_end_of_track {
                        self.phase = Phase::EndOfTrack;
                        if self.fading {
                            self.fading = false;
                            return SleepAction::Fade { gain: 1.0 };
                        }
                        return SleepAction::None;
                    }
                    self.phase = Phase::Idle;
                    self.stop_at_end_of_track = false;
                    self.fading = false;
                    return SleepAction::Stop;
                }
                let remaining = ends_at - now;
                if self.fade && !then_end_of_track && remaining <= FADE_MS {
                    self.fading = true;
                    return SleepAction::Fade {
                        gain: (remaining / FADE_MS).clamp(0.0, 1.0),
                    };
                }
                SleepAction::None
            }
        }
    }

    /// The current item finished playing naturally. Returns `Stop` if the
    /// machine was waiting for that.
    pub fn track_ended(&mut self) -> SleepAction {
        match self.phase {
            Phase::EndOfTrack => {
                self.phase = Phase::Idle;
                self.stop_at_end_of_track = false;
                self.fading = false;
                SleepAction::Stop
            }
            _ => SleepAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestClock(AtomicU64);
    impl TestClock {
        fn advance(&self, ms: u64) {
            self.0.fetch_add(ms, Ordering::SeqCst);
        }
    }
    impl Clock for TestClock {
        fn now_ms(&self) -> f64 {
            self.0.load(Ordering::SeqCst) as f64
        }
    }

    fn machine() -> (Arc<TestClock>, SleepTimerMachine) {
        let clock = Arc::new(TestClock(AtomicU64::new(100_000)));
        (clock.clone(), SleepTimerMachine::new(clock))
    }

    #[test]
    fn timed_stop_fires_once_and_clears() {
        let (clock, mut m) = machine();
        m.set_fade(false);
        assert_eq!(
            m.set(Some(SleepTimer {
                ends_at: Some(130_000.0),
                stop_at_end_of_track: false
            })),
            SleepAction::None
        );
        assert!(m.is_active());
        assert_eq!(m.remaining_ms(), Some(30_000.0));
        clock.advance(29_999);
        assert_eq!(m.tick(), SleepAction::None);
        clock.advance(1);
        assert_eq!(m.tick(), SleepAction::Stop);
        assert_eq!(m.tick(), SleepAction::None);
        assert_eq!(m.state(), None);
    }

    #[test]
    fn fade_ramps_over_the_last_ten_seconds() {
        let (clock, mut m) = machine();
        m.set(Some(SleepTimer {
            ends_at: Some(120_000.0),
            stop_at_end_of_track: false,
        }));
        clock.advance(9_000);
        assert_eq!(m.tick(), SleepAction::None);
        clock.advance(1_000); // 10 s left
        assert_eq!(m.tick(), SleepAction::Fade { gain: 1.0 });
        clock.advance(5_000);
        assert_eq!(m.tick(), SleepAction::Fade { gain: 0.5 });
        clock.advance(4_900);
        match m.tick() {
            SleepAction::Fade { gain } => assert!((gain - 0.01).abs() < 1e-9),
            other => panic!("{other:?}"),
        }
        clock.advance(100);
        assert_eq!(m.tick(), SleepAction::Stop);
    }

    #[test]
    fn cancelling_mid_fade_restores_volume() {
        let (clock, mut m) = machine();
        m.set(Some(SleepTimer {
            ends_at: Some(105_000.0),
            stop_at_end_of_track: false,
        }));
        clock.advance(2_000);
        assert!(matches!(m.tick(), SleepAction::Fade { .. }));
        assert_eq!(m.set(None), SleepAction::Fade { gain: 1.0 });
        assert!(!m.is_active());
        assert_eq!(m.tick(), SleepAction::None);
    }

    #[test]
    fn end_of_track_waits_for_the_boundary() {
        let (clock, mut m) = machine();
        assert_eq!(
            m.set(Some(SleepTimer {
                ends_at: None,
                stop_at_end_of_track: true
            })),
            SleepAction::None
        );
        clock.advance(600_000);
        assert_eq!(m.tick(), SleepAction::None);
        assert_eq!(
            m.state(),
            Some(SleepTimer {
                ends_at: None,
                stop_at_end_of_track: true
            })
        );
        assert_eq!(m.track_ended(), SleepAction::Stop);
        assert_eq!(m.track_ended(), SleepAction::None);
        assert!(!m.is_active());
    }

    #[test]
    fn timed_then_end_of_track_never_fades_and_arms_on_expiry() {
        let (clock, mut m) = machine();
        m.set(Some(SleepTimer {
            ends_at: Some(110_000.0),
            stop_at_end_of_track: true,
        }));
        clock.advance(5_000);
        assert_eq!(m.tick(), SleepAction::None);
        assert_eq!(
            m.track_ended(),
            SleepAction::None,
            "tracks ending before expiry don't stop"
        );
        clock.advance(5_000);
        assert_eq!(m.tick(), SleepAction::None);
        assert_eq!(
            m.state(),
            Some(SleepTimer {
                ends_at: None,
                stop_at_end_of_track: true
            })
        );
        assert_eq!(m.track_ended(), SleepAction::Stop);
    }

    #[test]
    fn already_elapsed_timer_stops_immediately() {
        let (_, mut m) = machine();
        assert_eq!(
            m.set(Some(SleepTimer {
                ends_at: Some(1.0),
                stop_at_end_of_track: false
            })),
            SleepAction::Stop
        );
        assert!(!m.is_active());
    }

    #[test]
    fn empty_timer_is_cleared() {
        let (_, mut m) = machine();
        m.set(Some(SleepTimer {
            ends_at: None,
            stop_at_end_of_track: false,
        }));
        assert!(!m.is_active());
        assert_eq!(m.state(), None);
    }
}
