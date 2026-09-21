//! A deterministic backend for tests and the simulation harness.
//!
//! Plays virtual time from an injected [`Clock`]: nothing happens on its own,
//! the harness advances the clock and calls [`ScriptedBackend::poll`], which
//! emits the [`BackendReport`]s a real backend would have produced in that
//! interval — `Position` at the 1 Hz cadence, `Ended` and
//! `TransitionedToNext` at item boundaries, `Ready`/`Playing`/`Paused` after
//! loads. Durations come from `MediaSource.track.duration_ms`.
//!
//! Failures are scripted per key with [`ScriptedBackend::fail_key`]: a load of
//! that key reports `Error{fatal: true}` instead of `Ready`, which is how the
//! actor's "skip after consecutive failures" path is exercised. Every emitted
//! command is also recorded in [`ScriptedBackend::log`] for assertions.

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::api::{BackendReport, MediaSource, OutputDevice, QueueKey};
use crate::audio::backend::{clamp_volume, BackendError, PlaybackBackend, ReportSink};
use crate::util::Clock;

/// Interval between `Position` reports while playing, matching the ≥ 1 Hz
/// contract in [`BackendReport::Position`].
pub const POSITION_INTERVAL_MS: f64 = 1000.0;

/// What the scripted backend was asked to do, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptedCall {
    Load {
        key: QueueKey,
        next: Option<QueueKey>,
        position_ms: u32,
        play: bool,
    },
    SetNext {
        next: Option<QueueKey>,
    },
    Play,
    Pause,
    Stop,
    Seek {
        position_ms: u32,
    },
    SetVolume {
        volume: f64,
    },
    PreBuffer {
        key: QueueKey,
        position_ms: u32,
    },
    DiscardPreBuffer,
    SetGapless {
        enabled: bool,
    },
    SetOutputDevice {
        id: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct Item {
    source: MediaSource,
    /// Position at `anchor_ms` on the virtual clock.
    position_ms: f64,
    anchor_ms: f64,
}

impl Item {
    fn duration(&self) -> f64 {
        f64::from(self.source.track.duration_ms)
    }
    fn position_at(&self, now: f64, playing: bool) -> f64 {
        if playing {
            (self.position_ms + (now - self.anchor_ms).max(0.0)).min(self.duration())
        } else {
            self.position_ms
        }
    }
    fn rebase(&mut self, now: f64, playing: bool) {
        self.position_ms = self.position_at(now, playing);
        self.anchor_ms = now;
    }
}

#[derive(Debug, Default)]
struct State {
    current: Option<Item>,
    next: Option<MediaSource>,
    pre_buffer: Option<MediaSource>,
    playing: bool,
    /// Loads that have not yet reported `Ready` (they do on the next poll).
    pending_ready: bool,
    pending_pre_buffer: bool,
    last_position_report_ms: f64,
    volume: f64,
    gapless: bool,
    device: Option<String>,
    failing: HashSet<QueueKey>,
    /// Track ids whose loads fail (whatever key they carry).
    failing_tracks: HashSet<String>,
    /// Extra virtual milliseconds a load takes before `Ready`.
    load_latency_ms: f64,
    load_started_ms: f64,
    log: Vec<ScriptedCall>,
    devices: Vec<OutputDevice>,
}

/// See the module docs.
pub struct ScriptedBackend {
    clock: Arc<dyn Clock>,
    sink: ReportSink,
    state: Mutex<State>,
}

impl ScriptedBackend {
    pub fn new(clock: Arc<dyn Clock>, sink: ReportSink) -> Self {
        let state = State {
            volume: 1.0,
            gapless: true,
            ..Default::default()
        };
        Self {
            clock,
            sink,
            state: Mutex::new(state),
        }
    }

    /// Make every future load of `key` fail fatally.
    pub fn fail_key(&self, key: impl Into<QueueKey>) {
        self.state.lock().failing.insert(key.into());
    }

    /// Clear a scripted failure.
    pub fn unfail_key(&self, key: &str) {
        self.state.lock().failing.remove(key);
    }

    /// Make every future load of a track fail fatally, however it is keyed
    /// (queue keys are re-materialised by the reducer; tracks are stable).
    pub fn fail_track(&self, track_id: impl Into<String>) {
        self.state.lock().failing_tracks.insert(track_id.into());
    }

    pub fn unfail_track(&self, track_id: &str) {
        self.state.lock().failing_tracks.remove(track_id);
    }

    /// Virtual time a load takes before reporting `Ready` (default 0).
    pub fn set_load_latency_ms(&self, ms: f64) {
        self.state.lock().load_latency_ms = ms.max(0.0);
    }

    /// The device list [`PlaybackBackend::output_devices`] returns.
    pub fn set_devices(&self, devices: Vec<OutputDevice>) {
        self.state.lock().devices = devices.clone();
        (self.sink)(BackendReport::OutputDevicesChanged { devices });
    }

    /// Every call made so far, oldest first.
    pub fn log(&self) -> Vec<ScriptedCall> {
        self.state.lock().log.clone()
    }

    pub fn clear_log(&self) {
        self.state.lock().log.clear();
    }

    /// Current key and position on the virtual clock, or `None` when idle.
    pub fn current(&self) -> Option<(QueueKey, u32)> {
        let st = self.state.lock();
        let now = self.clock.now_ms();
        st.current
            .as_ref()
            .map(|c| (c.source.key.clone(), c.position_at(now, st.playing) as u32))
    }

    pub fn is_playing(&self) -> bool {
        self.state.lock().playing
    }

    pub fn volume(&self) -> f64 {
        self.state.lock().volume
    }

    /// Advance the virtual playback to the clock's current time, emitting
    /// whatever reports fall due. Call after every clock advance. Returns the
    /// reports emitted, in order (they are also sent to the sink).
    pub fn poll(&self) -> Vec<BackendReport> {
        let now = self.clock.now_ms();
        let mut out = Vec::new();
        let mut st = self.state.lock();

        if st.pending_pre_buffer && now >= st.load_started_ms + st.load_latency_ms {
            st.pending_pre_buffer = false;
            if let Some(p) = &st.pre_buffer {
                out.push(BackendReport::PreBufferReady { key: p.key.clone() });
            }
        }

        if st.pending_ready && now >= st.load_started_ms + st.load_latency_ms {
            st.pending_ready = false;
            let (key, duration, position, track_id) = {
                let cur = st.current.as_mut().expect("pending_ready implies current");
                cur.anchor_ms = now;
                (
                    cur.source.key.clone(),
                    cur.source.track.duration_ms,
                    cur.position_ms as u32,
                    cur.source.track.id.clone(),
                )
            };
            let failing = st.failing.contains(&key) || st.failing_tracks.contains(&track_id);
            if failing {
                st.current = None;
                st.playing = false;
                out.push(BackendReport::Error {
                    key,
                    message: "scripted failure".into(),
                    fatal: true,
                });
            } else {
                out.push(BackendReport::Ready {
                    key: key.clone(),
                    duration_ms: Some(duration),
                });
                if st.playing {
                    out.push(BackendReport::Playing {
                        key,
                        position_ms: position,
                    });
                } else {
                    out.push(BackendReport::Paused {
                        key,
                        position_ms: position,
                    });
                }
                st.last_position_report_ms = now;
            }
        }

        // Walk item boundaries. A single poll may cross several short items.
        loop {
            if !st.playing || st.pending_ready {
                break;
            }
            let Some(cur) = st.current.as_ref() else {
                break;
            };
            let end_at = cur.anchor_ms + (cur.duration() - cur.position_ms).max(0.0);
            if now < end_at {
                break;
            }
            let ended_key = cur.source.key.clone();
            out.push(BackendReport::Ended { key: ended_key });
            match st.next.take() {
                Some(next)
                    if !st.failing.contains(&next.key)
                        && !st.failing_tracks.contains(&next.track.id) =>
                {
                    let key = next.key.clone();
                    st.current = Some(Item {
                        source: next,
                        position_ms: 0.0,
                        anchor_ms: end_at,
                    });
                    out.push(BackendReport::TransitionedToNext { key: key.clone() });
                    out.push(BackendReport::Position {
                        key,
                        position_ms: 0,
                    });
                    st.last_position_report_ms = end_at;
                }
                Some(next) => {
                    st.current = None;
                    st.playing = false;
                    out.push(BackendReport::Error {
                        key: next.key,
                        message: "scripted failure".into(),
                        fatal: true,
                    });
                }
                None => {
                    st.current = None;
                    st.playing = false;
                }
            }
        }

        if st.playing && !st.pending_ready {
            if let Some(cur) = st.current.as_ref() {
                if now - st.last_position_report_ms >= POSITION_INTERVAL_MS {
                    out.push(BackendReport::Position {
                        key: cur.source.key.clone(),
                        position_ms: cur.position_at(now, true) as u32,
                    });
                    st.last_position_report_ms = now;
                }
            }
        }

        drop(st);
        for r in &out {
            (self.sink)(r.clone());
        }
        out
    }

    fn emit(&self, report: BackendReport) {
        (self.sink)(report);
    }
}

impl PlaybackBackend for ScriptedBackend {
    fn load(
        &self,
        source: MediaSource,
        next: Option<MediaSource>,
        position_ms: u32,
        play: bool,
    ) -> Result<(), BackendError> {
        let now = self.clock.now_ms();
        let mut st = self.state.lock();
        st.log.push(ScriptedCall::Load {
            key: source.key.clone(),
            next: next.as_ref().map(|n| n.key.clone()),
            position_ms,
            play,
        });
        let position = f64::from(position_ms).min(f64::from(source.track.duration_ms));
        if st.pre_buffer.as_ref().map(|p| &p.key) == Some(&source.key) {
            st.pre_buffer = None;
            st.pending_pre_buffer = false;
        }
        st.current = Some(Item {
            source,
            position_ms: position,
            anchor_ms: now,
        });
        st.next = next;
        st.playing = play;
        st.pending_ready = true;
        st.load_started_ms = now;
        Ok(())
    }

    fn set_next(&self, next: Option<MediaSource>) -> Result<(), BackendError> {
        let mut st = self.state.lock();
        st.log.push(ScriptedCall::SetNext {
            next: next.as_ref().map(|n| n.key.clone()),
        });
        st.next = next;
        Ok(())
    }

    fn play(&self) -> Result<(), BackendError> {
        let now = self.clock.now_ms();
        let report = {
            let mut st = self.state.lock();
            st.log.push(ScriptedCall::Play);
            let playing = st.playing;
            let Some(cur) = st.current.as_mut() else {
                return Ok(());
            };
            if playing {
                return Ok(());
            }
            cur.anchor_ms = now;
            let r = BackendReport::Playing {
                key: cur.source.key.clone(),
                position_ms: cur.position_ms as u32,
            };
            st.playing = true;
            st.last_position_report_ms = now;
            if st.pending_ready {
                return Ok(());
            }
            r
        };
        self.emit(report);
        Ok(())
    }

    fn pause(&self) -> Result<(), BackendError> {
        let now = self.clock.now_ms();
        let report = {
            let mut st = self.state.lock();
            st.log.push(ScriptedCall::Pause);
            let playing = st.playing;
            let pending = st.pending_ready;
            let Some(cur) = st.current.as_mut() else {
                return Ok(());
            };
            if !playing {
                return Ok(());
            }
            cur.rebase(now, true);
            st.playing = false;
            if pending {
                return Ok(());
            }
            let cur = st.current.as_ref().expect("checked");
            BackendReport::Paused {
                key: cur.source.key.clone(),
                position_ms: cur.position_ms as u32,
            }
        };
        self.emit(report);
        Ok(())
    }

    fn stop(&self) -> Result<(), BackendError> {
        let mut st = self.state.lock();
        st.log.push(ScriptedCall::Stop);
        st.current = None;
        st.next = None;
        st.pre_buffer = None;
        st.playing = false;
        st.pending_ready = false;
        st.pending_pre_buffer = false;
        Ok(())
    }

    fn seek(&self, position_ms: u32) -> Result<(), BackendError> {
        let now = self.clock.now_ms();
        let report = {
            let mut st = self.state.lock();
            st.log.push(ScriptedCall::Seek { position_ms });
            let pending = st.pending_ready;
            let Some(cur) = st.current.as_mut() else {
                return Ok(());
            };
            cur.position_ms = f64::from(position_ms).min(cur.duration());
            cur.anchor_ms = now;
            st.last_position_report_ms = now;
            if pending {
                return Ok(());
            }
            let cur = st.current.as_ref().expect("checked");
            BackendReport::Position {
                key: cur.source.key.clone(),
                position_ms: cur.position_ms as u32,
            }
        };
        self.emit(report);
        Ok(())
    }

    fn set_volume(&self, volume: f64) -> Result<(), BackendError> {
        let mut st = self.state.lock();
        let volume = clamp_volume(volume);
        st.log.push(ScriptedCall::SetVolume { volume });
        st.volume = volume;
        Ok(())
    }

    fn pre_buffer(&self, source: MediaSource, position_ms: u32) -> Result<(), BackendError> {
        let now = self.clock.now_ms();
        let mut st = self.state.lock();
        st.log.push(ScriptedCall::PreBuffer {
            key: source.key.clone(),
            position_ms,
        });
        st.pre_buffer = Some(source);
        st.pending_pre_buffer = true;
        if !st.pending_ready {
            st.load_started_ms = now;
        }
        Ok(())
    }

    fn discard_pre_buffer(&self) -> Result<(), BackendError> {
        let mut st = self.state.lock();
        st.log.push(ScriptedCall::DiscardPreBuffer);
        st.pre_buffer = None;
        st.pending_pre_buffer = false;
        Ok(())
    }

    fn set_gapless(&self, enabled: bool) -> Result<(), BackendError> {
        let mut st = self.state.lock();
        st.log.push(ScriptedCall::SetGapless { enabled });
        st.gapless = enabled;
        Ok(())
    }

    fn set_output_device(&self, id: Option<String>) -> Result<(), BackendError> {
        let mut st = self.state.lock();
        st.log
            .push(ScriptedCall::SetOutputDevice { id: id.clone() });
        if let Some(id) = &id {
            if !st.devices.iter().any(|d| &d.id == id) {
                return Err(BackendError::UnknownDevice(id.clone()));
            }
        }
        st.device = id;
        Ok(())
    }

    fn output_devices(&self) -> Vec<OutputDevice> {
        self.state.lock().devices.clone()
    }

    fn name(&self) -> &'static str {
        "scripted"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TrackSummary;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Minimal virtual clock for these tests (the sim harness has its own).
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

    fn source(key: &str, duration_ms: u32) -> MediaSource {
        MediaSource {
            key: key.into(),
            track: TrackSummary {
                id: format!("t-{key}"),
                duration_ms,
                ..Default::default()
            },
            url: format!("file:///{key}"),
            headers: Default::default(),
            mime_type: None,
            gain_db: 0.0,
            transcoded: false,
        }
    }

    fn harness() -> (
        Arc<TestClock>,
        ScriptedBackend,
        Arc<Mutex<Vec<BackendReport>>>,
    ) {
        let clock = Arc::new(TestClock(AtomicU64::new(1_000_000)));
        let reps: Arc<Mutex<Vec<BackendReport>>> = Default::default();
        let r2 = reps.clone();
        let b = ScriptedBackend::new(clock.clone(), Arc::new(move |r| r2.lock().push(r)));
        (clock, b, reps)
    }

    #[test]
    fn load_reports_ready_then_playing_and_positions_at_one_hz() {
        let (clock, b, _) = harness();
        b.load(source("a", 5000), None, 500, true).unwrap();
        let r = b.poll();
        assert_eq!(
            r[0],
            BackendReport::Ready {
                key: "a".into(),
                duration_ms: Some(5000)
            }
        );
        assert_eq!(
            r[1],
            BackendReport::Playing {
                key: "a".into(),
                position_ms: 500
            }
        );
        clock.advance(999);
        assert!(b.poll().is_empty());
        clock.advance(1);
        assert_eq!(
            b.poll(),
            vec![BackendReport::Position {
                key: "a".into(),
                position_ms: 1500
            }]
        );
        assert_eq!(b.current(), Some(("a".into(), 1500)));
    }

    #[test]
    fn ends_and_transitions_to_next_at_the_exact_boundary() {
        let (clock, b, _) = harness();
        b.load(source("a", 3000), Some(source("b", 2000)), 0, true)
            .unwrap();
        b.poll();
        clock.advance(2999);
        b.poll();
        clock.advance(1);
        let r = b.poll();
        assert_eq!(r[0], BackendReport::Ended { key: "a".into() });
        assert_eq!(r[1], BackendReport::TransitionedToNext { key: "b".into() });
        assert_eq!(
            r[2],
            BackendReport::Position {
                key: "b".into(),
                position_ms: 0
            }
        );
        // b started at the boundary, not at poll time: 1 ms in.
        assert_eq!(b.current(), Some(("b".into(), 0)));
        clock.advance(2000);
        let r = b.poll();
        assert_eq!(r, vec![BackendReport::Ended { key: "b".into() }]);
        assert!(!b.is_playing());
        assert_eq!(b.current(), None);
    }

    #[test]
    fn a_long_gap_crosses_several_items() {
        let (clock, b, _) = harness();
        b.load(source("a", 1000), Some(source("b", 1000)), 0, true)
            .unwrap();
        b.poll();
        clock.advance(5000);
        let r = b.poll();
        assert!(r.contains(&BackendReport::Ended { key: "a".into() }));
        assert!(r.contains(&BackendReport::TransitionedToNext { key: "b".into() }));
        assert!(r.contains(&BackendReport::Ended { key: "b".into() }));
        assert!(!b.is_playing());
    }

    #[test]
    fn pause_freezes_position_and_play_resumes() {
        let (clock, b, reps) = harness();
        b.load(source("a", 10_000), None, 0, true).unwrap();
        b.poll();
        clock.advance(1500);
        b.pause().unwrap();
        assert_eq!(
            reps.lock().last(),
            Some(&BackendReport::Paused {
                key: "a".into(),
                position_ms: 1500
            })
        );
        clock.advance(5000);
        assert!(b.poll().is_empty());
        assert_eq!(b.current(), Some(("a".into(), 1500)));
        b.play().unwrap();
        assert_eq!(
            reps.lock().last(),
            Some(&BackendReport::Playing {
                key: "a".into(),
                position_ms: 1500
            })
        );
        clock.advance(1000);
        assert_eq!(
            b.poll(),
            vec![BackendReport::Position {
                key: "a".into(),
                position_ms: 2500
            }]
        );
    }

    #[test]
    fn seek_reports_a_discontinuity_and_clamps() {
        let (_, b, reps) = harness();
        b.load(source("a", 10_000), None, 0, false).unwrap();
        b.poll();
        b.seek(4000).unwrap();
        assert_eq!(
            reps.lock().last(),
            Some(&BackendReport::Position {
                key: "a".into(),
                position_ms: 4000
            })
        );
        b.seek(40_000).unwrap();
        assert_eq!(b.current(), Some(("a".into(), 10_000)));
    }

    #[test]
    fn scripted_failure_reports_fatal_error_instead_of_ready() {
        let (_, b, _) = harness();
        b.fail_key("bad");
        b.load(source("bad", 1000), None, 0, true).unwrap();
        let r = b.poll();
        assert_eq!(
            r,
            vec![BackendReport::Error {
                key: "bad".into(),
                message: "scripted failure".into(),
                fatal: true
            }]
        );
        assert!(!b.is_playing());
        assert_eq!(b.current(), None);
    }

    #[test]
    fn load_latency_delays_ready() {
        let (clock, b, _) = harness();
        b.set_load_latency_ms(250.0);
        b.load(source("a", 1000), None, 0, true).unwrap();
        assert!(b.poll().is_empty());
        clock.advance(250);
        let r = b.poll();
        assert!(matches!(r[0], BackendReport::Ready { .. }));
        // Playback anchors at Ready, not at load.
        clock.advance(1000);
        assert_eq!(b.poll(), vec![BackendReport::Ended { key: "a".into() }]);
    }

    #[test]
    fn prebuffer_ready_and_discard() {
        let (_, b, _) = harness();
        b.pre_buffer(source("p", 1000), 100).unwrap();
        assert_eq!(
            b.poll(),
            vec![BackendReport::PreBufferReady { key: "p".into() }]
        );
        b.pre_buffer(source("q", 1000), 100).unwrap();
        b.discard_pre_buffer().unwrap();
        assert!(b.poll().is_empty());
        assert_eq!(b.log().last(), Some(&ScriptedCall::DiscardPreBuffer));
    }

    #[test]
    fn devices_and_stop() {
        let (_, b, reps) = harness();
        b.set_devices(vec![OutputDevice {
            id: "d1".into(),
            name: "Speakers".into(),
            is_default: true,
        }]);
        assert!(
            matches!(reps.lock().last(), Some(BackendReport::OutputDevicesChanged { devices }) if devices.len() == 1)
        );
        assert!(b.set_output_device(Some("nope".into())).is_err());
        assert!(b.set_output_device(Some("d1".into())).is_ok());
        b.load(source("a", 1000), None, 0, true).unwrap();
        b.stop().unwrap();
        assert!(b.poll().is_empty());
        assert_eq!(b.current(), None);
    }
}
