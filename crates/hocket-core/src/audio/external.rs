//! The external backend: a bridge to a platform player (Android / Media3
//! ExoPlayer) on the other side of the FFI.
//!
//! Every [`PlaybackBackend`] call becomes an [`api::BackendCommand`] emitted
//! through the command callback the actor wires to [`api::Event::Backend`].
//! The platform executes it and posts [`api::BackendReport`]s back through
//! [`ExternalBackend::report`], which forwards them to the actor's
//! [`ReportSink`] unchanged, so the actor sees the same reports it would from
//! the native backend.
//!
//! ## Who applies what
//!
//! - **Gain** — the core cannot touch the platform player's samples, so it
//!   folds ReplayGain, preamp and normalisation into
//!   [`api::MediaSource::gain_db`] (via [`crate::audio::dsp::gain_for_track`])
//!   and the platform applies it (Media3: `setVolume(10^(gain_db/20))` on the
//!   item or a `ReplayGainAudioProcessor`; EQ through `android.media.audiofx`
//!   with the same band table). The value is absolute per item, not a delta.
//! - **Volume** — `SetVolume` is the user volume, distinct from `gain_db`.
//!   The platform multiplies them.
//! - **Output device** — the platform owns routing on Android;
//!   [`PlaybackBackend::set_output_device`] returns
//!   [`BackendError::Unsupported`] and [`PlaybackBackend::output_devices`] is
//!   empty. `OutputDevicesChanged` reports are still forwarded if the platform
//!   sends them.
//! - **Gapless** — Media3 preloads the `next` item; `SetGapless{false}` maps to
//!   not calling `setMediaItems` with the follow-up.
//!
//! The bridge tracks the key of the item it believes is current so it can
//! drop stale reports from a previous load (Media3 reports `Ended` for an
//! item that was replaced by a later `Load` before the report arrived).

use std::sync::Arc;

use parking_lot::Mutex;

use crate::api::{BackendCommand, BackendReport, MediaSource, OutputDevice, QueueKey};
use crate::audio::backend::{clamp_volume, BackendError, PlaybackBackend, ReportSink};

/// Receives outgoing [`BackendCommand`]s. The actor turns them into
/// [`crate::api::Event::Backend`].
pub type CommandSink = Arc<dyn Fn(BackendCommand) + Send + Sync>;

#[derive(Debug, Default)]
struct State {
    current: Option<QueueKey>,
    next: Option<QueueKey>,
    pre_buffer: Option<QueueKey>,
    /// Keys that were loaded then replaced; reports for them are dropped.
    retired: Vec<QueueKey>,
}

/// See the module docs.
pub struct ExternalBackend {
    commands: CommandSink,
    reports: ReportSink,
    state: Mutex<State>,
}

impl ExternalBackend {
    pub fn new(commands: CommandSink, reports: ReportSink) -> Self {
        Self {
            commands,
            reports,
            state: Mutex::new(State::default()),
        }
    }

    fn send(&self, command: BackendCommand) -> Result<(), BackendError> {
        tracing::trace!(target: "hocket::audio::external", ?command, "command");
        (self.commands)(command);
        Ok(())
    }

    /// Entry point for [`crate::api::Command::BackendReport`]. Forwards to the
    /// actor after filtering reports about items this backend no longer has
    /// loaded (late reports from a replaced item).
    pub fn report(&self, report: BackendReport) {
        let forward = {
            let mut st = self.state.lock();
            match &report {
                BackendReport::TransitionedToNext { key } => {
                    if st.next.as_ref() == Some(key) {
                        if let Some(old) = st.current.replace(key.clone()) {
                            st.retired.push(old);
                        }
                        st.next = None;
                    }
                    true
                }
                BackendReport::PreBufferReady { key } => st.pre_buffer.as_ref() == Some(key),
                BackendReport::Ready { key, .. }
                | BackendReport::Playing { key, .. }
                | BackendReport::Paused { key, .. }
                | BackendReport::Buffering { key, .. }
                | BackendReport::Position { key, .. }
                | BackendReport::Ended { key }
                | BackendReport::Error { key, .. } => !st.retired.contains(key),
                BackendReport::AudioFocusLost { .. }
                | BackendReport::OutputDevicesChanged { .. } => true,
            }
        };
        if forward {
            (self.reports)(report);
        } else {
            tracing::debug!(target: "hocket::audio::external", ?report, "dropped stale report");
        }
    }

    /// Key of the item the bridge believes is current, for diagnostics.
    pub fn current_key(&self) -> Option<QueueKey> {
        self.state.lock().current.clone()
    }
}

impl PlaybackBackend for ExternalBackend {
    fn load(
        &self,
        source: MediaSource,
        next: Option<MediaSource>,
        position_ms: u32,
        play: bool,
    ) -> Result<(), BackendError> {
        {
            let mut st = self.state.lock();
            let new_key = source.key.clone();
            if let Some(old) = st.current.take() {
                if old != new_key && !st.retired.contains(&old) {
                    st.retired.push(old);
                }
            }
            if let Some(old_next) = st.next.take() {
                if old_next != new_key && next.as_ref().map(|n| &n.key) != Some(&old_next) {
                    st.retired.push(old_next);
                }
            }
            st.retired.retain(|k| k != &new_key);
            if let Some(n) = &next {
                st.retired.retain(|k| k != &n.key);
            }
            if st.retired.len() > 32 {
                let excess = st.retired.len() - 32;
                st.retired.drain(..excess);
            }
            st.current = Some(new_key);
            st.next = next.as_ref().map(|n| n.key.clone());
            if st.pre_buffer.as_ref() == st.current.as_ref() {
                st.pre_buffer = None;
            }
        }
        self.send(BackendCommand::Load {
            source,
            next,
            position_ms,
            play,
        })
    }

    fn set_next(&self, next: Option<MediaSource>) -> Result<(), BackendError> {
        {
            let mut st = self.state.lock();
            if let Some(old) = st.next.take() {
                if next.as_ref().map(|n| &n.key) != Some(&old) && st.current.as_ref() != Some(&old)
                {
                    st.retired.push(old);
                }
            }
            if let Some(n) = &next {
                st.retired.retain(|k| k != &n.key);
            }
            st.next = next.as_ref().map(|n| n.key.clone());
        }
        self.send(BackendCommand::SetNext { next })
    }

    fn play(&self) -> Result<(), BackendError> {
        self.send(BackendCommand::Play)
    }

    fn pause(&self) -> Result<(), BackendError> {
        self.send(BackendCommand::Pause)
    }

    fn stop(&self) -> Result<(), BackendError> {
        {
            let mut st = self.state.lock();
            for k in st
                .current
                .take()
                .into_iter()
                .chain(st.next.take())
                .chain(st.pre_buffer.take())
            {
                st.retired.push(k);
            }
        }
        self.send(BackendCommand::Stop)
    }

    fn seek(&self, position_ms: u32) -> Result<(), BackendError> {
        self.send(BackendCommand::Seek { position_ms })
    }

    fn set_volume(&self, volume: f64) -> Result<(), BackendError> {
        self.send(BackendCommand::SetVolume {
            volume: clamp_volume(volume),
        })
    }

    fn pre_buffer(&self, source: MediaSource, position_ms: u32) -> Result<(), BackendError> {
        {
            let mut st = self.state.lock();
            st.retired.retain(|k| k != &source.key);
            st.pre_buffer = Some(source.key.clone());
        }
        self.send(BackendCommand::PreBuffer {
            source,
            position_ms,
        })
    }

    fn discard_pre_buffer(&self) -> Result<(), BackendError> {
        self.state.lock().pre_buffer = None;
        self.send(BackendCommand::DiscardPreBuffer)
    }

    fn set_gapless(&self, enabled: bool) -> Result<(), BackendError> {
        self.send(BackendCommand::SetGapless { enabled })
    }

    fn set_output_device(&self, _id: Option<String>) -> Result<(), BackendError> {
        Err(BackendError::Unsupported(
            "output device selection is owned by the platform",
        ))
    }

    fn output_devices(&self) -> Vec<OutputDevice> {
        Vec::new()
    }

    fn name(&self) -> &'static str {
        "external"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TrackSummary;

    fn source(key: &str) -> MediaSource {
        MediaSource {
            key: key.into(),
            track: TrackSummary {
                id: format!("t-{key}"),
                duration_ms: 1000,
                ..Default::default()
            },
            url: format!("https://example/{key}"),
            headers: Default::default(),
            mime_type: None,
            gain_db: -3.0,
            transcoded: false,
        }
    }

    type Log<T> = Arc<Mutex<Vec<T>>>;

    fn harness() -> (ExternalBackend, Log<BackendCommand>, Log<BackendReport>) {
        let cmds: Log<BackendCommand> = Default::default();
        let reps: Log<BackendReport> = Default::default();
        let c2 = cmds.clone();
        let r2 = reps.clone();
        let b = ExternalBackend::new(
            Arc::new(move |c| c2.lock().push(c)),
            Arc::new(move |r| r2.lock().push(r)),
        );
        (b, cmds, reps)
    }

    #[test]
    fn commands_are_forwarded_verbatim() {
        let (b, cmds, _) = harness();
        b.load(source("a"), Some(source("b")), 1500, true).unwrap();
        b.set_volume(1.5).unwrap();
        b.seek(20).unwrap();
        b.set_gapless(false).unwrap();
        let cmds = cmds.lock();
        assert!(
            matches!(&cmds[0], BackendCommand::Load { source, next: Some(n), position_ms: 1500, play: true } if source.key == "a" && n.key == "b")
        );
        assert_eq!(cmds[1], BackendCommand::SetVolume { volume: 1.0 });
        assert_eq!(cmds[2], BackendCommand::Seek { position_ms: 20 });
        assert_eq!(cmds[3], BackendCommand::SetGapless { enabled: false });
    }

    #[test]
    fn stale_reports_are_dropped_after_reload() {
        let (b, _, reps) = harness();
        b.load(source("a"), None, 0, true).unwrap();
        b.load(source("b"), None, 0, true).unwrap();
        b.report(BackendReport::Ended { key: "a".into() });
        b.report(BackendReport::Playing {
            key: "b".into(),
            position_ms: 0,
        });
        let reps = reps.lock();
        assert_eq!(reps.len(), 1);
        assert!(matches!(&reps[0], BackendReport::Playing { key, .. } if key == "b"));
    }

    #[test]
    fn transition_promotes_next_and_retires_current() {
        let (b, _, reps) = harness();
        b.load(source("a"), Some(source("b")), 0, true).unwrap();
        b.report(BackendReport::Ended { key: "a".into() });
        b.report(BackendReport::TransitionedToNext { key: "b".into() });
        assert_eq!(b.current_key().as_deref(), Some("b"));
        b.report(BackendReport::Position {
            key: "a".into(),
            position_ms: 5,
        });
        b.report(BackendReport::Position {
            key: "b".into(),
            position_ms: 5,
        });
        let reps = reps.lock();
        assert_eq!(reps.len(), 3);
        assert!(matches!(&reps[2], BackendReport::Position { key, .. } if key == "b"));
    }

    #[test]
    fn prebuffer_ready_only_for_the_live_prebuffer() {
        let (b, _, reps) = harness();
        b.pre_buffer(source("p"), 100).unwrap();
        b.discard_pre_buffer().unwrap();
        b.report(BackendReport::PreBufferReady { key: "p".into() });
        assert!(reps.lock().is_empty());
        b.pre_buffer(source("p"), 100).unwrap();
        b.report(BackendReport::PreBufferReady { key: "p".into() });
        assert_eq!(reps.lock().len(), 1);
    }

    #[test]
    fn device_selection_unsupported() {
        let (b, _, _) = harness();
        assert_eq!(
            b.set_output_device(None),
            Err(BackendError::Unsupported(
                "output device selection is owned by the platform"
            ))
        );
        assert!(b.output_devices().is_empty());
    }
}
